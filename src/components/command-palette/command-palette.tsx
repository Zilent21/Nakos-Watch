import { useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Command } from "cmdk";
import { ask } from "@tauri-apps/plugin-dialog";
import { Archive, DatabaseBackup, FileText, Lock, Plus, RefreshCw } from "lucide-react";
import { useVaultStore } from "@/stores/vault-store";
import { useShellStore } from "@/stores/shell-store";
import { toast } from "@/hooks/use-toast";
import * as api from "@/bindings/commands";
import { ALL_SCREENS } from "@/components/layout/nav-items";
import { handlePublicServerGate } from "@/components/public-server-gate";

const itemClass = "flex h-10 cursor-pointer items-center gap-3 rounded-md px-3 text-sm aria-selected:bg-accent";
const groupClass = "px-1 py-2 text-xs font-medium text-muted-foreground **:[[cmdk-group-heading]]:mb-1 **:[[cmdk-group-heading]]:px-2";

/**
 * Cmd/Ctrl+K. Searches every screen and runs the everyday actions: add a
 * wallet, sync or archive a wallet, export the family letter, back up, lock.
 */
export function CommandPalette() {
  const open = useShellStore((s) => s.paletteOpen);
  const setOpen = useShellStore((s) => s.setPaletteOpen);
  const setAddWalletOpen = useShellStore((s) => s.setAddWalletOpen);
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const lock = useVaultStore((s) => s.lock);

  const { data: wallets = [] } = useQuery({ queryKey: ["wallets", false], queryFn: () => api.listWallets(false), enabled: open });

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen(!useShellStore.getState().paletteOpen);
      }
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [setOpen]);

  const refreshWalletData = () => {
    for (const key of ["wallets", "portfolio-summary", "utxos", "utxo-distribution", "snapshot-wallets", "tax-readiness"]) {
      queryClient.invalidateQueries({ queryKey: [key] });
    }
  };

  const syncMutation = useMutation({
    mutationFn: ({ id }: { id: string; name: string }) => api.syncWallet(id),
    onSuccess: (r, v) => {
      refreshWalletData();
      toast({ title: `Synced ${v.name}`, description: r.new_utxos > 0 ? `${r.new_utxos} new coin${r.new_utxos === 1 ? "" : "s"}` : "No new activity", variant: "success" });
    },
    onError: (e, v) => {
      if (handlePublicServerGate(e, () => syncMutation.mutate(v))) return;
      toast({ title: `Couldn't sync ${v.name}`, description: String(e), variant: "destructive" });
    },
  });

  const archiveMutation = useMutation({
    mutationFn: async ({ id, name }: { id: string; name: string }) => {
      const ok = await ask(`Archive "${name}"? It leaves your totals and Coins, but nothing is deleted. Restore it from Portfolio with "Show archived".`, {
        title: "Archive wallet",
        kind: "warning",
      });
      if (!ok) return false;
      await api.archiveWallet(id, true);
      return true;
    },
    onSuccess: (done, v) => {
      if (!done) return;
      refreshWalletData();
      toast({ title: `Archived ${v.name}`, variant: "success" });
    },
    onError: (e, v) => toast({ title: `Couldn't archive ${v.name}`, description: String(e), variant: "destructive" }),
  });

  const exportLetter = useMutation({
    mutationFn: async () => {
      return api.exportInheritancePacket({ format: "letter", includeNotes: false, includeArchived: false });
    },
    onSuccess: (path) => path && toast({ title: "Letter saved", description: path, variant: "success" }),
    onError: (e) => toast({ title: "Export failed", description: String(e), variant: "destructive" }),
  });

  const backup = useMutation({
    mutationFn: api.runLocalBackup,
    onSuccess: () => toast({ title: "Backup created", variant: "success" }),
    onError: (e) => toast({ title: "Backup failed", description: String(e), variant: "destructive" }),
  });

  if (!open) return null;

  const close = () => setOpen(false);
  const run = (fn: () => void) => () => {
    close();
    fn();
  };

  return (
    <div className="fixed inset-0 z-100 flex items-start justify-center bg-black/60 pt-[15vh]" onClick={close}>
      <Command className="w-full max-w-xl overflow-hidden rounded-xl border border-border bg-popover shadow-2xl" onClick={(e) => e.stopPropagation()} loop>
        <Command.Input
          autoFocus
          placeholder="Search screens or run a command…"
          className="h-12 w-full border-b border-border bg-transparent px-4 text-sm outline-hidden placeholder:text-muted-foreground"
        />
        <Command.List className="max-h-96 overflow-y-auto p-2">
          <Command.Empty className="py-8 text-center text-sm text-muted-foreground">Nothing matches that.</Command.Empty>

          <Command.Group heading="Actions" className={groupClass}>
            <Command.Item value="add wallet new watch-only" onSelect={run(() => setAddWalletOpen(true))} className={itemClass}>
              <Plus className="h-4 w-4 text-muted-foreground" />
              Add wallet
            </Command.Item>
            <Command.Item value="export letter family inheritance" onSelect={run(() => exportLetter.mutate())} className={itemClass}>
              <FileText className="h-4 w-4 text-muted-foreground" />
              Export letter for family
            </Command.Item>
            <Command.Item value="run local backup" onSelect={run(() => backup.mutate())} className={itemClass}>
              <DatabaseBackup className="h-4 w-4 text-muted-foreground" />
              Run local backup
            </Command.Item>
            <Command.Item value="lock vault" onSelect={run(() => void lock())} className={itemClass}>
              <Lock className="h-4 w-4 text-muted-foreground" />
              Lock vault
            </Command.Item>
          </Command.Group>

          {wallets.length > 0 && (
            <Command.Group heading="Wallets" className={groupClass}>
              {wallets
                .filter((w) => w.source_type !== "manual")
                .map((w) => (
                  <Command.Item key={`sync-${w.id}`} value={`sync ${w.name}`} onSelect={run(() => syncMutation.mutate({ id: w.id, name: w.name }))} className={itemClass}>
                    <RefreshCw className="h-4 w-4 text-muted-foreground" />
                    Sync {w.name}
                  </Command.Item>
                ))}
              {wallets.map((w) => (
                <Command.Item key={`arch-${w.id}`} value={`archive ${w.name}`} onSelect={run(() => archiveMutation.mutate({ id: w.id, name: w.name }))} className={itemClass}>
                  <Archive className="h-4 w-4 text-muted-foreground" />
                  Archive {w.name}
                </Command.Item>
              ))}
            </Command.Group>
          )}

          <Command.Group heading="Go to" className={groupClass}>
            {ALL_SCREENS.map((p) => (
              <Command.Item key={p.to} value={`go ${p.label}`} onSelect={run(() => navigate(p.to))} className={itemClass}>
                <p.icon className="h-4 w-4 text-muted-foreground" />
                {p.label}
              </Command.Item>
            ))}
          </Command.Group>
        </Command.List>
      </Command>
    </div>
  );
}
