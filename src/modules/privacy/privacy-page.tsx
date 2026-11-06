import { useQuery } from "@tanstack/react-query";
import { EyeOff, AlertTriangle } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Badge } from "@/components/ui/badge";
import { PageHeader } from "@/components/ui/page-header";
import * as api from "@/bindings/commands";
import { useFormatAmount } from "@/lib/use-amount";
import { cn } from "@/lib/utils";
import type { Priority } from "@/bindings/types";

const PRIORITY_STYLE: Record<Priority, string> = {
  critical: "text-destructive",
  high: "text-warning",
  medium: "text-foreground",
  low: "text-muted-foreground",
};

function scoreColor(score: number) {
  if (score >= 80) return "text-success";
  if (score >= 50) return "text-warning";
  return "text-destructive";
}

const SCRIPT_LABELS: Record<string, string> = {
  p2wpkh: "Native SegWit (P2WPKH)",
  p2wsh: "Native SegWit script (P2WSH)",
  p2tr: "Taproot (P2TR)",
  p2sh: "P2SH / wrapped SegWit",
  p2pkh: "Legacy (P2PKH)",
  unknown: "Unknown",
};

const RISK_LABEL: Record<string, string> = {
  "n/a": "Not enough coins",
  fragmented: "Fragmented",
  concentrated: "Concentrated",
  balanced: "Balanced",
};

function shortAddr(a: string) {
  return a.length > 20 ? `${a.slice(0, 10)}…${a.slice(-8)}` : a;
}

function Metric({ title, value, children }: { title: string; value: string; children: React.ReactNode }) {
  return (
    <Card>
      <CardHeader>
        <CardDescription>{title}</CardDescription>
        <CardTitle className="mono-nums text-2xl">{value}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-2 text-xs text-muted-foreground">{children}</CardContent>
    </Card>
  );
}

function LocalMetrics() {
  const amount = useFormatAmount();
  const { data: d, error } = useQuery({ queryKey: ["privacy-dashboard"], queryFn: api.getPrivacyDashboard });
  const { data: dist } = useQuery({ queryKey: ["utxo-distribution"], queryFn: () => api.utxoDistribution() });
  const { data: nodes = [] } = useQuery({ queryKey: ["node-configs"], queryFn: api.listNodeConfigs });
  const { data: status, isLoading: statusLoading } = useQuery({ queryKey: ["node-status"], queryFn: api.getNodeStatus });

  if (error) return <p className="text-sm text-destructive">Couldn't load privacy metrics: {String(error)}</p>;
  if (!d) return null;
  if (d.utxo_count === 0) {
    return (
      <Card>
        <CardContent className="py-6 text-sm text-muted-foreground">
          No unspent coins in your unarchived wallets yet. Sync a wallet to see UTXO, script-type and address-reuse metrics.
        </CardContent>
      </Card>
    );
  }

  const coreConfigured = nodes.some((n) => n.kind === "bitcoin_core_rpc" && n.enabled);
  const core = status?.core;
  const nonzeroBuckets = (dist?.by_size ?? []).filter((b) => b.count > 0);
  const src = d.sync_source;

  return (
    <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
      <Metric title="UTXOs" value={`${d.utxo_count} coin${d.utxo_count === 1 ? "" : "s"}`}>
        <p><span className="mono-nums">{amount(d.total_sats)}</span> across {d.wallet_count} active wallet{d.wallet_count === 1 ? "" : "s"}.</p>
        {nonzeroBuckets.length > 0 && (
          <p className="mono-nums">{nonzeroBuckets.map((b) => `${b.bucket} ${b.count}`).join(" · ")}</p>
        )}
      </Metric>

      <Metric
        title="Script types"
        value={d.script_mix.length === 1 ? (SCRIPT_LABELS[d.script_mix[0].script] ?? d.script_mix[0].script) : `${d.script_mix.length} types`}
      >
        <p>
          {d.script_mix.length === 1
            ? "Every coin uses one script type, so spending them together doesn't reveal a mix."
            : "Mixed script types in one transaction make your inputs easier to group."}
        </p>
        <ul className="mono-nums">
          {d.script_mix.map((m) => (
            <li key={m.script}>
              {SCRIPT_LABELS[m.script] ?? m.script}: {m.count}
            </li>
          ))}
        </ul>
      </Metric>

      <Metric
        title="Address reuse"
        value={d.reuse.reused_addresses === 0 ? "None" : `${d.reuse.reused_addresses} address${d.reuse.reused_addresses === 1 ? "" : "es"}`}
      >
        <p>
          {d.reuse.reused_addresses === 0
            ? "No address holds more than one coin right now. Spent history isn't kept, so past reuse can't be checked."
            : `${d.reuse.coins_on_reused_addresses} coins sit on addresses that received more than once, which links those payments together.`}
        </p>
        {d.reuse.worst.map((r) => (
          <p key={r.address} className="font-mono">
            {shortAddr(r.address)} · {r.coins} coins
          </p>
        ))}
      </Metric>

      <Metric title="Consolidation risk" value={RISK_LABEL[d.consolidation.level]}>
        <p>{d.consolidation.detail}</p>
      </Metric>

      <Metric title="Sync source" value={src.configured ? "Electrum" : "Not set up"}>
        <p>{src.note}</p>
        {src.configured && (
          <p className="font-mono">
            {src.host}:{src.port} · {src.use_tls ? "TLS" : "no TLS"}
            {src.last_connected_at ? ` · last connected ${new Date(src.last_connected_at).toLocaleString()}` : " · never connected"}
          </p>
        )}
      </Metric>

      <Metric title="Bitcoin Core node" value={!coreConfigured ? "Not set up" : statusLoading ? "Checking…" : core ? "Reachable" : "Unreachable"}>
        {!coreConfigured ? (
          <p>No Bitcoin Core RPC configured. Add one in Node.</p>
        ) : core ? (
          <p className="mono-nums">
            height {core.blocks.toLocaleString()} · {core.connections_in + core.connections_out} peers · {core.chain}
          </p>
        ) : (
          <p>Couldn't reach your saved Bitcoin Core RPC connection. See Health Score for the error.</p>
        )}
      </Metric>
    </div>
  );
}

export function PrivacyPage() {
  const { data: score } = useQuery({ queryKey: ["privacy-score"], queryFn: api.getPrivacyScore });

  return (
    <div className="space-y-6">
      <PageHeader title="Privacy" description="What your own wallet data reveals." />

      <LocalMetrics />

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <Card>
          <CardHeader>
            <CardTitle>Privacy Score</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex items-center gap-4">
              <div className={cn("text-4xl font-bold mono-nums", scoreColor(score?.score ?? 0))}>{score?.score ?? "—"}</div>
              <EyeOff className={cn("h-8 w-8", scoreColor(score?.score ?? 0))} />
            </div>
            <Progress value={score?.score ?? 0} className="mt-3" />
          </CardContent>
        </Card>

        <Card className="md:col-span-2">
          <CardHeader>
            <CardTitle>Breakdown</CardTitle>
            <CardDescription>Address reuse, common-input-ownership exposure, KYC taint, change hygiene, coin merges.</CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            {(score?.factors ?? []).map((f) => (
              <div key={f.key} className="space-y-1">
                <div className="flex justify-between text-sm">
                  <span>{f.label}</span>
                  <span className="mono-nums text-muted-foreground">
                    {f.points}/{f.max_points}
                  </span>
                </div>
                <Progress value={(f.points / Math.max(f.max_points, 1)) * 100} />
                <p className="text-xs text-muted-foreground">{f.detail}</p>
              </div>
            ))}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Recommended actions</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          {(score?.recommended_actions ?? []).length === 0 && (
            <p className="text-sm text-muted-foreground">No privacy leaks detected in your recorded data.</p>
          )}
          {(score?.recommended_actions ?? []).map((a) => (
            <div key={a.id} className="flex items-start gap-3 rounded-md border border-border p-3">
              <AlertTriangle className={cn("mt-0.5 h-4 w-4 shrink-0", PRIORITY_STYLE[a.priority])} />
              <div className="flex-1">
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium">{a.title}</span>
                  <Badge variant="outline" className="text-[10px] capitalize">
                    {a.priority}
                  </Badge>
                </div>
                <p className="mt-0.5 text-xs text-muted-foreground">{a.description}</p>
              </div>
            </div>
          ))}
        </CardContent>
      </Card>
    </div>
  );
}
