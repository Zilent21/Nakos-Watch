//! Security Score (0-100): a real, deterministic scoring engine driven by
//! the user's actual reported setup. No hardcoded/fake scores — every
//! point is traceable to a `ScoreFactor`.

use super::{Priority, RecommendedAction, ScoreBreakdown, ScoreFactor};

#[derive(Debug, Clone, Default)]
pub struct SecurityInput {
    pub total_active_wallets: i64,
    pub multisig_wallet_count: i64,
    pub singlesig_hardware_wallet_count: i64,
    pub wallets_with_passphrase: i64,
    pub hardware_wallets_total: i64,
    pub hardware_wallets_firmware_current: i64,
    pub hardware_wallets_firmware_unknown: i64,
    pub max_geographic_locations_reported: i64,
    pub hardware_wallets_backup_tested: i64,
    pub exchange_wallet_count: i64,
    pub exchange_wallets_with_hardware_2fa: i64,
    pub inheritance_test_status: String, // never_tested | tested_pass | tested_fail
}

pub fn score(input: &SecurityInput) -> ScoreBreakdown {
    let mut factors = Vec::new();
    let mut actions = Vec::new();

    // 1. Multisig vs singlesig — 20 pts
    let multisig_max = 20i64;
    let signable_wallets = input.multisig_wallet_count + input.singlesig_hardware_wallet_count;
    let multisig_pts = if signable_wallets == 0 {
        0
    } else {
        let ratio = input.multisig_wallet_count as f64 / signable_wallets as f64;
        (ratio * multisig_max as f64).round() as i64
    };
    factors.push(ScoreFactor {
        key: "multisig".into(),
        label: "Multisig adoption".into(),
        points: multisig_pts,
        max_points: multisig_max,
        detail: format!(
            "{} of {} signable wallets use multisig",
            input.multisig_wallet_count, signable_wallets.max(input.multisig_wallet_count)
        ),
    });
    if multisig_pts < multisig_max {
        actions.push(RecommendedAction {
            id: "move-to-multisig".into(),
            title: "Move significant holdings to multisig".into(),
            description: "Single-signature wallets are a single point of failure. A 2-of-3 multisig vault removes that risk.".into(),
            priority: if multisig_pts == 0 { Priority::High } else { Priority::Medium },
            points_available: multisig_max - multisig_pts,
            category: "security".into(),
        });
    }

    // 2. Firmware status — 15 pts
    let firmware_max = 15i64;
    let firmware_pts = if input.hardware_wallets_total == 0 {
        firmware_max // nothing to be behind on
    } else {
        let ratio = input.hardware_wallets_firmware_current as f64 / input.hardware_wallets_total as f64;
        (ratio * firmware_max as f64).round() as i64
    };
    factors.push(ScoreFactor {
        key: "firmware".into(),
        label: "Hardware wallet firmware".into(),
        points: firmware_pts,
        max_points: firmware_max,
        detail: format!(
            "{} of {} hardware wallets confirmed up to date",
            input.hardware_wallets_firmware_current, input.hardware_wallets_total
        ),
    });
    if firmware_pts < firmware_max {
        actions.push(RecommendedAction {
            id: "update-firmware".into(),
            title: "Verify and update hardware wallet firmware".into(),
            description: "Outdated or unverified firmware can contain fixed vulnerabilities. Confirm each device is current.".into(),
            priority: Priority::Medium,
            points_available: firmware_max - firmware_pts,
            category: "security".into(),
        });
    }

    // 3. Passphrase usage — 15 pts
    let passphrase_max = 15i64;
    let passphrase_pts = if input.total_active_wallets == 0 {
        0
    } else {
        let ratio = input.wallets_with_passphrase as f64 / input.total_active_wallets as f64;
        (ratio * passphrase_max as f64).round() as i64
    };
    factors.push(ScoreFactor {
        key: "passphrase".into(),
        label: "BIP-39 passphrase usage".into(),
        points: passphrase_pts,
        max_points: passphrase_max,
        detail: format!(
            "{} of {} wallets use a passphrase",
            input.wallets_with_passphrase, input.total_active_wallets
        ),
    });
    if passphrase_pts < passphrase_max {
        actions.push(RecommendedAction {
            id: "enable-passphrase".into(),
            title: "Enable a BIP-39 passphrase".into(),
            description: "A passphrase creates plausible deniability and protects against physical seed compromise.".into(),
            priority: Priority::Medium,
            points_available: passphrase_max - passphrase_pts,
            category: "security".into(),
        });
    }

    // 4. Geographic redundancy — 15 pts
    let geo_max = 15i64;
    let geo_pts = match input.max_geographic_locations_reported {
        0 => 0,
        1 => 5,
        2 => 10,
        _ => 15,
    };
    factors.push(ScoreFactor {
        key: "geo_redundancy".into(),
        label: "Geographic redundancy".into(),
        points: geo_pts,
        max_points: geo_max,
        detail: format!("Backups reported across {} location(s)", input.max_geographic_locations_reported),
    });
    if geo_pts < geo_max {
        actions.push(RecommendedAction {
            id: "add-geo-redundancy".into(),
            title: "Distribute backups across more locations".into(),
            description: "A single-location backup is vulnerable to fire, flood, or theft. Aim for at least 3 geographically separated locations.".into(),
            priority: if input.max_geographic_locations_reported == 0 { Priority::Critical } else { Priority::Medium },
            points_available: geo_max - geo_pts,
            category: "security".into(),
        });
    }

    // 5. Backup testing — 15 pts
    let backup_max = 15i64;
    let backup_pts = if input.hardware_wallets_total == 0 {
        backup_max
    } else {
        let ratio = input.hardware_wallets_backup_tested as f64 / input.hardware_wallets_total as f64;
        (ratio * backup_max as f64).round() as i64
    };
    factors.push(ScoreFactor {
        key: "backup_tested".into(),
        label: "Backup restore testing".into(),
        points: backup_pts,
        max_points: backup_max,
        detail: format!(
            "{} of {} hardware wallets have a tested restore",
            input.hardware_wallets_backup_tested, input.hardware_wallets_total
        ),
    });
    if backup_pts < backup_max {
        actions.push(RecommendedAction {
            id: "test-backup-restore".into(),
            title: "Test a full seed restore".into(),
            description: "An untested backup is a hope, not a plan. Restore each seed onto a blank device to confirm it works.".into(),
            priority: Priority::High,
            points_available: backup_max - backup_pts,
            category: "security".into(),
        });
    }

    // 6. Exchange 2FA / hardware key — 10 pts
    let exchange_max = 10i64;
    let exchange_pts = if input.exchange_wallet_count == 0 {
        exchange_max
    } else {
        let ratio = input.exchange_wallets_with_hardware_2fa as f64 / input.exchange_wallet_count as f64;
        (ratio * exchange_max as f64).round() as i64
    };
    factors.push(ScoreFactor {
        key: "exchange_2fa".into(),
        label: "Exchange hardware-key 2FA".into(),
        points: exchange_pts,
        max_points: exchange_max,
        detail: format!(
            "{} of {} exchange accounts use hardware-key 2FA",
            input.exchange_wallets_with_hardware_2fa, input.exchange_wallet_count
        ),
    });
    if exchange_pts < exchange_max && input.exchange_wallet_count > 0 {
        actions.push(RecommendedAction {
            id: "upgrade-exchange-2fa".into(),
            title: "Upgrade exchange 2FA to a hardware security key".into(),
            description: "SMS and app-based 2FA are phishable and SIM-swappable. Use a FIDO2 hardware key where the exchange supports it.".into(),
            priority: Priority::High,
            points_available: exchange_max - exchange_pts,
            category: "security".into(),
        });
    }

    // 7. Inheritance readiness — 10 pts
    let inheritance_max = 10i64;
    let inheritance_pts = match input.inheritance_test_status.as_str() {
        "tested_pass" => inheritance_max,
        "tested_fail" => 3,
        _ => 0,
    };
    factors.push(ScoreFactor {
        key: "inheritance_readiness".into(),
        label: "Inheritance readiness".into(),
        points: inheritance_pts,
        max_points: inheritance_max,
        detail: format!("Recovery plan status: {}", input.inheritance_test_status),
    });
    if inheritance_pts < inheritance_max {
        actions.push(RecommendedAction {
            id: "test-inheritance-plan".into(),
            title: "Test your inheritance / recovery plan".into(),
            description: "An heir who has never rehearsed recovery will likely fail under stress. Run a supervised recovery drill.".into(),
            priority: Priority::High,
            points_available: inheritance_max - inheritance_pts,
            category: "inheritance".into(),
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
