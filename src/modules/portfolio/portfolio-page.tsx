import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip as RTooltip } from "recharts";
import { Plus, RefreshCw } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { PageHeader } from "@/components/ui/page-header";
import { GettingStartedCard } from "@/modules/onboarding/import-guide";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, THead, TBody, TR, TH, TD } from "@/components/ui/table";
import { ArchiveWalletButton } from "@/components/archive-wallet-button";
import * as api from "@/bindings/commands";
import { formatFiat, formatRelativeTime, satsToFiat } from "@/lib/format";
import { useFormatAmount } from "@/lib/use-amount";
import { usePreferencesStore } from "@/stores/preferences-store";
import { useShellStore } from "@/stores/shell-store";
import { toast } from "@/hooks/use-toast";
import { handlePublicServerGate } from "@/components/public-server-gate";
import { useNavigate } from "react-router-dom";
import { WalletDetailsDialog } from "./wallet-details";

// Muted, quiet palette: no orange here (orange is reserved for CTAs, focus and score rings).
const CUSTODY_COLORS: Record<string, string> = {
  cold_storage: "#9AA4B2",
  multisig_vault: "#6EA8A0",
  hot_wallet: "#C98B8B",
  lightning: "#A596C8",
  mining: "#C2A878",
  exchange: "#7B9CC4",
  etf: "#7BB0BC",
  retirement: "#B98DA6",
  other: "#6B7280",
};

const RISK_VARIANT = { high: "destructive", medium: "warning", low: "success" } as const;

export function PortfolioPage() {
  const amount = useFormatAmount();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const setAddWalletOpen = useShellStore((s) => s.setAddWalletOpen);
  const [showArchived, setShowArchived] = useState(false);
  const [detailsId, setDetailsId] = useState<string | null>(null);
  const { fiatCurrency, fiatPricePerBtc } = usePreferencesStore();

  const { data: summary, error: summaryError } = useQuery({ queryKey: ["portfolio-summary"], queryFn: api.portfolioSummary });
  const { data: snapshotWallets = [] } = useQuery({ queryKey: ["snapshot-wallets"], queryFn: api.snapshotWallets });
  const { data: taxReadiness } = useQuery({ queryKey: ["tax-readiness"], queryFn: api.getTaxReadiness });
  const { data: wallets, error: walletsError } = useQuery({
    queryKey: ["wallets", showArchived],
    queryFn: () => api.listWallets(showArchived),
  });
  // Per-wallet balances come from the same calculation as the headline total.
  // Archived rows get a balance when shown but are never part of that total.
  // Keyed under "portfolio-summary" so every existing invalidation (sync,
  // archive, add wallet, import) refreshes it too.
  const { data: balances } = useQuery({
    queryKey: ["portfolio-summary", "wallet-balances", showArchived],
    queryFn: () => api.listWalletBalances(showArchived),
  });
  const balanceById = new Map((balances ?? []).map((b) => [b.wallet_id, b]));
  const activeWallets = wallets?.filter((w) => !w.archived) ?? [];

  const syncMutation = useMutation({
    mutationFn: (id: string) => api.syncWallet(id),
    onSuccess: (result) => {
      queryClient.invalidateQueries({ queryKey: ["wallets"] });
      queryClient.invalidateQueries({ queryKey: ["portfolio-summary"] });
      queryClient.invalidateQueries({ queryKey: ["snapshot-wallets"] });
      const parts = [
        result.new_utxos > 0 ? `${result.new_utxos} new coin${result.new_utxos === 1 ? "" : "s"}` : null,
        result.new_transactions > 0 ? `${result.new_transactions} new transaction${result.new_transactions === 1 ? "" : "s"}` : null,
      ].filter(Boolean);
      toast({ title: "Sync complete", description: parts.length > 0 ? parts.join(", ") : "No new activity", variant: "success" });
    },
    onError: (e, id) => {
      if (handlePublicServerGate(e, () => syncMutation.mutate(id))) return;
      toast({ title: "Sync failed", description: String(e), variant: "destructive" });
    },
  });
  const { data: nodeConfigs } = useQuery({ queryKey: ["node-configs"], queryFn: api.listNodeConfigs });
  const noElectrum = nodeConfigs !== undefined && !nodeConfigs.some((c) => c.kind === "electrum" && c.enabled);

  const totalSats = summary?.total_sats ?? 0;
  const fiatValue = fiatPricePerBtc ? satsToFiat(totalSats, fiatPricePerBtc) : null;
  const pieData = (summary?.by_custody_type ?? []).filter((c) => c.total_sats > 0).map((c) => ({ name: c.custody_type, value: c.total_sats }));

  if (wallets && !showArchived && activeWallets.length === 0) {
    return (
      <>
        <PageHeader title="Portfolio" description="Every watch-only wallet, and what it holds." />
        <GettingStartedCard onAddWallet={() => setAddWalletOpen(true)} />
      </>
    );
  }

  return (
    <>
      <PageHeader
        title="Portfolio"
        description="Every watch-only wallet, and what it holds."
        actions={
          <>
            <label className="flex items-center gap-3 text-sm text-muted-foreground">
              <Switch checked={showArchived} onCheckedChange={setShowArchived} />
              Show archived
            </label>
            <Button onClick={() => setAddWalletOpen(true)}>
              <Plus className="h-4 w-4" />
              Add wallet
            </Button>
          </>
        }
      />

      {noElectrum && (
        <div role="note" className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-warning/50 bg-warning/10 p-4 text-sm">
          <div className="space-y-1">
            <p className="font-semibold text-foreground">Your wallets can't sync yet.</p>
            <p className="text-muted-foreground">Nakos syncs through an Electrum server: your own, or a public one if you don't run a node. A public server sees your addresses; the Node page explains the trade-off.</p>
          </div>
          <Button variant="outline" onClick={() => navigate("/node")}>
            Set up a server
          </Button>
        </div>
      )}

      {(snapshotWallets.length > 0 || (taxReadiness && taxReadiness.total_utxos > 0 && !taxReadiness.ready)) && (
        <div className="space-y-2 text-xs text-muted-foreground">
          {snapshotWallets.length > 0 && (
            <p className="rounded-lg border border-border bg-secondary/40 p-4">
              {snapshotWallets.length} wallet{snapshotWallets.length === 1 ? " shows" : "s show"} a last-known balance from an imported letter (
              {snapshotWallets.map((w) => `${w.name}, as of ${w.as_of.slice(0, 10)}`).join("; ")}). Connect a server in Node, then sync, to refresh.
            </p>
          )}
          {taxReadiness && taxReadiness.total_utxos > 0 && !taxReadiness.ready && (
            <p>
              Cost basis is missing for {taxReadiness.missing} of {taxReadiness.total_utxos} coin{taxReadiness.total_utxos === 1 ? "" : "s"}. Add it in Coins.
            </p>
          )}
        </div>
      )}

      <div className="grid grid-cols-1 gap-6 md:grid-cols-3">
        <Card className="md:col-span-2">
          <CardHeader>
            <CardTitle>Total holdings</CardTitle>
          </CardHeader>
          <CardContent>
            {summaryError ? (
              <p className="text-sm text-destructive">Couldn't load holdings: {String(summaryError)}</p>
            ) : !summary ? (
              <Skeleton className="h-10 w-64" />
            ) : (
              <div className="mono-nums text-3xl font-semibold">{amount(totalSats)}</div>
            )}
            <div className="mono-nums mt-2 text-sm text-muted-foreground">
              {fiatValue !== null ? formatFiat(fiatValue, fiatCurrency) : "Set an exchange rate in Settings to see fiat value"}
            </div>
            <div className="mt-4 flex flex-wrap gap-2">
              {(summary?.by_risk_level ?? []).map((r) => (
                <Badge key={r.risk_level} variant={RISK_VARIANT[r.risk_level as keyof typeof RISK_VARIANT] ?? "muted"}>
                  <span className="mono-nums">
                    {r.risk_level}: {amount(r.total_sats, 4)}
                  </span>
                </Badge>
              ))}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>By custody type</CardTitle>
          </CardHeader>
          <CardContent>
            {pieData.length > 0 ? (
              <ResponsiveContainer width="100%" height={140}>
                <PieChart>
                  <Pie data={pieData} dataKey="value" nameKey="name" innerRadius={35} outerRadius={60} paddingAngle={2} stroke="none">
                    {pieData.map((entry) => (
                      <Cell key={entry.name} fill={CUSTODY_COLORS[entry.name] ?? "#6B7280"} />
                    ))}
                  </Pie>
                  <RTooltip
                    formatter={(v: number) => amount(v, 4)}
                    contentStyle={{ background: "hsl(var(--popover))", border: "1px solid hsl(var(--border))", borderRadius: 8, fontSize: 12 }}
                  />
                </PieChart>
              </ResponsiveContainer>
            ) : (
              <p className="py-8 text-center text-sm text-muted-foreground">No balances yet.</p>
            )}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Wallets</CardTitle>
        </CardHeader>
        <CardContent className="p-0 pb-2">
          {walletsError ? (
            <p className="px-6 pb-4 text-sm text-destructive">Couldn't load wallets: {String(walletsError)}</p>
          ) : !wallets ? (
            <div className="space-y-3 px-6 pb-4">
              <Skeleton className="h-8 w-full" />
              <Skeleton className="h-8 w-full" />
            </div>
          ) : (
            <>
            <ul data-testid="wallet-list" className="divide-y divide-border md:hidden">
              {wallets.map((w) => {
                const b = balanceById.get(w.id);
                return (
                  <li key={w.id} className="space-y-2 px-4 py-3">
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex min-w-0 items-center gap-2">
                        <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: CUSTODY_COLORS[w.custody_type] ?? "#6B7280" }} />
                        <button type="button" className="truncate text-left text-sm font-medium underline-offset-4 hover:underline" onClick={() => setDetailsId(w.id)}>
                          {w.name}
                        </button>
                        {w.archived && (
                          <Badge variant="muted" className="text-xs">
                            archived
                          </Badge>
                        )}
                      </div>
                      <span className="mono-nums shrink-0 text-sm" title={b?.is_snapshot ? "Last-known balance from an imported letter. Sync to refresh." : undefined}>
                        {b ? `${amount(b.balance_sats)}${b.is_snapshot ? "*" : ""}` : "…"}
                      </span>
                    </div>
                    <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                      <Badge variant={RISK_VARIANT[w.risk_level as keyof typeof RISK_VARIANT] ?? "muted"}>{w.risk_level}</Badge>
                      <span>{w.custody_type.replace("_", " ")}</span>
                      <span>· {formatRelativeTime(w.last_synced_at)}</span>
                      <div className="ml-auto flex items-center gap-1">
                        {!w.archived && w.source_type !== "manual" && (
                          <Button size="sm" variant="ghost" onClick={() => syncMutation.mutate(w.id)} disabled={syncMutation.isPending}>
                            <RefreshCw className={`h-3.5 w-3.5 ${syncMutation.isPending && syncMutation.variables === w.id ? "animate-spin" : ""}`} />
                            Sync
                          </Button>
                        )}
                        <ArchiveWalletButton wallet={w} />
                      </div>
                    </div>
                  </li>
                );
              })}
            </ul>
            <div className="hidden md:block">
            <Table>
              <THead>
                <tr>
                  <TH>Name</TH>
                  <TH className="hidden xl:table-cell">Custody</TH>
                  <TH>Risk</TH>
                  <TH numeric>Balance</TH>
                  <TH>Last synced</TH>
                  <TH />
                </tr>
              </THead>
              <TBody>
                {wallets.map((w) => {
                  const b = balanceById.get(w.id);
                  return (
                    <TR key={w.id}>
                      <TD>
                        <div className="flex items-center gap-3">
                          <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: CUSTODY_COLORS[w.custody_type] ?? "#6B7280" }} />
                          <button type="button" className="text-left underline-offset-4 hover:underline focus-visible:underline focus-visible:outline-hidden" onClick={() => setDetailsId(w.id)} title="Show wallet details">
                            {w.name}
                          </button>
                          {w.archived && (
                            <Badge variant="muted" className="text-xs">
                              archived
                            </Badge>
                          )}
                        </div>
                      </TD>
                      <TD className="hidden text-muted-foreground xl:table-cell">{w.custody_type.replace("_", " ")}</TD>
                      <TD>
                        <Badge variant={RISK_VARIANT[w.risk_level as keyof typeof RISK_VARIANT] ?? "muted"}>{w.risk_level}</Badge>
                      </TD>
                      <TD numeric>
                        {b ? (
                          <span title={b.is_snapshot ? "Last-known balance from an imported letter. Sync to refresh." : undefined}>
                            {amount(b.balance_sats)}{b.is_snapshot ? "*" : ""}
                          </span>
                        ) : (
                          <span className="text-muted-foreground">…</span>
                        )}
                      </TD>
                      <TD className="text-muted-foreground">{formatRelativeTime(w.last_synced_at)}</TD>
                      <TD className="text-right">
                        <div className="flex items-center justify-end gap-1">
                          {!w.archived && w.source_type !== "manual" && (
                            <Button size="sm" variant="ghost" onClick={() => syncMutation.mutate(w.id)} disabled={syncMutation.isPending}>
                              <RefreshCw className={`h-3.5 w-3.5 ${syncMutation.isPending && syncMutation.variables === w.id ? "animate-spin" : ""}`} />
                              Sync
                            </Button>
                          )}
                          <ArchiveWalletButton wallet={w} />
                        </div>
                      </TD>
                    </TR>
                  );
                })}
              </TBody>
            </Table>
            </div>
            </>
          )}
        </CardContent>
      </Card>
      <WalletDetailsDialog
        wallet={wallets?.find((w) => w.id === detailsId) ?? null}
        balance={detailsId ? balanceById.get(detailsId)?.balance_sats : undefined}
        onOpenChange={(open) => !open && setDetailsId(null)}
      />
    </>
  );
}
