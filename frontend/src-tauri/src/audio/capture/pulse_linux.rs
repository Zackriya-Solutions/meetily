// Native PipeWire/PulseAudio system-audio capture for Linux.
//
// Bypasses cpal's ALSA-only host entirely by talking to the PulseAudio client
// protocol directly (which PipeWire also implements via pipewire-pulse). This
// avoids the need to hand-register monitor sources as named ALSA pseudo-devices
// in ~/.asoundrc: sink descriptions and monitor sources come straight from the
// server's real metadata.
//
// Device enumeration (list_sinks, list_sources, default_source_name)
// is delegated to `pulse_enumerator.rs`, which keeps a single persistent
// libpulse connection on a dedicated thread instead of opening and destroying
// two fresh connections every 2 seconds for the whole meeting. See TECH-02
// for the rationale.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::{context::{Context, FlagSet as ContextFlags, State as ContextState},
    mainloop::standard::{Mainloop, IterateResult}, stream::{Stream, FlagSet, State, PeekResult}};
use std::time::{Duration, Instant};
use log::{debug, warn};

use super::pulse_enumerator;

/// A PulseAudio sink, with the metadata needed to offer it as a "System Audio"
/// capture device: a real display description and its monitor source name.
#[derive(Debug, Clone)]
pub struct PulseSink {
    /// Exact PulseAudio sink name: the identity of the "System Audio" device.
    pub name: String,
    pub description: String,
    pub monitor_source_name: String,
}

/// A PulseAudio input source (microphone, line-in, …), excluding sink monitors.
#[derive(Debug, Clone)]
pub struct PulseSource {
    /// Human-readable description straight from the server — this is the exact
    /// string KDE's own audio settings show (e.g. "Ryzen HD Audio Controller
    /// Headset Mono Microphone").
    pub description: String,
    /// Real PulseAudio source name, e.g.
    /// "alsa_input.pci-0000_c1_00.6.HiFi__Headset__source".
    pub source_name: String,
}

/// Fixed capture format requested from the server. PulseAudio/PipeWire
/// transparently resamples and remixes the monitor source to this spec
/// server-side, so the rest of the pipeline (which expects 48kHz) never needs
/// to know the sink's native sample rate or channel count.
const CAPTURE_SAMPLE_RATE: u32 = 48000;

/// List all sinks (playback outputs) with their monitor source name, for use as
/// "System Audio" capture devices. Descriptions come straight from the server
/// and are refreshed on every call: a persistent connection does not cache
/// server state, it only avoids the repeated connect/handshake/disconnect churn
/// (see TECH-02).
pub fn list_sinks() -> Result<Vec<PulseSink>> {
    debug!("pulse_linux::list_sinks: requesting");
    let result = pulse_enumerator::list_sinks();
    if let Ok(ref sinks) = result {
        debug!("pulse_linux::list_sinks: got {} sink(s)", sinks.len());
    }
    result
}

/// The sink the server currently routes playback to (`default_sink_name`), or
/// `None` if the server reports no default or the sink has no monitor source.
pub fn default_sink() -> Result<Option<PulseSink>> {
    let Some(default_name) = pulse_enumerator::default_sink_name()? else {
        return Ok(None);
    };
    let sink = list_sinks()?.into_iter().find(|s| s.name == default_name);
    if sink.is_none() {
        warn!(
            "pulse_linux::default_sink: default sink '{}' not found in sink list",
            default_name
        );
    }
    Ok(sink)
}

/// Resolve a sink by its exact PulseAudio name. A missing sink is an error:
/// there is deliberately no fallback to another endpoint.
pub fn find_sink_by_name(name: &str) -> Result<PulseSink> {
    pick_sink(list_sinks()?, name)
}

fn pick_sink(sinks: Vec<PulseSink>, name: &str) -> Result<PulseSink> {
    sinks
        .into_iter()
        .find(|sink| sink.name == name)
        .ok_or_else(|| not_found_error(name))
}

fn not_found_error(name: &str) -> anyhow::Error {
    anyhow!("Audio device '{}' not found; select an available device", name)
}

/// List all real input sources (sink monitors excluded — those are offered as
/// "System Audio" devices via `list_sinks`).
pub fn list_sources() -> Result<Vec<PulseSource>> {
    debug!("pulse_linux::list_sources: requesting");
    let result = pulse_enumerator::list_sources();
    if let Ok(ref sources) = result {
        debug!("pulse_linux::list_sources: got {} source(s)", sources.len());
    }
    result
}

/// Resolve a source by its exact PulseAudio name. A missing source is an
/// error: there is deliberately no fallback to another endpoint.
pub fn find_source_by_name(name: &str) -> Result<PulseSource> {
    pick_source(list_sources()?, name)
}

fn pick_source(sources: Vec<PulseSource>, name: &str) -> Result<PulseSource> {
    sources
        .into_iter()
        .find(|source| source.source_name == name)
        .ok_or_else(|| not_found_error(name))
}

/// Exact name of the server's default input source, if any.
pub fn default_source_name() -> Result<Option<String>> {
    pulse_enumerator::default_source_name()
}

/// Exact name of the server's default sink, if any.
pub fn default_sink_name() -> Result<Option<String>> {
    pulse_enumerator::default_sink_name()
}

/// Capture configuration is Send; all libpulse objects are created and destroyed
/// on the capture worker because the standard mainloop is not Send.
pub struct PulseCapture {
    source: String,
    should_stop: Arc<AtomicBool>,
    channels: u16,
}

pub const SERVER_UNAVAILABLE: &str = "System audio is unavailable. Ensure PulseAudio or PipeWire’s PulseAudio compatibility service is running.";

impl PulseCapture {
    pub fn new_system(source: &str) -> Result<Self> { Self::new(source, 2) }
    pub fn new_microphone(source: &str) -> Result<Self> { Self::new(source, 1) }
    fn new(source: &str, channels: u16) -> Result<Self> {
        Ok(Self { source: source.into(), channels, should_stop: Arc::new(AtomicBool::new(false)) })
    }
    pub fn sample_rate(&self) -> u32 { CAPTURE_SAMPLE_RATE }
    pub fn channels(&self) -> u16 { self.channels }
    pub fn stop_handle(&self) -> Arc<AtomicBool> { self.should_stop.clone() }

    pub fn run(&self, on_samples: impl FnMut(&[f32])) -> Result<()> {
        self.run_ready(on_samples, || {})
    }

    pub fn run_ready(&self, mut on_samples: impl FnMut(&[f32]), ready: impl FnOnce()) -> Result<()> {
        if self.should_stop.load(Ordering::Acquire) { return Ok(()); }
        // Declaration order ensures stream/context are dropped before mainloop.
        let mut mainloop = Mainloop::new().ok_or_else(|| anyhow!("Cannot create audio mainloop"))?;
        let mut context = Context::new(&mainloop, "Meetily").ok_or_else(|| anyhow!("Cannot create audio context"))?;
        context.connect(None, ContextFlags::NOAUTOSPAWN, None)
            .map_err(|e| anyhow!("{} ({})", SERVER_UNAVAILABLE, e))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while context.get_state() != ContextState::Ready {
            if self.should_stop.load(Ordering::Acquire) { return Ok(()); }
            if matches!(context.get_state(), ContextState::Failed | ContextState::Terminated) || Instant::now() >= deadline {
                return Err(anyhow!(SERVER_UNAVAILABLE));
            }
            pump(&mut mainloop)?;
        }
        let spec = Spec { format: Format::FLOAT32NE, channels: self.channels as u8, rate: CAPTURE_SAMPLE_RATE };
        let mut stream = Stream::new(&mut context, "Capture", &spec, None)
            .ok_or_else(|| anyhow!("Cannot create audio stream"))?;
        stream.connect_record(Some(&self.source), None, FlagSet::DONT_MOVE)
            .map_err(|e| anyhow!("Cannot open audio device: {}", e))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while stream.get_state() != State::Ready {
            if self.should_stop.load(Ordering::Acquire) { return Ok(()); }
            if matches!(stream.get_state(), State::Failed | State::Terminated) || Instant::now() >= deadline {
                return Err(anyhow!("Audio device did not become ready"));
            }
            pump(&mut mainloop)?;
        }
        ready();
        let mut samples = Vec::new();
        while !self.should_stop.load(Ordering::Acquire) {
            pump(&mut mainloop)?;
            if self.should_stop.load(Ordering::Acquire) { break; }
            if context.get_state() != ContextState::Ready { return Err(anyhow!(SERVER_UNAVAILABLE)); }
            if stream.get_state() != State::Ready { return Err(anyhow!("Audio capture stopped unexpectedly")); }
            loop {
                if self.should_stop.load(Ordering::Acquire) { break; }
                samples.clear();
                match stream.peek().map_err(|e| anyhow!("Cannot read audio: {}", e))? {
                    PeekResult::Empty => break,
                    PeekResult::Hole(bytes) => samples.resize(bytes / 4, 0.0),
                    PeekResult::Data(bytes) => samples.extend(bytes.chunks_exact(4)
                        .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]]))),
                }
                stream.discard().map_err(|e| anyhow!("Cannot release audio buffer: {}", e))?;
                if !self.should_stop.load(Ordering::Acquire) { on_samples(&samples); }
            }
        }
        let _ = stream.disconnect();
        context.disconnect();
        Ok(())
    }
}

fn pump(mainloop: &mut Mainloop) -> Result<()> {
    if matches!(mainloop.iterate(false), IterateResult::Err(_) | IterateResult::Quit(_)) {
        return Err(anyhow!("Audio server mainloop stopped"));
    }
    std::thread::sleep(Duration::from_millis(10));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_capture_exits_without_opening_server_or_delivering_samples() {
        let capture = PulseCapture::new_microphone("missing").unwrap();
        capture.stop_handle().store(true, Ordering::Release);
        capture.run(|_| panic!("cancelled capture delivered samples")).unwrap();
    }

    #[test]
    fn formats_remain_mono_mic_and_stereo_system_at_48khz() {
        let mic = PulseCapture::new_microphone("source").unwrap();
        let system = PulseCapture::new_system("monitor").unwrap();
        assert_eq!((mic.channels(), mic.sample_rate()), (1, 48000));
        assert_eq!((system.channels(), system.sample_rate()), (2, 48000));
    }

    #[test]
    #[ignore] // Requires a running PulseAudio/PipeWire server; run manually.
    fn test_list_sinks() {
        let sinks = list_sinks().expect("Failed to list sinks");
        for sink in &sinks {
            println!("sink: {} -> monitor {}", sink.description, sink.monitor_source_name);
        }
        assert!(!sinks.is_empty(), "Expected at least one sink on a machine with audio output");
    }

    #[test]
    #[ignore] // Requires a running PulseAudio/PipeWire server; run manually.
    fn test_list_sources() {
        let sources = list_sources().expect("Failed to list sources");
        for source in &sources {
            println!("source: {} -> {}", source.description, source.source_name);
        }
        assert!(!sources.is_empty(), "Expected at least one real input source on a machine with audio input");
    }

    #[test]
    #[ignore] // Requires real audio playback during the test; run manually.
    fn test_capture_reads_nonzero_samples() {
        let sinks = list_sinks().expect("Failed to list sinks");
        let sink = sinks
            .into_iter()
            .find(|s| s.monitor_source_name.to_lowercase().contains("bluez")
                || s.monitor_source_name.to_lowercase().contains("running"))
            .or_else(|| list_sinks().unwrap().into_iter().next())
            .expect("No sink available");

        println!("Capturing from: {} ({})", sink.description, sink.monitor_source_name);
        let capture = PulseCapture::new_system(&sink.monitor_source_name).expect("Failed to open capture");
        let stop = capture.stop_handle();

        let received = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let received_clone = received.clone();
        let stop_clone = stop.clone();
        let handle = std::thread::spawn(move || {
            capture.run(|samples| {
                received_clone.fetch_add(samples.len(), std::sync::atomic::Ordering::Relaxed);
                if received_clone.load(std::sync::atomic::Ordering::Relaxed) > 48000 * 2 {
                    stop_clone.store(true, std::sync::atomic::Ordering::Release);
                }
            });
        });

        std::thread::sleep(std::time::Duration::from_secs(3));
        stop.store(true, std::sync::atomic::Ordering::Release);
        handle.join().unwrap();

        let total = received.load(std::sync::atomic::Ordering::Relaxed);
        println!("Received {} samples", total);
        assert!(total > 0, "Expected to receive some samples from the monitor source");
    }

    fn sink(name: &str, description: &str) -> PulseSink {
        PulseSink {
            name: name.to_string(),
            description: description.to_string(),
            monitor_source_name: format!("{}.monitor", name),
        }
    }

    fn source(name: &str, description: &str) -> PulseSource {
        PulseSource {
            description: description.to_string(),
            source_name: name.to_string(),
        }
    }

    #[test]
    fn pick_sink_matches_exact_name_among_same_description() {
        let sinks = vec![sink("sink_a", "Speakers"), sink("sink_b", "Speakers")];
        let picked = pick_sink(sinks, "sink_b").expect("sink_b exists");
        assert_eq!(picked.monitor_source_name, "sink_b.monitor");
    }

    #[test]
    fn pick_sink_missing_is_reselect_error() {
        let err = pick_sink(vec![sink("sink_a", "Speakers")], "gone").unwrap_err();
        assert!(err.to_string().contains("select an available device"));
        assert!(err.to_string().contains("gone"));
    }

    #[test]
    fn pick_sink_does_not_match_on_description() {
        assert!(pick_sink(vec![sink("sink_a", "Speakers")], "Speakers").is_err());
    }

    #[test]
    fn pick_source_matches_exact_name_among_same_description() {
        let sources = vec![source("src_a", "Mic"), source("src_b", "Mic")];
        assert_eq!(pick_source(sources, "src_b").unwrap().source_name, "src_b");
    }

    #[test]
    fn pick_source_missing_is_reselect_error() {
        let err = pick_source(vec![], "src_x").unwrap_err();
        assert!(err.to_string().contains("select an available device"));
    }

    #[test]
    #[ignore] // Needs two null sinks "meetily_test_a"/"meetily_test_b" with the same description, b being the default.
    fn test_exact_ids_with_duplicate_descriptions() {
        use crate::audio::devices::{configuration::DeviceType, default_output_device, default_input_device};

        let default = default_output_device().expect("default output");
        println!("default output id: {:?}", default);
        assert_eq!(default.name, "meetily_test_b", "default must be the server default sink, not the first listed");

        let host = cpal::default_host();
        let listed = crate::audio::devices::platform::configure_linux_audio(&host).expect("list");
        for d in listed.iter().filter(|d| d.device_type == DeviceType::Output) {
            println!("listed output: id={} label={:?} default={}", d.name, d.label, d.is_default);
        }
        let a = listed.iter().find(|d| d.name == "meetily_test_a").expect("a listed");
        let b = listed.iter().find(|d| d.name == "meetily_test_b").expect("b listed");
        assert_ne!(a.label, b.label, "equal descriptions must get distinct labels");
        assert!(b.is_default && !a.is_default);

        let sink_a = find_sink_by_name("meetily_test_a").unwrap();
        let sink_b = find_sink_by_name("meetily_test_b").unwrap();
        assert_eq!(sink_a.monitor_source_name, "meetily_test_a.monitor");
        assert_eq!(sink_b.monitor_source_name, "meetily_test_b.monitor");
        assert!(find_sink_by_name("meetily_test_missing").is_err());

        let mic = default_input_device().expect("default input");
        println!("default input id: {:?}", mic);
        assert!(find_source_by_name(&mic.name).is_ok());
    }
}
