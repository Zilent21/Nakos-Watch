use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CustodyType {
    ColdStorage,
    MultisigVault,
    HotWallet,
    Lightning,
    Mining,
    Exchange,
    Etf,
    Retirement,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Descriptor,
    Xpub,
    Ypub,
    Zpub,
    Address,
    MultisigDescriptor,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub id: String,
    pub name: String,
    pub custody_type: String,
    pub risk_level: String,
    pub source_type: String,
    pub descriptor: Option<String>,
    pub change_descriptor: Option<String>,
    pub xpub: Option<String>,
    /// Master key fingerprint (8 hex chars), user-supplied — never derived
    /// or guessed. Only ever set for the xpub/ypub/zpub add-wallet path.
    pub master_fingerprint: Option<String>,
    /// Base derivation path the xpub was derived at, e.g. "84'/0'/0'".
    pub derivation_path: Option<String>,
    pub network: String,
    pub gap_limit: i64,
    pub is_watch_only: bool,
    pub manual_balance_sats: Option<i64>,
    pub manual_cost_basis_fiat: Option<f64>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub notes: Option<String>,
    pub archived: bool,
    pub last_synced_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewWallet {
    pub name: String,
    pub custody_type: String,
    pub risk_level: String,
    pub source_type: String,
    pub descriptor: Option<String>,
    pub change_descriptor: Option<String>,
    pub xpub: Option<String>,
    pub master_fingerprint: Option<String>,
    pub derivation_path: Option<String>,
    pub network: String,
    pub manual_balance_sats: Option<i64>,
    pub manual_cost_basis_fiat: Option<f64>,
    pub color: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultisigConfig {
    pub id: String,
    pub wallet_id: String,
    pub threshold_m: i64,
    pub total_n: i64,
    pub script_type: String,
    pub cosigners: Vec<Cosigner>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cosigner {
    pub id: String,
    pub label: String,
    pub xpub: String,
    pub fingerprint: Option<String>,
    pub derivation_path: Option<String>,
    pub is_mine: bool,
    pub hardware_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Address {
    pub id: String,
    pub wallet_id: String,
    pub address: String,
    pub derivation_path: Option<String>,
    pub keychain: String,
    pub address_index: Option<i64>,
    pub is_used: bool,
    pub label: Option<String>,
    pub first_seen_at: Option<String>,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Utxo {
    pub id: String,
    pub wallet_id: String,
    pub address_id: Option<String>,
    pub txid: String,
    pub vout: i64,
    pub amount_sats: i64,
    pub confirmations: i64,
    pub block_height: Option<i64>,
    pub block_time: Option<String>,
    pub is_change: bool,
    pub is_frozen: bool,
    pub label: Option<String>,
    pub acquisition_cost_fiat: Option<f64>,
    pub acquisition_date: Option<String>,
    pub acquisition_note: Option<String>,
    pub privacy_risk_score: Option<i64>,
    pub spent: bool,
    pub spent_txid: Option<String>,
    pub spent_at: Option<String>,
    /// Set when a scan stopped reporting a live coin that carries user data; the row is kept
    /// (spent = 1) so its lot, label and freeze flag survive. Empty for known-spent coins.
    pub unseen_since: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub wallet_id: String,
    pub txid: String,
    pub fee_sats: Option<i64>,
    pub block_height: Option<i64>,
    pub block_time: Option<String>,
    pub confirmations: i64,
    pub direction: String,
    pub amount_sats: i64,
    pub label: Option<String>,
    pub category: Option<String>,
    pub counterparty: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub id: String,
    pub wallet_id: Option<String>,
    pub hardware_wallet_model: Option<String>,
    pub firmware_up_to_date: Option<bool>,
    pub passphrase_enabled: bool,
    pub is_multisig: bool,
    pub geographic_redundancy_locations: i64,
    pub backup_tested: bool,
    pub backup_tested_at: Option<String>,
    pub exchange_2fa_type: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryContact {
    pub id: String,
    pub name: String,
    pub relationship: Option<String>,
    pub contact_method: Option<String>,
    pub has_key_share: bool,
    pub has_notes: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InheritanceConfig {
    pub id: String,
    pub wallet_id: Option<String>,
    pub has_instructions: bool,
    pub last_tested_at: Option<String>,
    pub test_status: String,
    pub updated_at: String,
    pub contacts: Vec<RecoveryContact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthScoreSnapshot {
    pub id: String,
    pub taken_at: String,
    pub security_score: i64,
    pub privacy_score: i64,
    pub liquidity_score: i64,
    pub inheritance_score: i64,
    pub redundancy_score: i64,
    pub node_health_score: Option<i64>,
    pub lightning_health_score: Option<i64>,
    pub tax_readiness_score: i64,
    pub composite_score: i64,
    pub details_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub host: String,
    pub port: i64,
    pub rpc_user: Option<String>,
    pub cookie_path: Option<String>,
    pub use_tls: bool,
    pub use_tor: bool,
    pub enabled: bool,
    pub is_default: bool,
    pub last_connected_at: Option<String>,
    /// An Electrum server that is not on this computer or the person's own network: syncing
    /// through it shows its operator the wallet addresses (computed, not stored).
    #[serde(default)]
    pub third_party: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewNodeConfig {
    pub kind: String,
    pub label: String,
    pub host: String,
    pub port: i64,
    pub rpc_user: Option<String>,
    pub rpc_password: Option<String>,
    pub cookie_path: Option<String>,
    pub use_tls: bool,
    pub use_tor: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightningChannel {
    pub id: String,
    pub wallet_id: Option<String>,
    pub channel_id: Option<String>,
    pub remote_alias: Option<String>,
    pub remote_pubkey: Option<String>,
    pub capacity_sats: i64,
    pub local_balance_sats: i64,
    pub remote_balance_sats: i64,
    pub is_active: bool,
    pub is_private: bool,
    pub uptime_pct: Option<f64>,
    pub updated_at: String,
}
