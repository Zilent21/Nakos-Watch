import { useMemo } from "react";
import { Link } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, ArrowRight, CheckCircle2 } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { ScoreRing } from "@/components/ui/score-ring";
import { Skeleton } from "@/components/ui/skeleton";
import { AlertsCard } from "@/modules/receive/alerts-panel";
import * as api from "@/bindings/commands";
import { formatFiat, satsToFiat, formatRelativeTime } from "@/lib/format";
import { useFormatAmount } from "@/lib/use-amount";
import { usePreferencesStore } from "@/stores/preferences-store";
import type { NodeConfig, Priority } from "@/bindings/types";

type Action = { id: string; title: string; detail: string; priority: Priority; to: string };

const PRIORITY_RANK: Record<Priority, number> = { critical: 0, high: 1, medium: 2, low: 3 };
const PRIORITY_BADGE: Record<Priority, "destructive" | "warning" | "secondary" | "muted"> = {
  critical: "destructive",
  high: "warning",
  medium: "secondary",
  low: "muted",
};

function hostPort(configs: NodeConfig[], kind: NodeConfig["kind"]): string | null {
  const c = configs.filter((n) => n.kind === kind && n.enabled).sort((a, b) => Number(b.is_default) - Number(a.is_default))[0];
  return c ? `${c.host}:${c.port}` : null;
}

function StatusRow({ label, state, detail }: { label: string; state: "ok" | "down" | "unset" | "checking"; detail: string }) {
  const dot = { ok: "bg-success", down: "bg-destructive", unset: "bg-muted-foreground/50", checking: "bg-muted-foreground/50 animate-pulse" }[state];
  const word = { ok: "Reachable", down: "Unreachable", unset: "Not set up", checking: "Checking…" }[state];
  return (
    <div className="flex items-start justify-between gap-4 py-3">
      <div className="flex items-center gap-3">
        <span className={`h-2 w-2 shrink-0 rounded-full ${dot}`} aria-hidden />
        <span className="text-sm font-medium">{label}</span>
      </div>
      <div className="text-right">
        <div className="text-sm">{word}</div>
        <div className="mono-nums text-xs text-muted-foreground">{detail}</div>
      </div>
    </div>
  );
}

export function TodayPage() {
  const amount = useFormatAmount();
  const { fiatCurrency, fiatPricePerBtc } = usePreferencesStore();

  // Everything below except node status is local sqlite, so it renders even
  // when Core or the Electrum server is down.
  const { data: summary, error: summaryError } = useQuery({ queryKey: ["portfolio-summary"], queryFn: api.portfolioSummary });
  const { data: wallets } = useQuery({ queryKey: ["wallets", false], queryFn: () => api.listWallets(false) });
  const { data: snapshotWallets = [] } = useQuery({ queryKey: ["snapshot-wallets"], queryFn: api.snapshotWallets });
  const { data: history } = useQuery({ queryKey: ["health-history", 1], queryFn: () => api.getHealthHistory(1) });
  const { data: privacy } = useQuery({ queryKey: ["privacy-score"], queryFn: api.getPrivacyScore });
  const { data: security } = useQuery({ queryKey: ["security-score"], queryFn: api.getSecurityScore });
  const { data: privacyDash } = useQuery({ queryKey: ["privacy-dashboard"], queryFn: api.getPrivacyDashboard });
  const { data: tax } = useQuery({ queryKey: ["tax-readiness"], queryFn: api.getTaxReadiness });
  const { data: inheritance } = useQuery({ queryKey: ["inheritance-config"], queryFn: () => api.getInheritanceConfig() });
  const { data: reminder } = useQuery({ queryKey: ["review-reminder"], queryFn: api.getReviewReminder });
  const { data: configs = [] } = useQuery({ queryKey: ["node-configs"], queryFn: api.listNodeConfigs });
  // Node status can block on an unreachable host, so it goes last.
  const { data: status, isLoading: statusLoading } = useQuery({
    queryKey: ["node-status"],
    queryFn: api.getNodeStatus,
    enabled: summary !== undefined,
  });

  const totalSats = summary?.total_sats ?? 0;
  const fiat = fiatPricePerBtc ? satsToFiat(totalSats, fiatPricePerBtc) : null;
  const walletCount = wallets?.length ?? 0;

  const lastSynced = useMemo(() => {
    const times = (wallets ?? []).map((w) => w.last_synced_at).filter((t): t is string => !!t);
    return times.length ? times.sort().at(-1)! : null;
  }, [wallets]);

  const coreHost = hostPort(configs, "bitcoin_core_rpc");
  const electrumHost = hostPort(configs, "electrum");
  const coreState = !coreHost ? "unset" : statusLoading || !status ? "checking" : status.core ? "ok" : "down";
  const electrumState = !electrumHost ? "unset" : statusLoading || !status ? "checking" : status.electrum?.connected ? "ok" : "down";
  const nodeDown = status !== undefined && (coreState === "down" || electrumState === "down");

  const actions = useMemo<Action[]>(() => {
    const list: Action[] = [];
    if (wallets && walletCount === 0) list.push({ id: "no-wallet", title: "Add your first wallet", detail: "Portfolio shows how to export a descriptor or xpub and paste it in.", priority: "high", to: "/portfolio" });
    if (status) {
      if (coreState === "down") list.push({ id: "core-down", title: "Bitcoin Core isn't responding", detail: `Couldn't reach ${coreHost}. Health and fee data are last-known.`, priority: "high", to: "/node" });
      if (electrumState === "down") list.push({ id: "electrum-down", title: "Your Electrum server isn't responding", detail: `Couldn't reach ${electrumHost}. Balances are last-known until it is back.`, priority: "high", to: "/node" });
    }
    if (!electrumHost) list.push({ id: "electrum-unset", title: "Connect an Electrum server", detail: "Wallets need one to sync balances.", priority: "high", to: "/node" });
    if (!coreHost) list.push({ id: "core-unset", title: "Connect Bitcoin Core", detail: "Adds node health to your score.", priority: "low", to: "/node" });

    const neverSynced = (wallets ?? []).filter((w) => w.source_type !== "manual" && !w.last_synced_at).length;
    if (neverSynced > 0) list.push({ id: "never-synced", title: `${neverSynced} wallet${neverSynced === 1 ? " has" : "s have"} never synced`, detail: "Sync from Portfolio to see real balances.", priority: "medium", to: "/portfolio" });

    if (walletCount > 0 && inheritance) {
      if (reminder?.overdue) list.push({ id: "review-overdue", title: "Your inheritance review date has passed", detail: `It was due ${reminder.review_by}.`, priority: "high", to: "/inheritance" });
      if (!inheritance.has_instructions) list.push({ id: "no-letter", title: "Write your recovery letter", detail: "Your family has nothing to follow yet.", priority: "medium", to: "/inheritance" });
      if (inheritance.test_status === "never_tested") list.push({ id: "untested", title: "Test your recovery plan", detail: "A plan nobody has rehearsed tends to fail under stress.", priority: "medium", to: "/inheritance" });
      if (inheritance.contacts.length === 0) list.push({ id: "no-contact", title: "Name a recovery contact", detail: "Someone who knows where to start.", priority: "low", to: "/inheritance" });
    }

    if (privacyDash && privacyDash.utxo_count > 0) {
      if (privacyDash.reuse.reused_addresses > 0) list.push({ id: "reuse", title: `${privacyDash.reuse.reused_addresses} address${privacyDash.reuse.reused_addresses === 1 ? "" : "es"} hold more than one coin`, detail: "Reuse links your payments together.", priority: "medium", to: "/privacy" });
      if (privacyDash.consolidation.level === "fragmented" || privacyDash.consolidation.level === "concentrated") list.push({ id: "coins-shape", title: `Your coins look ${privacyDash.consolidation.level}`, detail: privacyDash.consolidation.detail, priority: "low", to: "/privacy" });
    }
    for (const a of (privacy?.recommended_actions ?? []).slice(0, 1)) list.push({ id: `privacy-${a.id}`, title: a.title, detail: a.description, priority: a.priority, to: "/privacy" });
    for (const a of (security?.recommended_actions ?? []).slice(0, 2)) list.push({ id: `security-${a.id}`, title: a.title, detail: a.description, priority: a.priority, to: "/security" });
    if (tax && tax.total_utxos > 0 && tax.missing > 0) list.push({ id: "tax", title: `Add cost basis for ${tax.missing} coin${tax.missing === 1 ? "" : "s"}`, detail: "Needed for lowest-tax spending.", priority: "low", to: "/utxos" });

    return list.sort((a, b) => PRIORITY_RANK[a.priority] - PRIORITY_RANK[b.priority]).slice(0, 7);
  }, [status, coreState, electrumState, coreHost, electrumHost, wallets, walletCount, inheritance, reminder, privacyDash, privacy, security, tax]);

  const health = history?.[0];

  return (
    <>
      <PageHeader
        title="Today"
        description={walletCount === 0 ? "Your Bitcoin, in one place. No wallets yet." : `Net holdings across ${walletCount} watch-only wallet${walletCount === 1 ? "" : "s"}.`}
      />

      <AlertsCard />

      {nodeDown && (
        <div role="status" className="flex items-start gap-3 rounded-lg border border-warning/40 bg-warning/10 p-4 text-sm">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-warning" />
          <p>
            {coreState === "down" && electrumState === "down"
              ? "Bitcoin Core and your Electrum server aren't responding."
              : coreState === "down"
                ? "Bitcoin Core isn't responding."
                : "Your Electrum server isn't responding."}{" "}
            Showing last-known balances{lastSynced ? `, updated ${formatRelativeTime(lastSynced)} (${new Date(lastSynced).toLocaleString()})` : ""}.
          </p>
        </div>
      )}

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
        <Card className="lg:col-span-2">
          <CardHeader>
            <CardTitle>Net holdings</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            {summaryError ? (
              <p className="text-sm text-destructive">Couldn't load holdings: {String(summaryError)}</p>
            ) : !summary ? (
              <Skeleton className="h-10 w-64" />
            ) : (
              <div className="mono-nums text-3xl font-semibold">{amount(totalSats)}</div>
            )}
            <div className="mono-nums text-sm text-muted-foreground">
              {fiat !== null ? formatFiat(fiat, fiatCurrency) : (
                <Link to="/settings" className="underline underline-offset-4 hover:text-foreground">
                  Set an exchange rate to see fiat value
                </Link>
              )}
            </div>
            <div className="text-xs text-muted-foreground">
              {lastSynced ? `Last synced ${formatRelativeTime(lastSynced)} · ${new Date(lastSynced).toLocaleString()}` : "No wallet has synced yet."}
              {snapshotWallets.length > 0 && ` Includes last-known balances from an imported letter for ${snapshotWallets.length} wallet${snapshotWallets.length === 1 ? "" : "s"}.`}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Scores</CardTitle>
            <CardDescription>{health ? `Health as of ${formatRelativeTime(health.taken_at)}` : "Open Health score to compute one."}</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="flex items-start justify-between">
              {[
                { label: "Health", score: health?.composite_score ?? null, to: "/health" },
                { label: "Privacy", score: privacy?.score ?? null, to: "/privacy" },
                { label: "Security", score: security?.score ?? null, to: "/security" },
              ].map((r) => (
                <Link key={r.label} to={r.to} className="flex flex-col items-center gap-2 rounded-lg p-1 focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-[hsl(var(--ring))]">
                  <ScoreRing score={r.score} label={r.label} size={60} />
                  <span className="text-xs text-muted-foreground">{r.label}</span>
                </Link>
              ))}
            </div>
          </CardContent>
        </Card>
      </div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Node</CardTitle>
          </CardHeader>
          <CardContent className="divide-y divide-border">
            <StatusRow
              label="Bitcoin Core"
              state={coreState}
              detail={coreState === "ok" && status?.core ? `height ${status.core.blocks.toLocaleString()} · ${status.core.connections_in + status.core.connections_out} peers` : (coreHost ?? "Add a connection in Node")}
            />
            <StatusRow
              label="Electrum server"
              state={electrumState}
              detail={electrumState === "ok" && status?.electrum ? `height ${status.electrum.tip_height.toLocaleString()}` : (electrumHost ?? "Add a connection in Node")}
            />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Inheritance</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            {!inheritance ? (
              <Skeleton className="h-16 w-full" />
            ) : (
              <ul className="space-y-2 text-sm">
                {[
                  { ok: inheritance.has_instructions, yes: "Recovery letter written", no: "No recovery letter yet" },
                  { ok: inheritance.test_status === "tested_pass", yes: "Recovery plan tested", no: inheritance.test_status === "tested_fail" ? "Last recovery test failed" : "Recovery plan never tested" },
                  { ok: !!reminder?.review_by && !reminder.overdue, yes: `Review due ${reminder?.review_by}`, no: reminder?.overdue ? `Review was due ${reminder.review_by}` : "No review date set" },
                ].map((row) => (
                  <li key={row.yes} className="flex items-center gap-3">
                    {row.ok ? <CheckCircle2 className="h-4 w-4 text-success" /> : <AlertTriangle className="h-4 w-4 text-muted-foreground" />}
                    <span className={row.ok ? "" : "text-muted-foreground"}>{row.ok ? row.yes : row.no}</span>
                  </li>
                ))}
              </ul>
            )}
            <Button asChild variant="outline" size="sm">
              <Link to="/inheritance">
                Open Inheritance <ArrowRight className="h-4 w-4" />
              </Link>
            </Button>
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Recommended today</CardTitle>
          <CardDescription>Drawn from your own wallet data and settings.</CardDescription>
        </CardHeader>
        <CardContent>
          {actions.length === 0 ? (
            <p className="py-4 text-sm text-muted-foreground">Nothing needs your attention right now.</p>
          ) : (
            <ul className="divide-y divide-border">
              {actions.map((a) => (
                <li key={a.id}>
                  <Link to={a.to} className="flex items-center gap-4 py-3 transition-colors hover:bg-accent/40 focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-[hsl(var(--ring))]">
                    <Badge variant={PRIORITY_BADGE[a.priority]} className="w-16 shrink-0 justify-center capitalize">
                      {a.priority}
                    </Badge>
                    <div className="min-w-0 flex-1">
                      <div className="text-sm font-medium">{a.title}</div>
                      <div className="truncate text-xs text-muted-foreground">{a.detail}</div>
                    </div>
                    <ArrowRight className="h-4 w-4 shrink-0 text-muted-foreground" />
                  </Link>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>
    </>
  );
}
