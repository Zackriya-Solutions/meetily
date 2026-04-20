use crate::summary::templates;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tauri::Runtime;
use tracing::{error, info, warn};

/// Template metadata for UI display
#[derive(Debug, Serialize, Deserialize)]
pub struct TemplateInfo {
    /// Template identifier (e.g., "daily_standup", "standard_meeting")
    pub id: String,

    /// Display name for the template
    pub name: String,

    /// Brief description of the template's purpose
    pub description: String,
}

/// Detailed template structure for preview/debugging
#[derive(Debug, Serialize, Deserialize)]
pub struct TemplateDetails {
    /// Template identifier
    pub id: String,

    /// Display name
    pub name: String,

    /// Description
    pub description: String,

    /// List of section titles in order
    pub sections: Vec<String>,
}

/// Lists all available templates
///
/// Returns templates from both built-in (embedded) and custom (user data directory) sources.
/// Templates are automatically discovered - no code changes needed to add new templates.
///
/// # Returns
/// Vector of TemplateInfo with id, name, and description for each template
#[tauri::command]
pub async fn api_list_templates<R: Runtime>(
    _app: tauri::AppHandle<R>,
) -> Result<Vec<TemplateInfo>, String> {
    info!("api_list_templates called");

    let templates = templates::list_templates();

    let template_infos: Vec<TemplateInfo> = templates
        .into_iter()
        .map(|(id, name, description)| TemplateInfo {
            id,
            name,
            description,
        })
        .collect();

    info!("Found {} available templates", template_infos.len());

    Ok(template_infos)
}

/// Gets detailed information about a specific template
///
/// # Arguments
/// * `template_id` - Template identifier (e.g., "daily_standup")
///
/// # Returns
/// TemplateDetails with full template structure
#[tauri::command]
pub async fn api_get_template_details<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_id: String,
) -> Result<TemplateDetails, String> {
    info!("api_get_template_details called for template_id: {}", template_id);

    let template = templates::get_template(&template_id)?;

    let section_titles: Vec<String> = template
        .sections
        .iter()
        .map(|section| section.title.clone())
        .collect();

    let details = TemplateDetails {
        id: template_id,
        name: template.name,
        description: template.description,
        sections: section_titles,
    };

    info!("Retrieved template details for '{}'", details.name);

    Ok(details)
}

/// Validates a custom template JSON string
///
/// Useful for template editor UI or validation before saving custom templates
///
/// # Arguments
/// * `template_json` - Raw JSON string of the template
///
/// # Returns
/// Ok(template_name) if valid, Err(error_message) if invalid
#[tauri::command]
pub async fn api_validate_template<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_json: String,
) -> Result<String, String> {
    info!("api_validate_template called");

    match templates::validate_and_parse_template(&template_json) {
        Ok(template) => {
            info!("Template '{}' validated successfully", template.name);
            Ok(template.name)
        }
        Err(e) => {
            warn!("Template validation failed: {}", e);
            Err(e)
        }
    }
}

/// Result of a template sync operation
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncResult {
    pub synced_count: u32,
    pub failed_count: u32,
    pub is_online: bool,
}

/// Sync response shape from backend API
#[derive(Debug, Deserialize)]
struct BackendSyncResponse {
    templates: Vec<BackendTemplate>,
}

#[derive(Debug, Deserialize)]
struct BackendTemplate {
    template_id: String,
    name: String,
    description: String,
    sections: Vec<serde_json::Value>,
    #[serde(default)]
    global_instruction: Option<String>,
    #[serde(default)]
    clinical_safety_rules: Option<Vec<String>>,
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    updated_at: Option<String>,
}

/// Sync templates from MongoDB directly.
/// Uses bson::Document to avoid schema mismatches with BSON types (e.g. DateTime).
async fn sync_templates_from_mongodb() -> Result<SyncResult, String> {
    use mongodb::bson::{doc, Document};
    use futures_util::TryStreamExt;

    let collection =
        crate::mongodb_client::get_collection::<Document>("templates").await?;

    let filter = doc! { "client_id": "default", "is_active": true };
    let mut cursor = collection
        .find(filter)
        .await
        .map_err(|e| format!("MongoDB template query failed: {e}"))?;

    let mut synced_count = 0u32;
    let mut failed_count = 0u32;
    let mut active_ids = HashSet::new();

    while let Some(doc) = cursor.try_next().await.map_err(|e| format!("MongoDB cursor error: {e}"))? {
        let template_id = match doc.get_str("template_id") {
            Ok(id) => id.to_string(),
            Err(_) => {
                warn!("Skipping MongoDB template without template_id");
                failed_count += 1;
                continue;
            }
        };

        active_ids.insert(template_id.clone());

        // Convert BSON arrays to serde_json::Value via full roundtrip to preserve nested fields
        let bson_to_json_array = |key: &str| -> Vec<serde_json::Value> {
            doc.get_array(key)
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|v| {
                    let json_str = serde_json::to_string(&v).ok()?;
                    serde_json::from_str(&json_str).ok()
                })
                .collect()
        };

        let sections = bson_to_json_array("sections");
        let output_addenda = bson_to_json_array("output_addenda");

        let local_json = serde_json::json!({
            "name": doc.get_str("name").unwrap_or_default(),
            "description": doc.get_str("description").unwrap_or_default(),
            "sections": sections,
            "output_addenda": if output_addenda.is_empty() { None } else { Some(output_addenda) },
            "global_instruction": doc.get_str("global_instruction").ok(),
            "clinical_safety_rules": match doc.get("clinical_safety_rules") {
                Some(mongodb::bson::Bson::Array(arr)) => {
                    Some(arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect::<Vec<_>>())
                }
                Some(mongodb::bson::Bson::Document(obj)) => {
                    obj.get_array("rules").ok().map(|rules| {
                        rules.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect::<Vec<_>>()
                    })
                }
                _ => None,
            },
            "version": doc.get_i32("version").ok(),
        });

        let json_string = serde_json::to_string_pretty(&local_json).unwrap_or_default();

        match templates::validate_and_parse_template(&json_string) {
            Ok(_) => match templates::save_synced_template(&template_id, &json_string) {
                Ok(_) => synced_count += 1,
                Err(e) => {
                    error!("Failed to save synced template '{}': {}", template_id, e);
                    failed_count += 1;
                }
            },
            Err(e) => {
                warn!("Skipping invalid template '{}' from MongoDB: {}", template_id, e);
                failed_count += 1;
            }
        }
    }

    let removed = templates::remove_stale_synced_templates(&active_ids);
    info!("MongoDB template sync: {} synced, {} failed, {} removed", synced_count, failed_count, removed);
    Ok(SyncResult {
        synced_count,
        failed_count,
        is_online: true,
    })
}

/// Sync templates from backend HTTP API (fallback).
async fn sync_templates_from_api() -> SyncResult {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to create HTTP client for template sync: {}", e);
            return SyncResult { synced_count: 0, failed_count: 0, is_online: false };
        }
    };

    let response = client
        .get(format!("{}/api/templates?client_id=default", crate::api::api::APP_SERVER_URL))
        .send()
        .await;

    let response = match response {
        Ok(r) => r,
        Err(e) => {
            info!("Template sync: backend not reachable ({}), using cached/bundled templates", e);
            return SyncResult { synced_count: 0, failed_count: 0, is_online: false };
        }
    };

    if !response.status().is_success() {
        warn!("Template sync: backend returned status {}", response.status());
        return SyncResult { synced_count: 0, failed_count: 0, is_online: false };
    }

    let body = match response.json::<BackendSyncResponse>().await {
        Ok(b) => b,
        Err(e) => {
            warn!("Template sync: failed to parse response: {}", e);
            return SyncResult { synced_count: 0, failed_count: 0, is_online: true };
        }
    };

    let mut synced_count = 0u32;
    let mut failed_count = 0u32;
    let mut active_ids = HashSet::new();

    for tmpl in &body.templates {
        active_ids.insert(tmpl.template_id.clone());

        let local_json = serde_json::json!({
            "name": tmpl.name,
            "description": tmpl.description,
            "sections": tmpl.sections,
            "global_instruction": tmpl.global_instruction,
            "clinical_safety_rules": tmpl.clinical_safety_rules,
            "version": tmpl.version,
            "updated_at": tmpl.updated_at,
        });

        let json_string = serde_json::to_string_pretty(&local_json).unwrap_or_default();

        match templates::validate_and_parse_template(&json_string) {
            Ok(_) => {
                match templates::save_synced_template(&tmpl.template_id, &json_string) {
                    Ok(_) => synced_count += 1,
                    Err(e) => {
                        error!("Failed to save synced template '{}': {}", tmpl.template_id, e);
                        failed_count += 1;
                    }
                }
            }
            Err(e) => {
                warn!("Skipping invalid template '{}' from backend: {}", tmpl.template_id, e);
                failed_count += 1;
            }
        }
    }

    let removed = templates::remove_stale_synced_templates(&active_ids);
    info!("API template sync: {} synced, {} failed, {} removed", synced_count, failed_count, removed);
    SyncResult { synced_count, failed_count, is_online: true }
}

/// Internal sync function (called from Tauri command and startup).
/// Tries MongoDB first, falls back to HTTP API.
pub async fn sync_templates_internal() -> SyncResult {
    let (result, source) = if crate::mongodb_client::is_configured() {
        match sync_templates_from_mongodb().await {
            Ok(r) => (r, "mongodb"),
            Err(e) => {
                warn!("MongoDB template sync failed, falling back to API: {}", e);
                (sync_templates_from_api().await, "api")
            }
        }
    } else {
        (sync_templates_from_api().await, "api")
    };

    // Fire PostHog analytics event
    if let Some(client) = crate::analytics::commands::get_analytics_client() {
        let mut props = HashMap::new();
        props.insert("source".to_string(), source.to_string());
        props.insert("synced_count".to_string(), result.synced_count.to_string());
        props.insert("failed_count".to_string(), result.failed_count.to_string());
        props.insert("is_online".to_string(), result.is_online.to_string());
        let _ = client.track_event("template_sync_completed", Some(props)).await;
    }

    result
}

/// Syncs templates from the backend (MongoDB) to the local synced cache
#[tauri::command]
pub async fn api_sync_templates<R: Runtime>(
    _app: tauri::AppHandle<R>,
) -> Result<SyncResult, String> {
    info!("api_sync_templates called");
    Ok(sync_templates_internal().await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_templates() {
        // This test requires the templates to be embedded/available
        // In a real test environment, you might want to mock the templates module

        // For now, just verify the function compiles and runs
        // You can expand this with more specific assertions
    }

    #[tokio::test]
    async fn test_validate_template_valid() {
        let valid_json = r#"
        {
            "name": "Test Template",
            "description": "A test template",
            "sections": [
                {
                    "title": "Summary",
                    "instruction": "Provide a summary",
                    "format": "paragraph"
                }
            ]
        }"#;

        // Mock app handle would be needed for actual testing
        // For now, test the validation logic directly
        let result = templates::validate_and_parse_template(valid_json);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_template_invalid() {
        let invalid_json = "invalid json";

        let result = templates::validate_and_parse_template(invalid_json);
        assert!(result.is_err());
    }
}
