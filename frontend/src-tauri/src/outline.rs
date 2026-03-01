use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct OutlineCollection {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OutlineDocument {
    pub id: String,
    pub url_id: String,
}

/// Fetch all collections from an Outline instance via the Rust HTTP client,
/// bypassing WebView CSP restrictions.
#[tauri::command]
pub async fn outline_fetch_collections(
    base_url: String,
    api_key: String,
) -> Result<Vec<OutlineCollection>, String> {
    let endpoint = format!("{}/api/collections.list", base_url.trim_end_matches('/'));
    log::info!("[Outline] fetch_collections → POST {}", endpoint);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let mut body = HashMap::new();
    body.insert("limit", serde_json::json!(100));

    let response = client
        .post(&endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            log::error!("[Outline] Network error fetching collections: {}", e);
            format!("Network error: {}", e)
        })?;

    let status = response.status();
    log::info!("[Outline] fetch_collections response status: {}", status);

    if !status.is_success() {
        let err_text = response.text().await.unwrap_or_default();
        log::error!("[Outline] fetch_collections error body: {}", err_text);
        // Try to extract message from JSON
        let message = serde_json::from_str::<serde_json::Value>(&err_text)
            .ok()
            .and_then(|v| v.get("message").or(v.get("error")).and_then(|m| m.as_str()).map(String::from))
            .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
        return Err(message);
    }

    let json: serde_json::Value = response.json().await.map_err(|e| {
        log::error!("[Outline] Failed to parse collections response: {}", e);
        format!("Failed to parse response: {}", e)
    })?;

    let collections = json
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    let id = c.get("id")?.as_str()?.to_string();
                    let name = c.get("name")?.as_str()?.to_string();
                    Some(OutlineCollection { id, name })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    log::info!("[Outline] fetch_collections success, count: {}", collections.len());
    Ok(collections)
}

/// Create a document in an Outline collection. Returns the document URL.
#[tauri::command]
pub async fn outline_create_document(
    base_url: String,
    api_key: String,
    collection_id: String,
    title: String,
    text: String,
) -> Result<String, String> {
    let endpoint = format!("{}/api/documents.create", base_url.trim_end_matches('/'));
    log::info!("[Outline] create_document → POST {} (collection: {})", endpoint, collection_id);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let payload = serde_json::json!({
        "title": title,
        "text": text,
        "collectionId": collection_id,
        "publish": true,
    });

    let response = client
        .post(&endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| {
            log::error!("[Outline] Network error creating document: {}", e);
            format!("Network error: {}", e)
        })?;

    let status = response.status();
    log::info!("[Outline] create_document response status: {}", status);

    if !status.is_success() {
        let err_text = response.text().await.unwrap_or_default();
        log::error!("[Outline] create_document error body: {}", err_text);
        let message = serde_json::from_str::<serde_json::Value>(&err_text)
            .ok()
            .and_then(|v| v.get("message").or(v.get("error")).and_then(|m| m.as_str()).map(String::from))
            .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
        return Err(message);
    }

    let json: serde_json::Value = response.json().await.map_err(|e| {
        log::error!("[Outline] Failed to parse create_document response: {}", e);
        format!("Failed to parse response: {}", e)
    })?;

    log::info!("[Outline] create_document raw response: {}", json);

    // Prefer the direct url field returned by the API; fall back to building slug-urlId
    let doc_url = if let Some(url) = json.get("data").and_then(|d| d.get("url")).and_then(|v| v.as_str()) {
        // url is relative e.g. "/doc/pdf-test-BaP0cBZLzl" — prepend base_url
        if url.starts_with("http") {
            url.to_string()
        } else {
            format!("{}{}", base_url.trim_end_matches('/'), url)
        }
    } else {
        // Build slug: "{title-kebab}-{urlId}"
        let url_id = json
            .get("data")
            .and_then(|d| d.get("urlId"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let title_slug = title
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        format!("{}/doc/{}-{}", base_url.trim_end_matches('/'), title_slug, url_id)
    };

    log::info!("[Outline] create_document success: {}", doc_url);
    Ok(doc_url)
}

