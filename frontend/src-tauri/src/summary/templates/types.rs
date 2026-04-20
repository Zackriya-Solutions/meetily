use serde::{Deserialize, Serialize};

/// Represents a structured question within a template section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateQuestion {
    pub question: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_guidance: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_instead_values: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cqc_kloes: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensitive: Option<bool>,
}

/// Represents a single section in a meeting template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateSection {
    pub title: String,

    pub instruction: String,

    /// Format type: "paragraph", "list", or "string"
    pub format: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_format: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example_item_format: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub questions: Option<Vec<TemplateQuestion>>,
}

/// Represents a complete meeting template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub name: String,

    pub description: String,

    pub sections: Vec<TemplateSection>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_instruction: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clinical_safety_rules: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_addenda: Option<Vec<TemplateSection>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

impl Template {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("Template name cannot be empty".to_string());
        }

        if self.description.is_empty() {
            return Err("Template description cannot be empty".to_string());
        }

        if self.sections.is_empty() {
            return Err("Template must have at least one section".to_string());
        }

        for (i, section) in self.sections.iter().enumerate() {
            if section.title.is_empty() {
                return Err(format!("Section {} has empty title", i));
            }

            if section.instruction.is_empty() {
                return Err(format!("Section '{}' has empty instruction", section.title));
            }

            match section.format.as_str() {
                "paragraph" | "list" | "string" => {},
                other => return Err(format!(
                    "Section '{}' has invalid format '{}'. Must be 'paragraph', 'list', or 'string'",
                    section.title, other
                )),
            }
        }

        Ok(())
    }

    /// Generates a clean markdown template structure including questions and addenda
    pub fn to_markdown_structure(&self) -> String {
        let mut markdown = String::from("# <Add Title here>\n\n");

        for section in &self.sections {
            markdown.push_str(&format!("**{}**\n\n", section.title));

            if let Some(questions) = &section.questions {
                for q in questions {
                    markdown.push_str(&format!("- {}\n", q.question));
                }
                markdown.push('\n');
            }
        }

        if let Some(addenda) = &self.output_addenda {
            for section in addenda {
                markdown.push_str(&format!("**{}**\n\n", section.title));
            }
        }

        markdown
    }

    /// Generates the full LLM instruction block including global rules and per-section detail
    pub fn to_section_instructions(&self) -> String {
        let mut instructions = String::new();

        if let Some(global) = &self.global_instruction {
            instructions.push_str(&format!("**GLOBAL RULES (apply to ALL sections):**\n{}\n\n", global));
        }

        if let Some(rules) = &self.clinical_safety_rules {
            if !rules.is_empty() {
                instructions.push_str("**CLINICAL SAFETY RULES:**\n");
                for rule in rules {
                    instructions.push_str(&format!("- {}\n", rule));
                }
                instructions.push('\n');
            }
        }

        instructions.push_str(
            "- **For the main title (`# [AI-Generated Title]`):** Analyse the entire transcript and create a concise, descriptive title.\n"
        );

        for section in &self.sections {
            instructions.push_str(&format!(
                "- **For the '{}' section:** {}\n",
                section.title, section.instruction
            ));

            let item_format = section.item_format.as_ref()
                .or(section.example_item_format.as_ref());

            if let Some(format) = item_format {
                instructions.push_str(&format!(
                    "  - Items in this section should follow the format: `{}`\n",
                    format
                ));
            }

            if let Some(questions) = &section.questions {
                instructions.push_str("  - **Questions to address:**\n");
                for (i, q) in questions.iter().enumerate() {
                    instructions.push_str(&format!("    {}. {}\n", i + 1, q.question));

                    if let Some(guidance) = &q.evidence_guidance {
                        instructions.push_str("       Evidence to extract:\n");
                        for g in guidance {
                            instructions.push_str(&format!("       - {}\n", g));
                        }
                    }

                    if q.sensitive == Some(true) {
                        instructions.push_str("       ⚠ SENSITIVE — capture verbatim under concerns for human review, do not summarise or classify.\n");
                    }
                }
            }
        }

        if let Some(addenda) = &self.output_addenda {
            instructions.push('\n');
            for section in addenda {
                instructions.push_str(&format!(
                    "- **For the '{}' section:** {}\n",
                    section.title, section.instruction
                ));

                let item_format = section.item_format.as_ref()
                    .or(section.example_item_format.as_ref());

                if let Some(format) = item_format {
                    instructions.push_str(&format!(
                        "  - Items in this section should follow the format: `{}`\n",
                        format
                    ));
                }
            }
        }

        instructions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_template() {
        let template = Template {
            name: "Test Template".to_string(),
            description: "A test template".to_string(),
            sections: vec![
                TemplateSection {
                    title: "Summary".to_string(),
                    instruction: "Provide a summary".to_string(),
                    format: "paragraph".to_string(),
                    item_format: None,
                    example_item_format: None,
                    questions: None,
                },
            ],
            global_instruction: None,
            clinical_safety_rules: None,
            output_addenda: None,
            version: None,
            updated_at: None,
        };

        assert!(template.validate().is_ok());
    }

    #[test]
    fn test_validate_empty_name() {
        let template = Template {
            name: "".to_string(),
            description: "A test template".to_string(),
            sections: vec![],
            global_instruction: None,
            clinical_safety_rules: None,
            output_addenda: None,
            version: None,
            updated_at: None,
        };

        assert!(template.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_format() {
        let template = Template {
            name: "Test".to_string(),
            description: "Test".to_string(),
            sections: vec![
                TemplateSection {
                    title: "Test".to_string(),
                    instruction: "Test".to_string(),
                    format: "invalid".to_string(),
                    item_format: None,
                    example_item_format: None,
                    questions: None,
                },
            ],
            global_instruction: None,
            clinical_safety_rules: None,
            output_addenda: None,
            version: None,
            updated_at: None,
        };

        assert!(template.validate().is_err());
    }

    #[test]
    fn test_section_instructions_with_questions() {
        let template = Template {
            name: "Interview".to_string(),
            description: "Interview template".to_string(),
            sections: vec![
                TemplateSection {
                    title: "Introduction".to_string(),
                    instruction: "Extract evidence from answers".to_string(),
                    format: "list".to_string(),
                    item_format: None,
                    example_item_format: None,
                    questions: Some(vec![
                        TemplateQuestion {
                            question: "Tell me about yourself".to_string(),
                            evidence_guidance: Some(vec![
                                "Background stated".to_string(),
                                "Motivation for care".to_string(),
                            ]),
                            home_instead_values: None,
                            cqc_kloes: None,
                            sensitive: None,
                        },
                    ]),
                },
            ],
            global_instruction: Some("Use UK English throughout.".to_string()),
            clinical_safety_rules: Some(vec!["Do not produce clinical advice.".to_string()]),
            output_addenda: None,
            version: None,
            updated_at: None,
        };

        let instructions = template.to_section_instructions();
        assert!(instructions.contains("GLOBAL RULES"));
        assert!(instructions.contains("UK English"));
        assert!(instructions.contains("CLINICAL SAFETY"));
        assert!(instructions.contains("Tell me about yourself"));
        assert!(instructions.contains("Background stated"));
    }

    #[test]
    fn test_markdown_structure_with_questions() {
        let template = Template {
            name: "Test".to_string(),
            description: "Test".to_string(),
            sections: vec![
                TemplateSection {
                    title: "Section One".to_string(),
                    instruction: "Do things".to_string(),
                    format: "list".to_string(),
                    item_format: None,
                    example_item_format: None,
                    questions: Some(vec![
                        TemplateQuestion {
                            question: "What is your name?".to_string(),
                            evidence_guidance: None,
                            home_instead_values: None,
                            cqc_kloes: None,
                            sensitive: None,
                        },
                    ]),
                },
            ],
            global_instruction: None,
            clinical_safety_rules: None,
            output_addenda: Some(vec![
                TemplateSection {
                    title: "Concerns".to_string(),
                    instruction: "List concerns".to_string(),
                    format: "list".to_string(),
                    item_format: None,
                    example_item_format: None,
                    questions: None,
                },
            ]),
            version: None,
            updated_at: None,
        };

        let md = template.to_markdown_structure();
        assert!(md.contains("**Section One**"));
        assert!(md.contains("- What is your name?"));
        assert!(md.contains("**Concerns**"));
    }
}
