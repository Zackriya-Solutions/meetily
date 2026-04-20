//! Meeting summary template management
//!
//! Templates are managed centrally via MongoDB and cached locally during sync.
//! Custom user templates can override synced templates.
//!
//! # Template Resolution
//!
//! 1. Custom templates (user's app data directory)
//! 2. Synced templates (cached from MongoDB)
//!
//! # Custom Templates
//!
//! Users can add custom templates to:
//! - macOS: `~/Library/Application Support/IQcapture/templates/`
//! - Windows: `%APPDATA%\IQcapture\templates\`
//! - Linux: `~/.config/IQcapture/templates/`
//!
//! Custom templates must follow the JSON schema defined in `types::Template`.

mod loader;
mod types;

// Re-export public API
pub use loader::{
    get_template, list_template_ids, list_templates, remove_stale_synced_templates,
    save_synced_template, set_synced_templates_dir, validate_and_parse_template,
};
pub use types::{Template, TemplateQuestion, TemplateSection};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_nonexistent_template() {
        let result = get_template("nonexistent_template");
        assert!(result.is_err());
    }
}
