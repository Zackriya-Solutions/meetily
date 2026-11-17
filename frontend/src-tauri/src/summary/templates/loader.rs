use super::defaults;
use super::types::Template;
use std::path::{Path, PathBuf};
use log::{debug, info, warn};
use once_cell::sync::Lazy;
use std::sync::RwLock;

// Global storage for the bundled templates directory path
static BUNDLED_TEMPLATES_DIR: Lazy<RwLock<Option<PathBuf>>> = Lazy::new(|| RwLock::new(None));

/// Set the bundled templates directory path (called once at app startup)
pub fn set_bundled_templates_dir(path: PathBuf) {
    info!("Bundled templates directory set to: {:?}", path);
    if let Ok(mut dir) = BUNDLED_TEMPLATES_DIR.write() {
        *dir = Some(path);
    }
}

/// Get the user's custom templates directory path
///
/// Returns the platform-specific application data directory for custom templates:
/// - macOS: ~/Library/Application Support/Meetily/templates/
/// - Windows: %APPDATA%\Meetily\templates\
/// - Linux: ~/.config/Meetily/templates/
fn get_custom_templates_dir() -> Option<PathBuf> {
    let mut path = dirs::data_dir()?;
    path.push("Meetily");
    path.push("templates");
    Some(path)
}

/// Load a template from the bundled resources directory
///
/// # Arguments
/// * `template_id` - Template identifier (without .json extension)
///
/// # Returns
/// The template JSON content if found, None otherwise
fn load_bundled_template(template_id: &str) -> Option<String> {
    let bundled_dir = BUNDLED_TEMPLATES_DIR.read().ok()?.clone()?;
    let template_path = bundled_dir.join(format!("{}.json", template_id));

    debug!("Checking for bundled template at: {:?}", template_path);

    match std::fs::read_to_string(&template_path) {
        Ok(content) => {
            info!("Loaded bundled template '{}' from {:?}", template_id, template_path);
            Some(content)
        }
        Err(e) => {
            debug!("No bundled template '{}' found: {}", template_id, e);
            None
        }
    }
}

/// Load a template from the user's custom templates directory
///
/// # Arguments
/// * `template_id` - Template identifier (without .json extension)
///
/// # Returns
/// The template JSON content if found, None otherwise
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

/// Load and parse a template by identifier
///
/// This function implements a fallback strategy:
/// 1. Check user's custom templates directory
/// 2. Check bundled resources directory (app templates)
/// 3. Fall back to built-in embedded templates
/// 4. Return error if not found in any location
///
/// # Arguments
/// * `template_id` - Template identifier (e.g., "delivery_sync", "standard_meeting")
///
/// # Returns
/// Parsed and validated Template struct
pub fn get_template(template_id: &str) -> Result<Template, String> {
    info!("Loading template: {}", template_id);

    // A user's custom file keeps a retired ID alive; otherwise the ID is
    // redirected to the template that replaced it.
    if let Some(custom_content) = load_custom_template(template_id) {
        debug!("Using custom template for '{}'", template_id);
        return validate_and_parse_template(&custom_content);
    }
    let template_id = match defaults::replacement_for_retired_template(template_id) {
        Some(replacement) => {
            info!("Template '{}' was retired; using '{}'", template_id, replacement);
            replacement
        }
        None => template_id,
    };

    // Try custom template first, then bundled, then built-in
    let json_content = if let Some(custom_content) = load_custom_template(template_id) {
        debug!("Using custom template for '{}'", template_id);
        custom_content
    } else if let Some(bundled_content) = load_bundled_template(template_id) {
        debug!("Using bundled template for '{}'", template_id);
        bundled_content
    } else if let Some(builtin_content) = defaults::get_builtin_template(template_id) {
        debug!("Using built-in template for '{}'", template_id);
        builtin_content.to_string()
    } else {
        return Err(format!(
            "Template '{}' not found. Available templates: {}",
            template_id,
            list_template_ids().join(", ")
        ));
    };

    // Parse and validate
    validate_and_parse_template(&json_content)
}

/// Validate and parse template JSON
///
/// # Arguments
/// * `json_content` - Raw JSON string
///
/// # Returns
/// Parsed and validated Template struct
pub fn validate_and_parse_template(json_content: &str) -> Result<Template, String> {
    let template: Template = serde_json::from_str(json_content)
        .map_err(|e| format!("Failed to parse template JSON: {}", e))?;

    template.validate()?;

    Ok(template)
}

/// IDs of the `.json` template files in `dir` (empty if it does not exist)
fn template_ids_in(dir: &Path, kind: &str) -> Vec<String> {
    if !dir.exists() {
        return Vec::new();
    }
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .filter_map(|entry| {
                let filename = entry.file_name();
                let id = filename.to_str()?.strip_suffix(".json")?;
                Some(id.to_string())
            })
            .collect(),
        Err(e) => {
            warn!("Failed to read {} templates directory: {}", kind, e);
            Vec::new()
        }
    }
}

/// Bundled template IDs to offer in the picker
///
/// A build or install made over an older one keeps the files of templates
/// retired since then. `get_template` loads the replacement for each, so
/// listing them would show the replacement's name once per leftover file.
/// A custom file with a retired ID is still listed: it loads as itself.
fn bundled_template_ids_in(dir: &Path) -> Vec<String> {
    template_ids_in(dir, "bundled")
        .into_iter()
        .filter(|id| defaults::replacement_for_retired_template(id).is_none())
        .collect()
}

/// List all available template identifiers
///
/// Returns a combined list of:
/// - Built-in template IDs
/// - Bundled template IDs (from app resources)
/// - Custom template IDs (from user's data directory)
pub fn list_template_ids() -> Vec<String> {
    let mut ids: Vec<String> = defaults::list_builtin_template_ids()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    // Add bundled templates if directory is set
    if let Ok(bundled_dir_lock) = BUNDLED_TEMPLATES_DIR.read() {
        if let Some(bundled_dir) = bundled_dir_lock.as_ref() {
            for id in bundled_template_ids_in(bundled_dir) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }

    // Add custom templates if directory exists
    if let Some(custom_dir) = get_custom_templates_dir() {
        for id in template_ids_in(&custom_dir, "custom") {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }

    ids.sort();
    ids
}

/// List all available templates with their metadata
///
/// Returns a list of (id, name, description) tuples
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
    fn test_get_builtin_template() {
        let template = get_template("standard_meeting");
        assert!(template.is_ok());

        let template = template.unwrap();
        assert_eq!(template.name, "Standard Meeting Notes");
        assert!(!template.sections.is_empty());
    }

    #[test]
    fn test_retired_template_ids_load_their_replacement() {
        set_bundled_templates_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("templates"),
        );
        for retired in [
            "daily_standup",
            "project_sync",
            "sales_marketing_client_call",
            "retrospective",
            "psychatric_session",
        ] {
            let replacement = defaults::replacement_for_retired_template(retired)
                .unwrap_or_else(|| panic!("'{}' has no replacement", retired));
            let loaded = get_template(retired)
                .unwrap_or_else(|e| panic!("retired '{}' failed to load: {}", retired, e));
            assert_eq!(loaded.name, get_template(replacement).unwrap().name);
        }
    }

    /// The laptop's build folder still held the five retired files on
    /// 2026-11-16, and the picker listed 11 templates instead of 6.
    #[test]
    fn test_leftover_retired_files_are_not_listed() {
        let dir = tempfile::tempdir().unwrap();
        for id in [
            "delivery_sync",
            "standard_meeting",
            "daily_standup",
            "project_sync",
            "sales_marketing_client_call",
            "retrospective",
            "psychatric_session",
        ] {
            std::fs::write(dir.path().join(format!("{id}.json")), "{}").unwrap();
        }
        std::fs::write(dir.path().join("README.md"), "").unwrap();

        let mut ids = bundled_template_ids_in(dir.path());
        ids.sort();
        assert_eq!(ids, ["delivery_sync", "standard_meeting"]);
    }

    #[test]
    fn test_get_nonexistent_template() {
        let result = get_template("nonexistent_template");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_template_ids() {
        let ids = list_template_ids();
        assert!(ids.contains(&"standard_meeting".to_string()));
    }

    /// Every file shipped in `templates/` is bundled as an app resource, and a
    /// broken one only logs a warning at runtime and vanishes from the picker.
    #[test]
    fn test_bundled_template_files_are_valid() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("templates dir exists") {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let content = std::fs::read_to_string(&path).unwrap();
            let template = validate_and_parse_template(&content)
                .unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
            for section in &template.sections {
                // to_section_instructions appends its own period.
                assert!(
                    !section.instruction.trim_end().ends_with('.'),
                    "{}: instruction for '{}' ends with a period",
                    path.display(),
                    section.title
                );
            }
            checked += 1;
        }
        assert!(checked > 0, "no templates found in {}", dir.display());
    }

    #[test]
    fn test_validate_invalid_json() {
        let result = validate_and_parse_template("invalid json");
        assert!(result.is_err());
    }
}
