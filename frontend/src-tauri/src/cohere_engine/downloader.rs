//! HuggingFace streaming downloader for Cohere ONNX weights.
//!
//! Pulls the minimum set of files required to run
//! [onnx-community/cohere-transcribe-03-2026-ONNX](https://huggingface.co/onnx-community/cohere-transcribe-03-2026-ONNX)
//! into the per-model directory and emits byte-level progress events.

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use log::{info, warn};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::io::AsyncWriteExt;

use super::engine::CohereEngine;

const HF_BASE: &str =
    "https://huggingface.co/onnx-community/cohere-transcribe-03-2026-ONNX/resolve/main";

/// One file the client must fetch. `remote` is the path relative to the HF repo,
/// `local` is the filename as stored on disk inside the per-model directory.
struct RemoteFile {
    remote: &'static str,
    local: &'static str,
}

const FILES: &[RemoteFile] = &[
    RemoteFile {
        remote: "onnx/encoder_model_q4f16.onnx",
        local: "encoder_model_q4f16.onnx",
    },
    RemoteFile {
        remote: "onnx/decoder_model_merged_q4f16.onnx",
        local: "decoder_model_merged_q4f16.onnx",
    },
    RemoteFile {
        remote: "tokenizer.json",
        local: "tokenizer.json",
    },
    RemoteFile {
        remote: "config.json",
        local: "config.json",
    },
    RemoteFile {
        remote: "generation_config.json",
        local: "generation_config.json",
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub model_name: String,
    pub file: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub file_index: usize,
    pub total_files: usize,
}

/// Download all required files for `model_name` into `engine`'s per-model dir.
/// Emits `cohere-download-progress` events on `app`. The shared `cancel` flag
/// allows aborting mid-stream.
pub async fn download_model<R: Runtime>(
    app: AppHandle<R>,
    engine: Arc<CohereEngine>,
    model_name: String,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let target_dir = engine.model_path(&model_name);
    if !target_dir.exists() {
        std::fs::create_dir_all(&target_dir)
            .with_context(|| format!("create dir {}", target_dir.display()))?;
    }

    let client = reqwest::Client::new();
    let total_files = FILES.len();

    for (idx, file) in FILES.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            return Err(anyhow!("download cancelled"));
        }
        let local_path = target_dir.join(file.local);
        if local_path.is_file() {
            info!(
                "cohere: skipping already-downloaded file {}",
                local_path.display()
            );
            continue;
        }

        let url = format!("{}/{}", HF_BASE, file.remote);
        info!("cohere: downloading {} → {}", url, local_path.display());

        let resp = client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {}", url))?;
        if !resp.status().is_success() {
            return Err(anyhow!("GET {} returned {}", url, resp.status()));
        }

        let total_bytes = resp.content_length();
        let mut stream = resp.bytes_stream();

        let tmp_path = local_path.with_extension("part");
        let mut out = tokio::fs::File::create(&tmp_path)
            .await
            .with_context(|| format!("create {}", tmp_path.display()))?;

        let mut downloaded: u64 = 0;
        while let Some(chunk) = stream.next().await {
            if cancel.load(Ordering::SeqCst) {
                // Remove the partial download on cancel.
                drop(out);
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err(anyhow!("download cancelled"));
            }
            let chunk = chunk.with_context(|| format!("stream chunk from {}", url))?;
            out.write_all(&chunk)
                .await
                .with_context(|| format!("write to {}", tmp_path.display()))?;
            downloaded += chunk.len() as u64;

            if let Err(e) = app.emit(
                "cohere-download-progress",
                DownloadProgress {
                    model_name: model_name.clone(),
                    file: file.local.to_string(),
                    downloaded_bytes: downloaded,
                    total_bytes,
                    file_index: idx + 1,
                    total_files,
                },
            ) {
                warn!("emit cohere-download-progress failed: {e}");
            }
        }

        out.flush().await.ok();
        drop(out);
        tokio::fs::rename(&tmp_path, &local_path)
            .await
            .with_context(|| format!("rename {} → {}", tmp_path.display(), local_path.display()))?;
    }

    if let Err(e) = app.emit(
        "cohere-download-complete",
        serde_json::json!({ "modelName": model_name }),
    ) {
        warn!("emit cohere-download-complete failed: {e}");
    }
    Ok(())
}
