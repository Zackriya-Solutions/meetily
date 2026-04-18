use crate::database::repositories::{
    meeting::MeetingsRepository, setting::SettingsRepository, summary::SummaryProcessesRepository,
};
use crate::summary::llm_client::LLMProvider;
use crate::summary::processor::{extract_meeting_name_from_markdown, generate_meeting_summary};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use once_cell::sync::Lazy;

// Global registry for cancellation tokens (thread-safe)
static CANCELLATION_REGISTRY: Lazy<Arc<Mutex<HashMap<String, CancellationToken>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

/// Summary service - handles all summary generation logic
pub struct SummaryService;

impl SummaryService {
    /// Registers a new cancellation token for a meeting
    fn register_cancellation_token(meeting_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        if let Ok(mut registry) = CANCELLATION_REGISTRY.lock() {
            registry.insert(meeting_id.to_string(), token.clone());
            info!("Registered cancellation token for meeting: {}", meeting_id);
        }
        token
    }

    /// Cancels the summary generation for a meeting
    pub fn cancel_summary(meeting_id: &str) -> bool {
        if let Ok(registry) = CANCELLATION_REGISTRY.lock() {
            if let Some(token) = registry.get(meeting_id) {
                info!("Cancelling summary generation for meeting: {}", meeting_id);
                token.cancel();
                return true;
            }
        }
        warn!("No active summary generation found for meeting: {}", meeting_id);
        false
    }

    /// Cleans up the cancellation token after processing completes
    fn cleanup_cancellation_token(meeting_id: &str) {
        if let Ok(mut registry) = CANCELLATION_REGISTRY.lock() {
            if registry.remove(meeting_id).is_some() {
                info!("Cleaned up cancellation token for meeting: {}", meeting_id);
            }
        }
    }

    /// Processes transcript in the background and generates summary
    ///
    /// This function is designed to be spawned as an async task and does not block
    /// the main thread. It updates the database with progress and results.
    ///
    /// # Arguments
    /// * `_app` - Tauri app handle (for future use)
    /// * `pool` - SQLx connection pool
    /// * `meeting_id` - Unique identifier for the meeting
    /// * `text` - Full transcript text
    /// * `model_provider` - LLM provider name (e.g., "custom-openai")
    /// * `model_name` - Specific model (e.g., "gpt-4", "llama3.2:latest")
    /// * `custom_prompt` - Optional user-provided context
    /// * `template_id` - Template identifier (e.g., "daily_standup", "standard_meeting")
    pub async fn process_transcript_background<R: tauri::Runtime>(
        _app: AppHandle<R>,
        pool: SqlitePool,
        meeting_id: String,
        text: String,
        model_provider: String,
        model_name: String,
        custom_prompt: String,
        template_id: String,
    ) {
        let start_time = Instant::now();
        info!(
            "Starting background processing for meeting_id: {}",
            meeting_id
        );

        // Register cancellation token for this meeting
        let cancellation_token = Self::register_cancellation_token(&meeting_id);

        // Parse provider
        let provider = match LLMProvider::from_str(&model_provider) {
            Ok(p) => p,
            Err(e) => {
                Self::update_process_failed(&pool, &meeting_id, &e).await;
                return;
            }
        };

        let (custom_openai_endpoint, final_api_key, custom_openai_max_tokens, custom_openai_temperature, custom_openai_top_p) =
            match SettingsRepository::get_custom_openai_config(&pool).await {
                Ok(Some(config)) => {
                    info!("✓ Using custom OpenAI endpoint: {}", config.endpoint);
                    (
                        Some(config.endpoint),
                        config.api_key.unwrap_or_default(),
                        config.max_tokens.map(|t| t as u32),
                        config.temperature,
                        config.top_p,
                    )
                }
                Ok(None) => {
                    let err_msg = "Custom OpenAI provider selected but no configuration found";
                    Self::update_process_failed(&pool, &meeting_id, err_msg).await;
                    return;
                }
                Err(e) => {
                    let err_msg = format!("Failed to retrieve custom OpenAI config: {}", e);
                    Self::update_process_failed(&pool, &meeting_id, &err_msg).await;
                    return;
                }
            };

        let token_threshold = 100000;

        let app_data_dir = _app.path().app_data_dir().ok();

        // Generate summary
        let client = reqwest::Client::new();
        let result = generate_meeting_summary(
            &client,
            &provider,
            &model_name,
            &final_api_key,
            &text,
            &custom_prompt,
            &template_id,
            token_threshold,
            custom_openai_endpoint.as_deref(),
            custom_openai_max_tokens,
            custom_openai_temperature,
            custom_openai_top_p,
            app_data_dir.as_ref(),
            Some(&cancellation_token),
        )
        .await;

        let duration = start_time.elapsed().as_secs_f64();

        // Clean up cancellation token regardless of outcome
        Self::cleanup_cancellation_token(&meeting_id);

        match result {
            Ok((mut final_markdown, num_chunks)) => {
                if num_chunks == 0 && final_markdown.is_empty() {
                    Self::update_process_failed(
                        &pool,
                        &meeting_id,
                        "Summary generation failed: No content was processed.",
                    )
                    .await;
                    return;
                }

                info!(
                    "✓ Successfully processed {} chunks for meeting_id: {}. Duration: {:.2}s",
                    num_chunks, meeting_id, duration
                );
                info!("final markdown is {}", &final_markdown);

                // Extract and update meeting name if present
                if let Some(name) = extract_meeting_name_from_markdown(&final_markdown) {
                    if !name.is_empty() {
                        info!(
                            "Updating meeting name to '{}' for meeting_id: {}",
                            name, meeting_id
                        );
                        if let Err(e) =
                            MeetingsRepository::update_meeting_title(&pool, &meeting_id, &name).await
                        {
                            error!("Failed to update meeting name for {}: {}", meeting_id, e);
                        }

                        // Strip the title line from markdown
                        info!("Stripping title from final_markdown");
                        if let Some(hash_pos) = final_markdown.find('#') {
                            // Find end of first line after '#'
                            let body_start =
                                if let Some(line_end) = final_markdown[hash_pos..].find('\n') {
                                    hash_pos + line_end
                                } else {
                                    final_markdown.len() // No newline, whole string is title
                                };

                            final_markdown = final_markdown[body_start..].trim_start().to_string();
                        } else {
                            // No '#' found, clear the string
                            final_markdown.clear();
                        }
                    }
                }

                // Create result JSON with markdown only (summary_json will be added on first edit)
                let result_json = serde_json::json!({
                    "markdown": final_markdown,
                });

                // Update database with completed status
                if let Err(e) = SummaryProcessesRepository::update_process_completed(
                    &pool,
                    &meeting_id,
                    result_json,
                    num_chunks,
                    duration,
                )
                .await
                {
                    error!(
                        "Failed to save completed process for {}: {}",
                        meeting_id, e
                    );
                } else {
                    info!(
                        "Summary saved successfully for meeting_id: {}",
                        meeting_id
                    );
                }
            }
            Err(e) => {
                // Check if error is due to cancellation
                if e.contains("cancelled") {
                    info!("Summary generation was cancelled for meeting_id: {}", meeting_id);
                    if let Err(db_err) = SummaryProcessesRepository::update_process_cancelled(&pool, &meeting_id).await {
                        error!("Failed to update DB status to cancelled for {}: {}", meeting_id, db_err);
                    }
                } else {
                    Self::update_process_failed(&pool, &meeting_id, &e).await;
                }
            }
        }
    }

    /// Updates the summary process status to failed with error message
    ///
    /// # Arguments
    /// * `pool` - SQLx connection pool
    /// * `meeting_id` - Meeting identifier
    /// * `error_msg` - Error message to store
    async fn update_process_failed(pool: &SqlitePool, meeting_id: &str, error_msg: &str) {
        error!(
            "Processing failed for meeting_id {}: {}",
            meeting_id, error_msg
        );
        if let Err(e) =
            SummaryProcessesRepository::update_process_failed(pool, meeting_id, error_msg).await
        {
            error!(
                "Failed to update DB status to failed for {}: {}",
                meeting_id, e
            );
        }
    }
}
