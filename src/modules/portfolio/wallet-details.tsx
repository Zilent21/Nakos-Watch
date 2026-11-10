import type { ReactNode } from "react";
import { Link } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { ArrowRight } from "lucide-react";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import * as api from "@/bindings/commands";
import type { SourceType, Wallet } from "@/bindings/types";
import { formatRelativeTime } from "@/lib/format";
import { useFormatAmount } from "@/lib/use-amount";
import { CopyButton } from "@/modules/receive/shared";

const SOURCE_LABEL: Record<SourceType, string> = {
  descriptor: "Descriptor",
  xpub: "xpub",
  ypub: "ypub",
  zpub: "zpub",
  address: "Single address",
  multisig_descriptor: "Multisig",
  manual: "Tracked by hand",
};

/** A descriptor with no `/*` range is one fixed address. */
const isRanged = (d: string) => d.includes("/*");

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </>
  );
}

/** A long public string (descriptor, xpub, address): wrapped, monospaced, with a copy button. */
function Key({ text, what }: { text: string; what: string }) {
  return (
    <div className="flex items-start gap-2">
      <code className="min-w-0 flex-1 break-all rounded bg-secondary/50 px-2 py-1 font-mono text-xs">{text}</code>
      <CopyButton text={text} what={what} label="Copy" />
    </div>
  );
}

function when(iso: string | null): string {
  return iso ? `${formatRelativeTime(iso)} · ${new Date(iso).toLocaleString()}` : "never";
}

/** Everything Nakos holds for one wallet. All of it is public: Nakos never stores a seed or a private key. */
export function WalletDetailsDialog({ wallet, balance, onOpenChange }: { wallet: Wallet | null; balance?: number; onOpenChange: (open: boolean) => void }) {
  const amount = useFormatAmount();
  const multisig = wallet?.source_type === "multisig_descriptor";
  const { data: ms } = useQuery({
    queryKey: ["multisig-config", wallet?.id],
    queryFn: () => api.getMultisigConfig(wallet!.id),
    enabled: !!wallet && multisig,
  });
  if (!wallet) return null;
  const w = wallet;
  const fromKeys = !!w.descriptor && w.source_type !== "address" && w.source_type !== "manual";

  return (
    <Dialog open onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex flex-wrap items-center gap-2">
            {w.name}
            {w.archived && <Badge variant="muted">archived</Badge>}
          </DialogTitle>
          <DialogDescription>What Nakos keeps for this wallet. All of it is public: it can watch the coins but never spend them.</DialogDescription>
        </DialogHeader>

        <dl className="grid grid-cols-[minmax(0,9rem)_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">
          <Row label="Balance">{balance !== undefined ? <span className="mono-nums">{amount(balance)}</span> : "…"}</Row>
          <Row label="Custody">{w.custody_type.replace("_", " ")}</Row>
          <Row label="Risk">
            <Badge variant={w.risk_level === "high" ? "destructive" : w.risk_level === "medium" ? "warning" : "success"}>{w.risk_level}</Badge>
          </Row>
          <Row label="Added from">
            {SOURCE_LABEL[w.source_type] ?? w.source_type}
            {fromKeys && w.descriptor && !isRanged(w.descriptor) && <span className="text-muted-foreground"> (one fixed address: no /* range)</span>}
          </Row>
          <Row label="Network">{w.network}</Row>
          {fromKeys && <Row label="Gap limit">{w.gap_limit} unused addresses</Row>}
          <Row label="Last synced">{w.source_type === "manual" ? "not synced (tracked by hand)" : when(w.last_synced_at)}</Row>
          <Row label="Added">{when(w.created_at)}</Row>
          {w.master_fingerprint && <Row label="Fingerprint"><code className="font-mono">{w.master_fingerprint}</code></Row>}
          {w.derivation_path && <Row label="Path"><code className="font-mono">{w.derivation_path}</code></Row>}
          {w.source_type === "manual" && w.manual_balance_sats !== null && <Row label="Entered balance">{amount(w.manual_balance_sats)}</Row>}
        </dl>

        {multisig && ms && (
          <div className="space-y-2">
            <p className="text-sm font-medium">
              {ms.threshold_m} of {ms.total_n} multisig · {ms.script_type}
            </p>
            <ul className="space-y-3">
              {ms.cosigners.map((c) => (
                <li key={c.id} className="space-y-1 rounded-md border border-border p-3 text-sm">
                  <p className="font-medium">
                    {c.label}
                    {c.hardware_model && <span className="font-normal text-muted-foreground"> · {c.hardware_model}</span>}
                    {c.is_mine && <span className="font-normal text-muted-foreground"> · mine</span>}
                  </p>
                  {(c.fingerprint || c.derivation_path) && (
                    <p className="font-mono text-xs text-muted-foreground">
                      {[c.fingerprint, c.derivation_path].filter(Boolean).join(" · ")}
                    </p>
                  )}
                  <Key text={c.xpub} what="Cosigner xpub" />
                </li>
              ))}
            </ul>
          </div>
        )}

        {w.xpub && (
          <div className="space-y-1">
            <p className="text-sm font-medium">Extended public key</p>
            <Key text={w.xpub} what="xpub" />
          </div>
        )}
        {w.descriptor && (
          <div className="space-y-1">
            <p className="text-sm font-medium">{w.source_type === "address" ? "Address" : "Receive descriptor"}</p>
            <Key text={w.descriptor} what={w.source_type === "address" ? "Address" : "Descriptor"} />
          </div>
        )}
        {w.change_descriptor && (
          <div className="space-y-1">
            <p className="text-sm font-medium">Change descriptor</p>
            <Key text={w.change_descriptor} what="Change descriptor" />
          </div>
        )}
        {w.notes && (
          <div className="space-y-1">
            <p className="text-sm font-medium">Notes</p>
            <p className="whitespace-pre-wrap text-sm text-muted-foreground">{w.notes}</p>
          </div>
        )}

        <div className="flex flex-wrap gap-2 border-t border-border pt-4">
          {fromKeys && !w.archived && (
            <Button variant="outline" size="sm" asChild>
              <Link to="/receive">
                Receive <ArrowRight className="h-4 w-4" />
              </Link>
            </Button>
          )}
          {w.source_type !== "manual" && (
            <Button variant="outline" size="sm" asChild>
              <Link to="/utxos">
                Coins <ArrowRight className="h-4 w-4" />
              </Link>
            </Button>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
