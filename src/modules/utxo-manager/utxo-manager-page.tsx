import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BarChart, Bar, XAxis, YAxis, ResponsiveContainer, Tooltip as RTooltip, CartesianGrid } from "recharts";
import { Snowflake, Sparkles, Layers } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from "@/components/ui/dialog";
import * as api from "@/bindings/commands";
import { formatTxid } from "@/lib/format";
import { useFormatAmount } from "@/lib/use-amount";
import { toast } from "@/hooks/use-toast";
import { usePreferencesStore } from "@/stores/preferences-store";
import type { Utxo } from "@/bindings/types";
import { useDeviceWord } from "@/lib/platform";

const BUCKET_LABEL: Record<string, string> = {
  dust: "Dust (<10k sats)",
  small: "Small (<100k)",
  medium: "Medium (<1M)",
  large: "Large (<10M)",
  whale: "Whale (10M+)",
};

export function UtxoManagerPage() {
  const amount = useFormatAmount();
  const device = useDeviceWord("machine");
  const queryClient = useQueryClient();
  const [walletFilter, setWalletFilter] = useState<string>("all");
  const [showFrozen, setShowFrozen] = useState<"all" | "frozen" | "unfrozen">("all");
  const [showArchived, setShowArchived] = useState(false);
  const [feeRate, setFeeRate] = useState(15);
  const [consolidationResult, setConsolidationResult] = useState<string | null>(null);

  // Cost-basis lot editor: a lot is a cost and the date acquired, together.
  const fiatCurrency = usePreferencesStore((s) => s.fiatCurrency);
  const [basisFor, setBasisFor] = useState<Utxo | null>(null);
  const [basisCost, setBasisCost] = useState("");
  const [basisDate, setBasisDate] = useState("");
  const [basisNote, setBasisNote] = useState("");
  const { data: readiness } = useQuery({ queryKey: ["tax-readiness"], queryFn: api.getTaxReadiness });

  function openBasis(u: Utxo) {
    setBasisFor(u);
    setBasisCost(u.acquisition_cost_fiat !== null ? String(u.acquisition_cost_fiat) : "");
    // Suggest the confirmation date for a new lot; the user confirms it.
    setBasisDate(u.acquisition_date ?? (u.block_time ? u.block_time.slice(0, 10) : ""));
    setBasisNote(u.acquisition_note ?? "");
  }

  const { data: wallets = [] } = useQuery({
    queryKey: ["wallets", showArchived],
    queryFn: () => api.listWallets(showArchived),
  });
  const { data: utxos = [] } = useQuery({
    queryKey: ["utxos", walletFilter, showArchived],
    queryFn: () =>
      api.listUtxos({
        wallet_id: walletFilter === "all" ? undefined : walletFilter,
        include_archived: showArchived,
      }),
  });
  const { data: distribution } = useQuery({
    queryKey: ["utxo-distribution", walletFilter, showArchived],
    queryFn: () => api.utxoDistribution(walletFilter === "all" ? undefined : walletFilter, showArchived),
  });

  const freezeMutation = useMutation({
    mutationFn: ({ id, frozen }: { id: string; frozen: boolean }) => api.freezeUtxo(id, frozen),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["utxos"] }),
  });

  const basisMutation = useMutation({
    mutationFn: (clear: boolean) => {
      if (!basisFor) throw new Error("no coin selected");
      if (clear) return api.setUtxoBasis(basisFor.id, null, null, basisNote.trim() || null);
      const cost = basisCost.trim() === "" ? null : Number(basisCost);
      if (cost !== null && !Number.isFinite(cost)) throw new Error("cost basis must be a number");
      return api.setUtxoBasis(basisFor.id, cost, basisDate.trim() || null, basisNote.trim() || null);
    },
    onSuccess: (_d, clear) => {
      queryClient.invalidateQueries({ queryKey: ["utxos"] });
      queryClient.invalidateQueries({ queryKey: ["tax-readiness"] });
      queryClient.invalidateQueries({ queryKey: ["health-score"] });
      toast({ title: clear ? "Cost basis cleared" : "Cost basis saved", variant: "success" });
      setBasisFor(null);
    },
    onError: (e) => toast({ title: "Couldn't save cost basis", description: String(e), variant: "destructive" }),
  });

  const consolidateMutation = useMutation({
    mutationFn: () => api.planConsolidation(walletFilter, feeRate),
    onSuccess: (plan) => setConsolidationResult(plan.psbt_base64),
    onError: (e) => setConsolidationResult(`Error: ${e}`),
  });

  const filtered = useMemo(
    () => utxos.filter((u) => (showFrozen === "all" ? true : showFrozen === "frozen" ? u.is_frozen : !u.is_frozen)),
    [utxos, showFrozen],
  );

  const chartData = (distribution?.by_size ?? []).map((b) => ({ name: BUCKET_LABEL[b.bucket] ?? b.bucket, sats: b.total_sats, count: b.count }));

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Coins</h1>
          <p className="text-sm text-muted-foreground">Filter, freeze, label and consolidate your coins.</p>
        </div>
        <div className="flex items-center gap-3">
          <label className="flex items-center gap-2 text-sm text-muted-foreground">
            <Switch checked={showArchived} onCheckedChange={setShowArchived} />
            Show archived
          </label>
          <Select value={walletFilter} onValueChange={setWalletFilter}>
            <SelectTrigger className="w-48">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All wallets</SelectItem>
              {wallets.map((w) => (
                <SelectItem key={w.id} value={w.id}>
                  {w.name}
                  {w.archived ? " (archived)" : ""}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Select value={showFrozen} onValueChange={(v) => setShowFrozen(v as typeof showFrozen)}>
            <SelectTrigger className="w-36">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All</SelectItem>
              <SelectItem value="frozen">Frozen only</SelectItem>
              <SelectItem value="unfrozen">Spendable only</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <Card className="md:col-span-2">
          <CardHeader>
            <CardTitle>Size distribution</CardTitle>
            <CardDescription>{distribution?.total_utxos ?? 0} UTXOs · {distribution?.dust_count ?? 0} dust</CardDescription>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={180}>
              <BarChart data={chartData}>
                <CartesianGrid strokeDasharray="3 3" stroke="hsl(var(--border))" />
                <XAxis dataKey="name" tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" />
                <YAxis tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" />
                <RTooltip
                  formatter={(v: number, k: string) => (k === "sats" ? amount(v, 4) : v)}
                  contentStyle={{ background: "hsl(var(--popover))", border: "1px solid hsl(var(--border))", fontSize: 12 }}
                />
                <Bar dataKey="sats" fill="hsl(var(--muted-foreground))" radius={[4, 4, 0, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Plan consolidation</CardTitle>
            <CardDescription>Merges dust/small UTXOs in the selected wallet.</CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            <div className="space-y-1.5">
              <label className="text-xs text-muted-foreground">Fee rate (sat/vB)</label>
              <Input type="number" value={feeRate} onChange={(e) => setFeeRate(Number(e.target.value))} />
            </div>
            <Button
              className="w-full"
              disabled={walletFilter === "all" || consolidateMutation.isPending}
              onClick={() => consolidateMutation.mutate()}
            >
              <Layers className="h-4 w-4" />
              Generate PSBT
            </Button>
            {walletFilter === "all" && <p className="text-xs text-muted-foreground">Select a single wallet to plan a consolidation.</p>}
            {consolidationResult && (
              <textarea
                readOnly
                value={consolidationResult}
                className="h-24 w-full resize-none rounded-md border border-border bg-secondary/40 p-2 font-mono text-[10px]"
              />
            )}
          </CardContent>
        </Card>
      </div>

      {readiness && (
        <p className="rounded-md border border-border bg-secondary/40 p-3 text-xs text-muted-foreground">
          {readiness.total_utxos === 0
            ? "Cost basis: no coins yet."
            : readiness.ready
              ? `Cost basis: ready. All ${readiness.total_utxos} coin${readiness.total_utxos === 1 ? "" : "s"} have a cost and date, so "Lowest tax impact" is available in Spend.`
              : `Cost basis: ${readiness.missing} of ${readiness.total_utxos} coin${readiness.total_utxos === 1 ? "" : "s"} missing a cost and date. "Lowest tax impact" only uses coins that have one.`}{" "}
          Amounts are in {fiatCurrency}.
        </p>
      )}

      <Card>
        <CardContent className="p-0">
          <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-xs text-muted-foreground">
                <th className="px-5 py-2 font-medium">Outpoint</th>
                <th className="px-5 py-2 font-medium">Amount</th>
                <th className="px-5 py-2 font-medium">Confirmations</th>
                <th className="px-5 py-2 font-medium">Label</th>
                <th className="px-5 py-2 font-medium">Cost basis</th>
                <th className="px-5 py-2 font-medium">Status</th>
                <th className="px-5 py-2 font-medium"></th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((u) => (
                <tr key={u.id} className="border-b border-border/60 last:border-0 hover:bg-accent/40">
                  <td className="px-5 py-3 font-mono text-xs">
                    {formatTxid(u.txid)}:{u.vout}
                  </td>
                  <td className="px-5 py-3 mono-nums">{amount(u.amount_sats)}</td>
                  <td className="px-5 py-3 text-muted-foreground">{u.confirmations}</td>
                  <td className="px-5 py-3 text-muted-foreground">{u.label ?? "—"}</td>
                  <td className="px-5 py-3 text-xs">
                    {u.acquisition_cost_fiat !== null && u.acquisition_date ? (
                      <span className="mono-nums" title={u.acquisition_note ?? undefined}>
                        {u.acquisition_cost_fiat.toLocaleString(undefined, { maximumFractionDigits: 2 })} {fiatCurrency} · {u.acquisition_date}
                      </span>
                    ) : (
                      <Badge variant="muted">missing</Badge>
                    )}
                  </td>
                  <td className="px-5 py-3">
                    {u.is_frozen && (
                      <Badge variant="muted">
                        <Snowflake className="mr-1 h-3 w-3" /> frozen
                      </Badge>
                    )}
                    {u.is_change && !u.is_frozen && (
                      <Badge variant="outline">
                        <Sparkles className="mr-1 h-3 w-3" /> change
                      </Badge>
                    )}
                  </td>
                  <td className="px-5 py-3 text-right">
                    <Button size="sm" variant="ghost" onClick={() => openBasis(u)}>
                      Basis
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() => freezeMutation.mutate({ id: u.id, frozen: !u.is_frozen })}
                    >
                      {u.is_frozen ? "Unfreeze" : "Freeze"}
                    </Button>
                  </td>
                </tr>
              ))}
              {filtered.length === 0 && (
                <tr>
                  <td colSpan={7} className="px-5 py-10 text-center text-sm text-muted-foreground">
                    No UTXOs match these filters.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
          </div>
        </CardContent>
      </Card>

      <Dialog open={basisFor !== null} onOpenChange={(o) => !o && setBasisFor(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Cost basis</DialogTitle>
            <DialogDescription>
              {basisFor ? <span className="mono-nums">{formatTxid(basisFor.txid)}:{basisFor.vout} · {amount(basisFor.amount_sats)}. </span> : null}
              What you paid for this coin and when you got it. Both are needed. Stored only on this {device}.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-1.5">
              <Label>Cost ({fiatCurrency})</Label>
              <Input type="number" min="0" step="0.01" value={basisCost} onChange={(e) => setBasisCost(e.target.value)} placeholder="e.g. 1250.00" />
            </div>
            <div className="space-y-1.5">
              <Label>Date acquired</Label>
              <Input type="date" value={basisDate} onChange={(e) => setBasisDate(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label>Note (optional)</Label>
              <Input value={basisNote} onChange={(e) => setBasisNote(e.target.value)} placeholder="e.g. bought on exchange, mined, gift" />
            </div>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => basisMutation.mutate(true)} disabled={basisMutation.isPending}>
              Clear basis
            </Button>
            <Button onClick={() => basisMutation.mutate(false)} disabled={basisMutation.isPending}>
              Save
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

    </div>
  );
}
