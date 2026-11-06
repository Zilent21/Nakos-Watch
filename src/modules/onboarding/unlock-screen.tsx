import { useState } from "react";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { PasswordInput } from "@/components/ui/password-input";
import { useStaysUnlockedNote } from "@/lib/platform";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { useVaultStore } from "@/stores/vault-store";
import { AuthFrame } from "./auth-frame";

export function UnlockScreen() {
  const staysUnlocked = useStaysUnlockedNote();
  const unlock = useVaultStore((s) => s.unlock);
  const keychainSupported = useVaultStore((s) => s.keychainSupported);
  const [passphrase, setPassphrase] = useState("");
  const [remember, setRemember] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      await unlock(passphrase, remember && keychainSupported);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  return (
    <AuthFrame
      title="Unlock Nakos"
      subtitle="Enter your vault passphrase."
      footer={
        <>
          <p>Nakos is watch-only. It never asks for a seed phrase or a private key.</p>
          <p data-testid="stays-unlocked">{staysUnlocked}</p>
        </>
      }
    >
      <form onSubmit={handleSubmit} className="space-y-4">
        <div className="space-y-2">
          <Label htmlFor="passphrase">Passphrase</Label>
          <PasswordInput id="passphrase" autoFocus autoComplete="current-password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} />
        </div>

        {keychainSupported && (
          <label className="flex items-center gap-3 text-xs text-muted-foreground">
            <Checkbox checked={remember} onCheckedChange={(v) => setRemember(v === true)} />
            Remember on this computer
          </label>
        )}

        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}

        <Button type="submit" className="w-full" disabled={loading || passphrase.length === 0}>
          {loading && <Loader2 className="h-4 w-4 animate-spin" />}
          Unlock
        </Button>
      </form>
    </AuthFrame>
  );
}
