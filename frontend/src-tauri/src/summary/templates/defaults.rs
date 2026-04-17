/// Embedded default templates using compile-time inclusion
///
/// These templates are bundled into the binary and serve as fallbacks
/// when custom templates are not available.

/// 표준 회의록 (기본 한국어 템플릿)
pub const KO_STANDARD_MEETING: &str = include_str!("../../../templates/ko_standard_meeting.json");

/// 1:1 미팅 템플릿
pub const KO_ONE_ON_ONE: &str = include_str!("../../../templates/ko_one_on_one.json");

/// 고객 미팅 템플릿
pub const KO_CLIENT_CALL: &str = include_str!("../../../templates/ko_client_call.json");

/// 데일리 스탠드업 템플릿
pub const KO_DAILY_STANDUP: &str = include_str!("../../../templates/ko_daily_standup.json");

/// 회고 템플릿
pub const KO_RETROSPECTIVE: &str = include_str!("../../../templates/ko_retrospective.json");

/// Registry of all built-in templates
///
/// Maps template identifiers to their embedded JSON content
pub fn get_builtin_templates() -> Vec<(&'static str, &'static str)> {
    vec![
        ("ko_standard_meeting", KO_STANDARD_MEETING),
        ("ko_one_on_one", KO_ONE_ON_ONE),
        ("ko_client_call", KO_CLIENT_CALL),
        ("ko_daily_standup", KO_DAILY_STANDUP),
        ("ko_retrospective", KO_RETROSPECTIVE),
    ]
}

/// Get a built-in template by identifier
pub fn get_builtin_template(id: &str) -> Option<&'static str> {
    match id {
        "ko_standard_meeting" => Some(KO_STANDARD_MEETING),
        "ko_one_on_one" => Some(KO_ONE_ON_ONE),
        "ko_client_call" => Some(KO_CLIENT_CALL),
        "ko_daily_standup" => Some(KO_DAILY_STANDUP),
        "ko_retrospective" => Some(KO_RETROSPECTIVE),
        _ => None,
    }
}

/// List all built-in template identifiers
pub fn list_builtin_template_ids() -> Vec<&'static str> {
    vec![
        "ko_standard_meeting",
        "ko_one_on_one",
        "ko_client_call",
        "ko_daily_standup",
        "ko_retrospective",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_templates_valid_json() {
        for (id, content) in get_builtin_templates() {
            let result = serde_json::from_str::<serde_json::Value>(content);
            assert!(
                result.is_ok(),
                "Built-in template '{}' contains invalid JSON: {:?}",
                id,
                result.err()
            );
        }
    }

    #[test]
    fn test_get_builtin_template() {
        assert!(get_builtin_template("ko_standard_meeting").is_some());
        assert!(get_builtin_template("ko_one_on_one").is_some());
        assert!(get_builtin_template("ko_client_call").is_some());
        assert!(get_builtin_template("ko_daily_standup").is_some());
        assert!(get_builtin_template("ko_retrospective").is_some());
        assert!(get_builtin_template("nonexistent").is_none());
    }

    #[test]
    fn test_list_builtin_template_ids() {
        let ids = list_builtin_template_ids();
        assert_eq!(ids.len(), 5);
        assert!(ids.contains(&"ko_standard_meeting"));
    }
}
