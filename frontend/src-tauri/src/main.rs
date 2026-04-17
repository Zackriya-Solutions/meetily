#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use log;
use env_logger;

fn main() {
    std::env::set_var("RUST_LOG", "info");
    env_logger::init();

    // On Linux, WebKitGTK's DMABUF renderer is unreliable on common GPU/driver
    // combinations (notably NVIDIA proprietary drivers and several Wayland
    // compositors), producing a blank white window on launch. Disabling DMABUF
    // falls back to the stable renderer and fixes the blank-window case with
    // negligible visual/performance cost for a desktop app of this scope.
    // Only set the flag if the user hasn't already chosen a value, so anyone
    // debugging WebKit rendering can still override it from the environment.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    // Async logger will be initialized lazily when first needed (after Tauri runtime starts)
    log::info!("Starting application...");
    app_lib::run();
}
