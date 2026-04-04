// Sidecar process lifecycle management for llama-helper
// Handles spawning, health checking, keep-alive, and graceful shutdown

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{Mutex, RwLock};

use super::models;
use crate::brand;

// ============================================================================
// Sidecar State Management
// ============================================================================

/// Sidecar process manager with keep-alive and health monitoring
pub struct SidecarManager {
    /// Child process handle
    child_process: Arc<Mutex<Option<Child>>>,

    /// Stdin writer for sending requests
    stdin_writer: Arc<Mutex<Option<ChildStdin>>>,

    /// Stdout reader for receiving responses
    stdout_reader: Arc<Mutex<Option<BufReader<ChildStdout>>>>,

    /// Last activity timestamp
    last_activity: Arc<RwLock<Instant>>,

    /// Health status
    is_healthy: Arc<AtomicBool>,

    /// Shutdown flag
    should_shutdown: Arc<AtomicBool>,

    /// Active request count (for graceful shutdown)
    active_request_count: Arc<AtomicUsize>,

    /// Path to llama-helper binary
    helper_binary_path: PathBuf,

    /// Current model path (if loaded)
    current_model_path: Arc<RwLock<Option<PathBuf>>>,

    /// Idle timeout in seconds (configurable via env var)
    idle_timeout_secs: u64,
}

/// RAII guard for tracking active requests
/// Decrements the active request count when dropped
struct RequestGuard {
    counter: Arc<AtomicUsize>,
}

impl RequestGuard {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self { counter }
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

impl SidecarManager {
    /// Create a new sidecar manager
    pub fn new(_app_data_dir: PathBuf) -> Result<Self> {
        let helper_binary_path = Self::resolve_helper_binary()?;

        // Get idle timeout from env var or use default
        let idle_timeout_secs = std::env::var("LLAMA_IDLE_TIMEOUT")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(models::DEFAULT_IDLE_TIMEOUT_SECS);

        log::info!(
            "SidecarManager initialized with idle timeout: {}s",
            idle_timeout_secs
        );
        log::info!("Helper binary path: {}", helper_binary_path.display());

        Ok(Self {
            child_process: Arc::new(Mutex::new(None)),
            stdin_writer: Arc::new(Mutex::new(None)),
            stdout_reader: Arc::new(Mutex::new(None)),
            last_activity: Arc::new(RwLock::new(Instant::now())),
            is_healthy: Arc::new(AtomicBool::new(false)),
            should_shutdown: Arc::new(AtomicBool::new(false)),
            active_request_count: Arc::new(AtomicUsize::new(0)),
            helper_binary_path,
            current_model_path: Arc::new(RwLock::new(None)),
            idle_timeout_secs,
        })
    }

    /// Resolve the path to llama-helper binary
    fn resolve_helper_binary() -> Result<PathBuf> {
        // 1. Check environment variable (dev mode or manual override)
        if let Some(path) = sidecar_override_from_env() {
            return Ok(path);
        }

        // In production, Tauri bundles the binary with target triple suffix
        // 2. Check relative to current executable (most reliable for AppImage/bundled apps)
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                log::info!(
                    "Searching for llama-helper relative to executable: {}",
                    exe_dir.display()
                );

                let target_triple = target_triple();

                let binary_name = if cfg!(windows) {
                    format!("llama-helper-{}.exe", target_triple)
                } else {
                    format!("llama-helper-{}", target_triple)
                };

                if let Some(path) =
                    find_bundled_sidecar(exe_dir, &binary_name, allow_fuzzy_sidecar_lookup())
                {
                    log::info!(
                        "Found bundled helper next to executable: {}",
                        path.display()
                    );
                    return Ok(path);
                }
            }
        }

        // 3. Check bundled resources (RESOURCE_DIR) - Fallback
        if let Ok(resource_dir) = std::env::var("RESOURCE_DIR") {
            log::info!(
                "Searching for llama-helper in RESOURCE_DIR: {}",
                resource_dir
            );
            let resource_path = PathBuf::from(&resource_dir);
            let target_triple = target_triple();

            let binary_name = if cfg!(windows) {
                format!("llama-helper-{}.exe", target_triple)
            } else {
                format!("llama-helper-{}", target_triple)
            };

            if let Some(path) =
                find_bundled_sidecar(&resource_path, &binary_name, allow_fuzzy_sidecar_lookup())
            {
                log::info!("Found bundled helper in RESOURCE_DIR: {}", path.display());
                return Ok(path);
            }
        } else {
            log::warn!("RESOURCE_DIR environment variable not set");
        }

        // 3. Fallback for dev: try relative paths from workspace (no target triple in dev builds)
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let project_root = PathBuf::from(&manifest_dir)
                .parent()
                .and_then(|p| p.parent())
                .ok_or_else(|| anyhow!("Failed to determine project root"))?
                .to_path_buf();

            let candidates = vec![
                project_root.join("target/release/llama-helper"),
                project_root.join("target/debug/llama-helper"),
                project_root.join("target/release/llama-helper.exe"),
                project_root.join("target/debug/llama-helper.exe"),
            ];

            for candidate in candidates {
                if candidate.exists() {
                    log::info!("Using dev llama-helper: {}", candidate.display());
                    return Ok(candidate);
                }
            }
        }

        Err(anyhow!(
            "llama-helper binary not found. Build with 'cd llama-helper && cargo build --release' or set {}.",
            brand::LLAMA_HELPER_ENV
        ))
    }

    /// Ensure sidecar is running, spawn if needed
    pub async fn ensure_running(&self, model_path: PathBuf) -> Result<()> {
        // Check if already running with correct model
        {
            let current_model = self.current_model_path.read().await;
            if current_model.as_ref() == Some(&model_path) && self.is_healthy() {
                log::debug!("Sidecar already running with correct model");
                self.update_activity().await;
                return Ok(());
            }
        }

        // Need to spawn or restart
        self.spawn(model_path).await
    }

    /// Spawn the sidecar process
    async fn spawn(&self, model_path: PathBuf) -> Result<()> {
        // Shutdown existing process if running
        self.shutdown().await?;

        log::info!("Spawning llama-helper sidecar");
        log::info!("Model path: {}", model_path.display());

        #[cfg(unix)]
        let mut command = tokio::process::Command::new("nice");

        #[cfg(not(unix))]
        let mut command = tokio::process::Command::new(&self.helper_binary_path);

        #[cfg(unix)]
        command.arg("-n").arg("10").arg(&self.helper_binary_path);

        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()); // Log stderr to main process

        #[cfg(target_os = "windows")]
        {
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x00004000;

            command.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
        }

        let mut child = command.spawn().with_context(|| {
            format!(
                "Failed to spawn llama-helper at {:?}",
                self.helper_binary_path
            )
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to get stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to get stdout"))?;

        // Store handles
        {
            let mut child_lock = self.child_process.lock().await;
            *child_lock = Some(child);
        }

        {
            let mut stdin_lock = self.stdin_writer.lock().await;
            *stdin_lock = Some(stdin);
        }

        {
            let mut stdout_lock = self.stdout_reader.lock().await;
            *stdout_lock = Some(BufReader::new(stdout));
        }

        // Update state
        {
            let mut current_model = self.current_model_path.write().await;
            *current_model = Some(model_path);
        }

        self.is_healthy.store(true, Ordering::SeqCst);
        self.should_shutdown.store(false, Ordering::SeqCst);
        self.update_activity().await;

        log::info!("Sidecar spawned successfully");

        // Start background tasks
        self.start_health_check_loop();
        self.start_idle_check_loop();

        Ok(())
    }

    /// Send a request to the sidecar and wait for response
    pub async fn send_request(&self, request_json: String, timeout: Duration) -> Result<String> {
        // Track active request
        let _guard = RequestGuard::new(self.active_request_count.clone());

        // Write request to stdin
        {
            let mut stdin_lock = self.stdin_writer.lock().await;
            let stdin = stdin_lock
                .as_mut()
                .ok_or_else(|| anyhow!("Sidecar not running"))?;

            stdin
                .write_all(request_json.as_bytes())
                .await
                .context("Failed to write request to stdin")?;
            stdin
                .write_all(b"\n")
                .await
                .context("Failed to write newline")?;
            stdin.flush().await.context("Failed to flush stdin")?;
        }

        // Read response from stdout with timeout
        match tokio::time::timeout(timeout, self.read_response()).await {
            Ok(Ok(response)) => {
                self.update_activity().await;
                Ok(response)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => {
                // Timeout reached - shutdown sidecar to stop generation
                log::error!("Request timeout after {:?}, shutting down sidecar", timeout);
                if let Err(shutdown_err) = self.shutdown().await {
                    log::error!("Failed to shutdown sidecar after timeout: {}", shutdown_err);
                }
                Err(anyhow!("Request timed out after {:?}", timeout))
            }
        }
    }

    /// Read a single line response from stdout
    async fn read_response(&self) -> Result<String> {
        let mut stdout_lock = self.stdout_reader.lock().await;
        let reader = stdout_lock
            .as_mut()
            .ok_or_else(|| anyhow!("Sidecar not running"))?;

        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .context("Failed to read response from stdout")?;

        if line.is_empty() {
            return Err(anyhow!("Sidecar closed stdout (process may have crashed)"));
        }

        Ok(line.trim().to_string())
    }

    /// Send ping to keep sidecar alive
    async fn send_ping(&self) -> Result<()> {
        let request = serde_json::json!({"type": "ping"}).to_string();
        let timeout = Duration::from_secs(5);

        // Note: We don't use send_request here to avoid incrementing active_request_count
        // for internal health checks, as that would prevent graceful shutdown

        // Write request
        {
            let mut stdin_lock = self.stdin_writer.lock().await;
            if let Some(stdin) = stdin_lock.as_mut() {
                stdin.write_all(request.as_bytes()).await?;
                stdin.write_all(b"\n").await?;
                stdin.flush().await?;
            } else {
                return Err(anyhow!("Sidecar not running"));
            }
        }

        // Read response
        let response = tokio::time::timeout(timeout, self.read_response()).await??;

        let resp: serde_json::Value = serde_json::from_str(&response)?;
        if resp.get("type").and_then(|t| t.as_str()) == Some("pong") {
            Ok(())
        } else {
            Err(anyhow!("Unexpected ping response: {}", response))
        }
    }

    /// Gracefully shutdown the sidecar
    /// Waits for active requests to complete before killing the process
    pub async fn shutdown_gracefully(&self) -> Result<()> {
        log::info!("Initiating graceful shutdown of sidecar");

        // Set shutdown flag to prevent new internal tasks
        self.should_shutdown.store(true, Ordering::SeqCst);

        // Wait for active requests to complete
        // We poll every 500ms
        let start = Instant::now();
        let max_wait = Duration::from_secs(600); // Wait up to 10 minutes for long generations

        loop {
            let count = self.active_request_count.load(Ordering::SeqCst);
            if count == 0 {
                log::info!("No active requests, proceeding with shutdown");
                break;
            }

            if start.elapsed() > max_wait {
                log::warn!(
                    "Timed out waiting for active requests ({} active), forcing shutdown",
                    count
                );
                break;
            }

            log::debug!("Waiting for {} active requests to complete...", count);
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        self.shutdown().await
    }

    /// Force shutdown the sidecar
    pub async fn shutdown(&self) -> Result<()> {
        // Set shutdown flag
        self.should_shutdown.store(true, Ordering::SeqCst);

        // Send shutdown command
        if self.is_healthy() {
            let request = serde_json::json!({"type": "shutdown"}).to_string();
            let _timeout = Duration::from_secs(5);

            // Try to send shutdown command, but ignore errors
            // We don't use send_request to avoid incrementing counter
            let _ = async {
                let mut stdin_lock = self.stdin_writer.lock().await;
                if let Some(stdin) = stdin_lock.as_mut() {
                    stdin.write_all(request.as_bytes()).await?;
                    stdin.write_all(b"\n").await?;
                    stdin.flush().await?;
                }
                Ok::<(), anyhow::Error>(())
            }
            .await;
        }

        // Kill process if still running
        {
            let mut child_lock = self.child_process.lock().await;
            if let Some(mut child) = child_lock.take() {
                match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
                    Ok(Ok(status)) => {
                        log::info!("Sidecar exited with status: {}", status);
                    }
                    Ok(Err(e)) => {
                        log::error!("Failed to wait for sidecar: {}", e);
                    }
                    Err(_) => {
                        log::warn!("Sidecar didn't exit gracefully, killing");
                        let _ = child.kill().await;
                    }
                }
            }
        }

        // Clear handles
        {
            let mut stdin_lock = self.stdin_writer.lock().await;
            *stdin_lock = None;
        }

        {
            let mut stdout_lock = self.stdout_reader.lock().await;
            *stdout_lock = None;
        }

        {
            let mut current_model = self.current_model_path.write().await;
            *current_model = None;
        }

        self.is_healthy.store(false, Ordering::SeqCst);

        log::info!("Sidecar shutdown complete");
        Ok(())
    }

    /// Check if sidecar is healthy
    pub fn is_healthy(&self) -> bool {
        self.is_healthy.load(Ordering::SeqCst)
    }

    /// Update last activity timestamp
    async fn update_activity(&self) {
        let mut last_activity = self.last_activity.write().await;
        *last_activity = Instant::now();
    }

    /// Get seconds since last activity
    async fn seconds_since_activity(&self) -> u64 {
        let last_activity = self.last_activity.read().await;
        last_activity.elapsed().as_secs()
    }

    /// Start health check loop (runs in background)
    fn start_health_check_loop(&self) {
        let manager = Self {
            child_process: self.child_process.clone(),
            stdin_writer: self.stdin_writer.clone(),
            stdout_reader: self.stdout_reader.clone(),
            last_activity: self.last_activity.clone(),
            is_healthy: self.is_healthy.clone(),
            should_shutdown: self.should_shutdown.clone(),
            active_request_count: self.active_request_count.clone(),
            helper_binary_path: self.helper_binary_path.clone(),
            current_model_path: self.current_model_path.clone(),
            idle_timeout_secs: self.idle_timeout_secs,
        };

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                if manager.should_shutdown.load(Ordering::SeqCst) {
                    log::debug!("Health check loop: shutdown flag set, exiting");
                    break;
                }

                if !manager.is_healthy() {
                    log::debug!("Health check loop: sidecar unhealthy, skipping ping");
                    continue;
                }

                // Don't ping if we are busy with a request
                if manager.active_request_count.load(Ordering::SeqCst) > 0 {
                    continue;
                }

                log::debug!("Health check: sending ping");
                if let Err(e) = manager.send_ping().await {
                    log::warn!("Health check failed: {}", e);
                    manager.is_healthy.store(false, Ordering::SeqCst);
                }
            }

            log::debug!("Health check loop exited");
        });
    }

    /// Start idle check loop (runs in background)
    fn start_idle_check_loop(&self) {
        let manager = Self {
            child_process: self.child_process.clone(),
            stdin_writer: self.stdin_writer.clone(),
            stdout_reader: self.stdout_reader.clone(),
            last_activity: self.last_activity.clone(),
            is_healthy: self.is_healthy.clone(),
            should_shutdown: self.should_shutdown.clone(),
            active_request_count: self.active_request_count.clone(),
            helper_binary_path: self.helper_binary_path.clone(),
            current_model_path: self.current_model_path.clone(),
            idle_timeout_secs: self.idle_timeout_secs,
        };

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                if manager.should_shutdown.load(Ordering::SeqCst) {
                    log::debug!("Idle check loop: shutdown flag set, exiting");
                    break;
                }

                // Don't shutdown if we are busy
                if manager.active_request_count.load(Ordering::SeqCst) > 0 {
                    // Update activity to prevent timeout immediately after request finishes
                    manager.update_activity().await;
                    continue;
                }

                let idle_secs = manager.seconds_since_activity().await;
                log::debug!("Idle check: {}s since last activity", idle_secs);

                if idle_secs > manager.idle_timeout_secs {
                    log::info!(
                        "Sidecar idle for {}s (timeout: {}s), shutting down",
                        idle_secs,
                        manager.idle_timeout_secs
                    );

                    if let Err(e) = manager.shutdown().await {
                        log::error!("Failed to shutdown idle sidecar: {}", e);
                    }

                    break;
                }
            }

            log::debug!("Idle check loop exited");
        });
    }
}

impl Drop for SidecarManager {
    fn drop(&mut self) {
        // Set shutdown flag
        self.should_shutdown.store(true, Ordering::SeqCst);

        // Note: Actual cleanup happens in shutdown() method
        // We can't do async work in Drop, so this is best-effort
        log::debug!("SidecarManager dropped");
    }
}

fn allow_fuzzy_sidecar_lookup() -> bool {
    cfg!(debug_assertions)
        && std::env::var(brand::LLAMA_HELPER_ALLOW_FUZZY_ENV)
            .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "True"))
}

fn find_bundled_sidecar(
    dir: &std::path::Path,
    binary_name: &str,
    allow_fuzzy: bool,
) -> Option<PathBuf> {
    let exact = dir.join(binary_name);
    if exact.exists() {
        return Some(exact);
    }

    if !allow_fuzzy {
        return None;
    }

    log::warn!(
        "Using fuzzy llama-helper lookup in {}. This is intended for explicit development overrides only.",
        dir.display()
    );

    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("llama-helper") && !name.ends_with(".d"))
        })
}

fn sidecar_override_from_env() -> Option<PathBuf> {
    if let Ok(env_path) = std::env::var(brand::LLAMA_HELPER_ENV) {
        if !env_path.is_empty() {
            let path = PathBuf::from(env_path);
            if path.exists() {
                log::info!(
                    "Using llama-helper from {}: {}",
                    brand::LLAMA_HELPER_ENV,
                    path.display()
                );
                return Some(path);
            }
        }
    }

    None
}

fn target_triple() -> String {
    std::env::var("TARGET").unwrap_or_else(|_| {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            return "x86_64-unknown-linux-gnu".to_string();
        }
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            return "aarch64-unknown-linux-gnu".to_string();
        }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            return "x86_64-apple-darwin".to_string();
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            return "aarch64-apple-darwin".to_string();
        }
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            return "x86_64-pc-windows-msvc".to_string();
        }
        #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
        {
            return "aarch64-pc-windows-msvc".to_string();
        }
        #[cfg(not(any(
            all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64")
            ),
            all(
                target_os = "macos",
                any(target_arch = "x86_64", target_arch = "aarch64")
            ),
            all(
                target_os = "windows",
                any(target_arch = "x86_64", target_arch = "aarch64")
            )
        )))]
        {
            "unknown".to_string()
        }
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use serde_json::json;
    use tempfile::tempdir;
    use tokio::time::sleep;

    use super::*;

    #[test]
    fn test_find_bundled_sidecar_requires_exact_match_without_fuzzy() {
        let dir = tempdir().unwrap();
        let fuzzy = dir.path().join("llama-helper-dev.exe");
        std::fs::write(&fuzzy, b"binary").unwrap();

        assert!(find_bundled_sidecar(dir.path(), "llama-helper-x86_64.exe", false).is_none());
        assert_eq!(
            find_bundled_sidecar(dir.path(), "llama-helper-x86_64.exe", true),
            Some(fuzzy)
        );
    }

    #[tokio::test]
    async fn test_ensure_running_spawns_sidecar() {
        let _guard = crate::summary::summary_engine::test_utils::test_env_lock();
        crate::summary::summary_engine::test_utils::clear_test_env();
        crate::summary::summary_engine::test_utils::set_fake_helper_env();

        let app_data_dir = tempdir().unwrap();
        let manager = SidecarManager::new(app_data_dir.path().to_path_buf()).unwrap();
        manager
            .ensure_running(PathBuf::from("C:/test/model.gguf"))
            .await
            .unwrap();

        assert!(manager.is_healthy());

        manager.shutdown().await.unwrap();
        crate::summary::summary_engine::test_utils::clear_test_env();
    }

    #[tokio::test]
    async fn test_sidecar_request_response_round_trip() {
        let _guard = crate::summary::summary_engine::test_utils::test_env_lock();
        crate::summary::summary_engine::test_utils::clear_test_env();
        crate::summary::summary_engine::test_utils::set_fake_helper_env();

        let app_data_dir = tempdir().unwrap();
        let manager = SidecarManager::new(app_data_dir.path().to_path_buf()).unwrap();
        manager
            .ensure_running(PathBuf::from("C:/test/model.gguf"))
            .await
            .unwrap();

        let request = json!({
            "type": "generate",
            "prompt": "TEST_OK",
            "model_path": "C:/test/model.gguf",
            "model_layer_count": 26
        });
        let response = manager
            .send_request(request.to_string(), Duration::from_secs(2))
            .await
            .unwrap();

        assert!(response.contains("\"type\":\"response\""));
        assert!(response.contains("ok-response"));

        manager.shutdown().await.unwrap();
        crate::summary::summary_engine::test_utils::clear_test_env();
    }

    #[tokio::test]
    async fn test_sidecar_timeout_forces_shutdown() {
        let _guard = crate::summary::summary_engine::test_utils::test_env_lock();
        crate::summary::summary_engine::test_utils::clear_test_env();
        crate::summary::summary_engine::test_utils::set_fake_helper_env();

        let app_data_dir = tempdir().unwrap();
        let manager = SidecarManager::new(app_data_dir.path().to_path_buf()).unwrap();
        manager
            .ensure_running(PathBuf::from("C:/test/model.gguf"))
            .await
            .unwrap();

        let request = json!({
            "type": "generate",
            "prompt": "TEST_TIMEOUT",
            "model_path": "C:/test/model.gguf",
            "model_layer_count": 26
        });
        let error = manager
            .send_request(request.to_string(), Duration::from_millis(100))
            .await
            .expect_err("request should time out");

        assert!(error.to_string().contains("timed out"));
        assert!(!manager.is_healthy());

        crate::summary::summary_engine::test_utils::clear_test_env();
    }

    #[tokio::test]
    async fn test_shutdown_gracefully_waits_for_active_request() {
        let _guard = crate::summary::summary_engine::test_utils::test_env_lock();
        crate::summary::summary_engine::test_utils::clear_test_env();
        crate::summary::summary_engine::test_utils::set_fake_helper_env();

        let app_data_dir = tempdir().unwrap();
        let manager = Arc::new(SidecarManager::new(app_data_dir.path().to_path_buf()).unwrap());
        manager
            .ensure_running(PathBuf::from("C:/test/model.gguf"))
            .await
            .unwrap();

        let request = json!({
            "type": "generate",
            "prompt": "TEST_SLOW",
            "model_path": "C:/test/model.gguf",
            "model_layer_count": 26
        })
        .to_string();

        let request_manager = manager.clone();
        let handle = tokio::spawn(async move {
            request_manager
                .send_request(request, Duration::from_secs(2))
                .await
                .unwrap()
        });

        sleep(Duration::from_millis(50)).await;

        let start = Instant::now();
        manager.shutdown_gracefully().await.unwrap();
        let elapsed = start.elapsed();
        let response = handle.await.unwrap();

        assert!(elapsed >= Duration::from_millis(150));
        assert!(response.contains("slow-response"));
        assert!(!manager.is_healthy());

        crate::summary::summary_engine::test_utils::clear_test_env();
    }

    #[tokio::test]
    async fn test_force_shutdown_marks_sidecar_unhealthy() {
        let _guard = crate::summary::summary_engine::test_utils::test_env_lock();
        crate::summary::summary_engine::test_utils::clear_test_env();
        crate::summary::summary_engine::test_utils::set_fake_helper_env();

        let app_data_dir = tempdir().unwrap();
        let manager = SidecarManager::new(app_data_dir.path().to_path_buf()).unwrap();
        manager
            .ensure_running(PathBuf::from("C:/test/model.gguf"))
            .await
            .unwrap();

        manager.shutdown().await.unwrap();
        assert!(!manager.is_healthy());

        crate::summary::summary_engine::test_utils::clear_test_env();
    }
}
