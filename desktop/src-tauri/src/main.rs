#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use env_logger;
use log;

fn configure_process_environment() {
    // This runs before the async runtime starts, which is the only safe point
    // for process-wide environment mutation on current Rust.
    unsafe {
        std::env::set_var("RUST_LOG", "info");
        std::env::set_var("GGML_METAL_LOG_LEVEL", "1");
        std::env::set_var("WHISPER_LOG_LEVEL", "1");
    }
}

fn main() {
    configure_process_environment();
    env_logger::init();

    // Async logger will be initialized lazily when first needed (after Tauri runtime starts)
    log::info!("Starting application...");
    app_lib::run();
}
