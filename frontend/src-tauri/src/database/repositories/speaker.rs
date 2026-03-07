use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SpeakerProfile {
    pub id: String,
    pub name: String,
    pub color: String,
    pub created_at: String,
    #[sqlx(default)]
    pub is_self: bool,
    #[sqlx(default)]
    pub global_auto_apply: bool,
}

/// A suggested name extracted from transcript text with the pattern that matched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameSuggestion {
    pub name: String,
    pub pattern: String,
    pub speaker_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedSpeaker {
    pub speaker_id: String,
    pub display_name: String,
    pub color: String,
    pub profile_id: Option<String>,
}

const FALLBACK_COLORS: &[&str] = &[
    "#6366f1", "#f59e0b", "#10b981", "#ef4444", "#3b82f6",
    "#8b5cf6", "#ec4899", "#14b8a6", "#f97316", "#84cc16",
];

pub struct SpeakerRepository;

impl SpeakerRepository {
    pub async fn list_profiles(pool: &SqlitePool) -> Result<Vec<SpeakerProfile>, sqlx::Error> {
        sqlx::query_as::<_, SpeakerProfile>(
            "SELECT id, name, color, created_at, CAST(COALESCE(is_self, 0) AS INTEGER) as is_self, CAST(COALESCE(global_auto_apply, 0) AS INTEGER) as global_auto_apply FROM speaker_profiles ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await
    }

    pub async fn create_profile(
        pool: &SqlitePool,
        name: &str,
        color: &str,
        is_self: bool,
        global_auto_apply: bool,
    ) -> Result<SpeakerProfile, sqlx::Error> {
        let id = format!("sp-{}", Uuid::new_v4());
        let created_at = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO speaker_profiles (id, name, color, created_at, is_self, global_auto_apply) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(color)
        .bind(is_self)
        .bind(global_auto_apply)
        .bind(&created_at)
        .execute(pool)
        .await?;
        Ok(SpeakerProfile {
            id,
            name: name.to_string(),
            color: color.to_string(),
            created_at,
            is_self,
            global_auto_apply,
        })
    }

    pub async fn update_profile(
        pool: &SqlitePool,
        id: &str,
        name: &str,
        color: &str,
        is_self: bool,
        global_auto_apply: bool,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE speaker_profiles SET name = ?, color = ?, is_self = ?, global_auto_apply = ? WHERE id = ?")
            .bind(name)
            .bind(color)
            .bind(is_self)
            .bind(global_auto_apply)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Get the global self-profile (the user themselves).
    pub async fn get_self_profile(pool: &SqlitePool) -> Result<Option<SpeakerProfile>, sqlx::Error> {
        sqlx::query_as::<_, SpeakerProfile>(
            "SELECT id, name, color, created_at, CAST(COALESCE(is_self, 0) AS INTEGER) as is_self, CAST(COALESCE(global_auto_apply, 0) AS INTEGER) as global_auto_apply FROM speaker_profiles WHERE is_self = 1 LIMIT 1",
        )
        .fetch_optional(pool)
        .await
    }

    /// Get or create a mic mapping for a meeting. If a global self-profile exists,
    /// automatically creates a mapping for 'mic' in this meeting.
    pub async fn ensure_mic_mapping(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<SpeakerProfile>, sqlx::Error> {
        // Check if there's already a mapping for 'mic' in this meeting
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT profile_id FROM speaker_mappings WHERE meeting_id = ? AND speaker_id = 'mic'",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await?;

        if let Some((profile_id,)) = existing {
            return sqlx::query_as::<_, SpeakerProfile>(
                "SELECT id, name, color, created_at, CAST(COALESCE(is_self, 0) AS INTEGER) as is_self, CAST(COALESCE(global_auto_apply, 0) AS INTEGER) as global_auto_apply FROM speaker_profiles WHERE id = ?",
            )
            .bind(&profile_id)
            .fetch_optional(pool)
            .await;
        }

        // No mapping — check for a global self-profile to auto-apply
        let self_profile = Self::get_self_profile(pool).await?;
        if let Some(ref profile) = self_profile {
            sqlx::query(
                "INSERT OR IGNORE INTO speaker_mappings (meeting_id, speaker_id, profile_id) VALUES (?, 'mic', ?)",
            )
            .bind(meeting_id)
            .bind(&profile.id)
            .execute(pool)
            .await?;
        }
        Ok(self_profile)
    }

    /// Detect speaker names from transcript text using simple NLP patterns.
    /// Returns suggestions of (speaker_id, name, pattern_description).
    pub async fn detect_names_from_transcripts(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<NameSuggestion>, sqlx::Error> {
        // Fetch all transcript rows with speaker info
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT COALESCE(speaker, 'unknown'), transcript FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time ASC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;

        let mut suggestions: Vec<NameSuggestion> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        let intro_patterns: &[(&str, &str)] = &[
            // "I'm James" / "I am James"
            (r"(?i)\bi'?m\s+([A-Z][a-z]{1,20})\b", "Self-introduction (I'm ...)"),
            (r"(?i)\bi\s+am\s+([A-Z][a-z]{1,20})\b", "Self-introduction (I am ...)"),
            // "My name is James"
            (r"(?i)\bmy\s+name\s+is\s+([A-Z][a-z]{1,20})\b", "Name introduction"),
            // "This is James" / "this is James speaking"
            (r"(?i)\bthis\s+is\s+([A-Z][a-z]{1,20})\b", "This is ..."),
            // "Hi I'm James" / "Hello I'm James"
            (r"(?i)\b(?:hi|hello|hey),?\s+i'?m\s+([A-Z][a-z]{1,20})\b", "Greeting introduction"),
            // "[Name] speaking" at start of segment
            (r"(?i)^([A-Z][a-z]{1,20})\s+speaking\b", "Speaker tag"),
            // "Thanks James" / "Thanks, James" — identifies the other person by name
            (r"(?i)\bthanks?,?\s+([A-Z][a-z]{1,20})\b", "Named in address"),
            // "over to you James" / "back to you James"
            (r"(?i)\bto\s+you,?\s+([A-Z][a-z]{1,20})\b", "Handover address"),
        ];

        // Common words to ignore (not real names)
        let ignore_words: std::collections::HashSet<&str> = [
            "the", "a", "an", "and", "or", "but", "for", "so", "yet",
            "this", "that", "there", "here", "it", "he", "she", "they",
            "we", "you", "i", "me", "my", "your", "our", "their",
            "is", "am", "are", "was", "were", "be", "been", "being",
            "have", "has", "had", "do", "does", "did", "will", "would",
            "could", "should", "may", "might", "shall", "can", "some",
            "not", "no", "yes", "ok", "okay", "sure", "just", "well",
            "right", "good", "great", "thanks", "thank", "please",
            "going", "know", "think", "also", "actually", "really",
        ].iter().copied().collect();

        for (speaker_id, text) in &rows {
            for (pattern_str, pattern_desc) in intro_patterns {
                let re = match regex::Regex::new(pattern_str) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                if let Some(caps) = re.captures(text) {
                    if let Some(name_match) = caps.get(1) {
                        let name = name_match.as_str().trim().to_string();
                        let key = format!("{}:{}", speaker_id, name.to_lowercase());
                        if !seen.contains(&key) && !ignore_words.contains(name.to_lowercase().as_str()) && name.len() >= 2 {
                            seen.insert(key);
                            suggestions.push(NameSuggestion {
                                speaker_id: speaker_id.clone(),
                                name,
                                pattern: pattern_desc.to_string(),
                            });
                        }
                    }
                }
            }
        }

        Ok(suggestions)
    }


    pub async fn delete_profile(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM speaker_profiles WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn set_mapping(
        pool: &SqlitePool,
        meeting_id: &str,
        speaker_id: &str,
        profile_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR REPLACE INTO speaker_mappings (meeting_id, speaker_id, profile_id) VALUES (?, ?, ?)",
        )
        .bind(meeting_id)
        .bind(speaker_id)
        .bind(profile_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn remove_mapping(
        pool: &SqlitePool,
        meeting_id: &str,
        speaker_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "DELETE FROM speaker_mappings WHERE meeting_id = ? AND speaker_id = ?",
        )
        .bind(meeting_id)
        .bind(speaker_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_mappings(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<(String, String)>, sqlx::Error> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT speaker_id, profile_id FROM speaker_mappings WHERE meeting_id = ? ORDER BY speaker_id ASC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_resolved_speakers(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<ResolvedSpeaker>, sqlx::Error> {
        // Collect all distinct non-null speakers in this meeting (including 'mic')
        let raw_ids: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT speaker FROM transcripts
             WHERE meeting_id = ? AND speaker IS NOT NULL AND speaker != '' AND speaker != 'unknown'
             ORDER BY speaker ASC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;

        if raw_ids.is_empty() {
            return Ok(vec![]);
        }

        let mappings = Self::get_mappings(pool, meeting_id).await?;
        let mapping_map: std::collections::HashMap<String, String> =
            mappings.into_iter().collect();

        let all_profiles = Self::list_profiles(pool).await?;
        let profiles: std::collections::HashMap<String, SpeakerProfile> =
            all_profiles.into_iter().map(|p| (p.id.clone(), p)).collect();

        // Auto-apply self-profile to 'mic' if no mapping exists yet
        let self_profile = Self::get_self_profile(pool).await?;

        // Build a speaker number index only for diarization speakers (for fallback naming)
        let mut diarization_idx = 0usize;

        let result = raw_ids
            .into_iter()
            .map(|(speaker_id,)| {
                let profile_id = mapping_map.get(&speaker_id)
                    .cloned()
                    .or_else(|| {
                        // Auto-apply self profile to mic if not explicitly mapped
                        if speaker_id == "mic" {
                            self_profile.as_ref().map(|p| p.id.clone())
                        } else {
                            None
                        }
                    });
                let profile = profile_id.as_ref().and_then(|pid| profiles.get(pid));

                let (fallback_name, fallback_color) = if speaker_id == "mic" {
                    ("Speaker 1 (You)".to_string(), FALLBACK_COLORS[0].to_string())
                } else if speaker_id == "system" {
                    ("System Audio".to_string(), "#64748b".to_string())
                } else {
                    // diarization speaker
                    let speaker_num = speaker_id
                        .strip_prefix("speaker_")
                        .and_then(|n| n.parse::<usize>().ok())
                        .unwrap_or(diarization_idx);
                    let color = FALLBACK_COLORS[diarization_idx % FALLBACK_COLORS.len()];
                    diarization_idx += 1;
                    (format!("Speaker {}", speaker_num + 1), color.to_string())
                };

                ResolvedSpeaker {
                    speaker_id,
                    display_name: profile.map(|p| p.name.clone()).unwrap_or(fallback_name),
                    color: profile.map(|p| p.color.clone()).unwrap_or(fallback_color),
                    profile_id,
                }
            })
            .collect();

        Ok(result)
    }
}
