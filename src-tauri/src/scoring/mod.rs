pub mod health;
pub mod privacy;
pub mod privacy_metrics;
pub mod security;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedAction {
    pub id: String,
    pub title: String,
    pub description: String,
    pub priority: Priority,
    pub points_available: i64,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub score: i64,
    pub max_score: i64,
    pub factors: Vec<ScoreFactor>,
    pub recommended_actions: Vec<RecommendedAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreFactor {
    pub key: String,
    pub label: String,
    pub points: i64,
    pub max_points: i64,
    pub detail: String,
}

/// Clamp a raw weighted sum into 0..=100.
pub fn clamp_score(raw: f64) -> i64 {
    raw.round().clamp(0.0, 100.0) as i64
}
