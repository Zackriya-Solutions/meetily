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

/// Latencia de un Segmento (Q31): tiempo de reloj desde que terminó el audio
/// (`audio_end_s`, relativo al inicio del pipeline) hasta `now`.
pub fn segment_latency_ms(pipeline_start: std::time::Instant, now: std::time::Instant, audio_end_s: f64) -> f64 {
    now.duration_since(pipeline_start).as_secs_f64() * 1000.0 - audio_end_s * 1000.0
}

static PIPELINE_START: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// El pipeline marca su inicio; los tiempos de audio de los Segmentos son relativos a él.
pub fn mark_pipeline_start() {
    *PIPELINE_START.lock().unwrap() = Some(std::time::Instant::now());
}

/// Latencia del Segmento que termina en `audio_end_s`, si el pipeline ya marcó su inicio.
pub fn current_segment_latency_ms(audio_end_s: f64) -> Option<f64> {
    let start = (*PIPELINE_START.lock().unwrap())?;
    Some(segment_latency_ms(start, std::time::Instant::now(), audio_end_s))
}

/// Mínimo de muestras para mandar un fragmento a transcribir (50 ms a 16 kHz), igual que pipeline.rs.
pub const MIN_SEGMENT_SAMPLES: usize = 800;

/// Duración máxima de un Segmento en vivo (issue #756 de Meetily).
pub const MAX_SEGMENT_SECONDS: f64 = 10.0;

fn clamp_to_vad_range(samples: &[f32]) -> Vec<f32> {
    samples.iter().map(|s| s.clamp(-1.0, 1.0)).collect()
}

pub trait SpeechSegmenter {
    fn process(&mut self, samples: &[f32]) -> Result<Vec<SpeechSegment>>;
    fn flush(&mut self) -> Result<Vec<SpeechSegment>>;
    /// Habla aún sin cerrar: inicio en ms y muestras a 16 kHz desde ese inicio.
    fn active_speech(&self) -> Option<(f64, &[f32])> {
        None
    }
}

impl SpeechSegmenter for ContinuousVadProcessor {
    fn process(&mut self, samples: &[f32]) -> Result<Vec<SpeechSegment>> {
        self.process_audio(samples)
    }

    fn flush(&mut self) -> Result<Vec<SpeechSegment>> {
        ContinuousVadProcessor::flush(self)
    }

    fn active_speech(&self) -> Option<(f64, &[f32])> {
        ContinuousVadProcessor::active_speech(self)
    }
}

const SAMPLES_PER_MS: f64 = 16.0;
const MAX_SEGMENT_SAMPLES: usize = (MAX_SEGMENT_SECONDS * 1000.0 * SAMPLES_PER_MS) as usize;

/// Corta la habla en curso cada `MAX_SEGMENT_SECONDS` sin tocar el estado del VAD:
/// recuerda cuántas muestras de la habla actual ya salió y las quita del Segmento que cierra.
struct BoundedSegmenter<S: SpeechSegmenter> {
    inner: S,
    emitted: usize,
}

impl<S: SpeechSegmenter> BoundedSegmenter<S> {
    fn new(inner: S) -> Self {
        Self { inner, emitted: 0 }
    }

    fn process(&mut self, samples: &[f32]) -> Result<Vec<SpeechSegment>> {
        let closed = self.inner.process(samples)?;
        let mut out = self.trim_closed(closed);
        out.extend(self.cut_active());
        Ok(out)
    }

    fn flush(&mut self) -> Result<Vec<SpeechSegment>> {
        let closed = self.inner.flush()?;
        Ok(self.trim_closed(closed))
    }

    /// El primer Segmento que cierra es la habla que ya se fue cortando: sale solo lo que falta.
    fn trim_closed(&mut self, mut closed: Vec<SpeechSegment>) -> Vec<SpeechSegment> {
        if let Some(first) = closed.first_mut() {
            let skip = std::mem::take(&mut self.emitted).min(first.samples.len());
            first.samples.drain(..skip);
            first.start_timestamp_ms += skip as f64 / SAMPLES_PER_MS;
        }
        closed
    }

    fn cut_active(&mut self) -> Vec<SpeechSegment> {
        let mut out = Vec::new();
        if let Some((start_ms, speech)) = self.inner.active_speech() {
            while speech.len() - self.emitted.min(speech.len()) >= MAX_SEGMENT_SAMPLES {
                let from = self.emitted;
                let to = from + MAX_SEGMENT_SAMPLES;
                out.push(SpeechSegment {
                    samples: speech[from..to].to_vec(),
                    start_timestamp_ms: start_ms + from as f64 / SAMPLES_PER_MS,
                    end_timestamp_ms: start_ms + to as f64 / SAMPLES_PER_MS,
                    confidence: 0.9,
                });
                self.emitted = to;
            }
        }
        out
    }
}

pub struct SpeakerSplitter<S: SpeechSegmenter> {
    mic: BoundedSegmenter<S>,
    system: BoundedSegmenter<S>,
    next_chunk_id: u64,
}

impl<S: SpeechSegmenter> SpeakerSplitter<S> {
    pub fn new(mic: S, system: S, first_chunk_id: u64) -> Self {
        Self { mic: BoundedSegmenter::new(mic), system: BoundedSegmenter::new(system), next_chunk_id: first_chunk_id }
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
        assert_eq!(splitter.mic.inner.seen, vec![vec![0.1, 0.2]]);
        assert_eq!(splitter.system.inner.seen, vec![vec![0.3]]);
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
        assert_eq!(splitter.mic.inner.seen, vec![vec![1.0, -1.0, 0.5]]);
        assert_eq!(splitter.system.inner.seen, vec![vec![1.0]]);
    }

    fn fixture_voice() -> Vec<f32> {
        let bytes = include_bytes!("../../tests/fixtures/sottoly/voz-sintetica-16k.s16le");
        bytes
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / i16::MAX as f32)
            .collect()
    }

    /// El fixture no tiene pausas de más de ~200 ms: repetido 8 veces son ~31 s de habla sin un
    /// silencio que el VAD (redención de 500 ms) acepte como cierre.
    fn continuous_voice() -> Vec<f32> {
        let voice = fixture_voice();
        voice.iter().copied().cycle().take(voice.len() * 8).collect()
    }

    /// Pasa los dos flujos por un SpeakerSplitter con el VAD real (Silero), en ventanas de 50 ms.
    fn split_with_real_vad(mic: &[f32], system: &[f32]) -> Vec<AudioChunk> {
        let new_vad = || ContinuousVadProcessor::new(16000, 500).unwrap();
        let mut splitter = SpeakerSplitter::new(new_vad(), new_vad(), 0);
        let mut chunks = Vec::new();
        for (mic, sys) in mic.chunks(800).zip(system.chunks(800)) {
            chunks.extend(splitter.process(mic, sys).expect("el VAD rechazó la ventana"));
        }
        chunks.extend(splitter.flush().unwrap());
        chunks
    }

    fn duration_s(chunk: &AudioChunk) -> f64 {
        chunk.data.len() as f64 / chunk.sample_rate as f64
    }

    #[test]
    fn real_vad_accepts_loud_audio_on_both_streams() {
        let loud: Vec<f32> = fixture_voice().iter().map(|s| 3.0 * s).collect();
        let chunks = split_with_real_vad(&loud, &loud);
        assert!(chunks.iter().any(|c| matches!(c.device_type, DeviceType::Microphone)));
        assert!(chunks.iter().any(|c| matches!(c.device_type, DeviceType::System)));
    }

    /// Issue #756 de Meetily: con habla continua el VAD no cierra nunca y entregó un Segmento
    /// de 41 s en la App real. El Motor queda ciego todo ese tiempo.
    #[test]
    fn continuous_speech_is_cut_into_segments_of_at_most_10_seconds() {
        let continuous = continuous_voice();
        let chunks = split_with_real_vad(&vec![0.0; continuous.len()], &continuous);

        let durations: Vec<f64> = chunks.iter().map(duration_s).collect();
        let total: f64 = durations.iter().sum();
        assert!(total > 25.0 && total < 32.0, "se perdió o se duplicó habla: {durations:?}");
        assert!(
            durations.iter().all(|d| *d <= MAX_SEGMENT_SECONDS),
            "Segmentos de más de {MAX_SEGMENT_SECONDS} s: {durations:?}"
        );
        // Los Segmentos acotados llegan mientras se habla, no todos al final.
        assert!(chunks.len() >= 3, "{durations:?}");
    }

    /// Los cortes no pierden ni duplican audio, y cada Segmento conserva su hora de inicio.
    #[test]
    fn cut_segments_are_contiguous_and_keep_their_start_time() {
        let continuous = continuous_voice();
        let chunks = split_with_real_vad(&continuous, &vec![0.0; continuous.len()]);

        assert!(chunks.len() >= 3);
        for pair in chunks.windows(2) {
            let expected_start = pair[0].timestamp + duration_s(&pair[0]);
            assert!(
                (pair[1].timestamp - expected_start).abs() < 0.001,
                "hueco o solape entre Segmentos: {} + {} != {}",
                pair[0].timestamp,
                duration_s(&pair[0]),
                pair[1].timestamp
            );
        }
    }

    #[test]
    fn segment_latency_is_wall_clock_minus_audio_end() {
        let start = std::time::Instant::now();
        let now = start + std::time::Duration::from_millis(5_400);
        // El Segmento terminó en el segundo 4.6 del audio y llegó en el 5.4 del reloj.
        let latency = segment_latency_ms(start, now, 4.6);
        assert!((latency - 800.0).abs() < 0.001, "latency = {latency}");
    }

    /// Con el VAD real (Silero): voz sintética solo por el micrófono → solo fragmentos del Usuario.
    #[test]
    fn real_vad_attributes_speech_to_the_stream_that_carries_it() {
        let voice = fixture_voice();
        let silence = vec![0.0f32; voice.len()];

        let chunks = split_with_real_vad(&voice, &silence);
        assert!(!chunks.is_empty(), "el VAD no detectó la voz sintética");
        assert!(chunks.iter().all(|c| matches!(c.device_type, DeviceType::Microphone)));

        // Mismo audio por el sistema → solo Contraparte.
        let chunks = split_with_real_vad(&silence, &voice);
        assert!(!chunks.is_empty());
        assert!(chunks.iter().all(|c| matches!(c.device_type, DeviceType::System)));
    }
}
