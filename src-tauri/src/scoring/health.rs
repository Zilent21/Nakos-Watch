//! BTC Health Score: the top-level composite across every other scoring
//! dimension. Recomputed continuously as wallet/security/node data changes;
//! computing it does not write to `health_score_snapshots`; the history
//! command only reads rows that table already holds.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct LiquidityInput {
    pub total_utxos: i64,
    pub dust_utxos: i64,       // amount_sats < dust threshold (10_000 sats default)
    pub frozen_utxos: i64,
    pub max_single_wallet_share_pct: f64, // concentration risk, 0..100
}

pub fn liquidity_score(input: &LiquidityInput) -> i64 {
    if input.total_utxos == 0 {
        return 100;
    }
    let dust_ratio = input.dust_utxos as f64 / input.total_utxos as f64;
    let frozen_ratio = input.frozen_utxos as f64 / input.total_utxos as f64;
    let concentration_penalty = (input.max_single_wallet_share_pct - 60.0).max(0.0) / 40.0; // penalize >60% concentration

    let mut score = 100.0;
    score -= dust_ratio * 35.0;
    score -= frozen_ratio * 25.0;
    score -= concentration_penalty.min(1.0) * 20.0;
    score.round().clamp(0.0, 100.0) as i64
}

#[derive(Debug, Clone, Default)]
pub struct TaxInput {
    pub utxos_total: i64,
    pub utxos_with_cost_basis: i64,
}

pub fn tax_readiness_score(input: &TaxInput) -> i64 {
    if input.utxos_total == 0 {
        return 100;
    }
    let ratio = input.utxos_with_cost_basis as f64 / input.utxos_total as f64;
    (ratio * 100.0).round().clamp(0.0, 100.0) as i64
}

/// Tax-readiness dimension for the composite: `None` while no coin has a
/// recorded lot (nothing real to score), otherwise the share of coins that do.
pub fn tax_readiness_option(utxos_total: i64, utxos_with_cost_basis: i64) -> Option<i64> {
    if utxos_with_cost_basis == 0 {
        None
    } else {
        Some(tax_readiness_score(&TaxInput { utxos_total, utxos_with_cost_basis }))
    }
}

#[derive(Debug, Clone, Default)]
pub struct NodeHealthInput {
    pub is_synced: bool,
    pub verification_progress: f64, // 0..1
    pub peer_count: i64,
    pub is_reachable_via_tor: bool,
}

pub fn node_health_score(input: &NodeHealthInput) -> i64 {
    let mut score = input.verification_progress.clamp(0.0, 1.0) * 80.0;
    if input.is_synced {
        score = score.max(80.0);
    }
    score += (input.peer_count.min(8) as f64 / 8.0) * 15.0;
    if input.is_reachable_via_tor {
        score += 5.0;
    }
    score.round().clamp(0.0, 100.0) as i64
}

#[derive(Debug, Clone, Default)]
pub struct LightningHealthInput {
    pub total_channels: i64,
    pub active_channels: i64,
    pub average_uptime_pct: f64, // 0..100
}

pub fn lightning_health_score(input: &LightningHealthInput) -> i64 {
    if input.total_channels == 0 {
        return 100;
    }
    let active_ratio = input.active_channels as f64 / input.total_channels as f64;
    let score = active_ratio * 60.0 + (input.average_uptime_pct.clamp(0.0, 100.0) / 100.0) * 40.0;
    score.round().clamp(0.0, 100.0) as i64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositeInput {
    pub security_score: i64,
    pub privacy_score: i64,
    pub liquidity_score: i64,
    pub inheritance_score: i64,
    pub redundancy_score: i64,
    pub node_health_score: Option<i64>,
    pub lightning_health_score: Option<i64>,
    /// `None` when no UTXO anywhere has a recorded cost basis — Nakos
    /// doesn't implement cost-basis entry, so there is nothing real to
    /// score. Absent dimensions redistribute their weight rather than
    /// showing a fabricated number (same mechanism as node/Lightning
    /// health above).
    pub tax_readiness_score: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DimensionWeight {
    pub key: &'static str,
    pub weight: f64,
    pub score: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompositeResult {
    pub composite_score: i64,
    pub dimensions: Vec<DimensionWeight>,
    /// Filled in by the command layer (needs RPC); `composite_score` leaves it empty.
    pub node: NodeHealthReport,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct NodeHealthReport {
    pub configured: bool,
    pub node_label: Option<String>,
    pub score: Option<i64>,
    pub reasons: Vec<String>,
    pub error: Option<String>,
}

/// Base weights when every dimension (including optional node/lightning)
/// is present. Missing optional dimensions redistribute their weight
/// proportionally across the remaining present dimensions.
pub fn composite_score(input: &CompositeInput) -> CompositeResult {
    let mut dims: Vec<(&'static str, f64, Option<i64>)> = vec![
        ("security", 20.0, Some(input.security_score)),
        ("privacy", 15.0, Some(input.privacy_score)),
        ("liquidity", 15.0, Some(input.liquidity_score)),
        ("inheritance", 15.0, Some(input.inheritance_score)),
        ("redundancy", 10.0, Some(input.redundancy_score)),
        ("node_health", 10.0, input.node_health_score),
        ("lightning_health", 5.0, input.lightning_health_score),
        ("tax_readiness", 10.0, input.tax_readiness_score),
    ];

    let present_weight: f64 = dims.iter().filter(|(_, _, s)| s.is_some()).map(|(_, w, _)| w).sum();
    let missing_weight: f64 = dims.iter().filter(|(_, _, s)| s.is_none()).map(|(_, w, _)| w).sum();

    if present_weight > 0.0 && missing_weight > 0.0 {
        let redistribution_factor = 1.0 + (missing_weight / present_weight);
        for (_, w, s) in dims.iter_mut() {
            if s.is_some() {
                *w *= redistribution_factor;
            } else {
                *w = 0.0;
            }
        }
    }

    let mut composite = 0.0;
    let mut out = Vec::new();
    for (key, weight, score) in dims {
        if let Some(s) = score {
            composite += (s as f64) * (weight / 100.0);
            out.push(DimensionWeight { key, weight, score: s });
        }
    }

    CompositeResult {
        composite_score: composite.round().clamp(0.0, 100.0) as i64,
        dimensions: out,
        node: NodeHealthReport::default(),
    }
}
