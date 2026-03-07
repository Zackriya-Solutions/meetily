// Speaker diarization using pyannote-rs
// Runs on the mixed recording audio after retranscription to assign speaker IDs
// to each transcript segment based on temporal overlap.

use anyhow::{anyhow, Result};
use log::{info, warn};
use pyannote_rs::{get_segments, EmbeddingExtractor, EmbeddingManager};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// Model filenames expected in the diarization models directory.
pub const SEGMENTATION_MODEL_FILE: &str = "segmentation-3.0.onnx";
pub const EMBEDDING_MODEL_FILE: &str = "wespeaker_en_voxceleb_CAM++.onnx";

/// Download URLs (hosted in the pyannote-rs GitHub releases).
const SEGMENTATION_URL: &str =
    "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/segmentation-3.0.onnx";
const EMBEDDING_URL: &str = "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/wespeaker_en_voxceleb_CAM%2B%2B.onnx";

/// Global diarization models directory (set once during app init).
static DIARIZATION_MODELS_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// A time-ranged speaker segment produced by diarization.
#[derive(Debug, Clone)]
pub struct DiarizationSegment {
    /// Speaker identifier, e.g. "Speaker 0", "Speaker 1".
    pub speaker_id: String,
    pub start_sec: f64,
    pub end_sec: f64,
}

/// Set (and create if necessary) the directory where diarization models are stored.
pub fn set_diarization_models_directory(app: &AppHandle) {
    match app.path().app_data_dir() {
        Ok(base) => {
            let dir = base.join("diarization_models");
            if !dir.exists() {
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    warn!("Could not create diarization models dir: {}", e);
                    return;
                }
            }
            let mut guard = DIARIZATION_MODELS_DIR.lock().unwrap();
            *guard = Some(dir.clone());
            info!("Diarization models directory: {}", dir.display());
        }
        Err(e) => warn!("Could not resolve app data dir for diarization: {}", e),
    }
}

fn models_dir() -> Option<PathBuf> {
    DIARIZATION_MODELS_DIR.lock().unwrap().clone()
}

/// Public accessor used by diarization_commands.
pub fn models_dir_pub() -> Option<PathBuf> {
    models_dir()
}

/// Returns true if both ONNX model files are present and non-empty.
pub fn diarization_models_available() -> bool {
    let Some(dir) = models_dir() else { return false };
    let seg = dir.join(SEGMENTATION_MODEL_FILE);
    let emb = dir.join(EMBEDDING_MODEL_FILE);
    seg.exists()
        && seg.metadata().map(|m| m.len() > 0).unwrap_or(false)
        && emb.exists()
        && emb.metadata().map(|m| m.len() > 0).unwrap_or(false)
}

/// Download one of the two diarization model files, emitting progress events.
///
/// `model_type` must be `"segmentation"` or `"embedding"`.
pub async fn download_diarization_model<R: Runtime>(
    app: AppHandle<R>,
    model_type: &str,
) -> Result<()> {
    let (url, filename, event_model_name) = match model_type {
        "segmentation" => (SEGMENTATION_URL, SEGMENTATION_MODEL_FILE, "segmentation-3.0"),
        "embedding" => (EMBEDDING_URL, EMBEDDING_MODEL_FILE, "wespeaker-CAM++"),
        other => return Err(anyhow!("Unknown diarization model type: {}", other)),
    };

    let dir = models_dir().ok_or_else(|| anyhow!("Diarization models directory not set"))?;
    let dest = dir.join(filename);

    info!("Downloading diarization model '{}' from {}", event_model_name, url);

    let client = reqwest::Client::new();
    let response = client.get(url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "HTTP {} when downloading {}",
            response.status(),
            url
        ));
    }

    let total_bytes = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();

    // Write to a temp file then rename atomically.
    let tmp_path = dest.with_extension("tmp");
    let mut file = tokio::fs::File::create(&tmp_path).await?;

    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let start = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        downloaded += chunk.len() as u64;
        file.write_all(&chunk).await?;

        let elapsed = start.elapsed().as_secs_f64().max(0.001);
        let speed_mbps = (downloaded as f64 / 1_048_576.0) / elapsed;
        let percent = if total_bytes > 0 {
            ((downloaded as f64 / total_bytes as f64) * 100.0) as u32
        } else {
            0
        };

        let _ = app.emit(
            "diarization-download-progress",
            serde_json::json!({
                "modelType": model_type,
                "modelName": event_model_name,
                "progress": percent,
                "downloaded_bytes": downloaded,
                "total_bytes": total_bytes,
                "downloaded_mb": downloaded as f64 / 1_048_576.0,
                "total_mb": total_bytes as f64 / 1_048_576.0,
                "speed_mbps": speed_mbps,
                "status": if percent == 100 { "completed" } else { "downloading" }
            }),
        );
    }

    file.flush().await?;
    drop(file);
    tokio::fs::rename(&tmp_path, &dest).await?;

    let _ = app.emit(
        "diarization-download-complete",
        serde_json::json!({ "modelType": model_type, "modelName": event_model_name }),
    );

    info!("Diarization model '{}' downloaded to {}", event_model_name, dest.display());
    Ok(())
}

/// Run speaker diarization on 16 kHz mono f32 audio.
/// Returns a sorted list of `DiarizationSegment` with speaker labels.
///
/// This is CPU-intensive and should be called inside `spawn_blocking`.
pub fn run_diarization(
    samples_16khz: &[f32],
    max_speakers: usize,
    similarity_threshold: f32,
) -> Result<Vec<DiarizationSegment>> {
    let dir = models_dir().ok_or_else(|| anyhow!("Diarization models directory not set"))?;
    let seg_path = dir.join(SEGMENTATION_MODEL_FILE);
    let emb_path = dir.join(EMBEDDING_MODEL_FILE);

    if !seg_path.exists() || !emb_path.exists() {
        return Err(anyhow!("Diarization model files not found"));
    }

    let seg_path_str = seg_path.to_string_lossy().to_string();
    let emb_path_str = emb_path.to_string_lossy().to_string();

    // pyannote-rs expects 16 kHz mono i16 samples.
    let samples_i16: Vec<i16> = samples_16khz
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect();

    let sample_rate = 16000u32;

    let mut extractor = EmbeddingExtractor::new(&emb_path_str)
        .map_err(|e| anyhow!("Failed to load embedding model: {:?}", e))?;
    let mut manager = EmbeddingManager::new(max_speakers);

    let segments = get_segments(&samples_i16, sample_rate, &seg_path_str)
        .map_err(|e| anyhow!("Segmentation failed: {:?}", e))?;

    let mut result: Vec<DiarizationSegment> = Vec::new();

    for segment in segments {
        match segment {
            Ok(seg) => {
                match extractor.compute(&seg.samples) {
                    Ok(embedding) => {
                        let speaker_idx = manager
                            .search_speaker(embedding.collect(), similarity_threshold)
                            .unwrap_or_else(|| {
                                manager.get_all_speakers().len().saturating_sub(1)
                            });
                        result.push(DiarizationSegment {
                            speaker_id: format!("speaker_{}", speaker_idx),
                            start_sec: seg.start,
                            end_sec: seg.end,
                        });
                    }
                    Err(e) => warn!("Embedding extraction failed for segment: {:?}", e),
                }
            }
            Err(e) => warn!("Diarization segment error: {:?}", e),
        }
    }

    info!(
        "Diarization complete: {} segments, {} speakers",
        result.len(),
        manager.get_all_speakers().len()
    );

    Ok(result)
}

/// Given a list of transcript `(text, start_ms, end_ms)` tuples and diarization
/// segments, return the best matching speaker ID for each transcript entry.
/// Returns `None` where no overlap is found.
pub fn assign_speakers_to_transcripts(
    transcripts: &[(String, f64, f64)],
    diarization: &[DiarizationSegment],
) -> Vec<Option<String>> {
    transcripts
        .iter()
        .map(|(_, start_ms, end_ms)| {
            let t_start = start_ms / 1000.0;
            let t_end = end_ms / 1000.0;
            let t_len = (t_end - t_start).max(1e-6);

            // Find the diarization segment with maximum overlap fraction.
            diarization
                .iter()
                .filter_map(|d| {
                    let overlap_start = t_start.max(d.start_sec);
                    let overlap_end = t_end.min(d.end_sec);
                    if overlap_end > overlap_start {
                        Some(((overlap_end - overlap_start) / t_len, &d.speaker_id))
                    } else {
                        None
                    }
                })
                .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(_, id)| id.clone())
        })
        .collect()
}


