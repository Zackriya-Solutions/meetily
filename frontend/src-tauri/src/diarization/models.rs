// diarization/models.rs
//
// Model location and download for speaker identification.
// Mirrors the parakeet_engine download pattern: stream from a stable URL
// into <app_data>/models/diarization/, .tmp + rename for atomicity,
// progress emitted as Tauri events.

use futures_util::StreamExt;
use std::io::Read;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// WeSpeaker CAM++ speaker-embedding model (Apache-2.0, exported to ONNX by
/// the sherpa-onnx project). ~28 MB. Input: fbank [1, T, 80]; output: [1, 192].
/// NOTE: "recongition" is the canonical (misspelled) sherpa-onnx release tag.
pub const EMBEDDING_MODEL_FILENAME: &str = "wespeaker_en_voxceleb_CAM++.onnx";
pub const EMBEDDING_MODEL_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_CAM%2B%2B.onnx";

/// Pyannote segmentation-3.0 model (CC-BY-4.0, exported to ONNX by sherpa-onnx).
/// ~6 MB tarball containing model.onnx. Input: raw 16kHz audio [1, 1, 160000];
/// output: [1, T, 7] logits over {none, spk1, spk2, spk3, spk1+2, spk1+3, spk2+3}.
/// Downloaded as tarball, extracted to get model.onnx.
pub const SEGMENTATION_MODEL_FILENAME: &str = "pyannote_segmentation_3_0.onnx";
const SEGMENTATION_MODEL_TARBALL_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2";

/// WeSpeaker embedding model for offline diarization.
/// ~95 MB. Input: fbank [1, T, 80]; output: [1, 256].
/// ResNet221 variant with LM (language modeling) for improved robustness.
/// NOTE: spec called for "SimAMResNet34-VoxBlink2" (multilingual); substituted
/// wespeaker_en_voxceleb_resnet221_LM.onnx (verified release asset, closest available tier).
/// Known gap: this model is EN-only (VoxCeleb), not truly multilingual. Follow-up needed
/// for genuinely multilingual sherpa-onnx embedding model. Fallback: resnet152_LM (79MB).
pub const EMBEDDING_MODEL_V2_FILENAME: &str = "wespeaker_en_voxceleb_resnet221_LM.onnx";
const EMBEDDING_MODEL_V2_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_resnet221_LM.onnx";

pub fn models_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    Ok(app_data_dir.join("models").join("diarization"))
}

pub fn embedding_model_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(models_dir(app)?.join(EMBEDDING_MODEL_FILENAME))
}

pub fn segmentation_model_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(models_dir(app)?.join(SEGMENTATION_MODEL_FILENAME))
}

pub fn embedding_model_v2_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(models_dir(app)?.join(EMBEDDING_MODEL_V2_FILENAME))
}

pub fn is_embedding_model_present<R: Runtime>(app: &AppHandle<R>) -> bool {
    embedding_model_path(app)
        .map(|p| p.exists() && std::fs::metadata(&p).map(|m| m.len() > 1_000_000).unwrap_or(false))
        .unwrap_or(false)
}

pub fn is_segmentation_model_present<R: Runtime>(app: &AppHandle<R>) -> bool {
    segmentation_model_path(app)
        .map(|p| p.exists() && std::fs::metadata(&p).map(|m| m.len() > 1_000_000).unwrap_or(false))
        .unwrap_or(false)
}

pub fn is_embedding_model_v2_present<R: Runtime>(app: &AppHandle<R>) -> bool {
    embedding_model_v2_path(app)
        .map(|p| p.exists() && std::fs::metadata(&p).map(|m| m.len() > 10_000_000).unwrap_or(false))
        .unwrap_or(false)
}

/// Offline diarization requires both segmentation and V2 embedding models.
pub fn is_offline_diarization_present<R: Runtime>(app: &AppHandle<R>) -> bool {
    is_segmentation_model_present(app) && is_embedding_model_v2_present(app)
}

/// Download the embedding model, emitting `diarization-model-download-progress`
/// events with { downloaded_bytes, total_bytes, percent }.
pub async fn download_embedding_model<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let dir = models_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create models dir: {}", e))?;

    let final_path = dir.join(EMBEDDING_MODEL_FILENAME);
    if is_embedding_model_present(app) {
        log::info!("Diarization embedding model already present at {}", final_path.display());
        return Ok(());
    }
    let tmp_path = dir.join(format!("{}.tmp", EMBEDDING_MODEL_FILENAME));

    log::info!("Downloading diarization embedding model from {}", EMBEDDING_MODEL_URL);
    let client = reqwest::Client::new();
    let response = client
        .get(EMBEDDING_MODEL_URL)
        .send()
        .await
        .map_err(|e| format!("Download request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed with HTTP {}", response.status()));
    }

    let total_bytes = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut last_emitted_percent: i64 = -1;

    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download stream error: {}", e))?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|e| format!("Failed to write model file: {}", e))?;
        downloaded += chunk.len() as u64;

        let percent = if total_bytes > 0 {
            (downloaded * 100 / total_bytes) as i64
        } else {
            0
        };
        if percent != last_emitted_percent {
            last_emitted_percent = percent;
            let _ = app.emit(
                "diarization-model-download-progress",
                serde_json::json!({
                    "downloaded_bytes": downloaded,
                    "total_bytes": total_bytes,
                    "percent": percent,
                }),
            );
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(|e| format!("Failed to flush model file: {}", e))?;
    drop(file);

    std::fs::rename(&tmp_path, &final_path)
        .map_err(|e| format!("Failed to finalize model file: {}", e))?;

    log::info!(
        "Diarization embedding model downloaded to {} ({} bytes)",
        final_path.display(),
        downloaded
    );
    Ok(())
}

/// Download the segmentation model (tarball), extract model.onnx, and save it.
/// Emits `diarization-model-download-progress` events during download.
pub async fn download_segmentation_model<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let dir = models_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create models dir: {}", e))?;

    let final_path = dir.join(SEGMENTATION_MODEL_FILENAME);
    if is_segmentation_model_present(app) {
        log::info!("Diarization segmentation model already present at {}", final_path.display());
        return Ok(());
    }
    let tmp_path = dir.join(format!("{}.tmp", SEGMENTATION_MODEL_FILENAME));

    log::info!("Downloading diarization segmentation model from {}", SEGMENTATION_MODEL_TARBALL_URL);
    let client = reqwest::Client::new();
    let response = client
        .get(SEGMENTATION_MODEL_TARBALL_URL)
        .send()
        .await
        .map_err(|e| format!("Download request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed with HTTP {}", response.status()));
    }

    let total_bytes = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut last_emitted_percent: i64 = -1;
    let mut tar_buffer = Vec::new();

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download stream error: {}", e))?;
        tar_buffer.extend_from_slice(&chunk);
        downloaded += chunk.len() as u64;

        let percent = if total_bytes > 0 {
            (downloaded * 100 / total_bytes) as i64
        } else {
            0
        };
        if percent != last_emitted_percent {
            last_emitted_percent = percent;
            let _ = app.emit(
                "diarization-model-download-progress",
                serde_json::json!({
                    "downloaded_bytes": downloaded,
                    "total_bytes": total_bytes,
                    "percent": percent,
                }),
            );
        }
    }

    // Extract model.onnx from the tarball (bz2 compressed)
    log::info!("Extracting model.onnx from tarball ({} bytes)", tar_buffer.len());
    let tar_cursor = std::io::Cursor::new(tar_buffer);
    let decompressed = bzip2::read::BzDecoder::new(tar_cursor);
    let mut tar = tar::Archive::new(decompressed);

    let mut model_found = false;
    for entry in tar
        .entries()
        .map_err(|e| format!("Failed to read tar entries: {}", e))?
    {
        let mut entry = entry.map_err(|e| format!("Failed to read tar entry: {}", e))?;
        let path = entry
            .path()
            .map_err(|e| format!("Failed to get tar entry path: {}", e))?
            .to_path_buf();

        if path.file_name().map_or(false, |n| n == "model.onnx") {
            let mut onnx_data = Vec::new();
            entry
                .read_to_end(&mut onnx_data)
                .map_err(|e| format!("Failed to read model.onnx from tar: {}", e))?;

            tokio::fs::write(&tmp_path, onnx_data)
                .await
                .map_err(|e| format!("Failed to write model.onnx: {}", e))?;

            model_found = true;
            break;
        }
    }

    if !model_found {
        return Err("model.onnx not found in tarball".to_string());
    }

    std::fs::rename(&tmp_path, &final_path)
        .map_err(|e| format!("Failed to finalize model file: {}", e))?;

    log::info!("Diarization segmentation model extracted to {}", final_path.display());
    Ok(())
}

/// Download the V2 embedding model, emitting `diarization-model-download-progress` events.
pub async fn download_embedding_model_v2<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let dir = models_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create models dir: {}", e))?;

    let final_path = dir.join(EMBEDDING_MODEL_V2_FILENAME);
    if is_embedding_model_v2_present(app) {
        log::info!("Diarization embedding V2 model already present at {}", final_path.display());
        return Ok(());
    }
    let tmp_path = dir.join(format!("{}.tmp", EMBEDDING_MODEL_V2_FILENAME));

    log::info!("Downloading diarization embedding V2 model from {}", EMBEDDING_MODEL_V2_URL);
    let client = reqwest::Client::new();
    let response = client
        .get(EMBEDDING_MODEL_V2_URL)
        .send()
        .await
        .map_err(|e| format!("Download request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed with HTTP {}", response.status()));
    }

    let total_bytes = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut last_emitted_percent: i64 = -1;

    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download stream error: {}", e))?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|e| format!("Failed to write model file: {}", e))?;
        downloaded += chunk.len() as u64;

        let percent = if total_bytes > 0 {
            (downloaded * 100 / total_bytes) as i64
        } else {
            0
        };
        if percent != last_emitted_percent {
            last_emitted_percent = percent;
            let _ = app.emit(
                "diarization-model-download-progress",
                serde_json::json!({
                    "downloaded_bytes": downloaded,
                    "total_bytes": total_bytes,
                    "percent": percent,
                }),
            );
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(|e| format!("Failed to flush model file: {}", e))?;
    drop(file);

    std::fs::rename(&tmp_path, &final_path)
        .map_err(|e| format!("Failed to finalize model file: {}", e))?;

    log::info!(
        "Diarization embedding V2 model downloaded to {} ({} bytes)",
        final_path.display(),
        downloaded
    );
    Ok(())
}

/// Download both offline diarization models sequentially with progress events.
pub async fn download_offline_diarization_models<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    log::info!("Starting offline diarization models download (segmentation + embedding V2)");
    download_segmentation_model(app).await?;
    download_embedding_model_v2(app).await?;
    log::info!("Offline diarization models downloaded successfully");
    Ok(())
}
