import { useState } from "react";
import { Link } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Bell, BellRing, CheckCheck } from "lucide-react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import * as api from "@/bindings/commands";
import type { AlertEvent, BalanceAlert, Wallet } from "@/bindings/types";
import { toast } from "@/hooks/use-toast";
import { formatRelativeTime } from "@/lib/format";
import { parseSats } from "@/lib/receive";
import { useFormatAmount } from "@/lib/use-amount";
import { usePreferencesStore } from "@/stores/preferences-store";

const KIND_LABEL: Record<AlertEvent["kind"], string> = {
  balance_over: "Balance",
  receive_paid: "Received",
  saved_used: "Address used",
  list_low: "Running low",
};

function useMarkRead() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (ids?: string[]) => api.markAlertsRead(ids),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["alerts"] }),
  });
}

function AlertList({ events }: { events: AlertEvent[] }) {
  return (
    <ul className="divide-y divide-border">
      {events.map((e) => (
        <li key={e.id} className="flex items-start justify-between gap-3 py-2.5 text-sm">
          <span className="flex min-w-0 items-start gap-2">
            <Badge variant={e.read_at ? "muted" : e.kind === "list_low" ? "warning" : "success"} className="mt-0.5 shrink-0">{KIND_LABEL[e.kind]}</Badge>
            <span className={e.read_at ? "text-muted-foreground" : undefined}>{e.message}</span>
          </span>
          <span className="shrink-0 text-xs text-muted-foreground">{formatRelativeTime(e.created_at)}</span>
        </li>
      ))}
    </ul>
  );
}

/** Unread alerts on Today. Renders nothing when there are none. */
export function AlertsCard() {
  const { data: events = [] } = useQuery({ queryKey: ["alerts", false], queryFn: () => api.listAlerts(false) });
  const markRead = useMarkRead();
  if (!events.length) return null;
  return (
    <Card>
      <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
        <div className="space-y-1">
          <CardTitle className="flex items-center gap-2">
            <BellRing className="h-4 w-4 text-bitcoin" />
            Alerts
          </CardTitle>
          <CardDescription>From your last sync or address check.</CardDescription>
        </div>
        <div className="flex gap-2">
          <Button variant="ghost" size="sm" asChild>
            <Link to="/receive?tab=alerts">All alerts</Link>
          </Button>
          <Button variant="outline" size="sm" onClick={() => markRead.mutate(undefined)}>
            <CheckCheck className="h-4 w-4" />
            Mark read
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        <AlertList events={events.slice(0, 6)} />
        {events.length > 6 && <p className="pt-2 text-xs text-muted-foreground">And {events.length - 6} more.</p>}
      </CardContent>
    </Card>
  );
}

function BalanceRow({ wallet, rule, balance }: { wallet: Wallet; rule: BalanceAlert | undefined; balance: number | undefined }) {
  const qc = useQueryClient();
  const amount = useFormatAmount();
  const [text, setText] = useState(rule ? rule.threshold_sats.toLocaleString("en-US") : "");
  const sats = text.trim() ? parseSats(text) : null;
  const save = useMutation({
    mutationFn: (t: number | null) => api.setBalanceAlert(wallet.id, t),
    onSuccess: (_, t) => {
      qc.invalidateQueries({ queryKey: ["balance-alerts"] });
      toast({ title: t ? `Alert set for ${wallet.name}` : `Alert removed for ${wallet.name}`, variant: "success" });
      if (!t) setText("");
    },
    onError: (e) => toast({ title: "Couldn't save", description: String(e), variant: "destructive" }),
  });
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 py-3">
      <div className="min-w-0">
        <p className="text-sm font-medium">{wallet.name}</p>
        <p className="text-xs text-muted-foreground">
          Holds {balance !== undefined ? amount(balance) : "…"}
          {rule ? ` · alert at ${amount(rule.threshold_sats)}${rule.is_above ? " (over now)" : ""}` : ""}
        </p>
      </div>
      <div className="flex items-center gap-2">
        <Input
          className="w-36"
          inputMode="numeric"
          placeholder="sats"
          aria-label={`Alert amount for ${wallet.name}`}
          value={text}
          aria-invalid={text.trim() !== "" && sats === null}
          onChange={(e) => setText(e.target.value)}
        />
        <Button size="sm" onClick={() => save.mutate(sats)} disabled={sats === null || save.isPending}>Save</Button>
        {rule && (
          <Button size="sm" variant="ghost" onClick={() => save.mutate(null)} disabled={save.isPending}>Remove</Button>
        )}
      </div>
    </div>
  );
}

export function AlertsPanel() {
  const { data: wallets = [] } = useQuery({ queryKey: ["wallets", false], queryFn: () => api.listWallets(false) });
  const { data: balances = [] } = useQuery({ queryKey: ["wallet-balances", false], queryFn: () => api.listWalletBalances(false) });
  const { data: rules = [] } = useQuery({ queryKey: ["balance-alerts"], queryFn: api.listBalanceAlerts });
  const { data: events = [] } = useQuery({ queryKey: ["alerts", true], queryFn: () => api.listAlerts(true) });
  const autoCheck = usePreferencesStore((s) => s.autoCheckMinutes);
  const markRead = useMarkRead();
  const watched = wallets.filter((w) => w.source_type !== "manual");

  return (
    <div className="grid gap-6 lg:grid-cols-2">
      <Card className="h-fit">
        <CardHeader>
          <CardTitle>Balance alerts</CardTitle>
          <CardDescription>
            Get told when a wallet goes over an amount, for example a hot wallet that's due to move coins to cold storage. Nakos only tells you;
            moving coins is up to you.
          </CardDescription>
        </CardHeader>
        <CardContent className="divide-y divide-border">
          {watched.length === 0 ? (
            <p className="text-sm text-muted-foreground">Add a wallet first.</p>
          ) : (
            watched.map((w) => {
              const rule = rules.find((r) => r.wallet_id === w.id);
              // Keyed on the saved amount, so the field fills in once the rules load (and after a save).
              return <BalanceRow key={`${w.id}:${rule?.threshold_sats ?? ""}`} wallet={w} rule={rule} balance={balances.find((b) => b.wallet_id === w.id)?.balance_sats} />;
            })
          )}
          <p className="pt-3 text-xs text-muted-foreground">
            Alerts are worked out after each sync and address check, while Nakos is open.{" "}
            {autoCheck ? `Nakos checks every ${autoCheck} minutes.` : "Turn on regular checks in Settings → Receiving."}
          </p>
        </CardContent>
      </Card>

      <Card className="h-fit">
        <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
          <div className="space-y-1">
            <CardTitle>History</CardTitle>
            <CardDescription>The last 100 alerts.</CardDescription>
          </div>
          {events.some((e) => !e.read_at) && (
            <Button variant="outline" size="sm" onClick={() => markRead.mutate(undefined)}>
              <CheckCheck className="h-4 w-4" />
              Mark all read
            </Button>
          )}
        </CardHeader>
        <CardContent>
          {events.length ? (
            <AlertList events={events} />
          ) : (
            <p className="flex items-center gap-2 text-sm text-muted-foreground">
              <Bell className="h-4 w-4" />
              No alerts yet.
            </p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
