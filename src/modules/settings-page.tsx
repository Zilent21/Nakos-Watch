import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Download, Upload, DatabaseBackup, Lock, Trash2, Loader2, KeyRound, Github, Heart, ShieldCheck } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { PasswordInput } from "@/components/ui/password-input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { PageHeader } from "@/components/ui/page-header";
import * as api from "@/bindings/commands";
import { usePreferencesStore } from "@/stores/preferences-store";
import { useVaultStore } from "@/stores/vault-store";
import { formatRelativeTime } from "@/lib/format";
import { toast } from "@/hooks/use-toast";
import { getVersion } from "@tauri-apps/api/app";
import { RestoreBackupDialog } from "@/components/restore-backup-dialog";
import { Switch } from "@/components/ui/switch";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from "@/components/ui/dialog";
import type { ExplorerPreset } from "@/bindings/types";
import { useDeviceWord, useIsMobile, useOs, useStaysUnlockedNote } from "@/lib/platform";
import { keychainNote } from "@/lib/os-text";
import { DEFAULT_PHONE_TABS, PHONE_SCREENS, chooseTab } from "@/lib/phone-tabs";
import { screenFor } from "@/components/layout/nav-items";
import { ReceivingSettingsCard } from "@/modules/receive/receiving-settings";

const CURRENCIES = ["USD", "EUR", "GBP", "JPY", "CAD", "AUD", "CHF"];

const EXPLORER_PRESETS: { value: ExplorerPreset; label: string; url: string }[] = [
  { value: "mempool", label: "mempool.space", url: "https://mempool.space" },
  { value: "esplora", label: "blockstream.info (Esplora)", url: "https://blockstream.info" },
  { value: "blockcypher", label: "BlockCypher", url: "https://api.blockcypher.com/v1/btc/main" },
  { value: "custom", label: "Custom (mempool-compatible)", url: "" },
];

/** Optional public explorer: off by default, and nothing is requested until it is on. */
function ExplorerCard() {
  const device = useDeviceWord();
  const queryClient = useQueryClient();
  const { data: saved, error } = useQuery({ queryKey: ["explorer-settings"], queryFn: api.getExplorerSettings });
  const [enabled, setEnabled] = useState(false);
  const [preset, setPreset] = useState<ExplorerPreset>("mempool");
  const [baseUrl, setBaseUrl] = useState("");
  const [token, setToken] = useState("");
  const [address, setAddress] = useState("");

  useEffect(() => {
    if (!saved) return;
    setEnabled(saved.enabled);
    setPreset(saved.preset);
    setBaseUrl(saved.base_url);
  }, [saved]);

  const refreshDependents = () => {
    queryClient.invalidateQueries({ queryKey: ["explorer-settings"] });
    queryClient.invalidateQueries({ queryKey: ["fee-estimates"] });
    queryClient.invalidateQueries({ queryKey: ["explorer-height"] });
  };

  const saveMutation = useMutation({
    mutationFn: (opts: { clearToken: boolean }) =>
      api.saveExplorerSettings({ enabled, preset, base_url: baseUrl, token: token || null, clear_token: opts.clearToken }),
    onSuccess: (_d, opts) => {
      setToken("");
      refreshDependents();
      toast({ title: enabled ? "Public explorer enabled" : "Public explorer off", description: opts.clearToken ? "API token removed." : undefined, variant: "success" });
    },
    onError: (e) => toast({ title: "Couldn't save explorer settings", description: String(e), variant: "destructive" }),
  });

  const lookupMutation = useMutation({
    mutationFn: () => api.explorerLookupAddress(address),
    onError: (e) => toast({ title: "Lookup failed", description: String(e), variant: "destructive" }),
  });

  const savedEnabled = saved?.enabled ?? false;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Public explorer (optional)</CardTitle>
        <CardDescription>
          Off by default. Your wallets sync through your own Electrum server, and node health comes from your own Bitcoin Core. When on, Nakos may ask this server for fee
          estimates (only if your own node has none), the chain tip height, and any single address you choose to look up. It is never used to find addresses from a key.{" "}
          <span className="text-foreground">{saved?.privacy_note ?? "This server sees the addresses or fee requests you send."}</span>
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {error ? <p className="text-sm text-destructive">Couldn't load explorer settings: {String(error)}</p> : null}
        <label className="flex items-center gap-3 text-sm">
          <Switch checked={enabled} onCheckedChange={setEnabled} />
          Use a public explorer {enabled ? "(on once saved)" : "(off)"}
        </label>

        <div className="grid grid-cols-2 gap-3">
          <div className="space-y-1.5">
            <Label>Preset</Label>
            <Select
              value={preset}
              onValueChange={(v) => {
                const p = EXPLORER_PRESETS.find((x) => x.value === v);
                setPreset(v as ExplorerPreset);
                if (p) setBaseUrl(p.url); // fills the box; calls nothing
              }}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {EXPLORER_PRESETS.map((p) => (
                  <SelectItem key={p.value} value={p.value}>
                    {p.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1.5">
            <Label>Base URL</Label>
            <Input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://…" className="font-mono text-xs" />
          </div>
        </div>

        <div className="space-y-1.5">
          <Label>API token (optional)</Label>
          <div className="flex gap-2">
            <Input
              type="password"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder={saved?.has_token ? "A token is stored (encrypted). Type to replace." : "Only if your server needs one"}
              autoComplete="off"
            />
            {saved?.has_token && (
              <Button variant="outline" onClick={() => saveMutation.mutate({ clearToken: true })} disabled={saveMutation.isPending}>
                Remove token
              </Button>
            )}
          </div>
          <p className="text-[11px] text-muted-foreground">
            Stored encrypted with your vault key. Never shown again, never exported. A token is tied to the server it was saved for and is only sent there, over https (or to this {device}).
          </p>
          {saved?.has_token && (
            <p className="text-[11px] text-muted-foreground">
              {saved.token_origin
                ? `The stored token is for ${saved.token_origin}. To use another server, enter its token or remove this one.`
                : "The stored token isn't tied to a server (saved by an older version) and won't be sent. Enter it again, or remove it."}
            </p>
          )}
        </div>

        <Button onClick={() => saveMutation.mutate({ clearToken: false })} disabled={saveMutation.isPending}>
          Save explorer settings
        </Button>

        <div className="space-y-2 border-t border-border pt-4">
          <Label>Look up one address</Label>
          <div className="flex gap-2">
            <Input value={address} onChange={(e) => setAddress(e.target.value)} placeholder="bc1q…" className="font-mono text-xs" disabled={!savedEnabled} />
            <Button variant="outline" onClick={() => lookupMutation.mutate()} disabled={!savedEnabled || !address.trim() || lookupMutation.isPending}>
              Look up
            </Button>
          </div>
          {!savedEnabled && <p className="text-xs text-muted-foreground">Disabled. Turn the explorer on and save to look up an address.</p>}
          {lookupMutation.data && (
            <div className="mono-nums text-xs text-muted-foreground">
              Confirmed {lookupMutation.data.confirmed_sats.toLocaleString()} sats · unconfirmed {lookupMutation.data.unconfirmed_sats.toLocaleString()} · received{" "}
              {lookupMutation.data.total_received_sats.toLocaleString()} · {lookupMutation.data.tx_count} tx
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

/** Erase everything Nakos keeps on this computer and go back to "Create your vault". */
function EraseDataCard() {
  const device = useDeviceWord();
  const mobile = useIsMobile();
  const queryClient = useQueryClient();
  const refresh = useVaultStore((s) => s.refresh);
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState("");
  const [error, setError] = useState<string | null>(null);

  const erase = useMutation({
    mutationFn: () => api.eraseAllData(typed),
    onSuccess: async (r) => {
      setOpen(false);
      setTyped("");
      queryClient.clear();
      await refresh();
      toast({
        title: "All Nakos data erased",
        description: r.keychain_cleared ? "Create a new vault, or restore one from an encrypted backup." : "The remembered passphrase could not be removed from the keychain; it will not be used again.",
        variant: "success",
      });
    },
    onError: (e) => setError(String(e)),
  });

  const close = (v: boolean) => {
    setOpen(v);
    if (!v) {
      setTyped("");
      setError(null);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Erase all data</CardTitle>
        <CardDescription>Delete this vault from this {device} and start again from "Create your vault".</CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <p className="text-sm text-muted-foreground">
          This deletes your wallets, coins, labels, node settings, inheritance notes and the automatic local copies{mobile ? "" : ", and removes the remembered passphrase from the keychain"}. It cannot be undone. Export an encrypted
          backup first if you might want this vault back.
        </p>
        <Button variant="destructive" onClick={() => setOpen(true)}>
          <Trash2 className="h-4 w-4" /> Erase all data…
        </Button>
        <Dialog open={open} onOpenChange={close}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Erase all Nakos data on this {device}?</DialogTitle>
              <DialogDescription>
                Everything in this vault and its automatic local copies is deleted, and Nakos opens at "Create your vault". Encrypted backups, family letters and packets you saved elsewhere are not touched.
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-2">
              <Label htmlFor="erase-confirm">Type ERASE to confirm</Label>
              <Input id="erase-confirm" autoComplete="off" spellCheck={false} value={typed} onChange={(e) => { setTyped(e.target.value); setError(null); }} />
              {error && (
                <p role="alert" className="text-sm text-destructive">
                  {error}
                </p>
              )}
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={() => close(false)}>
                Cancel
              </Button>
              <Button variant="destructive" disabled={typed !== "ERASE" || erase.isPending} onClick={() => erase.mutate()}>
                {erase.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
                Erase everything
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </CardContent>
    </Card>
  );
}

/** Vault protection, stated plainly, plus the optional keychain helper and the app version. */
function VaultCard() {
  const { lock, keychainSupported, keychainRemembered, refresh } = useVaultStore();
  const os = useOs();
  const mobile = useIsMobile();
  const staysUnlocked = useStaysUnlockedNote();
  const { data: version } = useQuery({ queryKey: ["app-version"], queryFn: getVersion });
  const [askPass, setAskPass] = useState(false);
  const [pass, setPass] = useState("");

  const toggle = useMutation({
    mutationFn: ({ enabled, passphrase }: { enabled: boolean; passphrase?: string }) => api.setKeychainHelper(enabled, passphrase),
    onSuccess: async (_d, v) => {
      setPass("");
      setAskPass(false);
      await refresh();
      toast({ title: v.enabled ? "Keychain helper on" : "Keychain helper off", variant: "success" });
    },
    onError: (e) => toast({ title: "Couldn't change the keychain helper", description: String(e), variant: "destructive" }),
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle>Vault</CardTitle>
        <CardDescription>What the passphrase protects, and what it does not.</CardDescription>
      </CardHeader>
      <CardContent className="space-y-6">
        <div className="space-y-2 text-sm">
          <p>
            Nakos is watch-only. It never stores a seed phrase or a private key. The vault passphrase protects these secrets: RPC passwords, the explorer API token, your inheritance letter
            and notes, and recovery contact notes.
          </p>
          <p className="text-muted-foreground">
            It does not encrypt every row. Descriptors, extended public keys, addresses, balances, coins, labels, cost basis and contact names are stored in the database file unencrypted, so they are readable
            by anyone who has that file while the disk is unlocked. {mobile ? "Use a screen lock on your phone." : "Turn on full-disk encryption in your operating system."}
          </p>
        </div>

        {mobile ? (
          <p className="text-xs text-muted-foreground">Nakos asks for the vault passphrase every time it opens. It is not saved on the phone.</p>
        ) : (
        <div className="space-y-2">
          <label className="flex items-center gap-3 text-sm">
            <Switch
              checked={keychainRemembered}
              disabled={!keychainSupported || toggle.isPending}
              onCheckedChange={(on) => (on ? setAskPass(true) : toggle.mutate({ enabled: false }))}
            />
            Remember helper in the OS keychain
          </label>
          <p className="text-xs text-muted-foreground">
            Off by default. The vault passphrase alone unlocks Nakos. If you turn this on, the passphrase is saved in your operating system's keychain so Nakos can open without
            asking after a restart. It is kept only while Nakos is unlocked: any lock, by you or by the timer, removes it from the keychain and turns this off. {keychainNote(os)}
            {!keychainSupported && " Unavailable in development builds."}
          </p>
          {askPass && (
            <div className="flex gap-2">
              <PasswordInput autoComplete="off" placeholder="Vault passphrase" value={pass} onChange={(e) => setPass(e.target.value)} />
              <Button disabled={!pass || toggle.isPending} onClick={() => toggle.mutate({ enabled: true, passphrase: pass })}>
                Turn on
              </Button>
              <Button variant="outline" onClick={() => { setAskPass(false); setPass(""); }}>
                Cancel
              </Button>
            </div>
          )}
        </div>
        )}

        <AutoLockSetting />

        <div className="flex items-center justify-between border-t border-border pt-4">
          <div className="space-y-1">
            <Button variant="outline" onClick={() => lock()}>
              <Lock className="h-4 w-4" /> Lock vault now
            </Button>
            <p data-testid="stays-unlocked" className="text-xs text-muted-foreground">{staysUnlocked}</p>
          </div>
          <span className="mono-nums text-xs text-muted-foreground">Nakos {version ?? ""}</span>
        </div>
      </CardContent>
    </Card>
  );
}

/** Optional daily health snapshot: off by default, written once per day after unlock. */
function HealthHistoryCard() {
  const queryClient = useQueryClient();
  const { data: pref } = useQuery({ queryKey: ["pref", "health_snapshot_daily"], queryFn: () => api.getPreference("health_snapshot_daily") });
  const toggle = useMutation({
    mutationFn: (on: boolean) => api.setPreference("health_snapshot_daily", on ? "true" : "false"),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["pref", "health_snapshot_daily"] }),
    onError: (e) => toast({ title: "Couldn't change the setting", description: String(e), variant: "destructive" }),
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle>Health history</CardTitle>
        <CardDescription>The Health score page charts saved snapshots. You can always save one there yourself.</CardDescription>
      </CardHeader>
      <CardContent className="space-y-2">
        <label className="flex items-center gap-3 text-sm">
          <Switch checked={pref === "true"} disabled={toggle.isPending} onCheckedChange={(on) => toggle.mutate(on)} />
          Save one snapshot a day
        </label>
        <p className="text-xs text-muted-foreground">
          Off by default. When on, Nakos saves one snapshot the first time you unlock it each day, unless one was already saved that day. Nothing runs while the vault is locked. Snapshots stay in your local database; this adds no network use beyond the node check the score already makes.
        </p>
      </CardContent>
    </Card>
  );
}

/** Phones only: which four screens sit in the bottom bar. The rest are under More. */
function PhoneTabsCard() {
  const { phoneTabs, setPhoneTabs } = usePreferencesStore();
  const save = (tabs: string[]) =>
    setPhoneTabs(tabs).catch((e) => toast({ title: "Couldn't save the bottom bar", description: String(e), variant: "destructive" }));
  const isDefault = phoneTabs.join() === DEFAULT_PHONE_TABS.join();

  return (
    <Card data-testid="phone-tabs-card">
      <CardHeader>
        <CardTitle>Bottom bar</CardTitle>
        <CardDescription>Choose the four screens in the bar at the bottom. Everything else is under More.</CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        {phoneTabs.map((path, slot) => (
          <div key={slot} className="flex items-center gap-3">
            <Label className="w-12 shrink-0 text-muted-foreground">Tab {slot + 1}</Label>
            <Select value={path} onValueChange={(screen) => save(chooseTab(phoneTabs, slot, screen))}>
              <SelectTrigger aria-label={`Tab ${slot + 1}`}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PHONE_SCREENS.map((screen) => (
                  <SelectItem key={screen} value={screen}>
                    {screenFor(screen).label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        ))}
        <p className="text-xs text-muted-foreground">Choosing a screen that is already in another tab swaps the two.</p>
        <Button variant="outline" size="sm" disabled={isDefault} onClick={() => save(DEFAULT_PHONE_TABS)}>
          Reset to Today, Portfolio, Coins, Node
        </Button>
      </CardContent>
    </Card>
  );
}

type ProjectLink = "source" | "support" | "help_import" | "security";
const LINK_TEXT: Record<ProjectLink, string> = {
  source: "nakos.org/source",
  support: "nakos.org/#support",
  help_import: "nakos.org/help/import",
  security: "nakos.org/security",
};

/** Opens one of Nakos's own pages in the browser. On a phone, where Nakos can't open the browser,
 * it says the address instead. */
export function openLink(which: ProjectLink) {
  api.openProjectLink(which).catch(() => toast({ title: "Open this in your browser", description: LINK_TEXT[which] }));
}

/** What Nakos is, where its source is, and how to support it. Nothing here is gated. */
function AboutCard() {
  const { data: version } = useQuery({ queryKey: ["app-version"], queryFn: getVersion });
  return (
    <Card data-testid="about-card">
      <CardHeader>
        <CardTitle>About Nakos</CardTitle>
        <CardDescription>
          Nakos {version ?? ""} is free and open-source software (MIT license). Every feature is included for everyone, with no account and no license key. It is watch-only:
          it never asks for, stores or signs with a seed phrase or a private key.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex flex-wrap gap-3">
          <Button variant="outline" onClick={() => openLink("source")}>
            <Github className="h-4 w-4" /> View source
          </Button>
          <Button variant="outline" onClick={() => openLink("security")}>
            <ShieldCheck className="h-4 w-4" /> Report a security issue
          </Button>
        </div>
        <div className="space-y-2 border-t border-border pt-4">
          <p className="text-sm font-medium">Support development</p>
          <p className="text-sm text-muted-foreground">
            If Nakos is useful to you, you can support its development with a one-time payment of any amount over Lightning or on-chain. It's optional and unlocks nothing,
            because nothing is locked. No account, no email.
          </p>
          <Button variant="outline" onClick={() => openLink("support")}>
            <Heart className="h-4 w-4" /> Support Nakos
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

/** How long Nakos waits before locking itself. The clock runs in Rust (`commands/autolock.rs`). */
function AutoLockSetting() {
  const queryClient = useQueryClient();
  const mobile = useIsMobile();
  const { data } = useQuery({ queryKey: ["auto-lock"], queryFn: api.getAutoLock });
  const save = useMutation({
    mutationFn: (m: number) => api.setAutoLock(m),
    onSuccess: (d) => {
      queryClient.setQueryData(["auto-lock"], d);
      toast({ title: d.minutes ? `Locks after ${d.minutes} min without use` : "Auto-lock off", variant: "success" });
    },
    onError: (e) => toast({ title: "Couldn't change auto-lock", description: String(e), variant: "destructive" }),
  });
  if (!data) return null;
  const label = (m: number) => (m === 0 ? "Never" : m === 60 ? "1 hour" : `${m} minute${m === 1 ? "" : "s"}`) + (m === data.default_minutes ? " (default)" : "");
  return (
    <div className="space-y-2">
      <Label>Lock automatically after</Label>
      <Select value={String(data.minutes)} onValueChange={(v) => save.mutate(Number(v))}>
        <SelectTrigger className="w-56" aria-label="Auto-lock">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {data.choices.map((m) => (
            <SelectItem key={m} value={String(m)}>
              {label(m)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <p className="text-xs text-muted-foreground">
        Counts time without a click, key press or touch in Nakos, including while it is minimised.
        {mobile && data.background_lock_seconds ? ` On a phone Nakos also locks ${data.background_lock_seconds} seconds after it leaves the screen, whatever you choose here.` : ""}
        {data.minutes === 0 && !mobile ? " With auto-lock off, Nakos stays unlocked until you lock it or quit." : ""}
      </p>
    </div>
  );
}

export function SettingsPage() {
  const queryClient = useQueryClient();
  const device = useDeviceWord();
  const mobile = useIsMobile();
  const { fiatCurrency, fiatPricePerBtc, displayUnit, setFiatCurrency, setFiatPrice, setDisplayUnit } = usePreferencesStore();
  const [priceInput, setPriceInput] = useState(fiatPricePerBtc?.toString() ?? "");
  const [message, setMessage] = useState<string | null>(null);

  const { data: backups = [] } = useQuery({ queryKey: ["backups"], queryFn: api.listBackups });

  const backupMutation = useMutation({
    mutationFn: api.runLocalBackup,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["backups"] });
      setMessage("Local copy created.");
    },
    onError: (e) => toast({ title: "Local copy failed", description: String(e), variant: "destructive" }),
  });

  const exportMutation = useMutation({
    mutationFn: async () => {
      return api.exportEncryptedBackup(); // Rust opens the save dialog; null if cancelled
    },
    onSuccess: (path) => {
      if (path) {
        queryClient.invalidateQueries({ queryKey: ["backups"] });
        toast({ title: "Encrypted backup saved", description: `${path} — restoring it needs your vault passphrase. It is not in the file.`, variant: "success" });
      }
    },
    onError: (e) => toast({ title: "Backup failed", description: String(e), variant: "destructive" }),
  });
  const [restoreOpen, setRestoreOpen] = useState(false);

  return (
    <div className="space-y-6">
      <PageHeader title="Settings" description="Preferences, backups and the vault." />

      {mobile && <PhoneTabsCard />}

      <ReceivingSettingsCard />

      <Card>
        <CardHeader>
          <CardTitle>Display & exchange rate</CardTitle>
          <CardDescription>Fiat values are computed locally from a rate you set — never fetched with your balances.</CardDescription>
        </CardHeader>
        <CardContent className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <div className="space-y-1.5">
            <Label>Display unit</Label>
            <Select value={displayUnit} onValueChange={(v) => setDisplayUnit(v as "sats" | "btc")}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="sats">Sats</SelectItem>
                <SelectItem value="btc">BTC</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1.5">
            <Label>Fiat currency</Label>
            <Select value={fiatCurrency} onValueChange={setFiatCurrency}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {CURRENCIES.map((c) => (
                  <SelectItem key={c} value={c}>
                    {c}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1.5">
            <Label>Manual price (BTC/{fiatCurrency})</Label>
            <div className="flex gap-2">
              <Input type="number" value={priceInput} onChange={(e) => setPriceInput(e.target.value)} />
              <Button variant="outline" onClick={() => setFiatPrice(Number(priceInput))}>
                Set
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Encrypted vault backup</CardTitle>
          <CardDescription>
            The whole vault in one file, sealed with your vault passphrase. This is for you. To give your family a way to see your bitcoin, use the letter under Inheritance; it is a different file and is not a backup.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-wrap gap-3">
            <Button variant="outline" onClick={() => exportMutation.mutate()} disabled={exportMutation.isPending}>
              <Download className="h-4 w-4" /> Export encrypted backup…
            </Button>
            <Button variant="outline" onClick={() => setRestoreOpen(true)}>
              <Upload className="h-4 w-4" /> Restore from encrypted backup…
            </Button>
          </div>
          <div role="note" className="flex gap-3 rounded-md border border-primary/60 bg-primary/10 p-3">
            <KeyRound className="mt-0.5 h-5 w-5 shrink-0 text-primary" />
            <div className="space-y-1 text-sm">
              <p className="font-semibold text-foreground">You need your vault passphrase to restore this backup.</p>
              <p className="text-muted-foreground">
                It is the passphrase you unlock Nakos with. It is not saved in the backup file, and nobody, including Nakos, can recover it. Write it down or keep it in a password manager, somewhere other than the backup file.
              </p>
            </div>
          </div>
          <p className="text-xs text-muted-foreground">Restoring replaces this vault with the backup, and the backup's passphrase becomes the vault passphrase.</p>
          <RestoreBackupDialog open={restoreOpen} onOpenChange={setRestoreOpen} hasVault />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Local copies</CardTitle>
          <CardDescription>
            Nakos keeps a few automatic copies of its database on this {device}. They are plain copies of the file, protected exactly like the live database (see Vault below), so they are not a safe place for secrets and not an off-site backup.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div>
            <Button variant="outline" onClick={() => backupMutation.mutate()}>
              <DatabaseBackup className="h-4 w-4" /> Make a local copy now
            </Button>
          </div>
          {message && <p className="text-xs text-muted-foreground">{message}</p>}
          <div className="space-y-2">
            {backups.slice(0, 8).map((b) => (
              <div key={b.id} className="flex items-center justify-between rounded-md border border-border px-3 py-2 text-xs">
                <span className="font-mono">{b.file_path.split("/").pop()}</span>
                <span className="text-muted-foreground">
                  {b.kind} · {(b.size_bytes / 1024).toFixed(0)} KB · {formatRelativeTime(b.created_at)}
                </span>
              </div>
            ))}
            {backups.length === 0 && <p className="text-sm text-muted-foreground">No local copies yet.</p>}
          </div>
        </CardContent>
      </Card>

      <HealthHistoryCard />

      <ExplorerCard />

      <VaultCard />

      <AboutCard />

      <EraseDataCard />
    </div>
  );
}
