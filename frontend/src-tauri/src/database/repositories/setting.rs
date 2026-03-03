use crate::database::models::{Setting, TranscriptSetting};
use crate::summary::CustomOpenAIConfig;
use sqlx::SqlitePool;

#[derive(serde::Deserialize, Debug)]
pub struct SaveModelConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "ollamaEndpoint")]
    pub ollama_endpoint: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
pub struct SaveTranscriptConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
}

pub struct SettingsRepository;

// Transcript providers: localWhisper, deepgram, elevenLabs, groq, openai
// Summary providers: openai, claude, ollama, groq, added openrouter
// NOTE: Handle data exclusion in the higher layer as this is database abstraction layer(using SELECT *)

impl SettingsRepository {
    pub async fn get_model_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<Setting>, sqlx::Error> {
        let setting = sqlx::query_as::<_, Setting>("SELECT * FROM settings LIMIT 1")
            .fetch_optional(pool)
            .await?;
        Ok(setting)
    }

    pub async fn save_model_config(
        pool: &SqlitePool,
        provider: &str,
        model: &str,
        whisper_model: &str,
        ollama_endpoint: Option<&str>,
    ) -> std::result::Result<(), sqlx::Error> {
        // Using id '1' for backward compatibility
        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, ollamaEndpoint)
            VALUES ('1', $1, $2, $3, $4)
            ON CONFLICT(id) DO UPDATE SET
                provider = excluded.provider,
                model = excluded.model,
                whisperModel = excluded.whisperModel,
                ollamaEndpoint = excluded.ollamaEndpoint
            "#,
        )
        .bind(provider)
        .bind(model)
        .bind(whisper_model)
        .bind(ollama_endpoint)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Store an API key in the platform keychain (not in the database).
    pub async fn save_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        // Custom OpenAI uses JSON config (customOpenAIConfig) instead of a separate API key column
        if provider == "custom-openai" {
            return Err(sqlx::Error::Protocol(
                "custom-openai provider should use save_custom_openai_config() instead of save_api_key()".into(),
            ));
        }

        if provider == "builtin-ai" {
            return Ok(()); // No API key needed
        }

        // Validate provider
        match provider {
            "openai" | "claude" | "ollama" | "groq" | "openrouter" => {}
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        }

        // Store in keychain instead of database
        crate::credentials::store_api_key(provider, api_key)
            .map_err(|e| sqlx::Error::Protocol(e.into()))?;

        // Ensure a settings row exists (for other columns like provider/model)
        let _ = sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel)
            VALUES ('1', 'openai', 'gpt-4o-2024-11-20', 'large-v3')
            ON CONFLICT(id) DO NOTHING
            "#,
        )
        .execute(pool)
        .await;

        Ok(())
    }

    /// Retrieve an API key from the platform keychain (not from the database).
    pub async fn get_api_key(
        _pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        // Custom OpenAI uses JSON config - extract API key from keychain
        if provider == "custom-openai" {
            return crate::credentials::get_api_key("custom-openai")
                .map_err(|e| sqlx::Error::Protocol(e.into()));
        }

        if provider == "builtin-ai" {
            return Ok(None); // No API key needed
        }

        match provider {
            "openai" | "claude" | "ollama" | "groq" | "openrouter" => {}
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        }

        crate::credentials::get_api_key(provider)
            .map_err(|e| sqlx::Error::Protocol(e.into()))
    }

    pub async fn get_transcript_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<TranscriptSetting>, sqlx::Error> {
        let setting =
            sqlx::query_as::<_, TranscriptSetting>("SELECT * FROM transcript_settings LIMIT 1")
                .fetch_optional(pool)
                .await?;
        Ok(setting)

    }

    pub async fn save_transcript_config(
        pool: &SqlitePool,
        provider: &str,
        model: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO transcript_settings (id, provider, model)
            VALUES ('1', $1, $2)
            ON CONFLICT(id) DO UPDATE SET
                provider = excluded.provider,
                model = excluded.model
            "#,
        )
        .bind(provider)
        .bind(model)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Store a transcript API key in the platform keychain.
    pub async fn save_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        // Validate provider
        match provider {
            "localWhisper" | "deepgram" | "elevenLabs" | "groq" | "openai" => {}
            "parakeet" => return Ok(()), // Parakeet doesn't need an API key, return early
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        }

        // Store in keychain with "transcript-" prefix
        let keychain_key = format!("transcript-{}", provider);
        crate::credentials::store_api_key(&keychain_key, api_key)
            .map_err(|e| sqlx::Error::Protocol(e.into()))?;

        // Ensure a transcript_settings row exists
        let _ = sqlx::query(
            r#"
            INSERT INTO transcript_settings (id, provider, model)
            VALUES ('1', 'parakeet', $1)
            ON CONFLICT(id) DO NOTHING
            "#,
        )
        .bind(crate::config::DEFAULT_PARAKEET_MODEL)
        .execute(pool)
        .await;

        Ok(())
    }

    /// Retrieve a transcript API key from the platform keychain.
    pub async fn get_transcript_api_key(
        _pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        match provider {
            "localWhisper" | "deepgram" | "elevenLabs" | "groq" | "openai" => {}
            "parakeet" => return Ok(None), // Parakeet doesn't need an API key
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        }

        let keychain_key = format!("transcript-{}", provider);
        crate::credentials::get_api_key(&keychain_key)
            .map_err(|e| sqlx::Error::Protocol(e.into()))
    }

    /// Delete an API key from the platform keychain.
    pub async fn delete_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        // Custom OpenAI: delete from keychain AND clear DB config
        if provider == "custom-openai" {
            // Delete key from keychain (ignore if not found)
            let _ = crate::credentials::delete_api_key("custom-openai");
            // Also clear the JSON config from database
            sqlx::query("UPDATE settings SET customOpenAIConfig = NULL WHERE id = '1'")
                .execute(pool)
                .await?;
            return Ok(());
        }

        if provider == "builtin-ai" {
            return Ok(()); // No API key needed
        }

        match provider {
            "openai" | "claude" | "ollama" | "groq" | "openrouter" => {}
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        }

        crate::credentials::delete_api_key(provider)
            .map_err(|e| sqlx::Error::Protocol(e.into()))
    }

    // ===== CUSTOM OPENAI CONFIG METHODS =====

    /// Gets the custom OpenAI configuration from DB JSON, with the API key
    /// retrieved from the platform keychain and merged back in.
    ///
    /// # Returns
    /// * `Ok(Some(CustomOpenAIConfig))` - Config exists and is valid JSON
    /// * `Ok(None)` - No config stored
    /// * `Err(sqlx::Error)` - Database error
    pub async fn get_custom_openai_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<CustomOpenAIConfig>, sqlx::Error> {
        use sqlx::Row;

        let row = sqlx::query(
            r#"
            SELECT customOpenAIConfig
            FROM settings
            WHERE id = '1'
            LIMIT 1
            "#
        )
        .fetch_optional(pool)
        .await?;

        match row {
            Some(record) => {
                let config_json: Option<String> = record.get("customOpenAIConfig");

                if let Some(json) = config_json {
                    // Parse JSON into CustomOpenAIConfig
                    let mut config: CustomOpenAIConfig = serde_json::from_str(&json)
                        .map_err(|e| sqlx::Error::Protocol(
                            format!("Invalid JSON in customOpenAIConfig: {}", e).into()
                        ))?;

                    // Merge API key from keychain
                    if config.api_key.is_none() {
                        config.api_key = crate::credentials::get_api_key("custom-openai")
                            .map_err(|e| sqlx::Error::Protocol(e.into()))?;
                    }

                    Ok(Some(config))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    /// Saves the custom OpenAI configuration as JSON, with the API key
    /// stored separately in the platform keychain.
    ///
    /// # Arguments
    /// * `pool` - Database connection pool
    /// * `config` - CustomOpenAIConfig to save (includes endpoint, apiKey, model, maxTokens, temperature, topP)
    ///
    /// # Returns
    /// * `Ok(())` - Config saved successfully
    /// * `Err(sqlx::Error)` - Database or JSON serialization error
    pub async fn save_custom_openai_config(
        pool: &SqlitePool,
        config: &CustomOpenAIConfig,
    ) -> std::result::Result<(), sqlx::Error> {
        // Extract API key and store it in the keychain
        if let Some(ref api_key) = config.api_key {
            if !api_key.is_empty() {
                crate::credentials::store_api_key("custom-openai", api_key)
                    .map_err(|e| sqlx::Error::Protocol(e.into()))?;
            }
        }

        // Save config to DB without the API key (it lives in the keychain now)
        let mut config_for_db = config.clone();
        config_for_db.api_key = None;

        let config_json = serde_json::to_string(&config_for_db)
            .map_err(|e| sqlx::Error::Protocol(
                format!("Failed to serialize config to JSON: {}", e).into()
            ))?;

        // Upsert into settings table
        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, customOpenAIConfig)
            VALUES ('1', 'custom-openai', $1, 'large-v3', $2)
            ON CONFLICT(id) DO UPDATE SET
                customOpenAIConfig = excluded.customOpenAIConfig
            "#,
        )
        .bind(&config.model)
        .bind(config_json)
        .execute(pool)
        .await?;

        Ok(())
    }
}
