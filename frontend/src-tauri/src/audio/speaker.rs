// SOTTOLY: separa Usuario (micrófono) y Contraparte (audio del sistema) antes de transcribir.
// Cada flujo pasa por su propio VAD; los fragmentos salen etiquetados con su DeviceType.
// Diarización gratis por canal (SPEC §10, Fase 1). Archivo nuevo para no tocar Meetily (ADR-0001).

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::recording_state::{AudioChunk, DeviceType};
use super::vad::{ContinuousVadProcessor, SpeechSegment};

/// Quién habla en un Segmento. Se serializa como en el protocolo del Motor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    User,
    Counterpart,
    Mixed,
}

impl Speaker {
    pub fn from_device(device: &DeviceType) -> Self {
        match device {
            DeviceType::Microphone => Speaker::User,
            DeviceType::System => Speaker::Counterpart,
        }
    }
}

/// Hablante que va en el TranscriptUpdate: si los flujos no se separaron, todo es "mixed".
pub fn transcript_speaker(device: &DeviceType, separated: bool) -> Speaker {
    if separated {
        Speaker::from_device(device)
    } else {
        Speaker::Mixed
    }
}

/// El pipeline lo enciende cuando transcribe los flujos por separado (Fase 1C).
static SEPARATION_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_separation_enabled(enabled: bool) {
    SEPARATION_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn separation_enabled() -> bool {
    SEPARATION_ENABLED.load(Ordering::SeqCst)
}

/// Mínimo de muestras para mandar un fragmento a transcribir (50 ms a 16 kHz), igual que pipeline.rs.
pub const MIN_SEGMENT_SAMPLES: usize = 800;

fn clamp_to_vad_range(samples: &[f32]) -> Vec<f32> {
    samples.iter().map(|s| s.clamp(-1.0, 1.0)).collect()
}

pub trait SpeechSegmenter {
    fn process(&mut self, samples: &[f32]) -> Result<Vec<SpeechSegment>>;
    fn flush(&mut self) -> Result<Vec<SpeechSegment>>;
}

impl SpeechSegmenter for ContinuousVadProcessor {
    fn process(&mut self, samples: &[f32]) -> Result<Vec<SpeechSegment>> {
        self.process_audio(samples)
    }

    fn flush(&mut self) -> Result<Vec<SpeechSegment>> {
        ContinuousVadProcessor::flush(self)
    }
}

pub struct SpeakerSplitter<S: SpeechSegmenter> {
    mic: S,
    system: S,
    next_chunk_id: u64,
}

impl<S: SpeechSegmenter> SpeakerSplitter<S> {
    pub fn new(mic: S, system: S, first_chunk_id: u64) -> Self {
        Self { mic, system, next_chunk_id: first_chunk_id }
    }

    /// Procesa una ventana de cada flujo y devuelve los fragmentos listos para transcribir.
    pub fn process(&mut self, mic_window: &[f32], system_window: &[f32]) -> Result<Vec<AudioChunk>> {
        // Sin el mezclador de Meetily nadie recorta el audio, y Silero rechaza muestras fuera de [-1, 1].
        let mic = self.mic.process(&clamp_to_vad_range(mic_window))?;
        let system = self.system.process(&clamp_to_vad_range(system_window))?;
        Ok(self.tag_segments(mic, system))
    }

    pub fn flush(&mut self) -> Result<Vec<AudioChunk>> {
        let mic = self.mic.flush()?;
        let system = self.system.flush()?;
        Ok(self.tag_segments(mic, system))
    }

    fn tag_segments(&mut self, mic: Vec<SpeechSegment>, system: Vec<SpeechSegment>) -> Vec<AudioChunk> {
        let tagged = mic
            .into_iter()
            .map(|s| (DeviceType::Microphone, s))
            .chain(system.into_iter().map(|s| (DeviceType::System, s)));

        let mut chunks = Vec::new();
        for (device_type, segment) in tagged {
            if segment.samples.len() < MIN_SEGMENT_SAMPLES {
                continue;
            }
            chunks.push(AudioChunk {
                data: segment.samples,
                sample_rate: 16000,
                timestamp: segment.start_timestamp_ms / 1000.0,
                chunk_id: self.next_chunk_id,
                device_type,
            });
            self.next_chunk_id += 1;
        }
        chunks
    }

    pub fn next_chunk_id(&self) -> u64 {
        self.next_chunk_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Segmentador falso: devuelve los segmentos programados y registra qué audio recibió.
    #[derive(Default)]
    struct FakeSegmenter {
        queued: VecDeque<Vec<SpeechSegment>>,
        on_flush: Vec<SpeechSegment>,
        seen: Vec<Vec<f32>>,
    }

    impl SpeechSegmenter for FakeSegmenter {
        fn process(&mut self, samples: &[f32]) -> Result<Vec<SpeechSegment>> {
            self.seen.push(samples.to_vec());
            Ok(self.queued.pop_front().unwrap_or_default())
        }
        fn flush(&mut self) -> Result<Vec<SpeechSegment>> {
            Ok(std::mem::take(&mut self.on_flush))
        }
    }

    fn segment(samples: usize, start_ms: f64) -> SpeechSegment {
        SpeechSegment {
            samples: vec![0.1; samples],
            start_timestamp_ms: start_ms,
            end_timestamp_ms: start_ms + samples as f64 / 16.0,
            confidence: 0.9,
        }
    }

    #[test]
    fn microphone_is_user_and_system_is_counterpart() {
        assert_eq!(Speaker::from_device(&DeviceType::Microphone), Speaker::User);
        assert_eq!(Speaker::from_device(&DeviceType::System), Speaker::Counterpart);
    }

    #[test]
    fn speaker_serializes_like_the_engine_protocol() {
        assert_eq!(serde_json::to_string(&Speaker::User).unwrap(), "\"user\"");
        assert_eq!(serde_json::to_string(&Speaker::Counterpart).unwrap(), "\"counterpart\"");
        assert_eq!(serde_json::to_string(&Speaker::Mixed).unwrap(), "\"mixed\"");
    }

    #[test]
    fn each_stream_goes_to_its_own_segmenter() {
        let mut splitter = SpeakerSplitter::new(FakeSegmenter::default(), FakeSegmenter::default(), 0);
        splitter.process(&[0.1, 0.2], &[0.3]).unwrap();
        assert_eq!(splitter.mic.seen, vec![vec![0.1, 0.2]]);
        assert_eq!(splitter.system.seen, vec![vec![0.3]]);
    }

    #[test]
    fn segments_are_tagged_with_their_source() {
        let mut mic = FakeSegmenter::default();
        mic.queued.push_back(vec![segment(1600, 1000.0)]);
        let mut system = FakeSegmenter::default();
        system.queued.push_back(vec![segment(3200, 2500.0)]);

        let chunks = SpeakerSplitter::new(mic, system, 7).process(&[0.0], &[0.0]).unwrap();

        assert_eq!(chunks.len(), 2);
        assert!(matches!(chunks[0].device_type, DeviceType::Microphone));
        assert_eq!(chunks[0].timestamp, 1.0);
        assert_eq!(chunks[0].chunk_id, 7);
        assert!(matches!(chunks[1].device_type, DeviceType::System));
        assert_eq!(chunks[1].timestamp, 2.5);
        assert_eq!(chunks[1].chunk_id, 8);
        assert!(chunks.iter().all(|c| c.sample_rate == 16000));
    }

    #[test]
    fn drops_segments_shorter_than_50ms() {
        let mut mic = FakeSegmenter::default();
        mic.queued.push_back(vec![segment(MIN_SEGMENT_SAMPLES - 1, 0.0), segment(MIN_SEGMENT_SAMPLES, 100.0)]);
        let mut splitter = SpeakerSplitter::new(mic, FakeSegmenter::default(), 0);

        let chunks = splitter.process(&[0.0], &[0.0]).unwrap();

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].data.len(), MIN_SEGMENT_SAMPLES);
        assert_eq!(splitter.next_chunk_id(), 1);
    }

    #[test]
    fn flush_drains_both_streams() {
        let mut mic = FakeSegmenter::default();
        mic.on_flush = vec![segment(1600, 0.0)];
        let mut system = FakeSegmenter::default();
        system.on_flush = vec![segment(1600, 0.0)];

        let chunks = SpeakerSplitter::new(mic, system, 0).flush().unwrap();

        let sources: Vec<Speaker> = chunks.iter().map(|c| Speaker::from_device(&c.device_type)).collect();
        assert_eq!(sources, vec![Speaker::User, Speaker::Counterpart]);
    }

    #[test]
    fn transcript_speaker_is_mixed_when_streams_are_not_separated() {
        assert_eq!(transcript_speaker(&DeviceType::Microphone, false), Speaker::Mixed);
        assert_eq!(transcript_speaker(&DeviceType::System, false), Speaker::Mixed);
    }

    #[test]
    fn transcript_speaker_follows_the_device_when_separated() {
        assert_eq!(transcript_speaker(&DeviceType::Microphone, true), Speaker::User);
        assert_eq!(transcript_speaker(&DeviceType::System, true), Speaker::Counterpart);
    }

    // Bug visto en la App real: el micrófono normalizado y el audio del sistema pasan de 1.0
    // y Silero rechaza la ventana entera ("Float sample must be in the range -1.0 to 1.0").
    #[test]
    fn samples_outside_the_vad_range_are_clamped_before_segmenting() {
        let mut splitter = SpeakerSplitter::new(FakeSegmenter::default(), FakeSegmenter::default(), 0);
        splitter.process(&[1.7, -2.0, 0.5], &[3.0]).unwrap();
        assert_eq!(splitter.mic.seen, vec![vec![1.0, -1.0, 0.5]]);
        assert_eq!(splitter.system.seen, vec![vec![1.0]]);
    }

    #[test]
    fn real_vad_accepts_loud_audio_on_both_streams() {
        let bytes = include_bytes!("../../tests/fixtures/sottoly/voz-sintetica-16k.s16le");
        let loud: Vec<f32> = bytes
            .chunks_exact(2)
            .map(|b| 3.0 * i16::from_le_bytes([b[0], b[1]]) as f32 / i16::MAX as f32)
            .collect();
        let new_vad = || ContinuousVadProcessor::new(16000, 500).unwrap();
        let mut splitter = SpeakerSplitter::new(new_vad(), new_vad(), 0);
        let mut chunks = Vec::new();
        for (mic, sys) in loud.chunks(800).zip(loud.chunks(800)) {
            chunks.extend(splitter.process(mic, sys).expect("el VAD rechazó audio fuera de rango"));
        }
        chunks.extend(splitter.flush().unwrap());
        assert!(chunks.iter().any(|c| matches!(c.device_type, DeviceType::Microphone)));
        assert!(chunks.iter().any(|c| matches!(c.device_type, DeviceType::System)));
    }

    /// Con el VAD real (Silero): voz sintética solo por el micrófono → solo fragmentos del Usuario.
    #[test]
    fn real_vad_attributes_speech_to_the_stream_that_carries_it() {
        let bytes = include_bytes!("../../tests/fixtures/sottoly/voz-sintetica-16k.s16le");
        let voice: Vec<f32> = bytes
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / i16::MAX as f32)
            .collect();
        let silence = vec![0.0f32; voice.len()];

        let new_vad = || ContinuousVadProcessor::new(16000, 500).unwrap();
        let mut splitter = SpeakerSplitter::new(new_vad(), new_vad(), 0);

        let mut chunks = Vec::new();
        for (mic, sys) in voice.chunks(800).zip(silence.chunks(800)) {
            chunks.extend(splitter.process(mic, sys).unwrap());
        }
        chunks.extend(splitter.flush().unwrap());

        assert!(!chunks.is_empty(), "el VAD no detectó la voz sintética");
        assert!(chunks.iter().all(|c| matches!(c.device_type, DeviceType::Microphone)));

        // Mismo audio por el sistema → solo Contraparte.
        let mut splitter = SpeakerSplitter::new(new_vad(), new_vad(), 0);
        let mut chunks = Vec::new();
        for (mic, sys) in silence.chunks(800).zip(voice.chunks(800)) {
            chunks.extend(splitter.process(mic, sys).unwrap());
        }
        chunks.extend(splitter.flush().unwrap());
        assert!(!chunks.is_empty());
        assert!(chunks.iter().all(|c| matches!(c.device_type, DeviceType::System)));
    }
}
