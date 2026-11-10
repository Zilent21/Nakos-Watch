export type CustodyType =
  | "cold_storage"
  | "multisig_vault"
  | "hot_wallet"
  | "lightning"
  | "mining"
  | "exchange"
  | "etf"
  | "retirement"
  | "other";

export type RiskLevel = "low" | "medium" | "high";

export type SourceType = "descriptor" | "xpub" | "ypub" | "zpub" | "address" | "multisig_descriptor" | "manual";

export interface Wallet {
  id: string;
  name: string;
  custody_type: CustodyType;
  risk_level: RiskLevel;
  source_type: SourceType;
  descriptor: string | null;
  change_descriptor: string | null;
  xpub: string | null;
  master_fingerprint: string | null;
  derivation_path: string | null;
  network: string;
  gap_limit: number;
  is_watch_only: boolean;
  manual_balance_sats: number | null;
  manual_cost_basis_fiat: number | null;
  color: string | null;
  icon: string | null;
  notes: string | null;
  archived: boolean;
  last_synced_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface NewWallet {
  name: string;
  custody_type: CustodyType;
  risk_level: RiskLevel;
  source_type: SourceType;
  descriptor?: string | null;
  change_descriptor?: string | null;
  xpub?: string | null;
  master_fingerprint?: string | null;
  derivation_path?: string | null;
  network: string;
  manual_balance_sats?: number | null;
  manual_cost_basis_fiat?: number | null;
  color?: string | null;
  notes?: string | null;
}

export interface Cosigner {
  id: string;
  label: string;
  xpub: string;
  fingerprint: string | null;
  derivation_path: string | null;
  is_mine: boolean;
  hardware_model: string | null;
}

export interface MultisigConfig {
  id: string;
  wallet_id: string;
  threshold_m: number;
  total_n: number;
  script_type: string;
  cosigners: Cosigner[];
}

export interface Utxo {
  id: string;
  wallet_id: string;
  address_id: string | null;
  txid: string;
  vout: number;
  amount_sats: number;
  confirmations: number;
  block_height: number | null;
  block_time: string | null;
  is_change: boolean;
  is_frozen: boolean;
  label: string | null;
  acquisition_cost_fiat: number | null;
  acquisition_date: string | null;
  acquisition_note: string | null;
  privacy_risk_score: number | null;
  spent: boolean;
  spent_txid: string | null;
  spent_at: string | null;
  unseen_since: string | null;
  created_at: string;
}

export interface Transaction {
  id: string;
  wallet_id: string;
  txid: string;
  fee_sats: number | null;
  block_height: number | null;
  block_time: string | null;
  confirmations: number;
  direction: "incoming" | "outgoing" | "self";
  amount_sats: number;
  label: string | null;
  category: string | null;
  counterparty: string | null;
  created_at: string;
}

export interface SecurityConfig {
  id: string;
  wallet_id: string | null;
  hardware_wallet_model: string | null;
  firmware_up_to_date: boolean | null;
  passphrase_enabled: boolean;
  is_multisig: boolean;
  geographic_redundancy_locations: number;
  backup_tested: boolean;
  backup_tested_at: string | null;
  exchange_2fa_type: string | null;
  updated_at: string;
}

export interface RecoveryContact {
  id: string;
  name: string;
  relationship: string | null;
  contact_method: string | null;
  has_key_share: boolean;
  has_notes: boolean;
  created_at: string;
}

export interface InheritanceConfig {
  id: string;
  wallet_id: string | null;
  has_instructions: boolean;
  last_tested_at: string | null;
  test_status: "never_tested" | "tested_pass" | "tested_fail";
  updated_at: string;
  contacts: RecoveryContact[];
}

export interface HealthScoreSnapshot {
  id: string;
  taken_at: string;
  security_score: number;
  privacy_score: number;
  liquidity_score: number;
  inheritance_score: number;
  redundancy_score: number;
  node_health_score: number | null;
  lightning_health_score: number | null;
  tax_readiness_score: number;
  composite_score: number;
  details_json: string;
}

export interface NodeConfig {
  id: string;
  kind: "bitcoin_core_rpc" | "electrum";
  label: string;
  host: string;
  port: number;
  rpc_user: string | null;
  cookie_path: string | null;
  use_tls: boolean;
  use_tor: boolean;
  enabled: boolean;
  is_default: boolean;
  last_connected_at: string | null;
  /** An Electrum server not on this computer or your own network: it sees the addresses Nakos syncs. */
  third_party: boolean;
}

export interface NewNodeConfig {
  kind: "bitcoin_core_rpc" | "electrum";
  label: string;
  host: string;
  port: number;
  rpc_user?: string | null;
  rpc_password?: string | null;
  cookie_path?: string | null;
  use_tls: boolean;
  use_tor: boolean;
}

export interface LightningChannel {
  id: string;
  wallet_id: string | null;
  channel_id: string | null;
  remote_alias: string | null;
  remote_pubkey: string | null;
  capacity_sats: number;
  local_balance_sats: number;
  remote_balance_sats: number;
  is_active: boolean;
  is_private: boolean;
  uptime_pct: number | null;
  updated_at: string;
}

export type Priority = "low" | "medium" | "high" | "critical";

export interface RecommendedAction {
  id: string;
  title: string;
  description: string;
  priority: Priority;
  points_available: number;
  category: string;
}

export interface ScoreFactor {
  key: string;
  label: string;
  points: number;
  max_points: number;
  detail: string;
}

export interface ScoreBreakdown {
  score: number;
  max_score: number;
  factors: ScoreFactor[];
  recommended_actions: RecommendedAction[];
}

export interface DimensionWeight {
  key: string;
  weight: number;
  score: number;
}

export interface NodeHealthReport {
  /** false → no enabled Bitcoin Core RPC row; show the empty state. */
  configured: boolean;
  node_label: string | null;
  score: number | null;
  reasons: string[];
  /** Set when the RPC call failed; score is null in that case. */
  error: string | null;
}

export interface CompositeResult {
  composite_score: number;
  dimensions: DimensionWeight[];
  node: NodeHealthReport;
}

export interface CustodyBreakdown {
  custody_type: string;
  total_sats: number;
  wallet_count: number;
}

export interface RiskBreakdown {
  risk_level: string;
  total_sats: number;
  wallet_count: number;
}

export interface PortfolioSummary {
  total_sats: number;
  wallet_count: number;
  by_custody_type: CustodyBreakdown[];
  by_risk_level: RiskBreakdown[];
}

export interface SizeBucket {
  bucket: "dust" | "small" | "medium" | "large" | "whale";
  count: number;
  total_sats: number;
}

export interface UtxoDistribution {
  by_size: SizeBucket[];
  total_utxos: number;
  total_sats: number;
  dust_count: number;
  average_amount_sats: number;
}

export interface ConsolidationPlan {
  wallet_id: string;
  input_count: number;
  total_input_sats: number;
  estimated_fee_sats: number;
  estimated_vbytes: number;
  fee_rate_sat_per_vb: number;
  consolidated_output_sats: number;
  destination_address: string;
  psbt_base64: string;
}

export type SpendOptimization = "tax" | "privacy" | "fee" | "fragmentation";

export interface SpendPlan {
  selected_utxo_ids: string[];
  input_count: number;
  total_input_sats: number;
  send_amount_sats: number;
  estimated_fee_sats: number;
  change_sats: number;
  estimated_realized_gain_fiat: number | null;
  optimization: string;
  psbt_base64: string;
  change_address: string | null;
  selection_note: string;
}

export interface NodeStatus {
  chain: string;
  blocks: number;
  headers: number;
  verification_progress: number;
  is_synced: boolean;
  initial_block_download: boolean;
  size_on_disk_bytes: number;
  prune_height: number | null;
  is_pruned: boolean;
  connections_in: number;
  connections_out: number;
  version: number;
  subversion: string;
  mempool_tx_count: number;
  mempool_size_bytes: number;
  network_reachable_via_tor: boolean;
}

export interface ElectrumServerStatus {
  server_version: string;
  protocol_version: string;
  tip_height: number;
  connected: boolean;
}

export interface NodeStatusResponse {
  core: NodeStatus | null;
  electrum: ElectrumServerStatus | null;
}

export interface FeeEstimate {
  target_blocks: number;
  sat_per_vb: number | null;
}

export interface PublicFeeEstimate {
  fastest_fee: number;
  half_hour_fee: number;
  hour_fee: number;
  economy_fee: number;
  minimum_fee: number;
}

export interface FeeEstimatesResponse {
  source: string;
  core_estimates: FeeEstimate[];
  public_estimate: PublicFeeEstimate | null;
  /** "disabled" (default, no request made), "ok", "error: …", or "not_needed". */
  explorer_status: string;
}

export interface VaultStatus {
  initialized: boolean;
  unlocked: boolean;
  keychain_remembered: boolean;
  keychain_supported: boolean;
  platform: "android" | "ios" | "desktop";
  /** Rust's std::env::consts::OS: "macos", "windows", "linux", "android", "ios". */
  os: string;
}

export interface BackupEntry {
  id: string;
  file_path: string;
  size_bytes: number;
  kind: "automatic" | "manual" | "export";
  created_at: string;
}

export interface SyncResult {
  wallet_id: string;
  new_addresses: number;
  new_utxos: number;
  new_transactions: number;
  balance_sats: number;
}

export interface ScriptMixEntry {
  script: string;
  count: number;
  sats: number;
}

export interface ReusedAddress {
  address: string;
  coins: number;
  sats: number;
}

export interface ReuseSummary {
  reused_addresses: number;
  coins_on_reused_addresses: number;
  sats_on_reused_addresses: number;
  worst: ReusedAddress[];
}

export interface ConsolidationRisk {
  level: "n/a" | "fragmented" | "concentrated" | "balanced";
  largest_share_pct: number;
  small_coin_share_pct: number;
  detail: string;
}

export interface SyncSourceInfo {
  configured: boolean;
  label: string | null;
  host: string | null;
  port: number | null;
  use_tls: boolean;
  last_connected_at: string | null;
  note: string;
}

export interface PrivacyDashboard {
  wallet_count: number;
  utxo_count: number;
  total_sats: number;
  script_mix: ScriptMixEntry[];
  reuse: ReuseSummary;
  consolidation: ConsolidationRisk;
  sync_source: SyncSourceInfo;
}

export interface PacketCosigner {
  label: string;
  xpub: string;
  fingerprint: string | null;
  derivation_path: string | null;
  hardware_model: string | null;
}

export interface PacketMultisig {
  threshold_m: number;
  total_n: number;
  script_type: string;
  cosigners: PacketCosigner[];
}

export interface PacketWallet {
  id: string;
  name: string;
  kind: "descriptor" | "xpub" | "multisig" | "address_only" | "manual";
  source_type: string;
  network: string;
  custody_type: string;
  archived: boolean;
  master_fingerprint: string | null;
  derivation_path: string | null;
  descriptor: string | null;
  change_descriptor: string | null;
  xpub: string | null;
  multisig: PacketMultisig | null;
  addresses: string[];
  heir_can_reconstruct: boolean;
  heir_warning: string | null;
  snapshot: PacketSnapshot | null;
  cost_basis: PacketLot[];
  note: string | null;
}

export interface PacketLot {
  txid: string;
  vout: number;
  amount_sats: number;
  acquired_at: string;
  cost_fiat: number;
  note: string | null;
}

export interface TaxReadiness {
  /** At least one coin, and every unspent coin in an unarchived wallet has a lot (cost + date). */
  ready: boolean;
  total_utxos: number;
  with_basis: number;
  missing: number;
}

export interface PacketSnapshot {
  balance_sats: number;
  utxo_count: number;
  as_of: string;
}

export interface PacketNode {
  label: string;
  kind: string;
  host: string;
  port: number;
  use_tls: boolean;
  use_tor: boolean;
  cookie_path: string | null;
  is_default: boolean;
}

export interface InheritancePacket {
  format_version: number;
  generated_at: string;
  watch_only_notice: string;
  wallets: PacketWallet[];
  nodes: PacketNode[];
  checklist: string[];
  letter: string | null;
  review_by: string | null;
  review_note: string | null;
}

export interface ReviewReminder {
  review_by: string | null;
  note: string | null;
  overdue: boolean;
}

export interface ImportSummary {
  created: string[];
  /** [wallet name, reason] */
  skipped: [string, string][];
  letter_saved: boolean;
  letter_note: string | null;
}

export interface SnapshotWallet {
  wallet_id: string;
  name: string;
  balance_sats: number;
  as_of: string;
}

export interface WalletBalance {
  wallet_id: string;
  balance_sats: number;
  /** True when this is a last-known balance carried in an imported letter. */
  is_snapshot: boolean;
}

export type ExplorerPreset = "mempool" | "esplora" | "blockcypher" | "custom";

export interface ExplorerSettings {
  enabled: boolean;
  preset: ExplorerPreset;
  base_url: string;
  /** A token is stored (encrypted). The token itself never reaches the UI. */
  has_token: boolean;
  /** The server the stored token belongs to; null for a token saved before this was recorded. */
  token_origin: string | null;
  privacy_note: string;
}

export interface ExplorerHeight {
  enabled: boolean;
  height: number | null;
  error: string | null;
}

export interface AddressLookup {
  address: string;
  confirmed_sats: number;
  unconfirmed_sats: number;
  total_received_sats: number;
  tx_count: number;
}

/** Inactivity auto-lock. `minutes` 0 means off. */
export interface AutoLockSettings {
  minutes: number;
  default_minutes: number;
  choices: number[];
  /** Android only: seconds in the background before the vault locks. */
  background_lock_seconds: number | null;
}

// -- receiving (0.6.0) -----------------------------------------------------

/** An address handed out by Receive, derived from the wallet's own descriptor. */
export interface ReceiveAddress {
  id: string;
  index: number;
  address: string;
  /** "m/84'/0'/0'/0/7", or "…/0/7" without an origin or for multisig. */
  path: string;
  label: string | null;
  amount_sats: number | null;
  device_checked_at: string | null;
  created_at: string;
  used: boolean;
  received_sats: number;
}

export interface ReceiveOverview {
  wallet_id: string;
  wallet_name: string;
  can_receive: boolean;
  reason: string | null;
  synced: boolean;
  fingerprints: string[];
  checksum: string | null;
  gap_limit: number;
  unused_given_out: number;
  /** No /* range: one address, given out once. */
  fixed: boolean;
  addresses: ReceiveAddress[];
}

/** A QR code as rows of '1' (dark) and '0' (light), made in Rust. */
export interface Qr {
  size: number;
  modules: string;
}

export interface SavedAddress {
  id: string;
  address: string;
  label: string | null;
  position: number;
  given_out_at: string | null;
  used_at: string | null;
  received_sats: number | null;
  device_checked_at: string | null;
  created_at: string;
  state: "fresh" | "given_out" | "used";
}

export interface AddressList {
  id: string;
  name: string;
  network: string;
  low_at: number;
  last_checked_at: string | null;
  fresh: number;
  given_out: number;
  used: number;
  low: boolean;
  addresses: SavedAddress[];
}

export interface AddAddressesResult {
  added: number;
  skipped: { text: string; reason: string }[];
}

export interface AddressCheckResult {
  checked: number;
  newly_used: number;
  new_alerts: number;
}

export type AlertKind = "balance_over" | "receive_paid" | "saved_used" | "list_low";

export interface AlertEvent {
  id: string;
  kind: AlertKind;
  wallet_id: string | null;
  list_id: string | null;
  message: string;
  created_at: string;
  read_at: string | null;
}

export interface BalanceAlert {
  wallet_id: string;
  threshold_sats: number;
  is_above: boolean;
}
