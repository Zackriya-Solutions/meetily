use super::types::Template;
use std::path::PathBuf;
use tracing::{debug, info, warn};
use once_cell::sync::Lazy;
use std::sync::RwLock;

// Global storage for the synced (MongoDB-cached) templates directory path
static SYNCED_TEMPLATES_DIR: Lazy<RwLock<Option<PathBuf>>> = Lazy::new(|| RwLock::new(None));

/// Set the synced templates directory path (called once at app startup)
pub fn set_synced_templates_dir(path: PathBuf) {
    info!("Synced templates directory set to: {:?}", path);
    if let Ok(mut dir) = SYNCED_TEMPLATES_DIR.write() {
        *dir = Some(path);
    }
}

/// Get the user's custom templates directory path
///
/// Returns the platform-specific application data directory for custom templates:
/// - macOS: ~/Library/Application Support/IQcapture/templates/
/// - Windows: %APPDATA%\IQcapture\templates\
/// - Linux: ~/.config/IQcapture/templates/
fn get_custom_templates_dir() -> Option<PathBuf> {
    let mut path = dirs::data_dir()?;
    path.push("IQcapture");
    path.push("templates");
    Some(path)
}

/// Load a template from the user's custom templates directory
fn load_custom_template(template_id: &str) -> Option<String> {
    let custom_dir = get_custom_templates_dir()?;
    let template_path = custom_dir.join(format!("{}.json", template_id));

    debug!("Checking for custom template at: {:?}", template_path);

    match std::fs::read_to_string(&template_path) {
        Ok(content) => {
            info!("Loaded custom template '{}' from {:?}", template_id, template_path);
            Some(content)
        }
        Err(e) => {
            debug!("No custom template '{}' found: {}", template_id, e);
            None
        }
    }
}

/// Load a template from the synced (MongoDB-cached) templates directory
fn load_synced_template(template_id: &str) -> Option<String> {
    let synced_dir = SYNCED_TEMPLATES_DIR.read().ok()?.clone()?;
    let template_path = synced_dir.join(format!("{}.json", template_id));

    debug!("Checking for synced template at: {:?}", template_path);

    match std::fs::read_to_string(&template_path) {
        Ok(content) => {
            info!("Loaded synced template '{}' from {:?}", template_id, template_path);
            Some(content)
        }
        Err(e) => {
            debug!("No synced template '{}' found: {}", template_id, e);
            None
        }
    }
}

/// Save a template to the synced templates directory (called during sync from MongoDB)
pub fn save_synced_template(template_id: &str, json_content: &str) -> Result<(), String> {
    let synced_dir = SYNCED_TEMPLATES_DIR
        .read()
        .map_err(|e| format!("Failed to read synced templates dir lock: {}", e))?
        .clone()
        .ok_or_else(|| "Synced templates directory not initialised".to_string())?;

    if !synced_dir.exists() {
        std::fs::create_dir_all(&synced_dir)
            .map_err(|e| format!("Failed to create synced templates directory: {}", e))?;
    }

    let template_path = synced_dir.join(format!("{}.json", template_id));
    std::fs::write(&template_path, json_content)
        .map_err(|e| format!("Failed to write synced template '{}': {}", template_id, e))?;

    debug!("Saved synced template '{}' to {:?}", template_id, template_path);
    Ok(())
}

/// Remove synced templates that are no longer present in MongoDB.
/// Called after a successful sync with the set of active template IDs.
pub fn remove_stale_synced_templates(active_ids: &std::collections::HashSet<String>) -> u32 {
    let synced_dir = match SYNCED_TEMPLATES_DIR.read().ok().and_then(|d| d.clone()) {
        Some(dir) if dir.exists() => dir,
        _ => return 0,
    };

    let mut removed = 0u32;
    if let Ok(entries) = std::fs::read_dir(&synced_dir) {
        for entry in entries.flatten() {
            if let Some(filename) = entry.file_name().to_str() {
                if filename.ends_with(".json") {
                    let id = filename.trim_end_matches(".json");
                    if !active_ids.contains(id) {
                        if let Err(e) = std::fs::remove_file(entry.path()) {
                            warn!("Failed to remove stale synced template '{}': {}", id, e);
                        } else {
                            info!("Removed stale synced template '{}'", id);
                            removed += 1;
                        }
                    }
                }
            }
        }
    }

    removed
}

/// Load and parse a template by identifier
///
/// Fallback strategy:
/// 1. Check user's custom templates directory
/// 2. Check synced templates directory (cached from MongoDB)
/// 3. Return error if not found
pub fn get_template(template_id: &str) -> Result<Template, String> {
    info!("Loading template: {}", template_id);

    let json_content = if let Some(custom_content) = load_custom_template(template_id) {
        debug!("Using custom template for '{}'", template_id);
        custom_content
    } else if let Some(synced_content) = load_synced_template(template_id) {
        debug!("Using synced template for '{}'", template_id);
        synced_content
    } else {
        return Err(format!(
            "Template '{}' not found. Available templates: {}",
            template_id,
            list_template_ids().join(", ")
        ));
    };

    validate_and_parse_template(&json_content)
}

/// Validate and parse template JSON
pub fn validate_and_parse_template(json_content: &str) -> Result<Template, String> {
    let template: Template = serde_json::from_str(json_content)
        .map_err(|e| format!("Failed to parse template JSON: {}", e))?;

    template.validate()?;

    Ok(template)
}

/// List all available template identifiers
///
/// Returns a combined list of synced and custom template IDs.
pub fn list_template_ids() -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();

    // Add synced templates if directory is set
    if let Ok(synced_dir_lock) = SYNCED_TEMPLATES_DIR.read() {
        if let Some(synced_dir) = synced_dir_lock.as_ref() {
            if synced_dir.exists() {
                match std::fs::read_dir(synced_dir) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            if let Some(filename) = entry.file_name().to_str() {
                                if filename.ends_with(".json") {
                                    let id = filename.trim_end_matches(".json").to_string();
                                    if !ids.contains(&id) {
                                        ids.push(id);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read synced templates directory: {}", e);
                    }
                }
            }
        }
    }

    // Add custom templates if directory exists
    if let Some(custom_dir) = get_custom_templates_dir() {
        if custom_dir.exists() {
            match std::fs::read_dir(&custom_dir) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        if let Some(filename) = entry.file_name().to_str() {
                            if filename.ends_with(".json") {
                                let id = filename.trim_end_matches(".json").to_string();
                                if !ids.contains(&id) {
                                    ids.push(id);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to read custom templates directory: {}", e);
                }
            }
        }
    }

    ids.sort();
    ids
}

/// List all available templates with their metadata
pub fn list_templates() -> Vec<(String, String, String)> {
    let mut templates = Vec::new();

    for id in list_template_ids() {
        match get_template(&id) {
            Ok(template) => {
                templates.push((id, template.name, template.description));
            }
            Err(e) => {
                warn!("Failed to load template '{}': {}", id, e);
            }
        }
    }

    templates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_nonexistent_template() {
        let result = get_template("nonexistent_template");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_invalid_json() {
        let result = validate_and_parse_template("invalid json");
        assert!(result.is_err());
    }
}
