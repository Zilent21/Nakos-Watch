import { invoke } from "@tauri-apps/api/core";
import type * as T from "./types";

// -- wallets ---------------------------------------------------------------

export const listWallets = (includeArchived?: boolean) =>
  invoke<T.Wallet[]>("list_wallets", { includeArchived });

export const addWallet = (input: T.NewWallet) => invoke<T.Wallet>("add_wallet", { input });

export const detectSourceType = (input: string) => invoke<string>("detect_source_type", { input });

export const updateWallet = (
  id: string,
  patch: Partial<Pick<T.Wallet, "name" | "custody_type" | "risk_level" | "color" | "notes" | "manual_balance_sats" | "manual_cost_basis_fiat">>,
) => invoke<T.Wallet>("update_wallet", { id, patch });

export const archiveWallet = (id: string, archived: boolean) => invoke<void>("archive_wallet", { id, archived });

export const syncWallet = (id: string) => invoke<T.SyncResult>("sync_wallet", { id });
/** The person read the privacy warning for someone else's Electrum server and chose to sync through it. */
export const acknowledgePublicElectrum = (host: string, port: number) => invoke<void>("acknowledge_public_electrum", { host, port });

export const addMultisigWallet = (input: {
  name: string;
  custody_type: T.CustodyType;
  risk_level: T.RiskLevel;
  network: string;
  threshold_m: number;
  cosigners: T.Cosigner[];
  script_type: string;
}) => invoke<T.Wallet>("add_multisig_wallet", input);

export const getMultisigConfig = (walletId: string) =>
  invoke<T.MultisigConfig | null>("get_multisig_config", { walletId });

export const listWalletBalances = (includeArchived?: boolean) =>
  invoke<T.WalletBalance[]>("list_wallet_balances", { includeArchived });

export const portfolioSummary = () => invoke<T.PortfolioSummary>("portfolio_summary");

// -- utxos -------------------------------------------------------------------

export interface UtxoFilter {
  wallet_id?: string;
  min_amount_sats?: number;
  max_amount_sats?: number;
  is_frozen?: boolean;
  include_spent?: boolean;
  label_contains?: string;
  include_archived?: boolean;
}

export const listUtxos = (filter?: UtxoFilter) => invoke<T.Utxo[]>("list_utxos", { filter });

export const freezeUtxo = (id: string, frozen: boolean) => invoke<void>("freeze_utxo", { id, frozen });

export const labelUtxo = (id: string, label: string | null) => invoke<void>("label_utxo", { id, label });

/** Set the cost-basis lot for a coin (cost and date together), or clear it with both null. */
export const setUtxoBasis = (id: string, costFiat: number | null, acquiredAt: string | null, note: string | null) =>
  invoke<void>("set_utxo_basis", { id, costFiat, acquiredAt, note });

export const getTaxReadiness = () => invoke<T.TaxReadiness>("get_tax_readiness");

export const utxoDistribution = (walletId?: string, includeArchived?: boolean) =>
  invoke<T.UtxoDistribution>("utxo_distribution", { walletId, includeArchived });

export const planConsolidation = (walletId: string, feeRateSatPerVb: number, maxInputs?: number) =>
  invoke<T.ConsolidationPlan>("plan_consolidation", { walletId, feeRateSatPerVb, maxInputs });

export const planSpend = (args: {
  walletId: string;
  destinationAddress: string;
  amountSats: number;
  optimization: T.SpendOptimization;
  feeRateSatPerVb: number;
  currentPriceFiatPerBtc?: number;
}) => invoke<T.SpendPlan>("plan_spend", args);

// -- scoring -------------------------------------------------------------------

export const getSecurityScore = () => invoke<T.ScoreBreakdown>("get_security_score");
export const getPrivacyScore = () => invoke<T.ScoreBreakdown>("get_privacy_score");
export const getPrivacyDashboard = () => invoke<T.PrivacyDashboard>("get_privacy_dashboard");
export const getHealthScore = () => invoke<T.CompositeResult>("get_health_score");
/** Explicit history write: recomputes in the backend and appends one snapshot row; returns its id. */
export const recordHealthSnapshot = () => invoke<string>("record_health_snapshot");
/** Fire-and-forget: returns at once. Writes a snapshot only if the user turned on the daily option and none exists for today. */
export const runDailyHealthSnapshot = () => invoke<void>("run_daily_health_snapshot");
export const getHealthHistory = (limit?: number) =>
  invoke<T.HealthScoreSnapshot[]>("get_health_history", { limit });

export const updateSecurityConfig = (patch: {
  wallet_id?: string | null;
  hardware_wallet_model?: string | null;
  firmware_up_to_date?: boolean | null;
  passphrase_enabled?: boolean | null;
  geographic_redundancy_locations?: number | null;
  backup_tested?: boolean | null;
  exchange_2fa_type?: string | null;
}) => invoke<T.SecurityConfig>("update_security_config", { patch });

// -- node ---------------------------------------------------------------------

export const listNodeConfigs = () => invoke<T.NodeConfig[]>("list_node_configs");
export const addNodeConfig = (input: T.NewNodeConfig) => invoke<T.NodeConfig>("add_node_config", { input });
export const updateNodeConfig = (id: string, input: T.NewNodeConfig) =>
  invoke<T.NodeConfig>("update_node_config", { id, input });
export const testNodeConnection = (id: string) => invoke<boolean>("test_node_connection", { id });
/** Make a connection that has passed "Test connection" the primary one for its type. */
/** Archive or restore a saved connection. Nothing is deleted; archived ones are hidden unless "Show archived" is on. */
export const archiveNodeConfig = (id: string, archived: boolean) => invoke<void>("archive_node_config", { id, archived });
export const setPrimaryNode = (id: string) => invoke<void>("set_primary_node", { id });
export const getNodeStatus = () => invoke<T.NodeStatusResponse>("get_node_status");
export const getExplorerSettings = () => invoke<T.ExplorerSettings>("get_explorer_settings");

export const saveExplorerSettings = (input: {
  enabled: boolean;
  preset: T.ExplorerPreset;
  base_url: string;
  token?: string | null;
  clear_token: boolean;
}) => invoke<void>("save_explorer_settings", { input });

export const getExplorerHeight = () => invoke<T.ExplorerHeight>("get_explorer_height");

export const explorerLookupAddress = (address: string) => invoke<T.AddressLookup>("explorer_lookup_address", { address });

export const getFeeEstimates = () => invoke<T.FeeEstimatesResponse>("get_fee_estimates");

// -- inheritance ----------------------------------------------------------------

export const getInheritanceConfig = (walletId?: string) =>
  invoke<T.InheritanceConfig>("get_inheritance_config", { walletId });

export const saveInheritanceInstructions = (walletId: string | undefined, instructions: string) =>
  invoke<void>("save_inheritance_instructions", { walletId, instructions });

export const getInheritanceInstructions = (walletId?: string) =>
  invoke<string | null>("get_inheritance_instructions", { walletId });

export const getInheritancePacket = (includeArchived?: boolean) =>
  invoke<T.InheritancePacket>("get_inheritance_packet", { includeArchived });

/** Rust opens the save dialog. Resolves to the path written, or null if the person cancelled. */
export const exportInheritancePacket = (args: {
  format: "json" | "markdown" | "markdown_pdf" | "letter" | "letter_pdf";
  includeNotes: boolean;
  includeArchived: boolean;
  /** Advanced JSON only; ignored for Markdown and the simple letter. */
  includeBasis?: boolean;
  /** Family letter only. */
  template?: "family" | "single_sig_cold" | "multisig";
}) => invoke<string | null>("export_inheritance_packet", args);

/** Rust opens the file dialog. Resolves to null if the person cancelled. */
export const importInheritancePacket = () => invoke<T.ImportSummary | null>("import_inheritance_packet");

export const snapshotWallets = () => invoke<T.SnapshotWallet[]>("snapshot_wallets");

export const getReviewReminder = () => invoke<T.ReviewReminder>("get_review_reminder");

export const saveReviewReminder = (reviewBy: string | null, note: string | null) =>
  invoke<void>("save_review_reminder", { reviewBy, note });

export const addRecoveryContact = (input: {
  name: string;
  relationship?: string;
  contact_method?: string;
  has_key_share: boolean;
  notes?: string;
  link_to_wallet_id?: string;
}) => invoke<T.RecoveryContact>("add_recovery_contact", { input });

export const markRecoveryTested = (walletId: string | undefined, passed: boolean) =>
  invoke<void>("mark_recovery_tested", { walletId, passed });

// -- vault / backup ---------------------------------------------------------------

export const vaultStatus = () => invoke<T.VaultStatus>("vault_status");
export const vaultInit = (passphrase: string, remember: boolean) =>
  invoke<void>("vault_init", { passphrase, remember });
export const vaultUnlock = (passphrase: string, remember: boolean) =>
  invoke<void>("vault_unlock", { passphrase, remember });
export const vaultLock = () => invoke<void>("vault_lock");
export const vaultTryKeychainUnlock = () => invoke<boolean>("vault_try_keychain_unlock");

/** Rust opens the save dialog. Resolves to the path written, or null if the person cancelled. */
export const exportEncryptedBackup = () => invoke<string | null>("export_encrypted_backup");
/** Rust opens the file dialog and remembers the choice; resolves to the chosen path, or null if cancelled. */
export const pickBackupToRestore = () => invoke<string | null>("pick_backup_to_restore");
/** Restores the file chosen with `pickBackupToRestore`; the frontend never names a path. */
export const restoreEncryptedBackup = (passphrase: string) => invoke<{ wallets: number }>("restore_encrypted_backup", { passphrase });
/** Deletes this computer's vault, its local copies and the remembered passphrase. Rust refuses unless `confirm` is "ERASE". */
export const eraseAllData = (confirm: string) => invoke<{ keychain_cleared: boolean }>("erase_all_data", { confirm });
export const setKeychainHelper =(enabled: boolean, passphrase?: string) => invoke<void>("set_keychain_helper", { enabled, passphrase });
export const runLocalBackup = () => invoke<string>("run_local_backup");
export const listBackups = () => invoke<T.BackupEntry[]>("list_backups");

// -- preferences ---------------------------------------------------------------

export const getPreference = (key: string) => invoke<string | null>("get_preference", { key });
export const setPreference = (key: string, value: string) => invoke<void>("set_preference", { key, value });
export const getAllPreferences = () => invoke<[string, string][]>("get_all_preferences");

// -- auto-lock ------------------------------------------------------------------

export const vaultTouch = () => invoke<void>("vault_touch");
export const getAutoLock = () => invoke<T.AutoLockSettings>("get_auto_lock");
export const setAutoLock = (minutes: number) => invoke<T.AutoLockSettings>("set_auto_lock", { minutes });
/** Android: the app went to the background (true) or came back (false). */
export const vaultAppHidden = (hidden: boolean) => invoke<void>("vault_app_hidden", { hidden });

// -- about ------------------------------------------------------------------------

/** Opens one of Nakos's own pages in the system browser. The webview can only name which one. */
export const openProjectLink = (which: "source" | "support" | "help_import" | "security") => invoke<void>("open_project_link", { which });
