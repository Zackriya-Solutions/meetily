use std::sync::Arc;
use std::collections::VecDeque;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use anyhow::Result;
use log::{debug, error, info, warn};
use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};

use super::devices::AudioDevice;
use super::recording_state::{AudioChunk, AudioError, RecordingState, DeviceType};
use super::audio_processing::{audio_to_mono, LoudnessNormalizer, HighPassFilter};
use super::vad::{ContinuousVadProcessor};

/// How long a silence must last before the VAD closes a speech segment, and
/// therefore how long the audio clips handed to the ASR engine are.
///
/// Live-path policy: 500ms (matches the established Meetily Pro live policy).
/// The batch paths (`import.rs` / `retranscription.rs`) use 2000ms instead —
/// they have no latency requirement, so they optimize purely for ASR request
/// length. The live path cannot: with continuous audio (e.g. a podcast played
/// as system audio) a 2000ms redemption keeps one VAD segment open
/// indefinitely, withholds live transcript emission, and overruns the
/// accumulated-speech-buffer warning threshold. Bounded live segmentation
/// during continuous speech is tracked separately in #756.
const VAD_REDEMPTION_TIME_MS: u32 = 500;

#[cfg(all(test, target_os = "linux"))]
mod linux_mixing_tests {
    use super::*;

    #[test]
    fn unequal_delivery_boundaries_do_not_expand_the_timeline() {
        let mut ring = AudioMixerRingBuffer::new(48000);
        let mut positions = [0u64; 2];
        let mut output = [Vec::new(), Vec::new()];
        for _ in 0..6 {
            for (role, index, count) in [(DeviceType::Microphone, 0, 16384), (DeviceType::System, 1, 8192), (DeviceType::System, 1, 8192)] {
                let samples = (positions[index]..positions[index] + count).map(|n| n as f32).collect();
                ring.add_samples_at(role, samples, positions[index]);
                positions[index] += count;
                while let Some((mic, system)) = ring.extract_aligned(false) {
                    output[0].extend(mic); output[1].extend(system);
                }
            }
        }
        while let Some((mic, system)) = ring.extract_aligned(true) {
            output[0].extend(mic); output[1].extend(system);
        }
        let expected: Vec<_> = (0..98304).map(|n| n as f32).collect();
        assert_eq!(output[0], expected);
        assert_eq!(output[1], expected);
    }

    #[test]
    fn counterpart_within_lead_is_kept_and_beyond_lead_is_padded_once() {
        let mut ring = AudioMixerRingBuffer::new(48000);
        // Mic leads by 330 ms, the delivery spread measured between PipeWire sources.
        ring.add_samples_at(DeviceType::Microphone, vec![1.0; 28800 + 15840], 0);
        assert!(ring.extract_aligned(false).is_none());
        ring.add_samples_at(DeviceType::System, vec![3.0; 28800], 0);
        let (mic, system) = ring.extract_aligned(false).unwrap();
        assert_eq!(mic, vec![1.0; 28800]);
        assert_eq!(system, vec![3.0; 28800]);
        // A silent counterpart is padded only once the leader is a full second past the window.
        ring.add_samples_at(DeviceType::Microphone, vec![2.0; 105599 - 44640], 44640);
        assert!(ring.extract_aligned(false).is_none());
        ring.add_samples_at(DeviceType::Microphone, vec![2.0], 105599);
        let (mic, system) = ring.extract_aligned(false).unwrap();
        assert_eq!(&mic[..15840], &[1.0; 15840]);
        assert_eq!(system, vec![0.0; 28800]);
        // Its late arrival is not replayed into later windows.
        ring.add_samples_at(DeviceType::System, vec![3.0; 28800], 28800);
        while let Some((_, system)) = ring.extract_aligned(true) {
            assert!(system.iter().all(|&sample| sample == 0.0));
        }
    }

    #[test]
    fn earlier_counterpart_is_retained_before_the_first_window_is_emitted() {
        let mut ring = AudioMixerRingBuffer::new(48000);
        ring.add_samples_at(DeviceType::System, vec![2.0; 3], 2);
        ring.add_samples_at(DeviceType::Microphone, vec![1.0; 5], 0);
        let (mic, system) = ring.extract_aligned(true).unwrap();
        assert_eq!(mic, vec![1.0; 5]);
        assert_eq!(system, vec![0.0, 0.0, 2.0, 2.0, 2.0]);
    }

    #[test]
    fn single_role_flush_preserves_exact_partial_window_and_gaps() {
        let mut ring = AudioMixerRingBuffer::new(48000);
        ring.add_samples_at(DeviceType::System, vec![1.0; 3], 10);
        ring.add_samples_at(DeviceType::System, vec![2.0; 2], 15);
        let (mic, system) = ring.extract_aligned(true).unwrap();
        assert_eq!(mic, vec![0.0; 7]);
        assert_eq!(system, vec![1.0, 1.0, 1.0, 0.0, 0.0, 2.0, 2.0]);
    }

    #[test]
    fn recovery_gap_preserves_the_tail_and_every_recovered_sample() {
        for role in [DeviceType::Microphone, DeviceType::System] {
            for gap in [230401, 6 * 48000] {
                let mut ring = AudioMixerRingBuffer::new(48000);
                ring.add_samples_at(role.clone(), vec![1.0; 28811], 0);
                ring.extract_aligned(true).unwrap();
                ring.add_samples_at(role.clone(), vec![0.75; 960], 28811 + gap);
                let mut output = Vec::new();
                while let Some((mic, system)) = ring.extract_aligned(true) {
                    output.extend(if role == DeviceType::Microphone { mic } else { system });
                }
                let mut expected = vec![1.0; 11];
                expected.resize(11 + gap as usize, 0.0);
                expected.extend(vec![0.75; 960]);
                assert_eq!(output.len(), expected.len());
                assert_eq!(output, expected);
                assert_eq!(ring.take_dropped_samples(), (0, 0));
            }
        }
    }

    #[test]
    fn overlapping_packets_keep_first_arrival_and_partial_tails() {
        let mut ring = AudioMixerRingBuffer::new(48000);
        ring.window_size_samples = 4;
        ring.add_samples_at(DeviceType::Microphone, vec![1.0, 2.0, 3.0, 4.0, 5.0], 10);
        ring.add_samples_at(DeviceType::Microphone, vec![9.0, 9.0, 6.0, 7.0], 13);
        let (first, _) = ring.extract_aligned(true).unwrap();
        let (tail, _) = ring.extract_aligned(true).unwrap();
        assert_eq!(first, vec![1.0, 2.0, 3.0, 4.0]);
        assert_eq!(tail, vec![5.0, 6.0, 7.0]);
        assert!(ring.extract_aligned(true).is_none());
    }

    #[test]
    fn capacity_counts_real_samples_and_drops_only_the_newest_overflow() {
        let mut ring = AudioMixerRingBuffer::new(48000);
        ring.window_size_samples = 4;
        ring.max_buffer_size = 8;
        ring.add_samples_at(DeviceType::Microphone, vec![1.0; 3], 0);
        ring.add_samples_at(DeviceType::Microphone, vec![2.0; 6], 100);
        assert_eq!(ring.take_dropped_samples(), (1, 0));
        let mut output = Vec::new();
        while let Some((mic, _)) = ring.extract_aligned(true) {
            output.extend(mic);
        }
        assert_eq!(output.len(), 105);
        assert_eq!(&output[..3], &[1.0; 3]);
        assert!(output[3..100].iter().all(|&sample| sample == 0.0));
        assert_eq!(&output[100..], &[2.0; 5]);
    }
}

/// How many whole windows one stream may run ahead of the other before the
/// mixer stops waiting and zero-pads the missing stream. Below this, a short
/// stream is treated as late (jitter, a delayed WASAPI/Core Audio callback)
/// and waited for; beyond it, as stalled (unplugged device, idle WASAPI
/// loopback that delivers no packets). Must stay below the 8-window
/// `max_buffer_size`, or the leading stream would drop samples first.
#[cfg(not(target_os = "linux"))]
const MAX_MIXER_LAG_WINDOWS: usize = 2;

/// A capture's first chunk (no previous callback to measure a gap from) is
/// placed against the other stream's timeline by timestamp. Callback timing
/// makes that estimate wobble by a few ms, so offsets below this are left
/// alone rather than padded or trimmed.
#[cfg(not(target_os = "linux"))]
const FIRST_CHUNK_TOLERANCE_MS: f64 = 20.0;

/// Where one stream stands on the shared mix timeline, in samples.
#[cfg(not(target_os = "linux"))]
#[derive(Default)]
struct StreamTimeline {
    /// Samples placed so far: real audio plus silence the mixer padded in.
    placed: u64,
    /// Timeline position right after the last captured chunk, and that
    /// chunk's capture time (`AudioChunk::timestamp`).
    last_capture: Option<(u64, f64)>,
}

/// Ring buffer for synchronized audio mixing
/// Accumulates samples from mic and system streams until we have aligned windows
struct AudioMixerRingBuffer {
    #[cfg(not(target_os = "linux"))]
    mic_buffer: VecDeque<f32>,
    #[cfg(not(target_os = "linux"))]
    system_buffer: VecDeque<f32>,
    #[cfg(target_os = "linux")]
    mic_buffer: VecDeque<PositionedAudio>,
    #[cfg(target_os = "linux")]
    system_buffer: VecDeque<PositionedAudio>,
    window_size_samples: usize,  // Fixed mixing window (600ms)
    max_buffer_size: usize,  // Safety limit (8 windows)
    /// Samples discarded on overflow since the last pipeline log summary.
    mic_dropped_samples: u64,
    system_dropped_samples: u64,
    #[cfg(target_os = "linux")]
    position: Option<u64>,
    #[cfg(target_os = "linux")]
    emitted: bool,
    /// Counts `add_samples` calls so the buffer-status diagnostic can be
    /// rate-limited.
    #[cfg(not(target_os = "linux"))]
    add_calls: u64,
    /// The stream that fell more than `MAX_MIXER_LAG_WINDOWS` behind. While
    /// set, windows are cut as soon as the other stream has one, padding this
    /// stream; cleared as soon as it delivers samples again.
    #[cfg(not(target_os = "linux"))]
    stalled: Option<DeviceType>,
    #[cfg(not(target_os = "linux"))]
    sample_rate: u32,
    #[cfg(not(target_os = "linux"))]
    mic_timeline: StreamTimeline,
    #[cfg(not(target_os = "linux"))]
    system_timeline: StreamTimeline,
}

/// How far one role may run ahead before the other is padded with silence (1 s at 48 kHz).
#[cfg(target_os = "linux")]
const LINUX_MAX_STREAM_LEAD_SAMPLES: u64 = 48_000;

#[cfg(target_os = "linux")]
struct PositionedAudio {
    start: u64,
    samples: VecDeque<f32>,
}

impl AudioMixerRingBuffer {
    fn new(sample_rate: u32) -> Self {
        // Use 50ms windows for mixing
        let window_ms = 600.0;
        let window_size_samples = (sample_rate as f32 * window_ms / 1000.0) as usize;

        // CRITICAL FIX: Increase max buffer to 400ms for system audio stability
        // System audio (especially Core Audio on macOS) can have significant jitter
        // due to sample-by-sample streaming → batching → channel transmission
        // Accounts for: Core Audio jitter + processing delays
        let max_buffer_size = window_size_samples * 8;  // 400ms (was 200ms)

        info!("🔊 Ring buffer initialized: window={}ms ({} samples), max={}ms ({} samples)",
              window_ms, window_size_samples,
              window_ms * 8.0, max_buffer_size);

        Self {
            #[cfg(not(target_os = "linux"))]
            mic_buffer: VecDeque::with_capacity(max_buffer_size),
            #[cfg(not(target_os = "linux"))]
            system_buffer: VecDeque::with_capacity(max_buffer_size),
            #[cfg(target_os = "linux")]
            mic_buffer: VecDeque::new(),
            #[cfg(target_os = "linux")]
            system_buffer: VecDeque::new(),
            window_size_samples,
            max_buffer_size,
            mic_dropped_samples: 0,
            system_dropped_samples: 0,
            #[cfg(target_os = "linux")]
            position: None,
            #[cfg(target_os = "linux")]
            emitted: false,
            #[cfg(not(target_os = "linux"))]
            add_calls: 0,
            #[cfg(not(target_os = "linux"))]
            stalled: None,
            #[cfg(not(target_os = "linux"))]
            sample_rate,
            #[cfg(not(target_os = "linux"))]
            mic_timeline: StreamTimeline::default(),
            #[cfg(not(target_os = "linux"))]
            system_timeline: StreamTimeline::default(),
        }
    }

    /// Add a raw capture chunk at its place on the timeline.
    ///
    /// Buffers pair mic and system audio by position, so a stream that skipped
    /// time (WASAPI loopback sends nothing while no app plays sound) must have
    /// that time filled with silence, or everything after it pairs with the
    /// wrong moment of the other stream. `gap` (`AudioChunk::capture_gap`) is
    /// the time the device skipped since its previous callback; when it is
    /// unknown (a capture's first callback: recording start, mic hot-swap)
    /// the chunk is placed against the other stream by `capture_end`.
    ///
    /// Silence the stall path already padded in counts toward the gap. If it
    /// overshot (the stream was very late rather than silent), the start of
    /// the chunk falls in time already mixed as silence and is dropped.
    #[cfg(not(target_os = "linux"))]
    fn add_captured(&mut self, device_type: DeviceType, mut samples: Vec<f32>, capture_end: f64, gap: Option<f64>) {
        let rate = self.sample_rate as f64;
        let len = samples.len() as i64;
        let (own, other, buffer) = match device_type {
            DeviceType::Microphone => (&self.mic_timeline, &self.system_timeline, &self.mic_buffer),
            DeviceType::System => (&self.system_timeline, &self.mic_timeline, &self.system_buffer),
        };

        let target_start = match gap {
            Some(gap) => own.last_capture.map(|(end, _)| end as i64 + (gap * rate).round() as i64),
            None => other
                .last_capture
                .map(|(end, at)| end as i64 + ((capture_end - at) * rate).round() as i64 - len),
        };
        let mut offset = target_start.map_or(0, |start| start - own.placed as i64);
        if gap.is_none() && (offset.abs() as f64) < FIRST_CHUNK_TOLERANCE_MS * rate / 1000.0 {
            offset = 0;
        }

        let mut pad = 0usize;
        if offset > 0 {
            let room = self.max_buffer_size.saturating_sub(buffer.len());
            pad = (offset as usize).min(room);
            if pad < offset as usize {
                warn!("🔊 Mixer: {:?} gap of {} samples exceeds buffer room, padding {}", device_type, offset, pad);
            }
            info!("🔊 Mixer: {:?} skipped {} ms, padding with silence", device_type, pad as u64 * 1000 / self.sample_rate as u64);
        } else if offset < 0 {
            let skip = (-offset as usize).min(samples.len());
            info!("🔊 Mixer: {:?} resumed inside {} ms already mixed as silence, dropping it", device_type, skip as u64 * 1000 / self.sample_rate as u64);
            samples.drain(..skip);
        }

        if pad > 0 {
            if self.stalled.as_ref() == Some(&device_type) {
                self.stalled = None;
            }
            let buffer = match device_type {
                DeviceType::Microphone => &mut self.mic_buffer,
                DeviceType::System => &mut self.system_buffer,
            };
            buffer.extend(std::iter::repeat(0.0).take(pad));
        }
        // add_samples counts the samples on the timeline; the padding is
        // counted here.
        self.add_samples(device_type.clone(), samples);
        let own = match device_type {
            DeviceType::Microphone => &mut self.mic_timeline,
            DeviceType::System => &mut self.system_timeline,
        };
        own.placed += pad as u64;
        own.last_capture = Some((own.placed, capture_end));
    }

    #[cfg(not(target_os = "linux"))]
    fn add_samples(&mut self, device_type: DeviceType, samples: Vec<f32>) {
        self.add_calls += 1;
        let should_report = self.add_calls % 200 == 0;

        // Log buffer health periodically for diagnostics
        if should_report {
            debug!("📊 Ring buffer status: mic={} samples, sys={} samples (max={})",
                   self.mic_buffer.len(), self.system_buffer.len(), self.max_buffer_size);
        }

        // A stalled stream that delivers again is back in lockstep: from here
        // on, windows wait for it.
        if !samples.is_empty() && self.stalled.as_ref() == Some(&device_type) {
            info!("🔊 Mixer: {:?} stream resumed, waiting for aligned windows again", device_type);
            self.stalled = None;
        }

        match device_type {
            DeviceType::Microphone => {
                self.mic_timeline.placed += samples.len() as u64;
                self.mic_buffer.extend(samples);
            }
            DeviceType::System => {
                self.system_timeline.placed += samples.len() as u64;
                self.system_buffer.extend(samples);
            }
        }

        // One stream running more than the lag bound ahead means the other is
        // not merely late. Mark it stalled so the backlog drains (padded)
        // instead of the leading stream overflowing.
        if self.stalled.is_none() {
            let lead_limit = (MAX_MIXER_LAG_WINDOWS + 1) * self.window_size_samples;
            let (mic, sys, w) = (self.mic_buffer.len(), self.system_buffer.len(), self.window_size_samples);
            if mic >= lead_limit && sys < w {
                warn!("🔊 Mixer: system stream {} windows behind, padding it until it resumes", MAX_MIXER_LAG_WINDOWS);
                self.stalled = Some(DeviceType::System);
            } else if sys >= lead_limit && mic < w {
                warn!("🔊 Mixer: microphone stream {} windows behind, padding it until it resumes", MAX_MIXER_LAG_WINDOWS);
                self.stalled = Some(DeviceType::Microphone);
            }
        }

        if self.mic_buffer.len() > self.max_buffer_size {
            self.mic_dropped_samples += (self.mic_buffer.len() - self.max_buffer_size) as u64;
        }
        if self.system_buffer.len() > self.max_buffer_size {
            self.system_dropped_samples += (self.system_buffer.len() - self.max_buffer_size) as u64;
        }
        while self.mic_buffer.len() > self.max_buffer_size {
            self.mic_buffer.pop_front();
        }
        while self.system_buffer.len() > self.max_buffer_size {
            self.system_buffer.pop_front();
        }
    }

    #[cfg(target_os = "linux")]
    fn add_samples_at(&mut self, role: DeviceType, samples: Vec<f32>, position: u64) {
        if samples.is_empty() { return; }
        let mut cursor = *self.position.get_or_insert(position);
        if !self.emitted && position < cursor {
            cursor = position;
            self.position = Some(position);
        }
        let (buffer, dropped) = match role {
            DeviceType::Microphone => (&mut self.mic_buffer, &mut self.mic_dropped_samples),
            DeviceType::System => (&mut self.system_buffer, &mut self.system_dropped_samples),
        };
        let end = buffer.back().map_or(cursor, |packet| packet.start + packet.samples.len() as u64).max(cursor);
        let skip = end.saturating_sub(position).min(samples.len() as u64) as usize;
        if skip == samples.len() { return; }
        // Gaps consume timeline, not storage that recovered speech needs.
        let buffered: usize = buffer.iter().map(|packet| packet.samples.len()).sum();
        let accepted = (samples.len() - skip).min(self.max_buffer_size - buffered);
        *dropped += (samples.len() - skip - accepted) as u64;
        if accepted > 0 {
            buffer.push_back(PositionedAudio {
                start: position + skip as u64,
                samples: samples.into_iter().skip(skip).take(accepted).collect(),
            });
        }
    }

    #[cfg(target_os = "linux")]
    fn extract_aligned(&mut self, flush: bool) -> Option<(Vec<f32>, Vec<f32>)> {
        let cursor = self.position?;
        let ends = [&self.mic_buffer, &self.system_buffer].map(|buffer|
            buffer.back().map_or(cursor, |packet| packet.start + packet.samples.len() as u64));
        let available = ends[0].max(ends[1]) - cursor;
        if available == 0 || (!flush && available < self.window_size_samples as u64) { return None; }
        let count = available.min(self.window_size_samples as u64) as usize;
        let end = cursor + count as u64;
        // Pad a lagging role only once the other is a full second past this window. Capture
        // sources differ in delivery latency (150-330 ms measured on PipeWire), and padding sooner
        // zero-filled the window and then discarded the real samples when they arrived.
        if !flush && ends[0].min(ends[1]) < end && ends[0].max(ends[1]) < end + LINUX_MAX_STREAM_LEAD_SAMPLES {
            return None;
        }
        let take = |buffer: &mut VecDeque<PositionedAudio>| {
            let mut samples = vec![0.0; count];
            while let Some(packet) = buffer.front_mut() {
                if packet.start >= end { break; }
                let offset = (packet.start - cursor) as usize;
                let take = packet.samples.len().min(count - offset);
                for (output, input) in samples[offset..offset + take].iter_mut().zip(packet.samples.drain(..take)) {
                    *output = input;
                }
                packet.start += take as u64;
                if packet.samples.is_empty() { buffer.pop_front(); }
                else { break; }
            }
            samples
        };
        let result = (take(&mut self.mic_buffer), take(&mut self.system_buffer));
        self.emitted = true;
        *self.position.as_mut().unwrap() += count as u64;
        Some(result)
    }

    fn take_dropped_samples(&mut self) -> (u64, u64) {
        let dropped = (self.mic_dropped_samples, self.system_dropped_samples);
        self.mic_dropped_samples = 0;
        self.system_dropped_samples = 0;
        dropped
    }

    /// A window is ready when both streams have one, or when the stream that
    /// is not stalled has one. A stream that is merely late is waited for —
    /// cutting the window early would zero-pad audio that is about to arrive
    /// and shift everything after it out of alignment.
    #[cfg(not(target_os = "linux"))]
    fn can_mix(&self) -> bool {
        let w = self.window_size_samples;
        match self.stalled {
            None => self.mic_buffer.len() >= w && self.system_buffer.len() >= w,
            Some(DeviceType::System) => self.mic_buffer.len() >= w,
            Some(DeviceType::Microphone) => self.system_buffer.len() >= w,
        }
    }

    /// Fill `mic_out` and `sys_out` with the next aligned window.
    ///
    /// Writes into caller-owned buffers that the pipeline reuses across windows.
    /// The previous version returned two freshly-allocated `Vec`s, which — with
    /// the mixer's own output buffer — meant three allocations and three full
    /// copies for every 600 ms of audio.
    ///
    /// Both outputs are always exactly `window_size_samples` long. Only a
    /// stalled stream is ever short, and it is zero-padded: silence is
    /// preferred over last-sample-hold to prevent repetition artifacts.
    #[cfg(not(target_os = "linux"))]
    fn extract_window_into(&mut self, mic_out: &mut Vec<f32>, sys_out: &mut Vec<f32>) -> bool {
        if !self.can_mix() {
            return false;
        }

        // A stalled stream is padded; that silence holds its place on the
        // timeline until the stream's next chunk says how long it was gone.
        self.mic_timeline.placed += drain_window(&mut self.mic_buffer, self.window_size_samples, mic_out) as u64;
        self.system_timeline.placed += drain_window(&mut self.system_buffer, self.window_size_samples, sys_out) as u64;
        true
    }

    /// Drain whatever remains in both buffers into one final window, as long as
    /// the longer stream (up to the lag bound, so possibly longer than
    /// `window_size_samples`); the shorter one is zero-padded. Returns false
    /// when both are empty.
    #[cfg(not(target_os = "linux"))]
    fn extract_partial_window_into(&mut self, mic_out: &mut Vec<f32>, sys_out: &mut Vec<f32>) -> bool {
        let len = self.mic_buffer.len().max(self.system_buffer.len());
        if len == 0 {
            return false;
        }

        self.mic_timeline.placed += drain_window(&mut self.mic_buffer, len, mic_out) as u64;
        self.system_timeline.placed += drain_window(&mut self.system_buffer, len, sys_out) as u64;
        true
    }
}

/// Move up to `window` samples out of `src` into `dst`, zero-padding the
/// tail. Returns how many padding samples were added.
#[cfg(not(target_os = "linux"))]
fn drain_window(src: &mut VecDeque<f32>, window: usize, dst: &mut Vec<f32>) -> usize {
    dst.clear();
    dst.reserve(window);

    let take = src.len().min(window);
    dst.extend(src.drain(0..take));
    dst.resize(window, 0.0);
    window - take
}

/// Simple audio mixer without aggressive ducking
/// Combines mic + system audio with basic clipping prevention
struct ProfessionalAudioMixer;

impl ProfessionalAudioMixer {
    fn new(_sample_rate: u32) -> Self {
        Self
    }

    /// Mix into a caller-owned buffer the pipeline reuses across windows.
    ///
    /// `extract_window_into` guarantees both inputs are the same length, so this
    /// zips the slices instead of doing two bounds-checked `get(i)` lookups per
    /// sample (96,000 of them a second).
    fn mix_window_into(&mut self, mic_window: &[f32], sys_window: &[f32], out: &mut Vec<f32>) {
        debug_assert_eq!(mic_window.len(), sys_window.len());

        out.clear();
        out.reserve(mic_window.len());

        // Sum without ducking — mic is already normalized to -23 LUFS by the
        // capture chain, system audio stays at its natural level.
        for (&mic, &sys) in mic_window.iter().zip(sys_window.iter()) {
            let sum = mic + sys;

            // Soft scaling prevents distortion artifacts: if the sum would
            // exceed ±1.0, scale down PROPORTIONALLY rather than hard clipping,
            // which sounds like "radio breaks".
            let sum_abs = sum.abs();
            out.push(if sum_abs > 1.0 { sum / sum_abs } else { sum });
        }
    }
}

/// Per-callback state for the microphone enhancement chain.
///
/// These were three separate `Arc<Mutex<Option<_>>>` fields, which cost three
/// lock acquisitions on the realtime audio thread for work that is inherently
/// sequential. One mutex covers the whole chain, and `scratch` lets the downmix
/// reuse a buffer instead of allocating one per callback.
struct MicChain {
    high_pass_filter: Option<HighPassFilter>,
    normalizer: Option<LoudnessNormalizer>,
}

/// Simplified audio capture without broadcast channels
/// Device-clock timing of one capture callback, from cpal.
#[derive(Clone, Copy)]
pub struct CaptureTiming {
    /// When the callback's first frame was captured.
    pub capture: cpal::StreamInstant,
    /// When the callback ran.
    pub callback: cpal::StreamInstant,
}

impl CaptureTiming {
    pub fn from_cpal(info: &cpal::InputCallbackInfo) -> Self {
        let ts = info.timestamp();
        Self { capture: ts.capture, callback: ts.callback }
    }
}

/// Gaps shorter than this are clock rounding, not skipped audio.
const MIN_CAPTURE_GAP_SECS: f64 = 0.002;

/// Measures, across one stream's callbacks, how much time the device skipped
/// (see `AudioChunk::capture_gap`). Gaps from callbacks that sent no chunk
/// (the resampler was still filling) carry over to the next chunk sent.
#[derive(Default)]
struct CaptureContinuity {
    /// cpal instants have no public absolute value, so times are kept as
    /// seconds since the stream's first timed callback.
    anchor: Option<cpal::StreamInstant>,
    /// Capture time just past the previous callback's last frame.
    prev_end: Option<f64>,
    /// Gap not yet attached to a chunk; `None` until the first chunk is sent,
    /// since the first callback has nothing to measure against.
    pending_gap: Option<f64>,
    started: bool,
}

impl CaptureContinuity {
    /// Record one callback of `frames` frames. Returns how long ago (seconds)
    /// its last frame was captured: 0 without device timing.
    fn observe(&mut self, timing: Option<CaptureTiming>, frames: usize, sample_rate: u32) -> f64 {
        let secs = timing.map(|t| {
            let anchor = *self.anchor.get_or_insert(t.capture);
            let since = |i: &cpal::StreamInstant| i.duration_since(&anchor).map_or(0.0, |d| d.as_secs_f64());
            (since(&t.capture), since(&t.callback))
        });
        self.observe_secs(secs, frames as f64 / sample_rate as f64)
    }

    /// `observe` on plain seconds: `(capture, callback)` of the first frame.
    fn observe_secs(&mut self, timing: Option<(f64, f64)>, duration: f64) -> f64 {
        let Some((capture, callback)) = timing else {
            // No device clock: assume the stream is continuous.
            self.prev_end = None;
            return 0.0;
        };
        if let (Some(prev_end), Some(pending)) = (self.prev_end, self.pending_gap.as_mut()) {
            let gap = capture - prev_end;
            if gap >= MIN_CAPTURE_GAP_SECS {
                *pending += gap;
            }
        }
        self.prev_end = Some(capture + duration);
        (callback - capture - duration).max(0.0)
    }

    /// The gap to attach to the chunk being sent; resets the running total.
    fn take_gap(&mut self) -> Option<f64> {
        if !self.started {
            self.started = true;
            self.pending_gap = Some(0.0);
            return None;
        }
        self.pending_gap.replace(0.0)
    }
}

// CRITICAL FIX: Persistent resampler to preserve energy across chunks.
// Creating a new resampler per chunk causes energy amplification and incorrect output sizes.
// Fixed chunk size of 512 samples, with buffering for variable-size input.
const RESAMPLER_CHUNK_SIZE: usize = 512;

/// Resampler from `sample_rate` to the pipeline's 48 kHz, or `None` if it
/// cannot be built (callers then fall back to per-chunk resampling).
fn build_resampler(device_name: &str, sample_rate: u32) -> Option<SincFixedIn<f32>> {
    const TARGET_SAMPLE_RATE: u32 = 48000;
    let ratio = TARGET_SAMPLE_RATE as f64 / sample_rate as f64;

    // Adaptive parameters based on sample rate ratio (same logic as resample_audio)
    let (sinc_len, interpolation_type, oversampling) = if ratio >= 2.0 {
        (512, SincInterpolationType::Cubic, 512)
    } else if ratio >= 1.5 {
        (384, SincInterpolationType::Cubic, 384)
    } else if ratio > 1.0 {
        (256, SincInterpolationType::Linear, 256)
    } else if ratio <= 0.5 {
        (512, SincInterpolationType::Cubic, 512)
    } else {
        (384, SincInterpolationType::Linear, 384)
    };

    let params = SincInterpolationParameters {
        sinc_len,
        f_cutoff: 0.95,
        interpolation: interpolation_type,
        oversampling_factor: oversampling,
        window: WindowFunction::BlackmanHarris2,
    };

    match SincFixedIn::<f32>::new(
        ratio,
        2.0,  // Maximum relative deviation
        params,
        RESAMPLER_CHUNK_SIZE,
        1,    // Mono
    ) {
        Ok(resampler) => {
            info!("✅ Persistent resampler initialized for '{}' ({}Hz → {}Hz, chunk_size={})",
                  device_name, sample_rate, TARGET_SAMPLE_RATE, RESAMPLER_CHUNK_SIZE);
            info!("   Buffering enabled for variable-size chunks (e.g., 320, 512, 1024, etc.)");
            Some(resampler)
        }
        Err(e) => {
            warn!("⚠️ Failed to create persistent resampler: {}, will use fallback", e);
            None
        }
    }
}

#[derive(Clone)]
pub struct AudioCapture {
    device: Arc<AudioDevice>,
    state: Arc<RecordingState>,
    sample_rate: u32,        // Original device sample rate
    channels: u16,
    chunk_counter: Arc<std::sync::atomic::AtomicU64>,
    device_type: DeviceType,
    needs_resampling: bool,  // Flag if resampling is required
    // CRITICAL FIX: Persistent resampler to preserve energy across chunks
    resampler: Arc<std::sync::Mutex<Option<SincFixedIn<f32>>>>,
    // Buffering for variable-size chunks → fixed-size resampler input
    resampler_input_buffer: Arc<std::sync::Mutex<Vec<f32>>>,
    resampler_chunk_size: usize,  // Fixed chunk size for resampler (512 samples)
    /// Microphone-only enhancement chain; `None` for system audio.
    mic_chain: Option<Arc<std::sync::Mutex<MicChain>>>,
    /// Touched only from this stream's callback thread, so never contended.
    continuity: Arc<std::sync::Mutex<CaptureContinuity>>,
    /// Linux sample clock for sources without capture positions (ALSA via
    /// cpal): origin on the active-recording clock, samples sent since.
    #[cfg(target_os = "linux")]
    fallback_clock: Arc<std::sync::Mutex<(Option<f64>, u64)>>,
}

impl AudioCapture {
    pub fn new(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        sample_rate: u32,
        channels: u16,
        device_type: DeviceType,
        recording_sender: Option<mpsc::UnboundedSender<AudioChunk>>,
    ) -> Self {
        // CRITICAL FIX: Detect if resampling is needed
        // Pipeline expects 48kHz, but Bluetooth devices often report 8kHz, 16kHz, or 44.1kHz
        const TARGET_SAMPLE_RATE: u32 = 48000;
        let needs_resampling = sample_rate != TARGET_SAMPLE_RATE;

        // Detect device kind (Bluetooth vs Wired) for adaptive processing
        // Use reasonable defaults for buffer size (512 samples is typical)
        let device_kind = super::device_detection::InputDeviceKind::detect(&device.name, 512, sample_rate);

        if needs_resampling {
            warn!(
                "⚠️ SAMPLE RATE MISMATCH DETECTED ⚠️"
            );
            warn!(
                "🔄 [{:?}] Audio device '{}' ({:?}) reports {} Hz (pipeline expects {} Hz)",
                device_type, device.name, device_kind, sample_rate, TARGET_SAMPLE_RATE
            );
            warn!(
                "🔄 Automatic resampling will be applied: {} Hz → {} Hz",
                sample_rate, TARGET_SAMPLE_RATE
            );

            // Log which resampling strategy will be used
            let ratio = TARGET_SAMPLE_RATE as f64 / sample_rate as f64;
            let strategy = if ratio >= 2.0 {
                "High-quality upsampling (sinc_len=512, Cubic interpolation)"
            } else if ratio >= 1.5 {
                "Moderate upsampling (sinc_len=384, Cubic)"
            } else if ratio > 1.0 {
                "Small upsampling (sinc_len=256, Linear)"
            } else if ratio <= 0.5 {
                "Anti-aliased downsampling (sinc_len=512, Cubic)"
            } else {
                "Moderate downsampling (sinc_len=384, Linear)"
            };
            info!("   Resampling strategy: {}", strategy);
        } else {
            info!(
                "✅ [{:?}] Audio device '{}' ({:?}) uses {} Hz (matches pipeline)",
                device_type, device.name, device_kind, sample_rate
            );
        }

        // Initialize audio enhancement processors for MICROPHONE ONLY
        // System audio doesn't need enhancement (already clean)
        let (high_pass_filter, normalizer) = if matches!(device_type, DeviceType::Microphone) {
            // Initialize high-pass filter (removes rumble below 80 Hz)
            let hpf = {
                let filter = HighPassFilter::new(TARGET_SAMPLE_RATE, 80.0);
                info!("✅ High-pass filter initialized for microphone '{}' (cutoff: 80 Hz)", device.name);
                Some(filter)
            };

            // Initialize EBU R128 normalizer (professional loudness standard)
            let norm = match LoudnessNormalizer::new(1, TARGET_SAMPLE_RATE) {
                Ok(normalizer) => {
                    info!("✅ EBU R128 normalizer initialized for microphone '{}' (target: -23 LUFS)", device.name);
                    Some(normalizer)
                }
                Err(e) => {
                    warn!("⚠️ Failed to create normalizer for microphone: {}, normalization disabled", e);
                    None
                }
            };

            (hpf, norm)
        } else {
            // System audio: no enhancement needed
            info!("ℹ️ System audio '{}' captured raw (no enhancement)", device.name);
            (None, None)
        };

        let resampler = if needs_resampling {
            build_resampler(&device.name, sample_rate)
        } else {
            None
        };

        // Raw capture is never sent straight to the recording saver — only the
        // mixed output from AudioPipeline is (see the note in
        // process_audio_data). The parameter is kept for call-site symmetry.
        let _ = recording_sender;

        let mic_chain = if matches!(device_type, DeviceType::Microphone) {
            Some(Arc::new(std::sync::Mutex::new(MicChain {
                high_pass_filter,
                normalizer,
            })))
        } else {
            None
        };

        Self {
            #[cfg(target_os = "linux")]
            fallback_clock: Arc::new(std::sync::Mutex::new((None, 0))),
            device,
            state,
            sample_rate,
            channels,
            chunk_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            device_type,
            needs_resampling,
            resampler: Arc::new(std::sync::Mutex::new(resampler)),
            resampler_input_buffer: Arc::new(std::sync::Mutex::new(Vec::with_capacity(RESAMPLER_CHUNK_SIZE * 2))),
            resampler_chunk_size: RESAMPLER_CHUNK_SIZE,
            mic_chain,
            continuity: Arc::new(std::sync::Mutex::new(CaptureContinuity::default())),
        }
    }

    /// Switch to a new input sample rate mid-stream, rebuilding the resampler.
    /// Samples still buffered at the old rate are discarded (under one
    /// resampler chunk). The new resampler state is private to this clone.
    pub fn set_input_sample_rate(&mut self, sample_rate: u32) {
        if sample_rate == self.sample_rate {
            return;
        }
        warn!(
            "🔄 [{:?}] '{}' sample rate changed {} Hz → {} Hz, retuning resampler",
            self.device_type, self.device.name, self.sample_rate, sample_rate
        );
        self.sample_rate = sample_rate;
        self.needs_resampling = sample_rate != 48000;
        let resampler = if self.needs_resampling {
            build_resampler(&self.device.name, sample_rate)
        } else {
            None
        };
        self.resampler = Arc::new(std::sync::Mutex::new(resampler));
        self.resampler_input_buffer =
            Arc::new(std::sync::Mutex::new(Vec::with_capacity(RESAMPLER_CHUNK_SIZE * 2)));
    }

    /// Process audio data from a source without device timestamps.
    pub fn process_audio_data(&self, data: &[f32]) {
        self.process_captured(data, None, None);
    }

    /// Process audio data directly from callback. `timing` lets the mixer
    /// fill time the device skipped (see `AudioChunk::capture_gap`).
    pub fn process_audio_data_at(&self, data: &[f32], timing: Option<CaptureTiming>) {
        self.process_captured(data, timing, None);
    }

    /// Linux PulseAudio/PipeWire capture: `timestamp` is the position of the
    /// first frame on the recording timeline, in seconds, as reported by the
    /// server. The Linux mixer places the chunk by it.
    #[cfg(target_os = "linux")]
    pub(super) fn process_audio_data_at_timestamp(&self, data: &[f32], timestamp: f64) {
        self.process_captured(data, None, Some(timestamp));
    }

    fn process_captured(&self, data: &[f32], timing: Option<CaptureTiming>, explicit_timestamp: Option<f64>) {
        // Check if still recording
        if !self.state.is_recording() || (cfg!(target_os = "linux") && self.state.is_paused()) {
            return;
        }

        let frames = data.len() / self.channels.max(1) as usize;
        let last_frame_age = self
            .continuity
            .lock()
            .map(|mut c| c.observe(timing, frames, self.sample_rate))
            .unwrap_or(0.0);

        // Convert to mono if needed. This buffer is eventually moved into the
        // AudioChunk, so it has to be owned — but the filter and normalizer
        // below now reuse it rather than each allocating their own copy.
        let mut mono_data = if self.channels > 1 {
            audio_to_mono(data, self.channels)
        } else {
            data.to_vec()
        };

        // CRITICAL FIX: Resample to 48kHz if device uses different sample rate
        // This fixes Bluetooth devices (like Sony WH-1000XM4) that report 16kHz or 44.1kHz
        // Without this, audio is sped up 3x and VAD fails
        //
        // IMPORTANT: Uses PERSISTENT resampler with BUFFERING to preserve energy across chunks
        // Creating a new resampler per chunk causes energy amplification (173.5% RMS)
        // Buffering handles variable chunk sizes (320, 512, 1024, etc.) by accumulating to fixed 512-sample chunks
        const TARGET_SAMPLE_RATE: u32 = 48000;
        if self.needs_resampling {
            // The counter only advances at the end of this callback, so this is
            // the same id the logging block below reads.
            let chunk_id = self.chunk_counter.load(std::sync::atomic::Ordering::SeqCst);
            // Release caps the log level at Info, so these `debug!`s never emit
            // there — and the RMS pass and buffer lock that feed them must not
            // run on the realtime thread either.
            let will_log_resampling = cfg!(debug_assertions) && chunk_id % 100 == 0;

            let before_len = mono_data.len();
            // Sum-of-squares over the whole buffer, on the realtime thread, for
            // a diagnostic printed once every 100 chunks — so only pay for it
            // on the chunks that actually log.
            let before_rms = if will_log_resampling && !mono_data.is_empty() {
                (mono_data.iter().map(|&x| x * x).sum::<f32>() / mono_data.len() as f32).sqrt()
            } else {
                0.0
            };

            // Use persistent resampler with buffering to handle variable chunk sizes
            let mut resampled_output = Vec::new();
            let mut used_persistent_resampler = false;

            if let Ok(mut buffer_lock) = self.resampler_input_buffer.lock() {
                // Add new samples to buffer
                buffer_lock.extend_from_slice(&mono_data);

                // Process complete chunks through the resampler
                if let Ok(mut resampler_lock) = self.resampler.lock() {
                    if let Some(ref mut resampler) = *resampler_lock {
                        used_persistent_resampler = true;

                        // Process as many complete chunks as we have
                        while buffer_lock.len() >= self.resampler_chunk_size {
                            // Extract exactly chunk_size samples
                            let chunk: Vec<f32> = buffer_lock.drain(0..self.resampler_chunk_size).collect();

                            // Rubato expects input as Vec<Vec<f32>> (one Vec per channel)
                            let waves_in = vec![chunk];

                            match resampler.process(&waves_in, None) {
                                Ok(mut waves_out) => {
                                    if let Some(output) = waves_out.pop() {
                                        resampled_output.extend_from_slice(&output);
                                    }
                                }
                                Err(e) => {
                                    warn!("⚠️ Persistent resampler processing failed: {}", e);
                                    used_persistent_resampler = false;
                                    break;
                                }
                            }
                        }
                        // Remaining samples in buffer will be processed in next iteration
                    }
                }
            }

            // CRITICAL: Only update mono_data if we got output from persistent resampler
            // If buffer is accumulating (< 512 samples), skip this chunk - data is safely buffered
            // and will be processed in next iteration with proper resampling
            let has_resampled_output = !resampled_output.is_empty();

            if has_resampled_output {
                mono_data = resampled_output;
            } else if !used_persistent_resampler {
                // Only fallback if persistent resampler is not available at all
                mono_data = super::audio_processing::resample_audio(
                    &mono_data,
                    self.sample_rate,
                    TARGET_SAMPLE_RATE,
                );
            } else {
                // Buffering: samples are accumulating in buffer, waiting for 512-sample chunk
                // Don't send partial/unprocessed data - return early
                // Audio is NOT lost - it's in the buffer and will be processed next iteration
                return;
            }

            // Log resampling only occasionally to avoid spam
            if will_log_resampling && has_resampled_output {
                let after_len = mono_data.len();
                let after_rms = if !mono_data.is_empty() {
                    (mono_data.iter().map(|&x| x * x).sum::<f32>() / mono_data.len() as f32).sqrt()
                } else {
                    0.0
                };
                let ratio = TARGET_SAMPLE_RATE as f64 / self.sample_rate as f64;
                let rms_preservation = if before_rms > 0.0 { (after_rms / before_rms) * 100.0 } else { 100.0 };

                let buffer_size = if let Ok(buf) = self.resampler_input_buffer.lock() {
                    buf.len()
                } else {
                    0
                };

                debug!(
                    "🔄 [{:?}] Persistent buffered resampler: {}Hz → {}Hz (ratio: {:.2}x)",
                    self.device_type,
                    self.sample_rate,
                    TARGET_SAMPLE_RATE,
                    ratio
                );
                debug!(
                    "   Chunk {}: {} → {} samples, RMS preservation: {:.1}%, buffer: {}",
                    chunk_id,
                    before_len,
                    after_len,
                    rms_preservation,
                    buffer_size
                );
            }
        }

        // AUDIO ENHANCEMENT PIPELINE (Microphone Only)
        // Processing order is critical: high-pass → normalization
        // This ensures noise is removed before being amplified by the normalizer
        //
        // All three stages live behind one mutex now: they always run together
        // on the same buffer, so three separate lock/unlock pairs per callback
        // bought nothing. The filter and normalizer also work in place, leaving
        // the mono downmix above as the only allocation on this path.
        if let Some(chain) = &self.mic_chain {
            if let Ok(mut chain) = chain.lock() {
                // STEP 1: Apply high-pass filter to remove low-frequency rumble (< 80 Hz)
                if let Some(ref mut filter) = chain.high_pass_filter {
                    filter.process_in_place(&mut mono_data);
                }

                // STEP 2: Apply EBU R128 normalization (professional loudness standard)
                if let Some(ref mut normalizer) = chain.normalizer {
                    normalizer.normalize_in_place(&mut mono_data);
                }
            }
        }

        // Create audio chunk with stream-specific timestamp (get ID first for logging)
        let chunk_id = self.chunk_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        // RAW AUDIO: No gain applied here - will be applied AFTER mixing
        // This prevents amplifying system audio bleed-through in the microphone

        // DIAGNOSTIC: Log audio levels for debugging (especially mic issues)
        // if chunk_id % 100 == 0 && !mono_data.is_empty() {
        //     let raw_rms = (mono_data.iter().map(|&x| x * x).sum::<f32>() / mono_data.len() as f32).sqrt();
        //     let raw_peak = mono_data.iter().map(|&x| x.abs()).fold(0.0f32, f32::max);

        //         info!("🎙️ [{:?}] Chunk {} - Raw: RMS={:.6}, Peak={:.6}",
        //               self.device_type, chunk_id, raw_rms, raw_peak);

        //     // Warn if microphone is completely silent
        //     if matches!(self.device_type, DeviceType::Microphone) && raw_rms == 0.0 && raw_peak == 0.0 {
        //         warn!("⚠️ Microphone producing ZERO audio - check permissions or hardware!");
        //     }
        // }
        // else if chunk_id % 100 == 0 && matches!(self.device_type, DeviceType::System) {
        //     let raw_rms = (mono_data.iter().map(|&x| x * x).sum::<f32>() / mono_data.len() as f32).sqrt();
        //     let raw_peak = mono_data.iter().map(|&x| x.abs()).fold(0.0f32, f32::max);
        //     info!("🔊 [{:?}] Chunk {} - Raw: RMS={:.6}, Peak={:.6}",
        //       self.device_type, chunk_id, raw_rms, raw_peak);
            
        //     // Warn if system audio is completely silent
        //     if raw_rms == 0.0 && raw_peak == 0.0 {
        //         warn!("⚠️ System audio producing ZERO audio - check permissions or hardware!");
        //     }
        // }

        // Capture time of this callback's last frame on the recording clock.
        #[cfg(not(target_os = "linux"))]
        let timestamp = {
            let _ = explicit_timestamp;
            (self.state.get_recording_duration().unwrap_or(0.0) - last_frame_age).max(0.0)
        };
        // Linux: position of the chunk's first sample. The Linux mixer places
        // chunks by this value, so it comes from the capture server when it
        // reports one and from a per-stream sample clock otherwise.
        #[cfg(target_os = "linux")]
        let timestamp = {
            let _ = last_frame_age;
            explicit_timestamp.unwrap_or_else(|| {
                // ALSA callback jitter must not become gaps in its single-role recording.
                let mut clock = self.fallback_clock.lock().unwrap();
                let origin = *clock.0.get_or_insert_with(||
                    (self.state.get_active_recording_duration().unwrap_or(0.0) - mono_data.len() as f64 / 48000.0).max(0.0));
                let timestamp = origin + clock.1 as f64 / 48000.0;
                clock.1 += mono_data.len() as u64;
                timestamp
            })
        };
        let capture_gap = self.continuity.lock().ok().and_then(|mut c| c.take_gap());

        // RAW AUDIO CHUNK: No gain applied - will be mixed and gained downstream
        // Use 48kHz if we resampled, otherwise use original rate
        let audio_chunk = AudioChunk {
            capture_gap,
            data: mono_data,  // Raw audio (resampled if needed), no gain yet
            sample_rate: if self.needs_resampling { 48000 } else { self.sample_rate },
            timestamp,
            chunk_id,
            device_type: self.device_type.clone(),
            dominant_source: None,
        };

        // NOTE: Raw audio is NOT sent to recording saver to prevent echo
        // Only the mixed audio (from AudioPipeline) is saved to file (see pipeline.rs:726-736)
        // This ensures we only record once: mic + system properly mixed
        // Individual raw streams go only to the transcription pipeline below

        // Send to processing pipeline for transcription
        if let Err(e) = self.state.send_audio_chunk(audio_chunk) {
            // Check if this is the "pipeline not ready" error
            if e.to_string().contains("Audio pipeline not ready") {
                // This is expected during initialization, just log it as debug
                debug!("Audio pipeline not ready yet, skipping chunk {}", chunk_id);
                return;
            }

            warn!("Failed to send audio chunk: {}", e);
            // More specific error handling based on failure reason
            let error = if e.to_string().contains("channel closed") {
                AudioError::ChannelClosed
            } else if e.to_string().contains("full") {
                AudioError::BufferOverflow
            } else {
                AudioError::ProcessingFailed
            };
            self.state.report_error(error);
        } else {
            // ~200 calls/second on the realtime audio thread; compiled out of
            // release builds entirely.
            perf_trace!("Sent audio chunk {} ({} samples)", chunk_id, data.len());
        }
    }

    /// Handle stream errors with enhanced disconnect detection
    pub fn handle_stream_error(&self, error: cpal::StreamError) {
        error!("Audio stream error for {}: {}", self.device.name, error);

        let error_str = error.to_string().to_lowercase();

        // Enhanced error detection for device disconnection
        let audio_error = if error_str.contains("device is no longer available")
            || error_str.contains("device not found")
            || error_str.contains("device disconnected")
            || error_str.contains("no such device")
            || error_str.contains("device unavailable")
            || error_str.contains("device removed")
        {
            warn!("🔌 Device disconnect detected for: {}", self.device.name);
            AudioError::DeviceDisconnected
        } else if error_str.contains("permission") || error_str.contains("access denied") {
            AudioError::PermissionDenied
        } else if error_str.contains("channel closed") {
            AudioError::ChannelClosed
        } else if error_str.contains("stream") && error_str.contains("failed") {
            AudioError::StreamFailed
        } else {
            warn!("Unknown audio error: {}", error);
            AudioError::StreamFailed
        };

        self.state.report_error(audio_error);
    }

    /// The capture stream produced its last sample. After a stop that is
    /// expected; during a recording it means this source has gone silent for
    /// good, so it goes through the same error path as a cpal stream error.
    pub fn handle_stream_end(&self) {
        if self.state.is_recording() {
            error!("Audio stream for {} ended while recording", self.device.name);
            self.state.report_error(AudioError::StreamFailed);
        }
    }
}

#[derive(Clone, Copy)]
enum VadOperation {
    Process,
    Flush,
}

#[derive(Default)]
struct PipelineLogStats {
    emitted: u64,
    short: u64,
    vad_process_invalid_data_failures: u64,
    vad_process_inference_failures: u64,
    vad_flush_invalid_data_failures: u64,
    vad_flush_inference_failures: u64,
    send_failures: u64,
    duration_sum_ms: f64,
    duration_min_ms: Option<f64>,
    duration_max_ms: f64,
}

impl PipelineLogStats {
    fn record_segment(&mut self, duration_ms: f64) {
        self.emitted += 1;
        self.duration_sum_ms += duration_ms;
        self.duration_min_ms = Some(self.duration_min_ms.map_or(duration_ms, |min| min.min(duration_ms)));
        self.duration_max_ms = self.duration_max_ms.max(duration_ms);
    }

    fn record_vad_failure(&mut self, operation: VadOperation, error: &anyhow::Error) {
        let invalid_data = error.chain().any(|cause| {
            matches!(
                cause.downcast_ref::<silero_rs::VadError>(),
                Some(silero_rs::VadError::InvalidData)
            )
        });
        match (operation, invalid_data) {
            (VadOperation::Process, true) => self.vad_process_invalid_data_failures += 1,
            (VadOperation::Process, false) => self.vad_process_inference_failures += 1,
            (VadOperation::Flush, true) => self.vad_flush_invalid_data_failures += 1,
            (VadOperation::Flush, false) => self.vad_flush_inference_failures += 1,
        }
    }

    fn snapshot_and_reset(&mut self) -> Self {
        std::mem::take(self)
    }
}

/// VAD-driven audio processing pipeline
/// Uses Voice Activity Detection to segment speech in real-time and send only speech to Whisper
pub struct AudioPipeline {
    receiver: mpsc::UnboundedReceiver<AudioChunk>,
    transcription_sender: mpsc::UnboundedSender<AudioChunk>,
    state: Arc<RecordingState>,
    /// `None` when realtime transcription is off. VAD is only useful for
    /// producing transcription segments, and Silero runs a neural inference per
    /// 30ms frame — so with transcription disabled we skip building it at all
    /// rather than feed a channel whose receiver has already been dropped.
    vad_processor: Option<ContinuousVadProcessor>,
    sample_rate: u32,
    chunk_id_counter: u64,
    last_summary_time: std::time::Instant,
    log_stats: PipelineLogStats,
    // PROFESSIONAL AUDIO MIXING: Ring buffer + RMS-based mixer
    ring_buffer: AudioMixerRingBuffer,
    mixer: ProfessionalAudioMixer,
    // Recording sender for pre-mixed audio
    recording_sender_for_mixed: Option<mpsc::UnboundedSender<AudioChunk>>,
    /// Continuous mixed audio for a streaming transcription provider (Gemini
    /// Live). Deliberately fed from the mixed stream rather than the VAD
    /// segments: a streaming recognizer runs its own endpointing and needs
    /// unbroken audio, and silence-stripped segments would delay every interim
    /// caption until after the utterance had already finished. `None` for
    /// every non-streaming provider.
    live_sender_for_mixed: Option<mpsc::UnboundedSender<AudioChunk>>,
    // Me/Others speaker attribution: label each pre-mix window by dominant
    // source, aggregate across the windows behind each VAD segment.
    window_labeler: super::source_attribution::WindowLabeler,
    segment_aggregator: super::source_attribution::SegmentAggregator,
    /// Reused across mix windows so the hot loop allocates nothing. The mixed
    /// buffer is still moved out per window (it becomes the recording chunk),
    /// but these two no longer are.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    mic_window: Vec<f32>,
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    sys_window: Vec<f32>,
    /// Timestamp of the most recent chunk, reused for the window flushed on stop.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    last_chunk_timestamp: f64,
}

impl AudioPipeline {
    pub fn new(
        receiver: mpsc::UnboundedReceiver<AudioChunk>,
        transcription_sender: mpsc::UnboundedSender<AudioChunk>,
        state: Arc<RecordingState>,
        target_chunk_duration_ms: u32,
        sample_rate: u32,
        mic_device_name: String,
        mic_device_kind: super::device_detection::InputDeviceKind,
        system_device_name: String,
        system_device_kind: super::device_detection::InputDeviceKind,
        vad_enabled: bool,
    ) -> Result<Self> {
        // Log device characteristics for adaptive buffering
        info!("🎛️ AudioPipeline initializing with device characteristics:");
        info!("   Mic: '{}' ({:?}) - Buffer: {:?}",
              mic_device_name, mic_device_kind, mic_device_kind.buffer_timeout());
        info!("   System: '{}' ({:?}) - Buffer: {:?}",
              system_device_name, system_device_kind, system_device_kind.buffer_timeout());

        // Device kind information can be used for adaptive buffering in the future
        // For now, we log it for monitoring and potential optimization
        let _ = (mic_device_name, mic_device_kind, system_device_name, system_device_kind);

        // Create VAD processor. The VAD processor handles 48kHz->16kHz resampling
        // internally.
        //
        // Redemption time is how long a silence must last before the VAD closes a
        // speech segment, so it decides how long the audio clips handed to the ASR
        // engine are. Conversational speech pauses constantly for breath and
        // mid-sentence thought, and every pause longer than this becomes a segment
        // boundary and therefore a separate transcription request.
        //
        // This was 400ms, which fragmented a 26-minute meeting into 322 requests with
        // a median length of 3.5s. Whisper is a fixed 30-second-window model: below
        // that it zero-pads the window and leans on its language-model prior, which
        // was trained on web subtitles, so short clips come back as memorised
        // boilerplate ("subscribe to the channel", "thank you") instead of speech.
        // Measured on a real recording, 47% of segment boundaries sat in the
        // 0.42-0.75s range that a longer redemption bridges.
        //
        // 500ms is the live-path policy (see the constant's doc comment). The
        // batch value (2000ms, `import.rs`/`retranscription.rs`) was tried here
        // first, but under continuous system audio it kept a VAD segment open
        // indefinitely and withheld live transcript emission, so live and batch
        // deliberately diverge. Bounded live segments under continuous speech
        // are tracked in #756.
        //
        // Silero runs an inference per 30ms frame, so the processor (and speaker
        // attribution with it) is only built when realtime transcription is on.
        let vad_processor = if vad_enabled {
            let processor = ContinuousVadProcessor::new(sample_rate, VAD_REDEMPTION_TIME_MS)?;
            info!(
                "VAD-driven pipeline: segments dispatched per speech burst (redemption_time={}ms)",
                VAD_REDEMPTION_TIME_MS
            );
            Some(processor)
        } else {
            info!("Realtime transcription disabled: skipping VAD, speaker attribution, and segment dispatch (recording/mixing unaffected)");
            None
        };

        // Initialize professional audio mixing components
        let ring_buffer = AudioMixerRingBuffer::new(sample_rate);
        let mixer = ProfessionalAudioMixer::new(sample_rate);

        // Note: target_chunk_duration_ms is ignored - VAD controls segmentation now
        let _ = target_chunk_duration_ms;

        Ok(Self {
            receiver,
            transcription_sender,
            state,
            vad_processor,
            sample_rate,
            chunk_id_counter: 0,
            last_summary_time: std::time::Instant::now(),
            log_stats: PipelineLogStats::default(),
            // Initialize professional audio mixing
            ring_buffer,
            mixer,
            recording_sender_for_mixed: None,  // Will be set by manager
            live_sender_for_mixed: None,       // Will be set by manager
            window_labeler: super::source_attribution::WindowLabeler::new(),
            segment_aggregator: super::source_attribution::SegmentAggregator::new(),
            mic_window: Vec::new(),
            sys_window: Vec::new(),
            last_chunk_timestamp: 0.0,
        })
    }

    /// Run the VAD-driven audio processing pipeline
    pub async fn run(mut self) -> Result<()> {
        info!("VAD-driven audio pipeline started - segments sent in real-time based on speech detection");

        // CRITICAL FIX: Continue processing until channel is closed, not based on recording state
        // This ensures ALL chunks are processed during shutdown, fixing premature meeting completion
        // Previous bug: Loop checked `while self.state.is_recording()` which caused early exit when
        // stop_recording() was called, losing remaining chunks in the pipeline
        loop {
            // Block until the next chunk. There is no periodic work to do here —
            // VAD drives all segmentation — so the previous 50ms timeout only
            // armed and cancelled a timer per chunk (~200/s) and woke this task
            // 20x/s through silence.
            match self.receiver.recv().await {
                Some(chunk) => {
                    if self.last_summary_time.elapsed() >= std::time::Duration::from_secs(60) {
                        self.emit_log_summary(false);
                    }

                    // Nobody downstream: no VAD to feed, no encoder to write to,
                    // and no live stream to send. Mixing here would be pure heat
                    // — drop the samples and keep draining so the capture
                    // threads never block.
                    //
                    // The live sender must be part of this test: with Gemini
                    // Live the VAD processor is deliberately absent, so omitting
                    // it here would skip mixing entirely and stream silence.
                    if self.vad_processor.is_none()
                        && self.recording_sender_for_mixed.is_none()
                        && self.live_sender_for_mixed.is_none()
                    {
                        continue;
                    }

                    self.mix_chunk(chunk);
                }
                None => break,
            }
        }

        // Channel closed: mix the partial window before flushing the VAD, so
        // speech in it ends the final utterance instead of being lost. (On
        // Linux, flush_remaining_audio drains the positioned buffers itself.)
        #[cfg(not(target_os = "linux"))]
        self.flush_partial_window();
        self.flush_remaining_audio()?;
        self.emit_log_summary(true);
        info!("VAD-driven audio pipeline ended");
        Ok(())
    }

    /// Place one raw capture chunk in the ring buffer and mix every window
    /// that became ready.
    #[cfg(not(target_os = "linux"))]
    fn mix_chunk(&mut self, chunk: AudioChunk) {
        // STEP 1: Add raw audio to ring buffer for mixing
        // Microphone audio is already normalized at capture level (AudioCapture)
        // System audio remains raw
        self.last_chunk_timestamp = chunk.timestamp;
        self.ring_buffer.add_captured(
            chunk.device_type.clone(),
            chunk.data,
            chunk.timestamp,
            chunk.capture_gap,
        );

        // STEP 2: Mix audio in fixed windows when both streams have
        // a window (or one is stalled — see MAX_MIXER_LAG_WINDOWS)
        while self.ring_buffer.can_mix() {
            // `mic_window`/`sys_window` are reused buffers owned by
            // self; move them out for the duration of the body so
            // the mixer and labeler can borrow self mutably.
            let mut mic_window = std::mem::take(&mut self.mic_window);
            let mut sys_window = std::mem::take(&mut self.sys_window);

            let extracted = self
                .ring_buffer
                .extract_window_into(&mut mic_window, &mut sys_window);

            if extracted {
                self.mix_and_send_window(&mic_window, &sys_window, chunk.timestamp);
            }

            // Hand the window buffers back for the next iteration.
            // Their capacity is what makes this loop allocation-free.
            self.mic_window = mic_window;
            self.sys_window = sys_window;

            if !extracted {
                break;
            }
        }
    }

    /// Linux: chunks carry their position on the recording timeline
    /// (`AudioChunk::timestamp` of the first sample); the mixer places them
    /// by it.
    #[cfg(target_os = "linux")]
    fn mix_chunk(&mut self, chunk: AudioChunk) {
        let position = (chunk.timestamp * self.sample_rate as f64).round().max(0.0) as u64;
        self.ring_buffer.add_samples_at(chunk.device_type, chunk.data, position);
        self.mix_ready(false);
    }

    /// Linux: mix every window the positioned buffers can release (all of
    /// them, including a final partial one, when `flush` is set).
    #[cfg(target_os = "linux")]
    fn mix_ready(&mut self, flush: bool) {
        while let Some((mic, system)) = self.ring_buffer.extract_aligned(flush) {
            let start = self.ring_buffer.position.unwrap_or(0).saturating_sub(mic.len() as u64);
            let timestamp = start as f64 / self.sample_rate as f64;
            self.mix_and_send_window(&mic, &system, timestamp);
        }
    }

    /// Label, mix, and dispatch one aligned window to speaker attribution,
    /// the VAD, the live stream, and the recording sender.
    fn mix_and_send_window(&mut self, mic_window: &[f32], sys_window: &[f32], timestamp: f64) {
        // Speaker attribution only labels transcript segments,
        // so it costs two RMS passes we can skip when there
        // will be no transcripts.
        let window_label = if self.vad_processor.is_some() {
            Some(self.window_labeler.label(&mic_window, &sys_window))
        } else {
            None
        };

        // Simple mixing without aggressive ducking.
        // NO POST-GAIN NEEDED: Microphone already normalized by EBU R128 to -23 LUFS
        // (broadcast-standard loudness); system audio at natural levels.
        let mut mixed_with_gain = Vec::new();
        self.mixer
            .mix_window_into(&mic_window, &sys_window, &mut mixed_with_gain);

        // Weight the window's label by its (mixed) energy so
        // loud speech outvotes quiet crosstalk per segment.
        if let Some(window_label) = window_label {
            let window_energy: f32 =
                mixed_with_gain.iter().map(|s| s * s).sum();
            self.segment_aggregator.add(window_label, window_energy);
        }

        // STEP 3: Send mixed audio for transcription (VAD + Whisper).
        // Skipped entirely when realtime transcription is off.
        // `map` ends the borrow of `self.vad_processor` before
        // the body touches `self.segment_aggregator`.
        let vad_result = self
            .vad_processor
            .as_mut()
            .map(|vad| vad.process_audio(&mixed_with_gain));

        match vad_result {
            Some(Ok(speech_segments)) => {
                // One label per emission batch: the windows
                // accumulated since the last VAD segment(s)
                // back everything emitted now. (VAD segment
                // boundaries can drift a window either way —
                // acceptable for a binary Me/Others label.)
                let batch_source = if speech_segments.is_empty() {
                    None
                } else {
                    self.segment_aggregator.finalize()
                };
                for segment in speech_segments {
                    let duration_ms = segment.end_timestamp_ms - segment.start_timestamp_ms;

                    if segment.samples.len() >= 800 {  // Minimum 50ms at 16kHz - matches Parakeet capability
                        self.log_stats.record_segment(duration_ms);

                        let transcription_chunk = AudioChunk {
                            capture_gap: None,
                            data: segment.samples,
                            sample_rate: 16000,
                            timestamp: segment.start_timestamp_ms / 1000.0,
                            chunk_id: self.chunk_id_counter,
                            device_type: DeviceType::Microphone,  // Mixed audio
                            dominant_source: batch_source.clone(),
                        };

                        if self.transcription_sender.send(transcription_chunk).is_err() {
                            self.log_stats.send_failures += 1;
                        } else {
                            self.chunk_id_counter += 1;
                        }
                    } else {
                        self.log_stats.short += 1;
                    }
                }
            }
            Some(Err(error)) => self.log_stats.record_vad_failure(VadOperation::Process, &error),
            // Realtime transcription disabled — no VAD.
            None => {}
        }

        // STEP 3b: Send the continuous mixed window to a
        // streaming transcription provider. Read by
        // reference — STEP 4 still needs to move the buffer
        // — and resampled by the live session, which owns
        // the wire format.
        if let Some(ref sender) = self.live_sender_for_mixed {
            let live_chunk = AudioChunk {
                capture_gap: None,
                data: mixed_with_gain.clone(),
                sample_rate: self.sample_rate,
                timestamp,
                chunk_id: self.chunk_id_counter,
                device_type: DeviceType::Microphone,  // Mixed audio
                // Mixed audio has no trustworthy per-window
                // attribution, and the streaming provider
                // does not use one.
                dominant_source: None,
            };
            let _ = sender.send(live_chunk);
        }

        // STEP 4: Send mixed audio to the encoder.
        // Last use of the window, so move it instead of
        // cloning another 115 KB per window.
        if let Some(ref sender) = self.recording_sender_for_mixed {
            let recording_chunk = AudioChunk {
                capture_gap: None,
                data: mixed_with_gain,
                sample_rate: self.sample_rate,
                timestamp,
                chunk_id: self.chunk_id_counter,
                device_type: DeviceType::Microphone,  // Mixed audio
                dominant_source: None,
            };
            let _ = sender.send(recording_chunk);
        }
    }

    /// Mix whatever is left in the ring buffer when the stream stops. A stream
    /// that ends short of a full window would otherwise lose that tail.
    #[cfg(not(target_os = "linux"))]
    fn flush_partial_window(&mut self) {
        let mut mic_window = Vec::new();
        let mut sys_window = Vec::new();
        if self.ring_buffer.extract_partial_window_into(&mut mic_window, &mut sys_window) {
            let timestamp = self.last_chunk_timestamp;
            self.mix_and_send_window(&mic_window, &sys_window, timestamp);
        }
    }

    fn flush_remaining_audio(&mut self) -> Result<()> {
        #[cfg(target_os = "linux")]
        self.mix_ready(true);

        // Flush any remaining audio from VAD processor and send segments to
        // transcription. No VAD means no pending segments to flush.
        let flushed = self.vad_processor.as_mut().map(|vad| vad.flush());
        let Some(flushed) = flushed else {
            return Ok(());
        };

        match flushed {
            Ok(final_segments) => {
                let batch_source = if final_segments.is_empty() {
                    None
                } else {
                    self.segment_aggregator.finalize()
                };
                for segment in final_segments {
                    let duration_ms = segment.end_timestamp_ms - segment.start_timestamp_ms;
                    // Send segments >= 50ms (800 samples at 16kHz) - matches main pipeline filter
                    if segment.samples.len() >= 800 {
                        self.log_stats.record_segment(duration_ms);
                        let transcription_chunk = AudioChunk {
                            capture_gap: None,
                            data: segment.samples,
                            sample_rate: 16000,
                            timestamp: segment.start_timestamp_ms / 1000.0,
                            chunk_id: self.chunk_id_counter,
                            device_type: DeviceType::Microphone,
                            dominant_source: batch_source.clone(),
                        };
                        if self.transcription_sender.send(transcription_chunk).is_err() {
                            self.log_stats.send_failures += 1;
                        } else {
                            self.chunk_id_counter += 1;
                        }
                    } else {
                        self.log_stats.short += 1;
                    }
                }
            }
            Err(error) => self.log_stats.record_vad_failure(VadOperation::Flush, &error),
        }
        Ok(())
    }

    fn emit_log_summary(&mut self, final_summary: bool) {
        let stats = self.log_stats.snapshot_and_reset();
        let (mic_dropped, system_dropped) = self.ring_buffer.take_dropped_samples();
        let mean_duration_ms = if stats.emitted == 0 {
            0.0
        } else {
            stats.duration_sum_ms / stats.emitted as f64
        };
        if final_summary {
            info!("Audio pipeline summary; emitted={}, short={}, vad_process_invalid_data_failures={}, vad_process_inference_failures={}, vad_flush_invalid_data_failures={}, vad_flush_inference_failures={}, send_failures={}, mic_dropped_samples={}, system_dropped_samples={}, segment_min_ms={:.1}, segment_mean_ms={:.1}, segment_max_ms={:.1}", stats.emitted, stats.short, stats.vad_process_invalid_data_failures, stats.vad_process_inference_failures, stats.vad_flush_invalid_data_failures, stats.vad_flush_inference_failures, stats.send_failures, mic_dropped, system_dropped, stats.duration_min_ms.unwrap_or(0.0), mean_duration_ms, stats.duration_max_ms);
        } else if stats.vad_process_invalid_data_failures > 0 || stats.vad_process_inference_failures > 0 || stats.vad_flush_invalid_data_failures > 0 || stats.vad_flush_inference_failures > 0 || stats.send_failures > 0 || mic_dropped > 0 || system_dropped > 0 {
            warn!("Audio pipeline summary; emitted={}, short={}, vad_process_invalid_data_failures={}, vad_process_inference_failures={}, vad_flush_invalid_data_failures={}, vad_flush_inference_failures={}, send_failures={}, mic_dropped_samples={}, system_dropped_samples={}", stats.emitted, stats.short, stats.vad_process_invalid_data_failures, stats.vad_process_inference_failures, stats.vad_flush_invalid_data_failures, stats.vad_flush_inference_failures, stats.send_failures, mic_dropped, system_dropped);
        } else {
            debug!("Audio pipeline summary; emitted={}, short={}, segment_min_ms={:.1}, segment_mean_ms={:.1}, segment_max_ms={:.1}", stats.emitted, stats.short, stats.duration_min_ms.unwrap_or(0.0), mean_duration_ms, stats.duration_max_ms);
        }
        self.last_summary_time = std::time::Instant::now();
    }

}

/// Simple audio pipeline manager
pub struct AudioPipelineManager {
    pipeline_handle: Option<JoinHandle<Result<()>>>,
    audio_sender: Option<mpsc::UnboundedSender<AudioChunk>>,
}

impl AudioPipelineManager {
    pub fn new() -> Self {
        Self {
            pipeline_handle: None,
            audio_sender: None,
        }
    }

    /// Start the audio pipeline with device information for adaptive buffering
    pub fn start(
        &mut self,
        state: Arc<RecordingState>,
        transcription_sender: mpsc::UnboundedSender<AudioChunk>,
        target_chunk_duration_ms: u32,
        sample_rate: u32,
        recording_sender: Option<mpsc::UnboundedSender<AudioChunk>>,
        mic_device_name: String,
        mic_device_kind: super::device_detection::InputDeviceKind,
        system_device_name: String,
        system_device_kind: super::device_detection::InputDeviceKind,
        vad_enabled: bool,
        streaming_live: bool,
    ) -> Result<()> {
        // Log device information for adaptive buffering
        info!("🎙️ Starting pipeline with device info:");
        info!("   Microphone: '{}' ({:?})", mic_device_name, mic_device_kind);
        info!("   System Audio: '{}' ({:?})", system_device_name, system_device_kind);

        // Create audio processing channel
        let (audio_sender, audio_receiver) = mpsc::unbounded_channel::<AudioChunk>();


        // A streaming provider runs its own endpointing on continuous audio, so
        // local VAD is both unnecessary and harmful (it would strip the silence
        // the recognizer uses to detect utterance boundaries).
        let vad_enabled = vad_enabled && !streaming_live;

        // Create and start pipeline with device information for adaptive mixing
        let mut pipeline = AudioPipeline::new(
            audio_receiver,
            transcription_sender.clone(),
            state.clone(),
            target_chunk_duration_ms,
            sample_rate,
            mic_device_name,
            mic_device_kind,
            system_device_name,
            system_device_kind,
            vad_enabled,
        )?;
        state.set_audio_sender(audio_sender.clone());

        // CRITICAL FIX: Connect recording sender to receive pre-mixed audio
        // This ensures both mic AND system audio are captured in recordings
        pipeline.recording_sender_for_mixed = recording_sender;

        // In streaming mode the transcription channel carries the continuous
        // mixed stream instead of VAD segments; the consumer
        // (start_transcription_task) branches on the same configuration.
        if streaming_live {
            info!("🔊 Streaming transcription: feeding continuous mixed audio to the live session (VAD bypassed)");
            pipeline.live_sender_for_mixed = Some(transcription_sender);
        }

        let handle = tokio::spawn(async move {
            pipeline.run().await
        });

        self.pipeline_handle = Some(handle);
        self.audio_sender = Some(audio_sender);

        info!("Audio pipeline manager started with mixed audio recording");
        Ok(())
    }

    /// Stop the audio pipeline
    pub async fn stop(&mut self) -> Result<()> {
        // Drop the sender to close the pipeline
        self.audio_sender = None;

        // Wait for pipeline to finish
        if let Some(handle) = self.pipeline_handle.take() {
            match handle.await {
                Ok(result) => result,
                Err(e) => {
                    error!("Pipeline task failed: {}", e);
                    Ok(())
                }
            }
        } else {
            Ok(())
        }
    }

}

#[cfg(test)]
mod tests {
    use super::{PipelineLogStats, VadOperation};

    #[test]
    fn logging_pipeline_diagnostics_aggregate_and_reset() {
        let mut stats = PipelineLogStats::default();
        stats.record_segment(10.0);
        stats.record_segment(30.0);
        stats.short = 1;
        for operation in [VadOperation::Process, VadOperation::Flush] {
            stats.record_vad_failure(
                operation,
                &anyhow::Error::from(silero_rs::VadError::InvalidData),
            );
            stats.record_vad_failure(operation, &anyhow::anyhow!("opaque VAD failure"));
        }
        let snapshot = stats.snapshot_and_reset();
        assert_eq!(snapshot.emitted, 2);
        assert_eq!(snapshot.short, 1);
        assert_eq!(snapshot.vad_process_invalid_data_failures, 1);
        assert_eq!(snapshot.vad_process_inference_failures, 1);
        assert_eq!(snapshot.vad_flush_invalid_data_failures, 1);
        assert_eq!(snapshot.vad_flush_inference_failures, 1);
        assert_eq!(snapshot.duration_min_ms, Some(10.0));
        assert_eq!(snapshot.duration_max_ms, 30.0);
        assert_eq!(snapshot.duration_sum_ms / snapshot.emitted as f64, 20.0);
        let reset = stats.snapshot_and_reset();
        assert_eq!(reset.vad_process_invalid_data_failures, 0);
        assert_eq!(reset.vad_process_inference_failures, 0);
        assert_eq!(reset.vad_flush_invalid_data_failures, 0);
        assert_eq!(reset.vad_flush_inference_failures, 0);
    }
}

impl Default for AudioPipelineManager {
    fn default() -> Self {
        Self::new()
    }
}
#[cfg(test)]
mod policy_tests {
    use super::*;

    #[test]
    fn test_live_vad_redemption_matches_pro_policy() {
        // Live uses the established 500ms pause policy; it does not bound
        // uninterrupted speech. Batch import/retranscription use 2000ms.
        // See #679 and #756.
        assert_eq!(VAD_REDEMPTION_TIME_MS, 500);
    }

    /// A partial mixer window left in the ring buffer when the stream closes
    /// must still reach the recording channel.
    #[tokio::test]
    async fn partial_window_is_flushed_on_stop() {
        let state = RecordingState::new();
        let (audio_tx, audio_rx) = mpsc::unbounded_channel::<AudioChunk>();
        let (transcription_tx, _transcription_rx) = mpsc::unbounded_channel::<AudioChunk>();
        let mut pipeline = AudioPipeline::new(
            audio_rx,
            transcription_tx,
            state,
            0,
            48_000,
            "mic".to_string(),
            crate::audio::device_detection::InputDeviceKind::Wired,
            "system".to_string(),
            crate::audio::device_detection::InputDeviceKind::Wired,
            false,
        )
        .expect("pipeline construction");
        let (recording_tx, mut recording_rx) = mpsc::unbounded_channel::<AudioChunk>();
        pipeline.recording_sender_for_mixed = Some(recording_tx);

        audio_tx
            .send(AudioChunk {
                capture_gap: None,
                data: vec![0.5f32; 14_400],
                sample_rate: 48_000,
                timestamp: 0.0,
                chunk_id: 0,
                device_type: DeviceType::Microphone,
                dominant_source: None,
            })
            .unwrap();
        drop(audio_tx);

        pipeline.run().await.expect("pipeline run");

        let mut total_samples = 0usize;
        while let Ok(chunk) = recording_rx.try_recv() {
            total_samples += chunk.data.len();
        }
        assert_eq!(total_samples, 14_400);
    }

    /// Mic and system audio that both stop short of a full window are mixed
    /// together and flushed on stop. The final window is as long as the longer
    /// stream, and the shorter stream is zero-padded to it.
    #[tokio::test]
    async fn partial_mic_and_system_windows_are_flushed_together_on_stop() {
        let state = RecordingState::new();
        let (audio_tx, audio_rx) = mpsc::unbounded_channel::<AudioChunk>();
        let (transcription_tx, _transcription_rx) = mpsc::unbounded_channel::<AudioChunk>();
        let mut pipeline = AudioPipeline::new(
            audio_rx,
            transcription_tx,
            state,
            0,
            48_000,
            "mic".to_string(),
            crate::audio::device_detection::InputDeviceKind::Wired,
            "system".to_string(),
            crate::audio::device_detection::InputDeviceKind::Wired,
            false,
        )
        .expect("pipeline construction");
        let (recording_tx, mut recording_rx) = mpsc::unbounded_channel::<AudioChunk>();
        pipeline.recording_sender_for_mixed = Some(recording_tx);

        audio_tx
            .send(AudioChunk {
                capture_gap: None,
                data: vec![0.25f32; 14_400],
                sample_rate: 48_000,
                timestamp: 0.0,
                chunk_id: 0,
                device_type: DeviceType::Microphone,
                dominant_source: None,
            })
            .unwrap();
        audio_tx
            .send(AudioChunk {
                capture_gap: None,
                data: vec![0.25f32; 9_600],
                sample_rate: 48_000,
                timestamp: 0.0,
                chunk_id: 1,
                device_type: DeviceType::System,
                dominant_source: None,
            })
            .unwrap();
        drop(audio_tx);

        pipeline.run().await.expect("pipeline run");

        let mut total_samples = 0usize;
        while let Ok(chunk) = recording_rx.try_recv() {
            total_samples += chunk.data.len();
        }
        assert_eq!(total_samples, 14_400);
    }

    /// A system chunk that arrives after the mic chunk of the same instant,
    /// just before the stream closes, is mixed into the same final window.
    #[tokio::test]
    async fn late_system_chunk_is_not_split_from_mic_on_stop() {
        let state = RecordingState::new();
        let (audio_tx, audio_rx) = mpsc::unbounded_channel::<AudioChunk>();
        let (transcription_tx, _transcription_rx) = mpsc::unbounded_channel::<AudioChunk>();
        let mut pipeline = AudioPipeline::new(
            audio_rx,
            transcription_tx,
            state,
            0,
            48_000,
            "mic".to_string(),
            crate::audio::device_detection::InputDeviceKind::Wired,
            "system".to_string(),
            crate::audio::device_detection::InputDeviceKind::Wired,
            false,
        )
        .expect("pipeline construction");
        let (recording_tx, mut recording_rx) = mpsc::unbounded_channel::<AudioChunk>();
        pipeline.recording_sender_for_mixed = Some(recording_tx);

        audio_tx
            .send(AudioChunk {
                capture_gap: None,
                data: vec![0.25f32; 9_600],
                sample_rate: 48_000,
                timestamp: 0.0,
                chunk_id: 0,
                device_type: DeviceType::Microphone,
                dominant_source: None,
            })
            .unwrap();
        audio_tx
            .send(AudioChunk {
                capture_gap: None,
                data: vec![0.25f32; 9_600],
                sample_rate: 48_000,
                timestamp: 0.0,
                chunk_id: 1,
                device_type: DeviceType::System,
                dominant_source: None,
            })
            .unwrap();
        drop(audio_tx);

        pipeline.run().await.expect("pipeline run");

        let mut lengths = Vec::new();
        while let Ok(chunk) = recording_rx.try_recv() {
            lengths.push(chunk.data.len());
        }
        assert_eq!(lengths, vec![9_600]);
    }

    /// Stop closes the stream while the ring buffer still holds a partial
    /// window of the utterance in progress. That tail must reach the
    /// transcript exactly once: not dropped by a VAD flushed before it was
    /// mixed, and not re-sent alongside the part flushed earlier.
    #[cfg(not(target_os = "linux"))]
    #[tokio::test]
    async fn stop_mid_speech_keeps_partial_window_speech_once() {
        let state = RecordingState::new();
        let (audio_tx, audio_rx) = mpsc::unbounded_channel::<AudioChunk>();
        let (transcription_tx, mut transcription_rx) = mpsc::unbounded_channel::<AudioChunk>();
        let pipeline = AudioPipeline::new(
            audio_rx,
            transcription_tx,
            state,
            0,
            48_000,
            "mic".to_string(),
            crate::audio::device_detection::InputDeviceKind::Wired,
            "system".to_string(),
            crate::audio::device_detection::InputDeviceKind::Wired,
            true,
        )
        .expect("pipeline construction");

        // 1s of silence, then speech still running when the recording stops at
        // 4.5s: seven full windows plus a 300ms partial window.
        let mut mic = vec![0.0f32; 48_000];
        mic.extend(crate::audio::vad::tests::generate_voiced_speech(3.5, 48_000));
        assert_eq!(mic.len() % 28_800, 14_400);

        for (i, data) in mic.chunks(CHUNK).enumerate() {
            let timestamp = (i + 1) as f64 * CHUNK as f64 / 48_000.0;
            let gap = if i == 0 { None } else { Some(0.0) };
            for (device_type, data) in [
                (DeviceType::Microphone, data.to_vec()),
                (DeviceType::System, vec![0.0f32; data.len()]),
            ] {
                audio_tx
                    .send(AudioChunk {
                        capture_gap: gap,
                        data,
                        sample_rate: 48_000,
                        timestamp,
                        chunk_id: i as u64,
                        device_type,
                        dominant_source: None,
                    })
                    .unwrap();
            }
        }
        drop(audio_tx);

        pipeline.run().await.expect("pipeline run");

        let mut segments = Vec::new();
        while let Ok(chunk) = transcription_rx.try_recv() {
            let start = chunk.timestamp;
            segments.push((start, start + chunk.data.len() as f64 / 16_000.0));
        }
        let covering = |t: f64| segments.iter().filter(|(s, e)| *s <= t && t < *e).count();
        assert_eq!(covering(4.4), 1, "speech in the partial window, segments: {segments:?}");
        assert_eq!(covering(3.0), 1, "speech before the partial window, segments: {segments:?}");
    }

    #[cfg(not(target_os = "linux"))]
    const CHUNK: usize = 480;

    #[cfg(not(target_os = "linux"))]
    fn drain_ready_windows(
        rb: &mut AudioMixerRingBuffer,
        mic_out: &mut Vec<f32>,
        sys_out: &mut Vec<f32>,
        mic_all: &mut Vec<f32>,
        sys_all: &mut Vec<f32>,
    ) -> usize {
        let mut windows = 0;
        while rb.can_mix() {
            assert!(rb.extract_window_into(mic_out, sys_out));
            mic_all.extend_from_slice(mic_out);
            sys_all.extend_from_slice(sys_out);
            windows += 1;
        }
        windows
    }

    /// H1-F2: a system chunk that is only one chunk late across a window
    /// boundary must be waited for, not zero-padded into the window.
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn late_system_chunk_is_waited_for_not_zero_padded() {
        let mut rb = AudioMixerRingBuffer::new(48_000);
        let window = rb.window_size_samples;
        let (mut mic_out, mut sys_out) = (Vec::new(), Vec::new());
        let (mut mic_all, mut sys_all) = (Vec::new(), Vec::new());

        // Window 1: 60 lockstep pairs (system pushed first), one clean window.
        for _ in 0..60 {
            rb.add_samples(DeviceType::System, vec![0.5; CHUNK]);
            rb.add_samples(DeviceType::Microphone, vec![0.5; CHUNK]);
            drain_ready_windows(&mut rb, &mut mic_out, &mut sys_out, &mut mic_all, &mut sys_all);
        }

        // Window 2: 59 lockstep pairs; the 60th system chunk is late.
        for _ in 0..59 {
            rb.add_samples(DeviceType::System, vec![0.5; CHUNK]);
            rb.add_samples(DeviceType::Microphone, vec![0.5; CHUNK]);
            drain_ready_windows(&mut rb, &mut mic_out, &mut sys_out, &mut mic_all, &mut sys_all);
        }
        // Mic chunk 60 completes the mic window while system holds 59 chunks.
        rb.add_samples(DeviceType::Microphone, vec![0.5; CHUNK]);
        drain_ready_windows(&mut rb, &mut mic_out, &mut sys_out, &mut mic_all, &mut sys_all);
        // The merely-late system chunk arrives.
        rb.add_samples(DeviceType::System, vec![0.5; CHUNK]);
        drain_ready_windows(&mut rb, &mut mic_out, &mut sys_out, &mut mic_all, &mut sys_all);

        assert_eq!(sys_all.len(), 2 * window);
        assert_eq!(sys_all.iter().filter(|s| **s == 0.0).count(), 0);
        assert_eq!(mic_all.iter().filter(|s| **s == 0.0).count(), 0);
    }

    // ---- WASAPI timing replay ----------------------------------------------
    //
    // These replay the delivery pattern of cpal's WASAPI backend on Windows:
    // the mic sends a 10 ms packet every 10 ms, but loopback (system audio)
    // jitters, arrives in bursts, and sends *nothing* while no app is
    // rendering. Every sample carries its capture time (sample index + 1, so
    // 0.0 still means "padding"), which makes alignment checkable: a mixed
    // window is aligned when, at every position where both streams carry
    // real audio, they carry the same capture time.

    /// One simulated delivery: at `arrival` (sample clock), `device` hands
    /// over the samples captured over `[start, start + CHUNK)`.
    #[cfg(not(target_os = "linux"))]
    struct Delivery {
        arrival: usize,
        device: DeviceType,
        start: usize,
    }

    #[cfg(not(target_os = "linux"))]
    fn mic_deliveries(seconds: usize) -> Vec<Delivery> {
        (0..seconds * 100)
            .map(|i| Delivery { arrival: (i + 1) * CHUNK, device: DeviceType::Microphone, start: i * CHUNK })
            .collect()
    }

    /// Loopback packets for the capture times in `active` (in samples),
    /// each delivered `delay(i)` samples after it was captured. WASAPI
    /// delivers in order, so a packet never arrives before the one ahead
    /// of it; a delayed packet holds back the ones behind it into a burst.
    #[cfg(not(target_os = "linux"))]
    fn loopback_deliveries(
        active: std::ops::Range<usize>,
        delay: impl Fn(usize) -> usize,
    ) -> Vec<Delivery> {
        let mut last_arrival = 0;
        (active.start / CHUNK..active.end / CHUNK)
            .map(|i| {
                last_arrival = ((i + 1) * CHUNK + delay(i)).max(last_arrival);
                Delivery { arrival: last_arrival, device: DeviceType::System, start: i * CHUNK }
            })
            .collect()
    }

    /// Replays deliveries in arrival order through the ring buffer, with the
    /// timing real capture attaches (capture-end timestamp; `capture_gap`
    /// `None` on a stream's first chunk, then the time skipped since the
    /// previous packet). Returns, per mixed window, the largest capture-time
    /// gap between mic and system at positions where both carry audio
    /// (0 = aligned).
    #[cfg(not(target_os = "linux"))]
    fn replay_misalignment(mut deliveries: Vec<Delivery>) -> Vec<usize> {
        // Stable sort: on equal arrival the mic (pushed first) goes first.
        deliveries.sort_by_key(|d| d.arrival);
        let mut rb = AudioMixerRingBuffer::new(48_000);
        let (mut mic_out, mut sys_out) = (Vec::new(), Vec::new());
        let mut prev_end: [Option<usize>; 2] = [None, None];
        let mut per_window = Vec::new();
        for d in deliveries {
            let slot = matches!(d.device, DeviceType::System) as usize;
            let gap = prev_end[slot].map(|end| (d.start - end) as f64 / 48_000.0);
            prev_end[slot] = Some(d.start + CHUNK);
            let samples = (d.start..d.start + CHUNK).map(|t| (t + 1) as f32).collect();
            rb.add_captured(d.device, samples, (d.start + CHUNK) as f64 / 48_000.0, gap);
            while rb.extract_window_into(&mut mic_out, &mut sys_out) {
                let worst = mic_out
                    .iter()
                    .zip(&sys_out)
                    .filter(|(m, s)| **m != 0.0 && **s != 0.0)
                    .map(|(m, s)| (*m as i64 - *s as i64).unsigned_abs() as usize)
                    .max()
                    .unwrap_or(0);
                per_window.push(worst);
            }
        }
        per_window
    }

    /// Loopback jitter up to 30 ms with occasional 4-packet bursts: every
    /// window must still pair mic and system audio captured at the same time.
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn wasapi_replay_jitter_and_bursts_stay_aligned() {
        let mut d = mic_deliveries(20);
        d.extend(loopback_deliveries(0..20 * 48_000, |i| {
            if i % 40 < 4 { (4 - i % 40) * CHUNK } else { (i * 7 % 3) * CHUNK }
        }));
        let gaps = replay_misalignment(d);
        assert!(gaps.len() >= 30, "only {} windows mixed", gaps.len());
        assert!(gaps.iter().all(|g| *g == 0), "misaligned windows: {:?}", gaps);
    }

    /// Nothing plays for the first 10 s (loopback silent), then audio starts.
    /// After it starts, system audio must line up with the mic again.
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn wasapi_replay_idle_loopback_then_playback_realigns() {
        let mut d = mic_deliveries(20);
        d.extend(loopback_deliveries(10 * 48_000 + 7 * CHUNK..20 * 48_000, |_| CHUNK));
        let gaps = replay_misalignment(d);
        let worst_ms = gaps.iter().max().copied().unwrap_or(0) / 48;
        assert_eq!(worst_ms, 0, "system audio offset by up to {} ms after playback started", worst_ms);
    }

    /// Playback pauses for 500 ms (below the stall bound) and resumes. The
    /// pause must not shift system audio against the mic for the rest of
    /// the recording.
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn wasapi_replay_short_loopback_pause_does_not_shift_alignment() {
        let mut d = mic_deliveries(20);
        d.extend(loopback_deliveries(0..8 * 48_000, |_| CHUNK));
        d.extend(loopback_deliveries(8 * 48_000 + 50 * CHUNK..20 * 48_000, |_| CHUNK));
        let gaps = replay_misalignment(d);
        let worst_ms = gaps.iter().max().copied().unwrap_or(0) / 48;
        assert_eq!(worst_ms, 0, "system audio offset by up to {} ms after a 500 ms pause", worst_ms);
    }

    /// Playback stops for 3 s (past the stall bound) mid-recording and
    /// resumes: stall padding plus the reported gap must land system audio
    /// back on the mic's timeline.
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn wasapi_replay_long_loopback_pause_realigns() {
        let mut d = mic_deliveries(20);
        d.extend(loopback_deliveries(0..6 * 48_000, |_| CHUNK));
        d.extend(loopback_deliveries(9 * 48_000 + 13 * CHUNK..20 * 48_000, |_| CHUNK));
        let gaps = replay_misalignment(d);
        let worst_ms = gaps.iter().max().copied().unwrap_or(0) / 48;
        assert_eq!(worst_ms, 0, "system audio offset by up to {} ms after a 3 s pause", worst_ms);
    }

    /// The mic stream starts 300 ms after system audio (slow device open):
    /// its first chunk is placed by timestamp, not at the buffer front.
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn wasapi_replay_late_mic_start_is_aligned() {
        let mut d: Vec<Delivery> = mic_deliveries(20).into_iter().filter(|m| m.start >= 30 * CHUNK).collect();
        d.extend(loopback_deliveries(0..20 * 48_000, |_| CHUNK));
        let gaps = replay_misalignment(d);
        assert!(gaps.iter().all(|g| *g == 0), "misaligned windows: {:?}", gaps);
    }

    #[test]
    fn capture_continuity_reports_skipped_time() {
        let mut c = CaptureContinuity::default();
        let dur = 0.01;
        // First callback: nothing to measure against.
        c.observe_secs(Some((0.0, 0.015)), dur);
        assert_eq!(c.take_gap(), None);
        // Contiguous packet, delivered late: no gap.
        c.observe_secs(Some((0.01, 0.05)), dur);
        assert_eq!(c.take_gap(), Some(0.0));
        // Loopback idle for 2 s, then a packet.
        c.observe_secs(Some((2.02, 2.03)), dur);
        let gap = c.take_gap().unwrap();
        assert!((gap - 2.0).abs() < 1e-9, "gap {}", gap);
    }

    #[test]
    fn capture_continuity_carries_gap_past_callbacks_that_sent_nothing() {
        let mut c = CaptureContinuity::default();
        c.observe_secs(Some((0.0, 0.01)), 0.01);
        assert_eq!(c.take_gap(), None);
        // A gap lands on a callback whose samples stay in the resampler...
        c.observe_secs(Some((0.51, 0.52)), 0.01);
        // ...and the next callback sends the chunk.
        c.observe_secs(Some((0.52, 0.53)), 0.01);
        let gap = c.take_gap().unwrap();
        assert!((gap - 0.5).abs() < 1e-9, "gap {}", gap);
    }

    #[test]
    fn capture_continuity_without_device_timing_assumes_continuous() {
        let mut c = CaptureContinuity::default();
        assert_eq!(c.observe_secs(None, 0.01), 0.0);
        assert_eq!(c.take_gap(), None);
        c.observe_secs(None, 0.01);
        assert_eq!(c.take_gap(), Some(0.0));
    }

    #[test]
    fn capture_continuity_reports_last_frame_age() {
        let mut c = CaptureContinuity::default();
        // First frame captured 25 ms before the callback; 10 ms of frames.
        let age = c.observe_secs(Some((1.0, 1.025)), 0.01);
        assert!((age - 0.015).abs() < 1e-9, "age {}", age);
    }

    /// A stream that stops delivering (unplugged device, idle WASAPI loopback)
    /// must not stall the mix: once the other stream is more than
    /// MAX_MIXER_LAG_WINDOWS ahead, mixing proceeds with the missing stream
    /// zero-padded, then keeps up window by window until the stream returns,
    /// after which windows wait for both streams again.
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn stalled_stream_is_padded_after_lag_bound_and_realigns_on_return() {
        let mut rb = AudioMixerRingBuffer::new(48_000);
        let window = rb.window_size_samples;
        let chunks_per_window = window / CHUNK;
        let (mut mic_out, mut sys_out) = (Vec::new(), Vec::new());
        let (mut mic_all, mut sys_all) = (Vec::new(), Vec::new());

        // Mic only. Nothing is mixed while the mic is within the lag bound.
        let bound_chunks = (MAX_MIXER_LAG_WINDOWS + 1) * chunks_per_window;
        for i in 0..bound_chunks {
            rb.add_samples(DeviceType::Microphone, vec![0.5; CHUNK]);
            let n = drain_ready_windows(&mut rb, &mut mic_out, &mut sys_out, &mut mic_all, &mut sys_all);
            if i + 1 < bound_chunks {
                assert_eq!(n, 0, "mixed early at mic chunk {}", i);
            }
        }
        // Reaching the bound releases the whole backlog, system padded.
        assert_eq!(mic_all.len(), (MAX_MIXER_LAG_WINDOWS + 1) * window);
        assert!(sys_all.iter().all(|s| *s == 0.0));

        // While system is stalled, each new mic window is mixed immediately.
        for _ in 0..chunks_per_window {
            rb.add_samples(DeviceType::Microphone, vec![0.5; CHUNK]);
            drain_ready_windows(&mut rb, &mut mic_out, &mut sys_out, &mut mic_all, &mut sys_all);
        }
        assert_eq!(mic_all.len(), (MAX_MIXER_LAG_WINDOWS + 2) * window);

        // System returns: windows wait for both streams again. Mic completes a
        // window one chunk before system; no zero may enter the system side.
        let sys_before = sys_all.len();
        for _ in 0..chunks_per_window - 1 {
            rb.add_samples(DeviceType::Microphone, vec![0.5; CHUNK]);
            rb.add_samples(DeviceType::System, vec![0.25; CHUNK]);
            drain_ready_windows(&mut rb, &mut mic_out, &mut sys_out, &mut mic_all, &mut sys_all);
        }
        rb.add_samples(DeviceType::Microphone, vec![0.5; CHUNK]);
        assert!(!rb.can_mix(), "mic must wait for the returning system stream");
        rb.add_samples(DeviceType::System, vec![0.25; CHUNK]);
        drain_ready_windows(&mut rb, &mut mic_out, &mut sys_out, &mut mic_all, &mut sys_all);
        assert_eq!(sys_all.len(), sys_before + window);
        assert!(sys_all[sys_before..].iter().all(|s| *s == 0.25));
    }

    /// Stop flush with a mic-only backlog longer than one window but still
    /// inside the lag bound: every sample reaches the recording, exactly once.
    #[cfg(not(target_os = "linux"))]
    #[tokio::test]
    async fn backlog_within_lag_bound_is_flushed_once_on_stop() {
        let state = RecordingState::new();
        let (audio_tx, audio_rx) = mpsc::unbounded_channel::<AudioChunk>();
        let (transcription_tx, _transcription_rx) = mpsc::unbounded_channel::<AudioChunk>();
        let mut pipeline = AudioPipeline::new(
            audio_rx,
            transcription_tx,
            state,
            0,
            48_000,
            "mic".to_string(),
            crate::audio::device_detection::InputDeviceKind::Wired,
            "system".to_string(),
            crate::audio::device_detection::InputDeviceKind::Wired,
            false,
        )
        .expect("pipeline construction");
        let (recording_tx, mut recording_rx) = mpsc::unbounded_channel::<AudioChunk>();
        pipeline.recording_sender_for_mixed = Some(recording_tx);

        // 2.5 windows of mic audio in 10 ms chunks; system never arrives.
        let total = 28_800 * 5 / 2;
        for i in 0..(total / CHUNK) {
            audio_tx
                .send(AudioChunk {
                    capture_gap: None,
                    data: vec![0.5f32; CHUNK],
                    sample_rate: 48_000,
                    timestamp: 0.0,
                    chunk_id: i as u64,
                    device_type: DeviceType::Microphone,
                    dominant_source: None,
                })
                .unwrap();
        }
        drop(audio_tx);

        pipeline.run().await.expect("pipeline run");

        let mut total_samples = 0usize;
        while let Ok(chunk) = recording_rx.try_recv() {
            total_samples += chunk.data.len();
        }
        assert_eq!(total_samples, total);
    }

    fn system_capture(state: &Arc<RecordingState>, sample_rate: u32) -> AudioCapture {
        let device = Arc::new(AudioDevice::new(
            "system".to_string(),
            crate::audio::devices::DeviceType::Output,
        ));
        AudioCapture::new(device, state.clone(), sample_rate, 1, DeviceType::System, None)
    }

    /// Feed `input_len` samples through `capture` in 1024-sample callbacks and
    /// return how many samples reached the pipeline and at what rate.
    fn captured_output(
        capture: &AudioCapture,
        rx: &mut mpsc::UnboundedReceiver<AudioChunk>,
        input_len: usize,
    ) -> (usize, u32) {
        for _ in 0..input_len / 1024 {
            capture.process_audio_data(&vec![0.1f32; 1024]);
        }
        let (mut total, mut rate) = (0usize, 0u32);
        while let Ok(chunk) = rx.try_recv() {
            total += chunk.data.len();
            rate = chunk.sample_rate;
        }
        (total, rate)
    }

    /// Core Audio's nominal rate changes mid-recording (Bluetooth headset
    /// switching to HFP). The capture must resample from the new rate, or
    /// system audio plays at the wrong pitch and drifts against the mic.
    #[test]
    fn input_rate_change_retunes_resampler() {
        let state = RecordingState::new();
        state.start_recording().unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel::<AudioChunk>();
        state.set_audio_sender(tx);

        let mut capture = system_capture(&state, 48_000);
        capture.set_input_sample_rate(16_000);
        // About one second at 16 kHz must come out as about one second at 48 kHz.
        let (total, rate) = captured_output(&capture, &mut rx, 16_384);
        assert_eq!(rate, 48_000);
        assert!((45_000..=50_000).contains(&total), "16 kHz second resampled to {} samples", total);

        capture.set_input_sample_rate(48_000);
        let (total, rate) = captured_output(&capture, &mut rx, 48_128);
        assert_eq!(rate, 48_000);
        assert_eq!(total, 48_128, "48 kHz input must pass through unresampled");
    }

    /// A capture stream that ends while still recording must be reported,
    /// not left to pad system audio with silence for the rest of the meeting.
    #[test]
    fn stream_end_while_recording_reports_stream_failed() {
        let state = RecordingState::new();
        let capture = system_capture(&state, 48_000);

        capture.handle_stream_end();
        assert!(state.get_last_error().is_none(), "an end after stop is not an error");

        state.start_recording().unwrap();
        capture.handle_stream_end();
        assert!(matches!(state.get_last_error(), Some(AudioError::StreamFailed)));
    }
}
