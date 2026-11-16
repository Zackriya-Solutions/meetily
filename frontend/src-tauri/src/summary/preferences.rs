use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::{Store, StoreExt};

use super::llm_client::LLMProvider;
use super::summary_engine::models::GENERATION_TIMEOUT_SECS;

const TIMEOUT_KEY: &str = "requestTimeoutSeconds";

fn preferences_store<R: Runtime>(app: &AppHandle<R>) -> Result<Arc<Store<R>>, String> {
    app.store_builder("summary_preferences.json")
        .disable_auto_save()
        .build()
        .map_err(|error| format!("Could not open summary preferences: {error}"))
}

fn validate_timeout(seconds: Option<u32>) -> Result<Option<u32>, String> {
    if seconds == Some(0) {
        return Err("Summary request timeout must be a positive whole number of seconds".into());
    }
    Ok(seconds)
}

fn read_timeout<R: Runtime>(store: &Store<R>) -> Result<Option<u32>, String> {
    let seconds = serde_json::from_value(store.get(TIMEOUT_KEY).unwrap_or_default())
        .map_err(|error| format!("Could not read summary request timeout: {error}"))?;
    validate_timeout(seconds)
}

fn save_timeout<R: Runtime>(store: &Store<R>, seconds: Option<u32>) -> Result<(), String> {
    validate_timeout(seconds)?;
    let previous = store.get(TIMEOUT_KEY);
    store.set(TIMEOUT_KEY, serde_json::json!(seconds));
    if let Err(error) = store.save() {
        if let Some(previous) = previous {
            store.set(TIMEOUT_KEY, previous);
        } else {
            store.delete(TIMEOUT_KEY);
        }
        return Err(format!("Could not save summary request timeout: {error}"));
    }
    Ok(())
}

fn request_timeout(seconds: Option<u32>, provider: &LLMProvider) -> Duration {
    Duration::from_secs(seconds.map(u64::from).unwrap_or_else(|| {
        if provider == &LLMProvider::BuiltInAI {
            GENERATION_TIMEOUT_SECS
        } else {
            300
        }
    }))
}

pub(crate) fn load_request_timeout<R: Runtime>(
    app: &AppHandle<R>,
    provider: &LLMProvider,
) -> Result<Duration, String> {
    Ok(request_timeout(
        api_get_summary_timeout(app.clone())?,
        provider,
    ))
}

#[tauri::command]
pub fn api_get_summary_timeout<R: Runtime>(app: AppHandle<R>) -> Result<Option<u32>, String> {
    let store = preferences_store(&app)?;
    read_timeout(&store)
}

#[tauri::command]
pub fn api_save_summary_timeout<R: Runtime>(
    app: AppHandle<R>,
    seconds: Option<u32>,
) -> Result<(), String> {
    let store = preferences_store(&app)?;
    save_timeout(&store, seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_preserve_each_provider_and_overrides_apply_to_both() {
        assert_eq!(request_timeout(None, &LLMProvider::Ollama).as_secs(), 300);
        assert_eq!(
            request_timeout(None, &LLMProvider::BuiltInAI).as_secs(),
            900
        );
        for provider in [
            LLMProvider::Ollama,
            LLMProvider::BuiltInAI,
            LLMProvider::Claude,
        ] {
            assert_eq!(request_timeout(Some(1200), &provider).as_secs(), 1200);
        }
        assert!(validate_timeout(Some(0)).is_err());
    }

    #[test]
    fn saved_timeout_survives_reloading_and_can_restore_defaults() {
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = app
            .store_builder(dir.path().join("preferences.json"))
            .disable_auto_save()
            .build()
            .unwrap();
        assert_eq!(read_timeout(&store).unwrap(), None);
        save_timeout(&store, Some(1200)).unwrap();
        store.clear();
        store.reload().unwrap();
        assert_eq!(read_timeout(&store).unwrap(), Some(1200));
        assert!(save_timeout(&store, Some(0)).is_err());
        assert_eq!(read_timeout(&store).unwrap(), Some(1200));
        save_timeout(&store, None).unwrap();
        store.reload().unwrap();
        assert_eq!(read_timeout(&store).unwrap(), None);
        store.set(TIMEOUT_KEY, serde_json::json!(-1));
        assert!(read_timeout(&store).is_err());
    }

    #[test]
    fn failed_save_restores_the_cached_preference() {
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        let store = app
            .store_builder(&path)
            .disable_auto_save()
            .build()
            .unwrap();
        save_timeout(&store, Some(1200)).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(save_timeout(&store, Some(1800)).is_err());
        assert_eq!(read_timeout(&store).unwrap(), Some(1200));
    }
}
