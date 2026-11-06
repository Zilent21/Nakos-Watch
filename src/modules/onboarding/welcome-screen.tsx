import { useState } from "react";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { PasswordInput } from "@/components/ui/password-input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { useVaultStore } from "@/stores/vault-store";
import { toast } from "@/hooks/use-toast";
import { AuthFrame } from "./auth-frame";
import { RestoreBackupDialog } from "@/components/restore-backup-dialog";
import { useDeviceWord, useIsMobile, useOs, useStaysUnlockedNote } from "@/lib/platform";
import { keychainNote } from "@/lib/os-text";

export function WelcomeScreen() {
  const init = useVaultStore((s) => s.init);
  const keychainSupported = useVaultStore((s) => s.keychainSupported);
  const device = useDeviceWord("machine");
  const mobile = useIsMobile();
  const os = useOs();
  const staysUnlocked = useStaysUnlockedNote();
  const [passphrase, setPassphrase] = useState("");
  const [confirm, setConfirm] = useState("");
  const [remember, setRemember] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [restoreOpen, setRestoreOpen] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    if (passphrase.length < 8) {
      setError("Use at least 8 characters.");
      return;
    }
    if (passphrase !== confirm) {
      setError("The two passphrases don't match.");
      return;
    }
    setLoading(true);
    try {
      await init(passphrase, remember && keychainSupported);
      toast({ title: "Vault created", variant: "success" });
    } catch (err) {
      setError(String(err));
      toast({ title: "Could not create vault", description: String(err), variant: "destructive" });
    } finally {
      setLoading(false);
    }
  }

  return (
    <AuthFrame
      title="Create your vault"
      subtitle={`Your passphrase stays on this ${device}. Bitcoin Core, electrs, and an explorer you turn on send traffic off it.`}
      footer={
        <>
          <p>Nakos is watch-only. It never asks for a seed phrase or a private key.</p>
          <p>If you forget this passphrase you lose your encrypted notes. Your bitcoin is never affected.</p>
          <p data-testid="stays-unlocked">{staysUnlocked}</p>
        </>
      }
    >
      <form onSubmit={handleSubmit} className="space-y-4">
        <div className="space-y-2">
          <Label htmlFor="passphrase">Passphrase</Label>
          <PasswordInput id="passphrase" autoFocus autoComplete="new-password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} placeholder="At least 8 characters" />
        </div>
        <div className="space-y-2">
          <Label htmlFor="confirm">Confirm passphrase</Label>
          <PasswordInput id="confirm" autoComplete="new-password" value={confirm} onChange={(e) => setConfirm(e.target.value)} />
        </div>

        {mobile ? (
          <p className="text-xs text-muted-foreground">Nakos asks for this passphrase every time it opens. It is not saved on the phone.</p>
        ) : (
          <label className="flex items-start gap-3 text-xs text-muted-foreground">
            <Checkbox className="mt-0.5" checked={remember && keychainSupported} disabled={!keychainSupported} onCheckedChange={(v) => setRemember(v === true)} />
            <span>
              Remember on this computer. Saves the passphrase in your OS keychain. {keychainNote(os)}
              {!keychainSupported && " Unavailable in development builds."}
            </span>
          </label>
        )}

        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}

        <Button type="submit" className="w-full" disabled={loading}>
          {loading && <Loader2 className="h-4 w-4 animate-spin" />}
          Create vault
        </Button>
      </form>
      <div className="mt-4 flex flex-col items-center gap-2 border-t border-border pt-4 text-center">
        <Button type="button" variant="link" className="h-auto p-0 text-xs" onClick={() => setRestoreOpen(true)}>
          Restore from an encrypted backup
        </Button>
        <p className="text-xs text-muted-foreground">Nakos is free and open source. No account, no license key.</p>
      </div>
      <RestoreBackupDialog open={restoreOpen} onOpenChange={setRestoreOpen} hasVault={false} />
    </AuthFrame>
  );
}
