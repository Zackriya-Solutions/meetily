use std::{path::PathBuf, process::Stdio, time::Duration};

use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use tauri::{path::BaseDirectory, AppHandle, Manager, Runtime};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};

use crate::parakeet_engine::TimestampedResult;

pub(crate) const EDGE_SECONDS: usize = 3;
const RATE: usize = 16_000;
const MAX_SAMPLES: usize = 30 * RATE;
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

/// One model process for live captions and saved-audio refinement.
pub(crate) struct MlxDecoder {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}

impl MlxDecoder {
    pub(crate) async fn start(helper: &std::path::Path, model: &std::path::Path) -> Result<Self> {
        let mut child = Command::new(helper)
            .arg(model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("Could not start Parakeet MLX")?;
        let input = child.stdin.take().context("No Parakeet MLX input")?;
        let output = child.stdout.take().context("No Parakeet MLX output")?;
        let mut decoder = Self { child, input, output };
        // The reply confirms that the local model can actually load and run on Metal.
        decoder.decode(vec![0.0; RATE / 10]).await?;
        Ok(decoder)
    }

    pub(crate) async fn decode(&mut self, samples: Vec<f32>) -> Result<TimestampedResult> {
        ensure!((1..=MAX_SAMPLES).contains(&samples.len()), "Invalid Parakeet MLX audio window");
        let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
        let request = timeout(Duration::from_secs(15), async {
            self.input.write_u32_le(samples.len() as u32).await?;
            self.input.write_all(&bytes).await?;
            let length = self.output.read_u32_le().await?;
            ensure!((1..=MAX_RESPONSE_BYTES).contains(&length), "Invalid Parakeet MLX reply");
            let mut reply = vec![0; length as usize];
            self.output.read_exact(&mut reply).await?;
            let reply: WindowTranscript = serde_json::from_slice(&reply)?;
            Ok::<_, anyhow::Error>(reply)
        }).await.context("Parakeet MLX window timed out").and_then(|result| result);

        // A failed request may leave the pipe out of sync. Close the worker before any retry.
        if request.is_err() {
            let _ = self.child.start_kill();
        }
        let reply = request?;
        Ok(TimestampedResult {
            text: reply.tokens.concat().trim().to_owned(),
            tokens: reply.tokens,
            timestamps: reply.timestamps,
        })
    }

    pub(crate) async fn text(&mut self, samples: Vec<f32>) -> Result<String> {
        if samples.len() <= MAX_SAMPLES {
            return Ok(self.decode(samples).await?.text);
        }
        let core = 24 * RATE;
        let edge = EDGE_SECONDS * RATE;
        let mut text = Vec::new();
        for start in (0..samples.len()).step_by(core) {
            let origin = start.saturating_sub(edge);
            let end = (start + core).min(samples.len());
            let result = self.decode(samples[origin..(end + edge).min(samples.len())].to_vec()).await?;
            if let Some((owned, _)) = super::final_transcription::owned_text(result, origin, start, end)? {
                text.push(owned);
            }
        }
        Ok(text.join(" "))
    }
}
