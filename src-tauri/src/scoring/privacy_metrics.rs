//! Privacy Dashboard metrics: pure functions over local UTXO rows. No I/O,
//! no network. Inputs are unspent coins of unarchived wallets.
//!
//! Sync prunes spent outputs that carry no user data and keeps the rest
//! marked `spent = 1`; every query here filters `spent = 0`, so past reuse
//! can't be seen and every metric describes the coins held now.
//!
//! future: optional explorer / LN / cloud privacy providers could add
//! metrics alongside these (e.g. external address-label lookups). None exist
//! in this pass and nothing here calls out.

use crate::bitcoin::coin_select::script_class;
use serde::Serialize;
use std::collections::BTreeMap;

/// One unspent coin. `address` is `None` when the row has no address on file.
#[derive(Debug, Clone)]
pub struct CoinRow {
    pub address: Option<String>,
    pub amount_sats: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScriptMixEntry {
    pub script: String,
    pub count: usize,
    pub sats: u64,
}

/// Coins and sats per script type (same classifier as coin selection),
/// largest count first. Coins without an address count as "unknown".
pub fn script_mix(coins: &[CoinRow]) -> Vec<ScriptMixEntry> {
    let mut by: BTreeMap<&'static str, (usize, u64)> = BTreeMap::new();
    for c in coins {
        let class = c.address.as_deref().map(script_class).unwrap_or("unknown");
        let e = by.entry(class).or_default();
        e.0 += 1;
        e.1 += c.amount_sats;
    }
    let mut out: Vec<ScriptMixEntry> =
        by.into_iter().map(|(s, (count, sats))| ScriptMixEntry { script: s.to_string(), count, sats }).collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.script.cmp(&b.script)));
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReusedAddress {
    pub address: String,
    pub coins: usize,
    pub sats: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReuseSummary {
    /// Addresses that currently hold two or more coins.
    pub reused_addresses: usize,
    pub coins_on_reused_addresses: usize,
    pub sats_on_reused_addresses: u64,
    /// Up to 5, most coins first.
    pub worst: Vec<ReusedAddress>,
}

/// Address reuse among coins held now: the same address holding >1 coin.
pub fn address_reuse(coins: &[CoinRow]) -> ReuseSummary {
    let mut by: BTreeMap<&str, (usize, u64)> = BTreeMap::new();
    for c in coins {
        if let Some(a) = &c.address {
            let e = by.entry(a.as_str()).or_default();
            e.0 += 1;
            e.1 += c.amount_sats;
        }
    }
    let mut reused: Vec<ReusedAddress> = by
        .into_iter()
        .filter(|(_, (n, _))| *n > 1)
        .map(|(a, (coins, sats))| ReusedAddress { address: a.to_string(), coins, sats })
        .collect();
    reused.sort_by(|a, b| b.coins.cmp(&a.coins).then_with(|| a.address.cmp(&b.address)));
    ReuseSummary {
        reused_addresses: reused.len(),
        coins_on_reused_addresses: reused.iter().map(|r| r.coins).sum(),
        sats_on_reused_addresses: reused.iter().map(|r| r.sats).sum(),
        worst: reused.into_iter().take(5).collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConsolidationRisk {
    /// "n/a" (fewer than 2 coins), "fragmented", "concentrated", "balanced".
    pub level: String,
    pub largest_share_pct: f64,
    pub small_coin_share_pct: f64,
    pub detail: String,
}

/// Coins under this are "small" here (same cutoff as the UTXO dust bucket).
pub const SMALL_COIN_SATS: u64 = 10_000;

/// - fragmented: >= 10 coins and >= 50% of them under `SMALL_COIN_SATS`.
///   A spend needs many inputs, which links them and costs more fee.
/// - concentrated: >= 2 coins and the largest holds >= 80% of the total.
///   Few large coins: any spend tends to move most of the wallet at once.
/// - balanced: otherwise. n/a: fewer than 2 coins.
pub fn consolidation_risk(amounts: &[u64]) -> ConsolidationRisk {
    let n = amounts.len();
    let total: u64 = amounts.iter().sum();
    let largest = amounts.iter().copied().max().unwrap_or(0);
    let small = amounts.iter().filter(|a| **a < SMALL_COIN_SATS).count();
    let largest_share_pct = if total == 0 { 0.0 } else { largest as f64 * 100.0 / total as f64 };
    let small_coin_share_pct = if n == 0 { 0.0 } else { small as f64 * 100.0 / n as f64 };

    let (level, detail) = if n < 2 {
        ("n/a", "Fewer than two coins, so there is nothing to compare.".to_string())
    } else if n >= 10 && small_coin_share_pct >= 50.0 {
        (
            "fragmented",
            format!("{small} of {n} coins are under {SMALL_COIN_SATS} sats. Spending them takes many inputs, which links them together and costs more in fees."),
        )
    } else if largest_share_pct >= 80.0 {
        (
            "concentrated",
            format!("One coin holds {largest_share_pct:.0}% of the total. Most spends will move most of your balance in one transaction."),
        )
    } else {
        ("balanced", "No single coin dominates and small coins are not piling up.".to_string())
    };
    ConsolidationRisk { level: level.to_string(), largest_share_pct, small_coin_share_pct, detail }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WPKH_A: &str = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";
    const WPKH_B: &str = "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq";
    const TAPROOT: &str = "bc1p5cyxnuxmeuwuvkwfem96lqzszd02n6xdcjrs20cac6yqjjwudpxqkedrcr";

    fn coin(addr: Option<&str>, sats: u64) -> CoinRow {
        CoinRow { address: addr.map(String::from), amount_sats: sats }
    }

    #[test]
    fn script_mix_counts_types_and_unknown() {
        let coins = vec![coin(Some(WPKH_A), 1_000), coin(Some(WPKH_B), 2_000), coin(Some(TAPROOT), 5_000), coin(None, 700)];
        let mix = script_mix(&coins);
        assert_eq!(mix[0], ScriptMixEntry { script: "p2wpkh".into(), count: 2, sats: 3_000 });
        assert!(mix.contains(&ScriptMixEntry { script: "p2tr".into(), count: 1, sats: 5_000 }));
        assert!(mix.contains(&ScriptMixEntry { script: "unknown".into(), count: 1, sats: 700 }));
    }

    #[test]
    fn reuse_flags_only_addresses_with_multiple_coins() {
        let coins = vec![
            coin(Some(WPKH_A), 6_010),
            coin(Some(WPKH_A), 8_406),
            coin(Some(WPKH_A), 100),
            coin(Some(WPKH_B), 5_000),
            coin(None, 1),
            coin(None, 1),
        ];
        let r = address_reuse(&coins);
        assert_eq!(r.reused_addresses, 1);
        assert_eq!(r.coins_on_reused_addresses, 3);
        assert_eq!(r.sats_on_reused_addresses, 14_516);
        assert_eq!(r.worst[0], ReusedAddress { address: WPKH_A.into(), coins: 3, sats: 14_516 });
    }

    #[test]
    fn no_reuse_when_every_address_holds_one_coin() {
        let r = address_reuse(&[coin(Some(WPKH_A), 1), coin(Some(WPKH_B), 1)]);
        assert_eq!(r.reused_addresses, 0);
        assert!(r.worst.is_empty());
    }

    #[test]
    fn consolidation_levels() {
        assert_eq!(consolidation_risk(&[]).level, "n/a");
        assert_eq!(consolidation_risk(&[5_000]).level, "n/a");
        assert_eq!(consolidation_risk(&[900_000, 50_000]).level, "concentrated");
        assert_eq!(consolidation_risk(&[60_000, 40_000, 50_000]).level, "balanced");
        let mut many = vec![1_000u64; 12];
        many.push(500_000);
        assert_eq!(consolidation_risk(&many).level, "fragmented");
    }
}
