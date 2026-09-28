//! Offline speaker diarization: who spoke when, per individual person.

pub mod assign;
pub mod cluster;
pub mod commands;
pub mod diarizer;
pub mod embedding;
pub mod fbank;
pub mod jobs;
pub mod models;
pub mod reconstruct;
pub mod segmentation;
pub mod timing;

use serde::{Deserialize, Serialize};

/// A contiguous stretch of speech attributed to one speaker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub start_s: f64,
    pub end_s: f64,
    pub key: String,
}

impl Turn {
    pub fn duration(&self) -> f64 {
        (self.end_s - self.start_s).max(0.0)
    }
}

/// Returned when a diarization run is cancelled by the user.
#[derive(thiserror::Error, Debug)]
#[error("Speaker identification cancelled")]
pub struct Cancelled;
