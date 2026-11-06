//! Privacy Score (0-100). Computed from real on-chain heuristics applied to
//! the user's own UTXO/address/transaction data — address reuse,
//! common-input-ownership exposure, KYC-linked taint, and change-address
//! hygiene. Nothing here is estimated or faked.

use super::{Priority, RecommendedAction, ScoreBreakdown, ScoreFactor};

#[derive(Debug, Clone, Default)]
pub struct PrivacyInput {
    pub total_addresses_used: i64,
    pub reused_addresses: i64,
    pub total_utxos: i64,
    /// UTXOs whose creating tx had >1 input from this wallet's own address set
    /// (a common-input-ownership heuristic exposure).
    pub utxos_with_cioh_exposure: i64,
    pub total_transactions: i64,
    pub kyc_linked_transactions: i64,
    /// Change outputs detectable by heuristic (e.g. address type mismatch,
    /// round-number-output ambiguity) — the lower the better.
    pub detectable_change_outputs: i64,
    pub total_outgoing_transactions_with_change: i64,
    pub merged_coin_events: i64,
}

pub fn score(input: &PrivacyInput) -> ScoreBreakdown {
    let mut factors = Vec::new();
    let mut actions = Vec::new();

    // 1. Address reuse — 30 pts
    let reuse_max = 30i64;
    let reuse_pts = if input.total_addresses_used == 0 {
        reuse_max
    } else {
        let reuse_ratio = input.reused_addresses as f64 / input.total_addresses_used as f64;
        ((1.0 - reuse_ratio) * reuse_max as f64).round() as i64
    };
    factors.push(ScoreFactor {
        key: "address_reuse".into(),
        label: "Address reuse".into(),
        points: reuse_pts,
        max_points: reuse_max,
        detail: format!(
            "{} of {} used addresses have been reused",
            input.reused_addresses, input.total_addresses_used
        ),
    });
    if reuse_pts < reuse_max {
        actions.push(RecommendedAction {
            id: "stop-address-reuse".into(),
            title: "Stop reusing addresses".into(),
            description: "Reused addresses link every payment to the same identity on-chain. Always generate a fresh address per receipt.".into(),
            priority: Priority::High,
            points_available: reuse_max - reuse_pts,
            category: "privacy".into(),
        });
    }

    // 2. Common-input-ownership heuristic exposure — 25 pts
    let cioh_max = 25i64;
    let cioh_pts = if input.total_utxos == 0 {
        cioh_max
    } else {
        let ratio = input.utxos_with_cioh_exposure as f64 / input.total_utxos as f64;
        ((1.0 - ratio) * cioh_max as f64).round() as i64
    };
    factors.push(ScoreFactor {
        key: "cioh_exposure".into(),
        label: "Common-input-ownership exposure".into(),
        points: cioh_pts,
        max_points: cioh_max,
        detail: format!(
            "{} of {} UTXOs originated from a multi-input transaction linking addresses together",
            input.utxos_with_cioh_exposure, input.total_utxos
        ),
    });
    if cioh_pts < cioh_max {
        actions.push(RecommendedAction {
            id: "avoid-input-merging".into(),
            title: "Avoid merging UTXOs from different sources".into(),
            description: "Spending multiple UTXOs together reveals they share an owner. Consider coin control before sending.".into(),
            priority: Priority::Medium,
            points_available: cioh_max - cioh_pts,
            category: "privacy".into(),
        });
    }

    // 3. KYC-linked taint — 20 pts
    let kyc_max = 20i64;
    let kyc_pts = if input.total_transactions == 0 {
        kyc_max
    } else {
        let ratio = input.kyc_linked_transactions as f64 / input.total_transactions as f64;
        ((1.0 - ratio) * kyc_max as f64).round() as i64
    };
    factors.push(ScoreFactor {
        key: "kyc_taint".into(),
        label: "KYC-linked exposure".into(),
        points: kyc_pts,
        max_points: kyc_max,
        detail: format!(
            "{} of {} transactions are tagged as KYC exchange withdrawals/deposits",
            input.kyc_linked_transactions, input.total_transactions
        ),
    });
    if kyc_pts < kyc_max {
        actions.push(RecommendedAction {
            id: "break-kyc-taint".into(),
            title: "Break KYC taint on exchange-sourced coins".into(),
            description: "Coins withdrawn from KYC exchanges are directly linked to your identity. Consider coinjoin or clean-source consolidation for coins you plan to hold long-term.".into(),
            priority: Priority::Medium,
            points_available: kyc_max - kyc_pts,
            category: "privacy".into(),
        });
    }

    // 4. Change-address hygiene — 15 pts
    let change_max = 15i64;
    let change_pts = if input.total_outgoing_transactions_with_change == 0 {
        change_max
    } else {
        let ratio = input.detectable_change_outputs as f64 / input.total_outgoing_transactions_with_change as f64;
        ((1.0 - ratio) * change_max as f64).round() as i64
    };
    factors.push(ScoreFactor {
        key: "change_hygiene".into(),
        label: "Change address hygiene".into(),
        points: change_pts,
        max_points: change_max,
        detail: format!(
            "{} of {} spends had a heuristically-detectable change output",
            input.detectable_change_outputs, input.total_outgoing_transactions_with_change
        ),
    });
    if change_pts < change_max {
        actions.push(RecommendedAction {
            id: "improve-change-hygiene".into(),
            title: "Use matching address types for change".into(),
            description: "Change outputs that differ in script type from the payment output are trivially identifiable. Keep change on the same address type.".into(),
            priority: Priority::Low,
            points_available: change_max - change_pts,
            category: "privacy".into(),
        });
    }

    // 5. Coin merge history — 10 pts (fewer merges of unrelated coins is better)
    let merge_max = 10i64;
    let merge_pts = if input.merged_coin_events == 0 {
        merge_max
    } else if input.merged_coin_events < 3 {
        6
    } else {
        2
    };
    factors.push(ScoreFactor {
        key: "coin_merge_history".into(),
        label: "Coin merge history".into(),
        points: merge_pts,
        max_points: merge_max,
        detail: format!("{} detected coin-merge event(s) across your wallets", input.merged_coin_events),
    });
    if merge_pts < merge_max {
        actions.push(RecommendedAction {
            id: "reduce-coin-merges".into(),
            title: "Reduce merging of unrelated UTXOs".into(),
            description: "Each merge event permanently links previously-separate coin histories. Use coin control to keep sources apart when it matters.".into(),
            priority: Priority::Low,
            points_available: merge_max - merge_pts,
            category: "privacy".into(),
        });
    }

    let score: i64 = factors.iter().map(|f| f.points).sum();
    actions.sort_by(|a, b| b.priority.cmp(&a.priority).then(b.points_available.cmp(&a.points_available)));

    ScoreBreakdown {
        score: score.clamp(0, 100),
        max_score: 100,
        factors,
        recommended_actions: actions,
    }
}
