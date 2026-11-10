import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Plus, X } from "lucide-react";
import * as api from "@/bindings/commands";
import { toast } from "@/hooks/use-toast";
import type { Cosigner, CustodyType, RiskLevel, SourceType } from "@/bindings/types";
import { SECRET_WARNING, secretKind } from "@/lib/key-guard";
import { ImportGuideDialog } from "@/modules/onboarding/import-guide";

const CUSTODY_OPTIONS: { value: CustodyType; label: string }[] = [
  { value: "cold_storage", label: "Cold storage" },
  { value: "multisig_vault", label: "Multisig vault" },
  { value: "hot_wallet", label: "Hot wallet" },
  { value: "lightning", label: "Lightning" },
  { value: "mining", label: "Mining" },
  { value: "exchange", label: "Exchange (watch-only)" },
  { value: "etf", label: "ETF (manual)" },
  { value: "retirement", label: "Retirement account (manual)" },
  { value: "other", label: "Other" },
];

const RISK_OPTIONS: { value: RiskLevel; label: string }[] = [
  { value: "low", label: "Low risk" },
  { value: "medium", label: "Medium risk" },
  { value: "high", label: "High risk" },
];

export function AddWalletDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const [custodyType, setCustodyType] = useState<CustodyType>("cold_storage");
  const [riskLevel, setRiskLevel] = useState<RiskLevel>("medium");
  const [network] = useState("bitcoin");
  const [source, setSource] = useState("");
  const [detected, setDetected] = useState<string>("unknown");
  const [manualBalance, setManualBalance] = useState("");
  const [error, setError] = useState<string | null>(null);

  const isManual = custodyType === "exchange" || custodyType === "etf" || custodyType === "retirement";
  const isMultisig = custodyType === "multisig_vault";

  const [thresholdM, setThresholdM] = useState(2);
  const [cosigners, setCosigners] = useState<
    { label: string; xpub: string; fingerprint: string; derivationPath: string }[]
  >([
    { label: "Cosigner 1", xpub: "", fingerprint: "", derivationPath: "" },
    { label: "Cosigner 2", xpub: "", fingerprint: "", derivationPath: "" },
    { label: "Cosigner 3", xpub: "", fingerprint: "", derivationPath: "" },
  ]);

  // Optional for a bare xpub/ypub/zpub wallet — without these, Nakos can
  // still track balances, but will refuse to build a spend PSBT later
  // (it won't guess a fingerprint or derivation path).
  const [masterFingerprint, setMasterFingerprint] = useState("");
  const [derivationPath, setDerivationPath] = useState("");

  const secret = secretKind(source);
  const [guideOpen, setGuideOpen] = useState(false);

  async function onSourceChange(value: string) {
    setSource(value);
    // A pasted secret is not even sent to the detector; the box says why and Save stays off.
    if (secretKind(value)) {
      setDetected("unknown");
      return;
    }
    if (value.trim().length > 3) {
      const kind = await api.detectSourceType(value);
      setDetected(kind);
    } else {
      setDetected("unknown");
    }
  }

  const mutation = useMutation({
    mutationFn: async () => {
      if (isMultisig) {
        const cosignerInputs: Cosigner[] = cosigners
          .filter((c) => c.xpub.trim())
          .map((c) => ({
            id: "",
            label: c.label,
            xpub: c.xpub.trim(),
            fingerprint: c.fingerprint.trim() || null,
            derivation_path: c.derivationPath.trim() || null,
            is_mine: false,
            hardware_model: null,
          }));
        return api.addMultisigWallet({
          name,
          custody_type: custodyType,
          risk_level: riskLevel,
          network,
          threshold_m: thresholdM,
          cosigners: cosignerInputs,
          script_type: "p2wsh",
        });
      }
      const sourceType: SourceType = isManual ? "manual" : (detected as SourceType);
      return api.addWallet({
        name,
        custody_type: custodyType,
        risk_level: riskLevel,
        source_type: sourceType,
        network,
        descriptor: !isManual && (sourceType === "descriptor" || sourceType === "address") ? source : undefined,
        xpub: !isManual && ["xpub", "ypub", "zpub"].includes(sourceType) ? source : undefined,
        master_fingerprint:
          !isManual && ["xpub", "ypub", "zpub"].includes(sourceType) ? masterFingerprint.trim() || undefined : undefined,
        derivation_path:
          !isManual && ["xpub", "ypub", "zpub"].includes(sourceType) ? derivationPath.trim() || undefined : undefined,
        manual_balance_sats: isManual && manualBalance ? Math.round(Number(manualBalance) * 100_000_000) : undefined,
      });
    },
    onSuccess: (wallet) => {
      queryClient.invalidateQueries({ queryKey: ["wallets"] });
      queryClient.invalidateQueries({ queryKey: ["portfolio-summary"] });
      onOpenChange(false);
      setName("");
      setSource("");
      setManualBalance("");
      setMasterFingerprint("");
      setDerivationPath("");
      toast({ title: "Wallet saved", description: wallet.name, variant: "success" });
    },
    onError: (e) => {
      setError(String(e));
      toast({ title: "Could not save wallet", description: String(e), variant: "destructive" });
    },
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add a wallet</DialogTitle>
          <DialogDescription>
            Watch-only. Nakos never accepts private keys or seed phrases — only descriptors, xpubs, or addresses.
          </DialogDescription>
        </DialogHeader>


        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label>Name</Label>
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Trezor Vault" />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>Custody type</Label>
              <Select value={custodyType} onValueChange={(v) => setCustodyType(v as CustodyType)}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {CUSTODY_OPTIONS.map((o) => (
                    <SelectItem key={o.value} value={o.value}>
                      {o.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label>Risk level</Label>
              <Select value={riskLevel} onValueChange={(v) => setRiskLevel(v as RiskLevel)}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {RISK_OPTIONS.map((o) => (
                    <SelectItem key={o.value} value={o.value}>
                      {o.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          {isMultisig ? (
            <div className="space-y-3">
              <div className="space-y-1.5">
                <Label>Threshold (M-of-N)</Label>
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    min={1}
                    max={cosigners.length}
                    className="w-20"
                    value={thresholdM}
                    onChange={(e) => setThresholdM(Number(e.target.value))}
                  />
                  <span className="text-sm text-muted-foreground">of {cosigners.length} cosigners</span>
                </div>
              </div>
              <div className="space-y-2">
                <Label>Cosigners</Label>
                <p className="text-[11px] text-muted-foreground">
                  Fingerprint + derivation path are required for every cosigner — without them Nakos can't build a
                  PSBT any signer will accept, so it won't save this vault.
                </p>
                {cosigners.map((c, i) => (
                  <div key={i} className="space-y-1 rounded-md border border-border p-2">
                    <div className="flex gap-2">
                      <Input
                        value={c.label}
                        onChange={(e) => {
                          const next = [...cosigners];
                          next[i] = { ...next[i], label: e.target.value };
                          setCosigners(next);
                        }}
                        className="w-32 text-xs"
                      />
                      <Input
                        value={c.xpub}
                        onChange={(e) => {
                          const next = [...cosigners];
                          next[i] = { ...next[i], xpub: e.target.value };
                          setCosigners(next);
                        }}
                        placeholder="xpub / Zpub..."
                        className="flex-1 font-mono text-xs"
                      />
                      <Button variant="ghost" size="icon" onClick={() => setCosigners(cosigners.filter((_, idx) => idx !== i))}>
                        <X className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                    <div className="flex gap-2 pl-0">
                      <Input
                        value={c.fingerprint}
                        onChange={(e) => {
                          const next = [...cosigners];
                          next[i] = { ...next[i], fingerprint: e.target.value };
                          setCosigners(next);
                        }}
                        placeholder="Fingerprint (e.g. 73c5da0a)"
                        className="w-40 font-mono text-xs"
                      />
                      <Input
                        value={c.derivationPath}
                        onChange={(e) => {
                          const next = [...cosigners];
                          next[i] = { ...next[i], derivationPath: e.target.value };
                          setCosigners(next);
                        }}
                        placeholder="Derivation path (e.g. 48'/0'/0'/2')"
                        className="flex-1 font-mono text-xs"
                      />
                    </div>
                  </div>
                ))}
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    setCosigners([
                      ...cosigners,
                      { label: `Cosigner ${cosigners.length + 1}`, xpub: "", fingerprint: "", derivationPath: "" },
                    ])
                  }
                >
                  <Plus className="h-3.5 w-3.5" /> Add cosigner
                </Button>
              </div>
            </div>
          ) : isManual ? (
            <div className="space-y-1.5">
              <Label>Balance (BTC)</Label>
              <Input
                type="number"
                step="0.00000001"
                value={manualBalance}
                onChange={(e) => setManualBalance(e.target.value)}
                placeholder="0.00000000"
              />
            </div>
          ) : (
            <div className="space-y-3">
              <div className="space-y-1.5">
                <Label>Descriptor, xpub/ypub/zpub, or address</Label>
                <Input
                  value={source}
                  onChange={(e) => onSourceChange(e.target.value)}
                  placeholder="zpub6... / bc1q... / wpkh([fp/84'/0'/0']xpub.../0/*)"
                  className="font-mono text-xs"
                />
                {secret ? (
                  <p role="alert" data-testid="secret-warning" className="rounded-md border border-destructive/60 bg-destructive/10 p-2 text-xs text-destructive">
                    {SECRET_WARNING[secret]}
                  </p>
                ) : (
                  source && (
                    <p className="text-xs text-muted-foreground">
                      Detected: <span className="text-foreground">{detected}</span>
                    </p>
                  )
                )}
                <Button type="button" variant="link" className="h-auto p-0 text-xs" onClick={() => setGuideOpen(true)}>
                  Where do I find this?
                </Button>
                <ImportGuideDialog open={guideOpen} onOpenChange={setGuideOpen} />
              </div>
              {["xpub", "ypub", "zpub"].includes(detected) && (
                <div className="space-y-1.5">
                  <Label>Master fingerprint &amp; derivation path (optional)</Label>
                  <div className="flex gap-2">
                    <Input
                      value={masterFingerprint}
                      onChange={(e) => setMasterFingerprint(e.target.value)}
                      placeholder="73c5da0a"
                      className="w-40 font-mono text-xs"
                    />
                    <Input
                      value={derivationPath}
                      onChange={(e) => setDerivationPath(e.target.value)}
                      placeholder="84'/0'/0'"
                      className="flex-1 font-mono text-xs"
                    />
                  </div>
                  <p className="text-[11px] text-muted-foreground">
                    Needed later to build a spendable PSBT — Nakos never guesses these. Leave blank if you only want
                    to track balances.
                  </p>
                </div>
              )}
            </div>
          )}

          {error && <p className="text-sm text-destructive">{error}</p>}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            onClick={() => {
              setError(null);
              mutation.mutate();
            }}
            disabled={
              !!secret ||
              !name ||
              (isMultisig
                ? (() => {
                    const filled = cosigners.filter((c) => c.xpub.trim());
                    return (
                      filled.length < 2 || filled.some((c) => !c.fingerprint.trim() || !c.derivationPath.trim())
                    );
                  })()
                : !isManual && !source) ||
              mutation.isPending
            }
          >
            {mutation.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
            Add wallet
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
