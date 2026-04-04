use serde::{Deserialize, Serialize};
use sysinfo::System;

use super::models::ModelDef;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelCompatibility {
    Recommended,
    Compatible,
    MayBeSlow,
    NotRecommended,
}

#[derive(Debug, Clone, Copy)]
pub struct SystemProfile {
    pub total_ram_gb: u64,
    pub is_macos: bool,
}

pub fn detect_system_profile() -> Result<SystemProfile, String> {
    let mut sys = System::new_all();
    sys.refresh_memory();

    // sysinfo exposes bytes for total_memory on current versions.
    let total_memory_bytes = sys.total_memory();
    let total_memory_gb = total_memory_bytes / (1024 * 1024 * 1024);

    Ok(SystemProfile {
        total_ram_gb: total_memory_gb,
        is_macos: cfg!(target_os = "macos"),
    })
}

pub fn fallback_system_profile() -> SystemProfile {
    SystemProfile {
        total_ram_gb: 8,
        is_macos: cfg!(target_os = "macos"),
    }
}

pub fn evaluate_model_compatibility(
    model_memory_estimate_gb: f32,
    system_profile: &SystemProfile,
) -> ModelCompatibility {
    let ram = system_profile.total_ram_gb as f32;
    let os_bias = if system_profile.is_macos { 0.2 } else { 0.0 };

    let recommended_threshold = (model_memory_estimate_gb * 2.5) - os_bias;
    let compatible_threshold = (model_memory_estimate_gb * 1.8) - os_bias;
    let may_be_slow_threshold = (model_memory_estimate_gb * 1.25) - os_bias;

    if ram >= recommended_threshold {
        ModelCompatibility::Recommended
    } else if ram >= compatible_threshold {
        ModelCompatibility::Compatible
    } else if ram >= may_be_slow_threshold {
        ModelCompatibility::MayBeSlow
    } else {
        ModelCompatibility::NotRecommended
    }
}

pub fn compatibility_rank(value: &ModelCompatibility) -> u8 {
    match value {
        ModelCompatibility::Recommended => 4,
        ModelCompatibility::Compatible => 3,
        ModelCompatibility::MayBeSlow => 2,
        ModelCompatibility::NotRecommended => 1,
    }
}

pub fn select_recommended_model(models: &[ModelDef], system_profile: &SystemProfile) -> Option<String> {
    models
        .iter()
        .map(|model| {
            let compatibility =
                evaluate_model_compatibility(model.memory_estimate_gb, system_profile);
            (
                compatibility_rank(&compatibility),
                model.size_mb,
                model.name.clone(),
            )
        })
        .max_by(|a, b| {
            a.0.cmp(&b.0)
                // If compatibility ties, prefer larger model for better quality.
                .then_with(|| a.1.cmp(&b.1))
        })
        .map(|(_, _, name)| name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::summary_engine::models::get_available_models;

    #[test]
    fn high_ram_prefers_larger_model() {
        let models = get_available_models();
        let system = SystemProfile {
            total_ram_gb: 32,
            is_macos: false,
        };

        let picked = select_recommended_model(&models, &system);
        assert_eq!(picked.as_deref(), Some("gemma3:4b"));
    }

    #[test]
    fn constrained_ram_prefers_smaller_model() {
        let models = get_available_models();
        let system = SystemProfile {
            total_ram_gb: 4,
            is_macos: false,
        };

        let picked = select_recommended_model(&models, &system);
        assert_eq!(picked.as_deref(), Some("gemma3:1b"));
    }

    #[test]
    fn compatibility_tiers_are_stable() {
        let low_ram = SystemProfile {
            total_ram_gb: 2,
            is_macos: false,
        };
        let mid_ram = SystemProfile {
            total_ram_gb: 6,
            is_macos: false,
        };
        let high_ram = SystemProfile {
            total_ram_gb: 16,
            is_macos: false,
        };

        assert_eq!(
            evaluate_model_compatibility(3.5, &low_ram),
            ModelCompatibility::NotRecommended
        );
        assert_eq!(
            evaluate_model_compatibility(3.5, &mid_ram),
            ModelCompatibility::MayBeSlow
        );
        assert_eq!(
            evaluate_model_compatibility(3.5, &high_ram),
            ModelCompatibility::Recommended
        );
    }
}
