//! A recording owns this supervisor and its tap. Reconnection never mutates a
//! global stream or looks up a new session, so stopped sessions cannot reattach.
use super::{
    application_audio::{self, ResolvedApplication},
    capture::{CoreAudioCapture, CoreAudioStream},
    devices::AudioDevice,
    pipeline::AudioCapture,
    recording_state::{DeviceType, RecordingState},
    source::{ApplicationIdentity, SourceState},
};
use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use std::{sync::Arc, time::Duration};

struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn open(resolved: &ResolvedApplication) -> Result<Option<CoreAudioStream>> {
    if !resolved.running || resolved.processes.is_empty() {
        return Ok(None);
    }
    let ids: Vec<_> = resolved.processes.iter().map(|(id, _)| *id).collect();
    CoreAudioCapture::for_processes(Some(&ids))?
        .stream()
        .map(Some)
}

fn processor(
    stream: &CoreAudioStream,
    device: &Arc<AudioDevice>,
    state: &Arc<RecordingState>,
) -> AudioCapture {
    AudioCapture::new(
        device.clone(),
        state.clone(),
        stream.sample_rate(),
        1,
        DeviceType::System,
        None,
    )
}

pub async fn start(
    application: ApplicationIdentity,
    device: Arc<AudioDevice>,
    state: Arc<RecordingState>,
) -> Result<tokio::task::JoinHandle<()>> {
    let identity = application.clone();
    let initial =
        tokio::task::spawn_blocking(move || application_audio::resolve(&identity)).await??;
    anyhow::ensure!(
        initial.running,
        "Open {} before starting the recording.",
        application.name
    );
    // Open before returning success: a denied/failed tap cannot silently become mic-only.
    let mut stream = open(&initial).map_err(|e| {
        anyhow!(
            "Cannot capture {}: {e}. Check system audio recording permission in System Settings.",
            application.name
        )
    })?;
    let mut capture = stream.as_ref().map(|s| processor(s, &device, &state));
    if stream.is_none() {
        state.update_source_status(
            SourceState::Waiting,
            Some(format!("Waiting for {} to produce audio", application.name)),
        );
    }

    Ok(tokio::spawn(async move {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        let identity = application.clone();
        let poller = AbortOnDrop(tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let identity = identity.clone();
                let resolved =
                    tokio::task::spawn_blocking(move || application_audio::resolve(&identity))
                        .await
                        .map_err(|e| anyhow!(e))
                        .and_then(|r| r);
                if sender.send(resolved).await.is_err() {
                    break;
                }
            }
        }));
        let _poller = poller;
        let mut current = initial;
        let mut samples = Vec::with_capacity(1024);
        loop {
            if !state.is_recording() {
                break;
            }
            tokio::select! {
                result = receiver.recv() => {
                    let Some(result) = result else { break; };
                    match result {
                        Ok(next) => {
                            if next != current || stream.is_none() {
                                // Tear down old ownership BEFORE opening the new allowlist.
                                stream = None;
                                capture = None;
                                samples.clear();
                                current = next;
                                match open(&current) {
                                    Ok(Some(next_stream)) => {
                                        capture = Some(processor(&next_stream, &device, &state));
                                        stream = Some(next_stream);
                                        state.update_source_status(SourceState::Capturing, None);
                                    }
                                    Ok(None) => state.update_source_status(SourceState::Waiting,
                                        Some(format!("Waiting for {} — reconnecting automatically", application.name))),
                                    Err(e) => state.update_source_status(SourceState::Error,
                                        Some(format!("Cannot capture {}: {e}. Check system audio permission; retrying automatically.", application.name))),
                                }
                            }
                        }
                        Err(e) => {
                            // Ownership cannot be verified. Stop this tap until a fresh snapshot succeeds.
                            stream = None;
                            capture = None;
                            samples.clear();
                            state.update_source_status(SourceState::Error, Some(format!("Cannot verify {} audio: {e}. Retrying automatically.", application.name)));
                        }
                    }
                }
                sample = async {
                    match stream.as_mut() {
                        Some(stream) => stream.next().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match sample {
                        Some(sample) => {
                            if !state.is_active() { samples.clear(); continue; }
                            samples.push(sample);
                            if samples.len() >= 1024 {
                                if let Some(capture) = &capture { capture.process_audio_data(&samples); }
                                samples.clear();
                            }
                        }
                        None => {
                            stream = None;
                            capture = None;
                            samples.clear();
                            state.update_source_status(SourceState::Waiting,
                                Some(format!("Waiting for {} — audio stream ended", application.name)));
                        }
                    }
                }
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_audio_waits_without_creating_any_tap() {
        for running in [false, true] {
            assert!(open(&ResolvedApplication {
                running,
                processes: vec![],
                output_device: 0,
                output_sample_rate: 48000
            })
            .unwrap()
            .is_none());
        }
    }
    #[tokio::test]
    async fn dropping_supervisor_cancels_discovery_worker() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
        struct Notify(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for Notify {
            fn drop(&mut self) {
                let _ = self.0.take().unwrap().send(());
            }
        }
        let worker = AbortOnDrop(tokio::spawn(async move {
            let _notify = Notify(Some(dropped_tx));
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        }));
        started_rx.await.unwrap();
        drop(worker);
        tokio::time::timeout(Duration::from_secs(1), dropped_rx)
            .await
            .unwrap()
            .unwrap();
    }
}
