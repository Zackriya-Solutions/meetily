use std::{
    path::PathBuf,
    process::Stdio,
    time::Duration,
};

use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use tauri::{path::BaseDirectory, AppHandle, Manager, Runtime};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

use super::final_transcription::transcribe_saved_audio_with_context;
use crate::parakeet_engine::TimestampedResult;

const EDGE_SECONDS: usize = 3;
const MAX_RESPONSE_BYTES: u32 = 1024 * 1024;

#[derive(Deserialize)]
struct WindowTranscript {
    tokens: Vec<String>,
    timestamps: Vec<f32>,
}

pub(crate) fn helper_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    let path = app.path()
        .resolve("binaries/parakeet-mlx/parakeet-mlx", BaseDirectory::Resource)
        .ok()?;
    path.is_file().then_some(path)
}

pub async fn transcribe(
    path: PathBuf,
    helper: PathBuf,
    model: PathBuf,
) -> Result<Vec<(String, f64, f64)>> {
    let mut child = Command::new(helper)
        .arg(model)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("Could not start Parakeet MLX")?;
    let mut input = child.stdin.take().context("No Parakeet MLX input")?;
    let mut output = child.stdout.take().context("No Parakeet MLX output")?;
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transcript = transcribe_saved_audio_with_context(&path, EDGE_SECONDS, |samples| {
            let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
            let reply = runtime.block_on(timeout(Duration::from_secs(60), async {
                input.write_u32_le(samples.len() as u32).await?;
                input.write_all(&bytes).await?;
                let length = output.read_u32_le().await?;
                ensure!((1..=MAX_RESPONSE_BYTES).contains(&length), "Invalid Parakeet MLX reply");
                let mut reply = vec![0; length as usize];
                output.read_exact(&mut reply).await?;
                Ok::<_, anyhow::Error>(reply)
            })).context("Parakeet MLX window timed out")??;
            let reply: WindowTranscript = serde_json::from_slice(&reply)?;
            Ok(TimestampedResult {
                text: String::new(),
                tokens: reply.tokens,
                timestamps: reply.timestamps,
            })
        })?;

        drop(input);
        let status = runtime.block_on(timeout(Duration::from_secs(10), child.wait()))
            .context("Parakeet MLX did not exit")??;
        ensure!(status.success(), "Parakeet MLX stopped unexpectedly");
        Ok(transcript)
    }).await?
}
