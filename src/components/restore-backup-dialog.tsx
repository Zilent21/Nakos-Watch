import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { PasswordInput } from "@/components/ui/password-input";
import { Label } from "@/components/ui/label";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from "@/components/ui/dialog";
import * as api from "@/bindings/commands";
import { useVaultStore } from "@/stores/vault-store";
import { toast } from "@/hooks/use-toast";

/**
 * Restore an encrypted vault backup (not a family letter). Works on first run
 * and while unlocked. It replaces the current vault, and the backup's
 * passphrase becomes the vault passphrase.
 */
export function RestoreBackupDialog({ open: isOpen, onOpenChange, hasVault }: { open: boolean; onOpenChange: (v: boolean) => void; hasVault: boolean }) {
  const queryClient = useQueryClient();
  const refresh = useVaultStore((s) => s.refresh);
  const [path, setPath] = useState<string | null>(null);
  const [passphrase, setPassphrase] = useState("");
  const [error, setError] = useState<string | null>(null);

  const choose = async () => {
    // Rust opens the dialog and remembers the file; the restore below uses only that.
    try {
      const picked = await api.pickBackupToRestore();
      if (picked) {
        setPath(picked);
        setError(null);
      }
    } catch (e) {
      setError(String(e));
    }
  };

  const restore = useMutation({
    mutationFn: () => api.restoreEncryptedBackup(passphrase),
    onSuccess: async (r) => {
      setPassphrase("");
      setPath(null);
      onOpenChange(false);
      queryClient.invalidateQueries();
      await refresh();
      toast({ title: "Backup restored", description: `${r.wallets} wallet${r.wallets === 1 ? "" : "s"} in this vault. The backup's passphrase is now your vault passphrase.`, variant: "success" });
    },
    onError: (e) => setError(String(e)),
  });

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(v) => {
        onOpenChange(v);
        if (!v) {
          setError(null);
          setPassphrase("");
        }
      }}
    >
      <DialogContent className="max-w-xl">
        <DialogHeader>
          <DialogTitle>Restore an encrypted backup</DialogTitle>
          <DialogDescription>
            {hasVault ? "This replaces everything in this vault with the backup. A copy of the current database is kept next to it." : "This loads your vault from a backup made with Settings, Encrypted backup."}{" "}
            The passphrase you enter is the one the backup was made with, and it becomes the vault passphrase. A family letter is not a backup and will be refused.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label>Backup file</Label>
            <div className="flex items-center gap-3">
              <Button variant="outline" onClick={choose}>
                Choose file…
              </Button>
              {/* A desktop path can be long; it wraps rather than widening the dialog. */}
              <span className="min-w-0 break-all text-xs text-muted-foreground">{path ?? "No file chosen"}</span>
            </div>
          </div>
          <div className="space-y-2">
            <Label htmlFor="restore-pass">Vault passphrase this backup was made with</Label>
            <p className="text-xs font-semibold text-foreground">The passphrase you used to unlock Nakos when you exported it. It is not stored in the file.</p>
            <PasswordInput id="restore-pass" autoComplete="off" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} />
          </div>
          {error && (
            <p role="alert" className="text-sm text-destructive">
              {error}
            </p>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant={hasVault ? "destructive" : "default"} disabled={!path || !passphrase || restore.isPending} onClick={() => restore.mutate()}>
            {restore.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
            {hasVault ? "Replace this vault" : "Restore"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
