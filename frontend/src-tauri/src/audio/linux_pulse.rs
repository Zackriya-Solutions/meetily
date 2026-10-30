//! PulseAudio's protocol is also served by pipewire-pulse. Keep server source
//! names intact: ALSA PCM names are not PulseAudio monitor source names.
use anyhow::{anyhow, bail, Context as _, Result};
use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::{Context, FlagSet as ContextFlags, State as ContextState};
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::operation::State as OperationState;
use pulse::sample::{Format, Spec};
use pulse::stream::{FlagSet, PeekResult, State as StreamState, Stream};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u8 = 2;
const TIMEOUT: Duration = Duration::from_secs(5);

struct Connection {
    // Drop the context before its mainloop.
    context: Context,
    mainloop: Mainloop,
}

impl Connection {
    fn new() -> Result<Self> {
        let mainloop =
            Mainloop::new().ok_or_else(|| anyhow!("Cannot create PulseAudio mainloop"))?;
        let mut context = Context::new(&mainloop, "Meetily")
            .ok_or_else(|| anyhow!("Cannot create PulseAudio context"))?;
        context
            .connect(None, ContextFlags::NOAUTOSPAWN, None)
            .context("Cannot connect to PulseAudio/PipeWire")?;
        let mut connection = Self { context, mainloop };
        // All PulseAudio objects stay on the thread which created them.
        let deadline = Instant::now() + TIMEOUT;
        while connection.context.get_state() != ContextState::Ready {
            connection.tick(deadline)?;
        }
        Ok(connection)
    }

    fn tick(&mut self, deadline: Instant) -> Result<()> {
        if matches!(
            self.context.get_state(),
            ContextState::Failed | ContextState::Terminated
        ) {
            bail!("PulseAudio/PipeWire disconnected: {}", self.context.errno());
        }
        if Instant::now() >= deadline {
            bail!("Timed out waiting for PulseAudio/PipeWire");
        }
        match self.mainloop.iterate(false) {
            IterateResult::Success(_) => {}
            other => bail!("PulseAudio mainloop stopped: {:?}", other),
        }
        thread::sleep(Duration::from_millis(2));
        Ok(())
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.context.disconnect();
    }
}

#[derive(Clone, Debug)]
pub struct MonitorSource {
    pub name: String,
    sink_name: String,
}

pub struct MonitorSources {
    pub sources: Vec<MonitorSource>,
    default_sink: Option<String>,
}

impl MonitorSources {
    pub fn resolve(&self, requested: Option<&str>) -> Result<String> {
        // Accept names saved by earlier releases, but never capture the default
        // microphone when an output/monitor was requested.
        let requested = requested.map(|s| s.trim_end_matches(" (System Audio)"));
        let is_default = matches!(requested, None | Some("default" | "pulse" | "pipewire"));
        self.sources
            .iter()
            .find(|source| {
                if is_default {
                    self.default_sink.as_deref() == Some(source.sink_name.as_str())
                } else {
                    requested == Some(source.name.as_str())
                        || requested == Some(source.sink_name.as_str())
                }
            })
            .map(|source| source.name.clone())
            .ok_or_else(|| {
                anyhow!(
                    "No monitor source for output {:?}; check PulseAudio/PipeWire output devices",
                    requested
                )
            })
    }
}

pub fn monitor_sources() -> Result<MonitorSources> {
    let mut connection = Connection::new()?;
    let sources = Rc::new(RefCell::new(Vec::new()));
    let failed = Rc::new(RefCell::new(false));
    let result = sources.clone();
    let failure = failed.clone();
    let operation = connection
        .context
        .introspect()
        .get_source_info_list(move |info| match info {
            ListResult::Item(info) if info.monitor_of_sink.is_some() => {
                if let (Some(name), Some(sink)) = (&info.name, &info.monitor_of_sink_name) {
                    result.borrow_mut().push(MonitorSource {
                        name: name.to_string(),
                        sink_name: sink.to_string(),
                    });
                }
            }
            ListResult::Error => *failure.borrow_mut() = true,
            _ => {}
        });
    let deadline = Instant::now() + TIMEOUT;
    while operation.get_state() == OperationState::Running {
        connection.tick(deadline)?;
    }
    if *failed.borrow() {
        bail!("Cannot enumerate PulseAudio monitor sources");
    }

    let default_sink = Rc::new(RefCell::new(None));
    let result = default_sink.clone();
    let operation = connection
        .context
        .introspect()
        .get_server_info(move |info| {
            *result.borrow_mut() = info.default_sink_name.as_ref().map(ToString::to_string);
        });
    let deadline = Instant::now() + TIMEOUT;
    while operation.get_state() == OperationState::Running {
        connection.tick(deadline)?;
    }
    let sources = sources.borrow().clone();
    let default_sink = default_sink.borrow().clone();
    Ok(MonitorSources {
        sources,
        default_sink,
    })
}

/// A cancellable capture thread. No blocking reads: stopping also works while
/// the output is suspended or the server has disappeared.
pub struct PulseCapture {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl PulseCapture {
    pub fn start<F, E>(requested: &str, mut on_audio: F, on_error: E) -> Result<Self>
    where
        F: FnMut(&[f32]) + Send + 'static,
        E: FnOnce(String) + Send + 'static,
    {
        let source = monitor_sources()?.resolve(Some(requested))?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("meetily-system-audio".into())
            .spawn(move || {
                let result = (|| -> Result<()> {
                    let mut connection = Connection::new()?;
                    let spec = Spec {
                        format: Format::FLOAT32NE,
                        channels: CHANNELS,
                        rate: SAMPLE_RATE,
                    };
                    let mut stream =
                        Stream::new(&mut connection.context, "System audio", &spec, None)
                            .ok_or_else(|| anyhow!("Cannot create PulseAudio recording stream"))?;
                    let attributes = pulse::def::BufferAttr {
                        maxlength: u32::MAX,
                        tlength: u32::MAX,
                        prebuf: u32::MAX,
                        minreq: u32::MAX,
                        fragsize: (SAMPLE_RATE / 50) * CHANNELS as u32 * 4,
                    };
                    stream.connect_record(
                        Some(&source),
                        Some(&attributes),
                        FlagSet::ADJUST_LATENCY | FlagSet::DONT_MOVE,
                    )?;
                    let deadline = Instant::now() + TIMEOUT;
                    while stream.get_state() != StreamState::Ready {
                        if matches!(
                            stream.get_state(),
                            StreamState::Failed | StreamState::Terminated
                        ) {
                            bail!(
                                "Cannot open monitor {}: {}",
                                source,
                                connection.context.errno()
                            );
                        }
                        connection.tick(deadline)?;
                    }
                    log::info!(
                        "Recording PulseAudio/PipeWire monitor: {} (48000 Hz, stereo)",
                        source
                    );
                    let _ = ready_tx.send(Ok(()));
                    while !stopped.load(Ordering::Acquire) {
                        connection.tick(Instant::now() + TIMEOUT)?;
                        if stream.get_state() != StreamState::Ready {
                            bail!("Monitor {} disconnected", source);
                        }
                        for _ in 0..16 {
                            if stopped.load(Ordering::Acquire) {
                                break;
                            }
                            let samples = match stream.peek()? {
                                PeekResult::Empty => break,
                                PeekResult::Hole(bytes) => vec![0.0; bytes / 4],
                                PeekResult::Data(bytes) => bytes
                                    .chunks_exact(4)
                                    .map(|chunk| f32::from_ne_bytes(chunk.try_into().unwrap()))
                                    .collect(),
                            };
                            stream.discard()?;
                            on_audio(&samples);
                        }
                    }
                    stream.disconnect()?;
                    Ok(())
                })();
                if let Err(error) = result {
                    let message = format!("System audio capture failed: {error:#}");
                    let _ = ready_tx.send(Err(message.clone()));
                    log::error!("{}", message);
                    on_error(message);
                }
            })?;
        let capture = Self {
            stop,
            worker: Some(worker),
        };
        match ready_rx.recv_timeout(TIMEOUT * 3) {
            Ok(Ok(())) => Ok(capture),
            Ok(Err(error)) => Err(anyhow!(error)),
            Err(error) => Err(anyhow!("Monitor capture did not start: {}", error)),
        }
    }
}

impl Drop for PulseCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources() -> MonitorSources {
        MonitorSources {
            default_sink: Some("fiio".into()),
            sources: vec![
                MonitorSource {
                    name: "hdmi.monitor".into(),
                    sink_name: "hdmi".into(),
                },
                MonitorSource {
                    name: "fiio.monitor".into(),
                    sink_name: "fiio".into(),
                },
            ],
        }
    }

    #[test]
    fn default_resolves_to_active_sink_not_first_monitor() {
        for name in [None, Some("default"), Some("pulse"), Some("pipewire")] {
            assert_eq!(sources().resolve(name).unwrap(), "fiio.monitor");
        }
    }

    #[test]
    fn explicit_and_legacy_names_resolve_without_falling_back() {
        for name in ["hdmi.monitor", "hdmi", "hdmi.monitor (System Audio)"] {
            assert_eq!(sources().resolve(Some(name)).unwrap(), "hdmi.monitor");
        }
        assert!(sources().resolve(Some("disconnected.monitor")).is_err());
        let mut sources = sources();
        sources.default_sink = Some("disconnected".into());
        assert!(sources.resolve(None).is_err());
    }

    #[test]
    #[ignore = "requires a running PulseAudio or pipewire-pulse server"]
    fn live_monitor_records_tone_and_stops_without_microphone() -> Result<()> {
        use std::sync::atomic::AtomicUsize;
        let mut connection = Connection::new()?;
        let sink = format!("meetily_test_{}", std::process::id());
        let module = Rc::new(RefCell::new(u32::MAX));
        let result = module.clone();
        let operation = connection.context.introspect().load_module(
            "module-null-sink",
            &format!("sink_name={sink} rate=48000 channels=2"),
            move |index| *result.borrow_mut() = index,
        );
        let deadline = Instant::now() + TIMEOUT;
        while operation.get_state() == OperationState::Running {
            connection.tick(deadline)?;
        }
        let module = *module.borrow();
        anyhow::ensure!(module != u32::MAX, "Cannot create isolated test output");
        let result = (|| -> Result<()> {
            let source = monitor_sources()?.resolve(Some(&sink))?;
            let samples = Arc::new(AtomicUsize::new(0));
            let loud_samples = Arc::new(AtomicUsize::new(0));
            let errors = Arc::new(std::sync::Mutex::new(Vec::new()));
            let received = samples.clone();
            let loud = loud_samples.clone();
            let failures = errors.clone();
            let capture = PulseCapture::start(
                &source,
                move |data| {
                    received.fetch_add(data.len(), Ordering::Relaxed);
                    loud.fetch_add(
                        data.iter().filter(|s| s.abs() > 0.1).count(),
                        Ordering::Relaxed,
                    );
                },
                move |error| failures.lock().unwrap().push(error),
            )?;
            let spec = Spec {
                format: Format::FLOAT32NE,
                channels: CHANNELS,
                rate: SAMPLE_RATE,
            };
            let mut playback = Stream::new(&mut connection.context, "Test tone", &spec, None)
                .ok_or_else(|| anyhow!("Cannot create test playback"))?;
            playback.connect_playback(Some(&sink), None, FlagSet::NOFLAGS, None, None)?;
            let deadline = Instant::now() + TIMEOUT;
            while playback.get_state() != StreamState::Ready {
                connection.tick(deadline)?;
            }
            let mut frame = 0;
            while Instant::now() < deadline && loud_samples.load(Ordering::Relaxed) < 4_800 {
                connection.tick(deadline)?;
                let frames = playback.writable_size().unwrap_or(0).min(7_680) / 8;
                let mut tone = Vec::with_capacity(frames * 8);
                for _ in 0..frames {
                    let value = (frame as f32 * 440.0 * std::f32::consts::TAU / SAMPLE_RATE as f32)
                        .sin()
                        * 0.25;
                    frame += 1;
                    tone.extend_from_slice(&value.to_ne_bytes());
                    tone.extend_from_slice(&value.to_ne_bytes());
                }
                if !tone.is_empty() {
                    playback.write(&tone, None, 0, pulse::stream::SeekMode::Relative)?;
                }
            }
            playback.disconnect()?;
            let stopped_at = Instant::now();
            drop(capture);
            anyhow::ensure!(
                stopped_at.elapsed() < Duration::from_secs(1),
                "Capture shutdown blocked"
            );
            anyhow::ensure!(
                errors.lock().unwrap().is_empty(),
                "Capture reported an error"
            );
            anyhow::ensure!(
                loud_samples.load(Ordering::Relaxed) >= 4_800,
                "Monitor captured no test tone"
            );
            println!(
                "Captured {} stereo samples, {} above 0.1; shutdown {:?}",
                samples.load(Ordering::Relaxed),
                loud_samples.load(Ordering::Relaxed),
                stopped_at.elapsed()
            );
            Ok(())
        })();
        let operation = connection
            .context
            .introspect()
            .unload_module(module, |_| {});
        let deadline = Instant::now() + TIMEOUT;
        while operation.get_state() == OperationState::Running {
            connection.tick(deadline)?;
        }
        result
    }
}
