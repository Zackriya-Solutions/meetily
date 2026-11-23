// diarization/models.rs
//
// Location and download of the speaker-embedding model, into
// <app_data>/models/diarization/. The file is checked against a pinned
// SHA-256 before it is used, so a changed or truncated download is never
// loaded.

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// WeSpeaker CAM++ speaker-embedding model (Apache-2.0, exported to ONNX by
/// the sherpa-onnx project). Input: fbank [1, T, 80]; output: [1, 512].
pub const EMBEDDING_MODEL_FILENAME: &str = "wespeaker_en_voxceleb_CAM++.onnx";
/// Fetched from a fixed Hugging Face commit, not the sherpa-onnx GitHub
/// release: that asset was replaced in place on 2026-11-19, which broke the
/// checksum for every install.
pub const EMBEDDING_MODEL_URL: &str =
    "https://huggingface.co/csukuangfj/speaker-embedding-models/resolve/a8055d6daef723a6a4ecf4de233708575bf4994f/wespeaker_en_voxceleb_CAM%2B%2B.onnx";
pub const EMBEDDING_MODEL_SHA256: &str =
    "7acd352a0372de23ce49379b02864833b7f3f501b4ea25248fc977389f08e5fc";
const EMBEDDING_MODEL_SIZE: u64 = 29_281_636;

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

/// A connection that sends nothing for this long is dropped, so a stall ends
/// instead of holding the lock; a slow but moving download is never cut off.
const STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The verified model path, once this process has checked or downloaded it.
/// Held across the download so two recordings never write the same file.
static VERIFIED_MODEL: tokio::sync::Mutex<Option<PathBuf>> = tokio::sync::Mutex::const_new(None);

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}

/// True if the model file exists and matches the pinned size and hash.
pub fn is_embedding_model_valid(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() == EMBEDDING_MODEL_SIZE => {}
        _ => return false,
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).is_ok() && hex(&hasher.finalize()) == EMBEDDING_MODEL_SHA256
}

/// Downloads the embedding model unless a valid copy is already there,
/// emitting `diarization-model-download-progress` events with
/// { downloaded_bytes, total_bytes, percent }. Returns the model path.
pub async fn ensure_embedding_model<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let mut verified = VERIFIED_MODEL.lock().await;
    if let Some(path) = verified.as_ref() {
        return Ok(path.clone());
    }
    let path = download_if_missing(app).await?;
    *verified = Some(path.clone());
    Ok(path)
}

async fn download_if_missing<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let dir = models_dir(app)?;
    let final_path = dir.join(EMBEDDING_MODEL_FILENAME);
    let check_path = final_path.clone();
    if tokio::task::spawn_blocking(move || is_embedding_model_valid(&check_path))
        .await
        .unwrap_or(false)
    {
        return Ok(final_path);
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create models dir: {}", e))?;
    let tmp_path = dir.join(format!("{}.tmp", EMBEDDING_MODEL_FILENAME));

    log::info!("Downloading diarization embedding model from {}", EMBEDDING_MODEL_URL);
    let request = reqwest::Client::builder()
        .connect_timeout(STALL_TIMEOUT)
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?
        .get(EMBEDDING_MODEL_URL)
        .send();
    // A server that accepts the connection but never answers would otherwise
    // hold the lock until the app restarts.
    let response = tokio::time::timeout(STALL_TIMEOUT, request)
        .await
        .map_err(|_| format!("No response for {} s", STALL_TIMEOUT.as_secs()))?
        .map_err(|e| format!("Download request failed: {}", e))?;
    if !response.status().is_success() {
        return Err(format!("Download failed with HTTP {}", response.status()));
    }

    let total_bytes = response.content_length().unwrap_or(EMBEDDING_MODEL_SIZE);
    let mut downloaded: u64 = 0;
    let mut last_emitted_percent: i64 = -1;
    let mut hasher = Sha256::new();
    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    let mut stream = response.bytes_stream();
    loop {
        let chunk = match tokio::time::timeout(STALL_TIMEOUT, stream.next()).await {
            Ok(Some(Ok(chunk))) => chunk,
            Ok(None) => break,
            Ok(Some(Err(e))) => {
                drop(file);
                let _ = std::fs::remove_file(&tmp_path);
                return Err(format!("Download stream error: {}", e));
            }
            Err(_) => {
                drop(file);
                let _ = std::fs::remove_file(&tmp_path);
                return Err(format!("Download stalled for {} s", STALL_TIMEOUT.as_secs()));
            }
        };
        downloaded += chunk.len() as u64;
        if downloaded > EMBEDDING_MODEL_SIZE {
            drop(file);
            let _ = std::fs::remove_file(&tmp_path);
            return Err("Downloaded model is larger than expected".to_string());
        }
        hasher.update(&chunk);
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|e| format!("Failed to write model file: {}", e))?;

        let percent = (downloaded * 100 / total_bytes.max(1)) as i64;
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

    let digest = hex(&hasher.finalize());
    if digest != EMBEDDING_MODEL_SHA256 {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("Downloaded model failed its checksum ({} bytes, sha256 {})", downloaded, digest));
    }
    std::fs::rename(&tmp_path, &final_path)
        .map_err(|e| format!("Failed to finalize model file: {}", e))?;
    log::info!("Diarization embedding model downloaded to {}", final_path.display());
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_lowercase_hex() {
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_file_of_the_wrong_size_or_content_is_not_valid() {
        let dir = std::env::temp_dir().join(format!("diar-model-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.onnx");
        assert!(!is_embedding_model_valid(&path));
        std::fs::write(&path, b"not a model").unwrap();
        assert!(!is_embedding_model_valid(&path));
        std::fs::write(&path, vec![0u8; EMBEDDING_MODEL_SIZE as usize]).unwrap();
        assert!(!is_embedding_model_valid(&path));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
