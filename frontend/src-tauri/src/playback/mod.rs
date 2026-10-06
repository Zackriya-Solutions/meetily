//! Playing a meeting's recording in the app: asset-protocol source, time table and WAV clips.
pub mod clip;

use crate::audio::decoder::container_duration_s;
use crate::state::AppState;
use serde::Serialize;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime};

/// What the player needs to play a meeting's recording.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlaybackSource {
    /// Asset-protocol URL of the audio file.
    pub url: String,
    /// Length in seconds of container time, which is the recording clock.
    pub duration_s: f64,
    /// (clock_s, file_s) points the player interpolates; see `playback_time_table`.
    pub time_table: Vec<[f64; 2]>,
}

const NO_RECORDING: &str = "This meeting has no recording";

/// The meeting's audio file, found in the folder stored for it.
pub(crate) async fn meeting_audio_path(pool: &SqlitePool, meeting_id: &str) -> Result<PathBuf, String> {
    let folder: Option<Option<String>> = sqlx::query_scalar("SELECT folder_path FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Failed to read the meeting: {}", e))?;
    let folder = folder
        .ok_or_else(|| "Meeting not found".to_string())?
        .filter(|f| !f.trim().is_empty())
        .ok_or_else(|| NO_RECORDING.to_string())?;
    crate::audio::retranscription::find_audio_file(Path::new(&folder)).map_err(|_| NO_RECORDING.to_string())
}

/// `encodeURIComponent`: everything except A-Z a-z 0-9 - _ . ! ~ * ' ( ) as %XX of its UTF-8 bytes.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// The URL `convertFileSrc(path)` gives in the webview.
pub fn asset_url(path: &Path) -> String {
    let encoded = encode_uri_component(&path.to_string_lossy());
    if cfg!(any(windows, target_os = "android")) {
        format!("http://asset.localhost/{}", encoded)
    } else {
        format!("asset://localhost/{}", encoded)
    }
}

/// Transcript clock → playback position. The player seeks the `<audio>` element and the WAV
/// clips in container time, and container time is the recording clock: joined 30 s checkpoints
/// advance the container by exactly 30 s each. The checkpoint drift of `TimeMap` exists only
/// when decoded frames are counted, so the table is the identity for every recording. If a
/// webview is ever found to follow decoded frames, return the checkpoint table here.
fn playback_time_table(duration_s: f64) -> Vec<[f64; 2]> {
    vec![[0.0, 0.0], [duration_s, duration_s]]
}

/// Source for `audio`. Reads the file's packet table but does not decode it.
pub(crate) fn playback_source(audio: &Path) -> anyhow::Result<PlaybackSource> {
    let duration_s = container_duration_s(audio)?;
    Ok(PlaybackSource { url: asset_url(audio), duration_s, time_table: playback_time_table(duration_s) })
}

/// Lets the webview load exactly this meeting's audio file and returns how to play it.
#[tauri::command]
pub async fn api_prepare_meeting_playback<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<PlaybackSource, String> {
    let audio = meeting_audio_path(state.db_manager.pool(), &meeting_id).await?;
    app.asset_protocol_scope()
        .allow_file(&audio)
        .map_err(|e| format!("Failed to allow playback of the recording: {}", e))?;
    tokio::task::spawn_blocking(move || playback_source(&audio))
        .await
        .map_err(|e| format!("Reading the recording failed: {}", e))?
        .map_err(|e| format!("Failed to read the recording: {:#}", e))
}

/// A WAV clip (16 kHz mono) of the meeting's recording from container time `start_file_s`, for
/// webviews that cannot play the recording itself. Raw bytes: an ArrayBuffer in JS.
#[tauri::command]
pub async fn api_render_playback_clip(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    start_file_s: f64,
    seconds: f64,
) -> Result<tauri::ipc::Response, String> {
    if !start_file_s.is_finite() || !seconds.is_finite() {
        return Err("Invalid clip range".into());
    }
    log::info!("Rendering a {:.1}s playback clip at {:.1}s of meeting {}", seconds, start_file_s, meeting_id);
    let audio = meeting_audio_path(state.db_manager.pool(), &meeting_id).await?;
    let wav = tokio::task::spawn_blocking(move || clip::render_clip(&audio, start_file_s, seconds))
        .await
        .map_err(|e| format!("Rendering the clip failed: {}", e))?
        .map_err(|e| format!("Failed to render the clip: {:#}", e))?;
    Ok(tauri::ipc::Response::new(wav))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::decoder::test_audio::{joined_checkpoints, write_wav};
    use crate::database::test_support::{migrated_pool, seed_meeting};

    async fn set_folder(pool: &SqlitePool, meeting_id: &str, folder: &Path) {
        sqlx::query("UPDATE meetings SET folder_path = ? WHERE id = ?")
            .bind(folder.to_string_lossy().to_string())
            .bind(meeting_id)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn audio_path_comes_from_the_meeting_folder() {
        let dir = tempfile::tempdir().unwrap();
        write_wav(&dir.path().join("audio.wav"), 16_000, 1, &[0.0; 1600]);
        let pool = migrated_pool().await;
        seed_meeting(&pool, "m", &[]).await;
        set_folder(&pool, "m", dir.path()).await;
        assert_eq!(meeting_audio_path(&pool, "m").await.unwrap(), dir.path().join("audio.wav"));
    }

    #[tokio::test]
    async fn meeting_without_audio_is_an_error() {
        let pool = migrated_pool().await;
        seed_meeting(&pool, "no-folder", &[]).await;
        assert_eq!(meeting_audio_path(&pool, "no-folder").await.unwrap_err(), "This meeting has no recording");
        let empty = tempfile::tempdir().unwrap();
        seed_meeting(&pool, "no-file", &[]).await;
        set_folder(&pool, "no-file", empty.path()).await;
        assert_eq!(meeting_audio_path(&pool, "no-file").await.unwrap_err(), "This meeting has no recording");
        assert_eq!(meeting_audio_path(&pool, "missing").await.unwrap_err(), "Meeting not found");
    }

    #[cfg(not(any(windows, target_os = "android")))]
    #[test]
    fn asset_url_matches_convert_file_src() {
        // encodeURIComponent: everything but A-Z a-z 0-9 - _ . ! ~ * ' ( ) is percent-encoded as UTF-8.
        assert_eq!(
            asset_url(Path::new("/home/a b/Réunion/audio (1).mp4")),
            "asset://localhost/%2Fhome%2Fa%20b%2FR%C3%A9union%2Faudio%20(1).mp4"
        );
        assert_eq!(asset_url(Path::new("/x/it's_a-b.~!*.m4a")), "asset://localhost/%2Fx%2Fit's_a-b.~!*.m4a");
        assert_eq!(asset_url(Path::new("/x/a#b?c&d+e.wav")), "asset://localhost/%2Fx%2Fa%23b%3Fc%26d%2Be.wav");
    }

    #[test]
    fn live_recording_plays_on_the_container_clock() {
        let dir = tempfile::tempdir().unwrap();
        let audio = joined_checkpoints(dir.path(), 2, |_| false);
        let source = playback_source(&audio).unwrap();
        // Two 30 s checkpoints plus the first checkpoint's 1024 frames of priming.
        let d = 60.0 + 1024.0 / 48_000.0;
        assert_eq!(source.url, asset_url(&audio));
        assert!((source.duration_s - d).abs() < 1e-6, "duration {}", source.duration_s);
        assert_eq!(source.time_table, vec![[0.0, 0.0], [source.duration_s, source.duration_s]]);
    }

    #[test]
    fn imported_meeting_source_is_identity() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("audio.wav");
        write_wav(&audio, 16_000, 1, &vec![0.0; 32_000]);
        let source = playback_source(&audio).unwrap();
        assert_eq!(source, PlaybackSource { url: asset_url(&audio), duration_s: 2.0, time_table: vec![[0.0, 0.0], [2.0, 2.0]] });
    }

    #[test]
    fn csp_allows_recording_playback() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json"))).unwrap();
        assert_eq!(conf["app"]["security"]["csp"]["media-src"], "'self' asset: http://asset.localhost blob:");
    }
}
