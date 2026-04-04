use anyhow::{anyhow, Result};
use std::path::PathBuf;

pub const APP_NAME: &str = "MeetFree";
pub const APP_SLUG: &str = "meetfree";
pub const APP_IDENTIFIER: &str = "com.meetfree.ai";

pub const LLAMA_HELPER_ENV: &str = "MEETFREE_LLAMA_HELPER";
pub const LLAMA_HELPER_ALLOW_FUZZY_ENV: &str = "MEETFREE_LLAMA_HELPER_ALLOW_FUZZY";
pub const MODEL_CDN_BASE_URL_ENV: &str = "MEETFREE_MODEL_CDN_BASE_URL";

pub const RECORDINGS_DIR_NAME: &str = "meetfree-recordings";

pub fn data_root() -> Result<PathBuf> {
    dirs::data_dir()
        .map(|p| p.join(APP_SLUG))
        .ok_or_else(|| anyhow!("Could not find system data directory"))
}

pub fn config_root() -> Result<PathBuf> {
    dirs::config_dir()
        .map(|p| p.join(APP_SLUG))
        .ok_or_else(|| anyhow!("Could not find config directory"))
}

pub fn custom_template_dir() -> Result<PathBuf> {
    Ok(data_root()?.join("templates"))
}

pub fn model_cdn_base_url() -> String {
    std::env::var(MODEL_CDN_BASE_URL_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "https://meetily.towardsgeneralintelligence.com/models".to_string())
}

pub fn model_download_url(path: &str) -> String {
    format!(
        "{}/{}",
        model_cdn_base_url().trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}
