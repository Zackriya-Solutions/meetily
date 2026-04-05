use crate::database::models::{Setting, TranscriptSetting};
use crate::summary::CustomOpenAIConfig;
use keyring::{Entry, Error as KeyringError};
use sqlx::{Row, SqlitePool};

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

const SECRET_SERVICE: &str = "ai.meetfree.desktop";
const SUMMARY_SECRET_SCOPE: &str = "summary";
const TRANSCRIPT_SECRET_SCOPE: &str = "transcript";
const CUSTOM_OPENAI_SECRET_SCOPE: &str = "custom-openai";
const CUSTOM_OPENAI_SECRET_PROVIDER: &str = "config";

#[derive(Clone, Copy)]
enum LegacySecretTable {
    Settings,
    TranscriptSettings,
}

impl LegacySecretTable {
    fn as_sql(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::TranscriptSettings => "transcript_settings",
        }
    }

    fn allows_column(self, column: &str) -> bool {
        match self {
            Self::Settings => matches!(
                column,
                "openaiApiKey"
                    | "anthropicApiKey"
                    | "ollamaApiKey"
                    | "groqApiKey"
                    | "openRouterApiKey"
            ),
            Self::TranscriptSettings => matches!(
                column,
                "whisperApiKey"
                    | "deepgramApiKey"
                    | "elevenLabsApiKey"
                    | "groqApiKey"
                    | "openaiApiKey"
            ),
        }
    }
}

// Transcript providers: localWhisper, deepgram, elevenLabs, groq, openai
// Summary providers: openai, claude, ollama, groq, openrouter, custom-openai
// Secrets are stored in OS-backed credential storage; SQLite remains a legacy migration source.

impl SettingsRepository {
    fn secure_storage_error(context: &str, err: KeyringError) -> sqlx::Error {
        sqlx::Error::Protocol(format!("{}: {}", context, err).into())
    }

    fn secure_entry(scope: &str, provider: &str) -> std::result::Result<Entry, sqlx::Error> {
        let account = format!("{}:{}", scope, provider);
        Entry::new(SECRET_SERVICE, &account).map_err(|err| {
            Self::secure_storage_error("Failed to initialize secure storage entry", err)
        })
    }

    fn read_secret(
        scope: &str,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        let entry = Self::secure_entry(scope, provider)?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(err) => Err(Self::secure_storage_error(
                "Failed to read secret from secure storage",
                err,
            )),
        }
    }

    fn store_secret(
        scope: &str,
        provider: &str,
        secret: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        if secret.trim().is_empty() {
            return Self::delete_secret(scope, provider);
        }

        let entry = Self::secure_entry(scope, provider)?;
        entry.set_password(secret).map_err(|err| {
            Self::secure_storage_error("Failed to store secret in secure storage", err)
        })
    }

    fn delete_secret(scope: &str, provider: &str) -> std::result::Result<(), sqlx::Error> {
        let entry = Self::secure_entry(scope, provider)?;
        match entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(err) => Err(Self::secure_storage_error(
                "Failed to delete secret from secure storage",
                err,
            )),
        }
    }

    fn summary_api_key_column(
        provider: &str,
    ) -> std::result::Result<Option<&'static str>, sqlx::Error> {
        match provider {
            "openai" => Ok(Some("openaiApiKey")),
            "claude" => Ok(Some("anthropicApiKey")),
            "ollama" => Ok(Some("ollamaApiKey")),
            "groq" => Ok(Some("groqApiKey")),
            "openrouter" => Ok(Some("openRouterApiKey")),
            "builtin-ai" => Ok(None),
            "custom-openai" => Ok(None),
            _ => Err(sqlx::Error::Protocol(
                format!("Invalid provider: {}", provider).into(),
            )),
        }
    }

    fn transcript_api_key_column(
        provider: &str,
    ) -> std::result::Result<Option<&'static str>, sqlx::Error> {
        match provider {
            "localWhisper" => Ok(Some("whisperApiKey")),
            "parakeet" => Ok(None),
            "deepgram" => Ok(Some("deepgramApiKey")),
            "elevenLabs" => Ok(Some("elevenLabsApiKey")),
            "groq" => Ok(Some("groqApiKey")),
            "openai" => Ok(Some("openaiApiKey")),
            _ => Err(sqlx::Error::Protocol(
                format!("Invalid provider: {}", provider).into(),
            )),
        }
    }

    async fn get_legacy_secret_from_table(
        pool: &SqlitePool,
        table: LegacySecretTable,
        column: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        if !table.allows_column(column) {
            return Err(sqlx::Error::Protocol(
                format!(
                    "Invalid legacy secret target: {}.{}",
                    table.as_sql(),
                    column
                )
                .into(),
            ));
        }

        let query = format!(
            "SELECT {column} FROM {} WHERE id = '1' LIMIT 1",
            table.as_sql()
        );
        let row = sqlx::query(&query).fetch_optional(pool).await?;

        match row {
            Some(record) => record.try_get::<Option<String>, _>(column),
            None => Ok(None),
        }
    }

    async fn clear_legacy_secret_from_table(
        pool: &SqlitePool,
        table: LegacySecretTable,
        column: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        if !table.allows_column(column) {
            return Err(sqlx::Error::Protocol(
                format!(
                    "Invalid legacy secret target: {}.{}",
                    table.as_sql(),
                    column
                )
                .into(),
            ));
        }

        let query = format!(
            "UPDATE {} SET {column} = NULL WHERE id = '1'",
            table.as_sql()
        );
        sqlx::query(&query).execute(pool).await?;
        Ok(())
    }

    async fn load_custom_openai_config_from_db(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<CustomOpenAIConfig>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT customOpenAIConfig
            FROM settings
            WHERE id = '1'
            LIMIT 1
            "#,
        )
        .fetch_optional(pool)
        .await?;

        match row {
            Some(record) => {
                let config_json: Option<String> = record.get("customOpenAIConfig");

                if let Some(json) = config_json {
                    let config: CustomOpenAIConfig = serde_json::from_str(&json).map_err(|e| {
                        sqlx::Error::Protocol(
                            format!("Invalid JSON in customOpenAIConfig: {}", e).into(),
                        )
                    })?;

                    Ok(Some(config))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    async fn save_custom_openai_config_without_secret(
        pool: &SqlitePool,
        config: &CustomOpenAIConfig,
    ) -> std::result::Result<(), sqlx::Error> {
        let mut sanitized = config.clone();
        sanitized.api_key = None;

        let config_json = serde_json::to_string(&sanitized).map_err(|e| {
            sqlx::Error::Protocol(format!("Failed to serialize config to JSON: {}", e).into())
        })?;

        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, customOpenAIConfig)
            VALUES ('1', 'custom-openai', $1, 'large-v3', $2)
            ON CONFLICT(id) DO UPDATE SET
                provider = excluded.provider,
                model = excluded.model,
                customOpenAIConfig = excluded.customOpenAIConfig
            "#,
        )
        .bind(&sanitized.model)
        .bind(config_json)
        .execute(pool)
        .await?;

        Ok(())
    }

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

    pub async fn save_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        if provider == "custom-openai" {
            return Err(sqlx::Error::Protocol(
                "custom-openai provider should use save_custom_openai_config() instead of save_api_key()".into(),
            ));
        }

        let api_key_column = match Self::summary_api_key_column(provider)? {
            Some(column) => column,
            None => return Ok(()),
        };

        Self::store_secret(SUMMARY_SECRET_SCOPE, provider, api_key)?;
        Self::clear_legacy_secret_from_table(pool, LegacySecretTable::Settings, api_key_column)
            .await?;

        Ok(())
    }

    pub async fn get_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        if provider == "custom-openai" {
            let config = Self::get_custom_openai_config(pool).await?;
            return Ok(config.and_then(|c| c.api_key));
        }

        let api_key_column = match Self::summary_api_key_column(provider)? {
            Some(column) => column,
            None => return Ok(None),
        };

        if let Some(secret) = Self::read_secret(SUMMARY_SECRET_SCOPE, provider)? {
            return Ok(Some(secret));
        }

        if let Some(legacy_secret) =
            Self::get_legacy_secret_from_table(pool, LegacySecretTable::Settings, api_key_column)
                .await?
        {
            if !legacy_secret.trim().is_empty() {
                Self::store_secret(SUMMARY_SECRET_SCOPE, provider, &legacy_secret)?;
                Self::clear_legacy_secret_from_table(
                    pool,
                    LegacySecretTable::Settings,
                    api_key_column,
                )
                .await?;
                return Ok(Some(legacy_secret));
            }

            Self::clear_legacy_secret_from_table(pool, LegacySecretTable::Settings, api_key_column)
                .await?;
        }

        Ok(None)
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

    pub async fn save_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        let api_key_column = match Self::transcript_api_key_column(provider)? {
            Some(column) => column,
            None => return Ok(()),
        };

        Self::store_secret(TRANSCRIPT_SECRET_SCOPE, provider, api_key)?;
        Self::clear_legacy_secret_from_table(
            pool,
            LegacySecretTable::TranscriptSettings,
            api_key_column,
        )
        .await?;

        Ok(())
    }

    pub async fn get_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        let api_key_column = match Self::transcript_api_key_column(provider)? {
            Some(column) => column,
            None => return Ok(None),
        };

        if let Some(secret) = Self::read_secret(TRANSCRIPT_SECRET_SCOPE, provider)? {
            return Ok(Some(secret));
        }

        if let Some(legacy_secret) = Self::get_legacy_secret_from_table(
            pool,
            LegacySecretTable::TranscriptSettings,
            api_key_column,
        )
        .await?
        {
            if !legacy_secret.trim().is_empty() {
                Self::store_secret(TRANSCRIPT_SECRET_SCOPE, provider, &legacy_secret)?;
                Self::clear_legacy_secret_from_table(
                    pool,
                    LegacySecretTable::TranscriptSettings,
                    api_key_column,
                )
                .await?;
                return Ok(Some(legacy_secret));
            }

            Self::clear_legacy_secret_from_table(
                pool,
                LegacySecretTable::TranscriptSettings,
                api_key_column,
            )
            .await?;
        }

        Ok(None)
    }

    pub async fn delete_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        if provider == "custom-openai" {
            Self::delete_secret(CUSTOM_OPENAI_SECRET_SCOPE, CUSTOM_OPENAI_SECRET_PROVIDER)?;

            if let Some(mut config) = Self::load_custom_openai_config_from_db(pool).await? {
                config.api_key = None;
                Self::save_custom_openai_config_without_secret(pool, &config).await?;
            }

            return Ok(());
        }

        let api_key_column = match Self::summary_api_key_column(provider)? {
            Some(column) => column,
            None => return Ok(()),
        };

        Self::delete_secret(SUMMARY_SECRET_SCOPE, provider)?;
        Self::clear_legacy_secret_from_table(pool, LegacySecretTable::Settings, api_key_column)
            .await?;

        Ok(())
    }

    // ===== CUSTOM OPENAI CONFIG METHODS =====

    /// Gets the custom OpenAI configuration from the database plus secure credential storage.
    pub async fn get_custom_openai_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<CustomOpenAIConfig>, sqlx::Error> {
        let config = Self::load_custom_openai_config_from_db(pool).await?;

        match config {
            Some(mut config) => {
                if let Some(legacy_key) = config.api_key.clone() {
                    if !legacy_key.trim().is_empty() {
                        Self::store_secret(
                            CUSTOM_OPENAI_SECRET_SCOPE,
                            CUSTOM_OPENAI_SECRET_PROVIDER,
                            &legacy_key,
                        )?;
                    }

                    config.api_key = None;
                    Self::save_custom_openai_config_without_secret(pool, &config).await?;
                }

                config.api_key =
                    Self::read_secret(CUSTOM_OPENAI_SECRET_SCOPE, CUSTOM_OPENAI_SECRET_PROVIDER)?;
                Ok(Some(config))
            }
            None => Ok(None),
        }
    }

    /// Saves the custom OpenAI configuration with the API key stored in secure storage.
    pub async fn save_custom_openai_config(
        pool: &SqlitePool,
        config: &CustomOpenAIConfig,
    ) -> std::result::Result<(), sqlx::Error> {
        if let Some(api_key) = config.api_key.as_deref() {
            Self::store_secret(
                CUSTOM_OPENAI_SECRET_SCOPE,
                CUSTOM_OPENAI_SECRET_PROVIDER,
                api_key,
            )?;
        } else {
            Self::delete_secret(CUSTOM_OPENAI_SECRET_SCOPE, CUSTOM_OPENAI_SECRET_PROVIDER)?;
        }

        Self::save_custom_openai_config_without_secret(pool, config).await
    }
}
