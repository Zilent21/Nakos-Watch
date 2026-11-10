import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, Plus, RefreshCw, Wallet as WalletIcon } from "lucide-react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { EmptyState } from "@/components/ui/empty-state";
import { handlePublicServerGate } from "@/components/public-server-gate";
import * as api from "@/bindings/commands";
import type { ReceiveAddress, Wallet } from "@/bindings/types";
import { toast } from "@/hooks/use-toast";
import { formatRelativeTime } from "@/lib/format";
import { parseSats } from "@/lib/receive";
import { useFormatAmount } from "@/lib/use-amount";
import { PayPanel } from "./shared";

const RECEIVE_SOURCES = ["descriptor", "xpub", "ypub", "zpub", "multisig_descriptor"];

export function canReceive(w: Wallet): boolean {
  return !w.archived && RECEIVE_SOURCES.includes(w.source_type) && !!w.descriptor;
}

function HowToCheck({ addr }: { addr: ReceiveAddress }) {
  return (
    <ul className="list-disc space-y-1 pl-5">
      <li>
        Show receive address <span className="font-semibold">#{addr.index}</span> ({addr.path}) on the device: Trezor Suite, the BitBox app and Jade
        (through Sparrow or the Blockstream app) display it on the device's own screen. In Sparrow, open the address and choose Display address.
      </li>
      <li>Compare every character, not just the start and the end.</li>
    </ul>
  );
}

export function WalletReceive() {
  const qc = useQueryClient();
  const amount = useFormatAmount();
  const { data: wallets = [] } = useQuery({ queryKey: ["wallets", false], queryFn: () => api.listWallets(false) });
  const eligible = useMemo(() => wallets.filter(canReceive), [wallets]);
  const [walletId, setWalletId] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [label, setLabel] = useState("");
  const [amountText, setAmountText] = useState("");

  useEffect(() => {
    if (!walletId && eligible.length) setWalletId(eligible[0].id);
  }, [eligible, walletId]);

  const { data: ov, isLoading } = useQuery({
    queryKey: ["receive", walletId],
    queryFn: () => api.receiveOverview(walletId!),
    enabled: !!walletId,
  });

  const refresh = () => {
    qc.invalidateQueries({ queryKey: ["receive", walletId] });
    qc.invalidateQueries({ queryKey: ["alerts"] });
  };

  const sync = useMutation({
    mutationFn: () => api.syncWallet(walletId!),
    onSuccess: () => {
      refresh();
      qc.invalidateQueries({ queryKey: ["wallet-balances"] });
      qc.invalidateQueries({ queryKey: ["portfolio-summary"] });
      qc.invalidateQueries({ queryKey: ["wallets"] });
    },
    onError: (e) => {
      if (handlePublicServerGate(e, () => sync.mutate())) return;
      toast({ title: "Sync failed", description: String(e), variant: "destructive" });
    },
  });

  const amountSats = amountText.trim() ? parseSats(amountText) : null;
  const amountBad = amountText.trim() !== "" && amountSats === null;

  const create = useMutation({
    mutationFn: () => api.newReceiveAddress(walletId!, label.trim() || null, amountSats),
    onSuccess: (a) => {
      setSelectedId(a.id);
      setLabel("");
      setAmountText("");
      refresh();
    },
    onError: (e) => toast({ title: "No new address", description: String(e), variant: "destructive" }),
  });

  const check = useMutation({
    mutationFn: ({ id, checked }: { id: string; checked: boolean }) => api.setReceiveChecked(id, checked),
    onSuccess: refresh,
    onError: (e) => toast({ title: "Couldn't save", description: String(e), variant: "destructive" }),
  });

  if (!eligible.length) {
    return (
      <Card>
        <EmptyState
          icon={WalletIcon}
          title="No wallet to derive addresses from"
          helper="Receive needs a wallet added from a descriptor or an xpub. For addresses copied from another wallet app, use Saved addresses."
        />
      </Card>
    );
  }

  const selected = ov?.addresses.find((a) => a.id === selectedId) ?? ov?.addresses[0] ?? null;

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
      <div className="space-y-6">
        <Card>
          <CardHeader>
            <CardTitle>New address</CardTitle>
            <CardDescription>The next fresh address of the wallet, worked out from its public keys. Your hardware wallet can stay in the drawer.</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="space-y-2">
              <Label>Wallet</Label>
              <Select value={walletId ?? undefined} onValueChange={(v) => { setWalletId(v); setSelectedId(null); }}>
                <SelectTrigger><SelectValue placeholder="Choose a wallet" /></SelectTrigger>
                <SelectContent>
                  {eligible.map((w) => (
                    <SelectItem key={w.id} value={w.id}>{w.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="space-y-2">
                <Label htmlFor="rcv-label">Label (optional)</Label>
                <Input id="rcv-label" value={label} maxLength={100} placeholder="from Kraken, November" onChange={(e) => setLabel(e.target.value)} />
              </div>
              <div className="space-y-2">
                <Label htmlFor="rcv-amount">Amount in sats (optional)</Label>
                <Input id="rcv-amount" inputMode="numeric" value={amountText} placeholder="250,000" aria-invalid={amountBad} onChange={(e) => setAmountText(e.target.value)} />
                {amountBad && <p className="text-xs text-destructive">A whole number of sats, like 250,000.</p>}
              </div>
            </div>
            {ov && !ov.synced ? (
              <div className="space-y-2 rounded-md border border-border bg-secondary/40 p-3 text-sm">
                <p>Sync this wallet once first, so Nakos knows which addresses already have coins and never hands one out twice.</p>
                <Button size="sm" onClick={() => sync.mutate()} disabled={sync.isPending}>
                  {sync.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}
                  Sync now
                </Button>
              </div>
            ) : (
              <div className="flex flex-wrap items-center gap-2">
                <Button onClick={() => create.mutate()} disabled={!ov?.can_receive || amountBad || create.isPending || (ov.fixed && ov.addresses.length > 0)}>
                  {create.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Plus className="h-4 w-4" />}
                  {ov?.fixed ? "Show its address" : "New address"}
                </Button>
                <Button variant="outline" onClick={() => sync.mutate()} disabled={sync.isPending}>
                  {sync.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}
                  Sync
                </Button>
              </div>
            )}
            {ov && !ov.can_receive && ov.reason && <p className="text-sm text-muted-foreground">{ov.reason}</p>}
            {ov?.fixed && (
              <p className="rounded-md border border-border bg-secondary/40 p-3 text-sm text-muted-foreground">
                This descriptor has no /* range, so it is one fixed address and Nakos can't make new ones from it.
                {ov.addresses.length > 0 ? " It's shown here." : ""} Paying the same address more than once links those payments together. For a new address
                each time, add the wallet's xpub or a descriptor ending in /*.
              </p>
            )}
            {ov?.can_receive && !ov.fixed && (
              <p className="text-xs text-muted-foreground">
                {ov.unused_given_out} of {ov.gap_limit} unused addresses handed out. Nakos only knows the addresses it handed out and the ones used on chain:
                if you showed one in another wallet app and it hasn't been used yet, Nakos may show it again.
              </p>
            )}
          </CardContent>
        </Card>

        {ov && ov.addresses.length > 0 && (
          <Card>
            <CardHeader>
              <CardTitle>Handed out</CardTitle>
              <CardDescription>Newest first. Sync to see which ones received coins.</CardDescription>
            </CardHeader>
            <CardContent className="divide-y divide-border p-0">
              {ov.addresses.map((a) => (
                <button
                  key={a.id}
                  type="button"
                  onClick={() => setSelectedId(a.id)}
                  className={`flex w-full items-center justify-between gap-3 px-6 py-3 text-left text-sm hover:bg-accent/50 ${selected?.id === a.id ? "bg-accent/40" : ""}`}
                >
                  <span className="min-w-0">
                    <span className="mono-nums font-medium">#{a.index}</span>{" "}
                    <span className="font-mono text-muted-foreground">{a.address.slice(0, 10)}…{a.address.slice(-6)}</span>
                    {a.label && <span className="ml-2 text-muted-foreground">{a.label}</span>}
                  </span>
                  <span className="flex shrink-0 items-center gap-2">
                    {a.used ? <Badge variant="success">{a.received_sats > 0 ? amount(a.received_sats) : "Used"}</Badge> : <Badge variant="muted">Waiting</Badge>}
                    {a.device_checked_at ? <Badge variant="outline">Checked</Badge> : null}
                  </span>
                </button>
              ))}
            </CardContent>
          </Card>
        )}
      </div>

      <Card className="h-fit">
        <CardHeader>
          <CardTitle>{selected ? `Address #${selected.index}` : "Address"}</CardTitle>
          <CardDescription>
            {selected ? `${ov?.wallet_name} · handed out ${formatRelativeTime(selected.created_at)}` : "Make a new address to see its QR code here."}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {isLoading && <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />}
          {selected && (
            <>
              {selected.used && (
                <p className="rounded-md border border-success/40 bg-success/10 p-3 text-sm">
                  This address has received {selected.received_sats > 0 ? amount(selected.received_sats) : "coins"}. Use a new one for the next payment.
                </p>
              )}
              <PayPanel
                address={selected.address}
                amountSats={selected.amount_sats}
                label={selected.label}
                checkedAt={selected.device_checked_at}
                where="on device"
                howToCheck={<HowToCheck addr={selected} />}
                onChecked={(checked) => check.mutate({ id: selected.id, checked })}
              />
              <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 border-t border-border pt-3 text-xs text-muted-foreground">
                <dt>Path</dt>
                <dd className="font-mono">{selected.path}</dd>
                <dt>Fingerprint{ov && ov.fingerprints.length > 1 ? "s" : ""}</dt>
                <dd className="font-mono">{ov?.fingerprints.join(", ") || "unknown"}</dd>
                <dt>Descriptor checksum</dt>
                <dd className="font-mono">{ov?.checksum ?? "unknown"}</dd>
              </dl>
            </>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
