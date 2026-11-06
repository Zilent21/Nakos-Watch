import { useEffect } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { LineChart, Line, XAxis, YAxis, ResponsiveContainer, Tooltip as RTooltip, CartesianGrid } from "recharts";
import { HeartPulse, RefreshCw, Save } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import * as api from "@/bindings/commands";
import { cn } from "@/lib/utils";

const DIMENSION_LABELS: Record<string, string> = {
  security: "Security",
  privacy: "Privacy",
  liquidity: "Liquidity",
  inheritance: "Inheritance",
  redundancy: "Redundancy",
  node_health: "Node health",
  lightning_health: "Lightning health",
  tax_readiness: "Tax readiness",
};

function scoreColor(score: number) {
  if (score >= 80) return "text-success";
  if (score >= 50) return "text-warning";
  return "text-destructive";
}

export function HealthScorePage() {
  const queryClient = useQueryClient();
  const { data: health, mutate: recompute, isPending } = useMutation({
    mutationFn: api.getHealthScore,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["health-history"] }),
  });
  const { mutate: saveSnapshot, isPending: saving } = useMutation({
    mutationFn: api.recordHealthSnapshot,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["health-history"] }),
  });
  const { data: history = [] } = useQuery({ queryKey: ["health-history"], queryFn: () => api.getHealthHistory(60) });

  useEffect(() => {
    recompute();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const chartData = [...history].reverse().map((h) => ({
    date: new Date(h.taken_at).toLocaleDateString(undefined, { month: "short", day: "numeric" }),
    score: h.composite_score,
  }));

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Health score</h1>
          <p className="text-sm text-muted-foreground">One score across everything that matters.</p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" onClick={() => saveSnapshot()} disabled={saving || isPending}>
            <Save className="h-4 w-4" />
            Save snapshot
          </Button>
          <Button variant="outline" onClick={() => recompute()} disabled={isPending}>
            <RefreshCw className={cn("h-4 w-4", isPending && "animate-spin")} />
            Recompute
          </Button>
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <Card>
          <CardHeader>
            <CardTitle>Composite score</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex items-center gap-4">
              <div className={cn("text-4xl font-bold mono-nums", scoreColor(health?.composite_score ?? 0))}>
                {health?.composite_score ?? "—"}
              </div>
              <HeartPulse className={cn("h-8 w-8", scoreColor(health?.composite_score ?? 0))} />
            </div>
            <Progress value={health?.composite_score ?? 0} className="mt-3" />
          </CardContent>
        </Card>

        <Card className="md:col-span-2">
          <CardHeader>
            <CardTitle>Trend (last 60 snapshots)</CardTitle>
          </CardHeader>
          <CardContent>
            {chartData.length > 1 ? (
              <ResponsiveContainer width="100%" height={140}>
                <LineChart data={chartData}>
                  <CartesianGrid strokeDasharray="3 3" stroke="hsl(var(--border))" />
                  <XAxis dataKey="date" tick={{ fontSize: 10 }} stroke="hsl(var(--muted-foreground))" />
                  <YAxis domain={[0, 100]} tick={{ fontSize: 10 }} stroke="hsl(var(--muted-foreground))" />
                  <RTooltip contentStyle={{ background: "hsl(var(--popover))", border: "1px solid hsl(var(--border))", fontSize: 12 }} />
                  <Line type="monotone" dataKey="score" stroke="hsl(var(--bitcoin))" strokeWidth={2} dot={false} />
                </LineChart>
              </ResponsiveContainer>
            ) : (
              <p className="py-8 text-center text-xs text-muted-foreground">Not enough history yet — recompute over time to build a trend.</p>
            )}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Bitcoin Core node</CardTitle>
          <CardDescription>{health?.node.node_label ?? "Health from your saved Bitcoin Core RPC connection."}</CardDescription>
        </CardHeader>
        <CardContent>
          {!health ? null : !health.node.configured ? (
            <p className="text-sm text-muted-foreground">No Bitcoin Core RPC configured. Add one in Node to score node health.</p>
          ) : health.node.error ? (
            <p className="text-sm text-destructive">RPC error: {health.node.error}</p>
          ) : (
            <div className="flex items-start gap-6">
              <div className={cn("text-3xl font-bold mono-nums", scoreColor(health.node.score ?? 0))}>{health.node.score}</div>
              <ul className="space-y-1 text-sm text-muted-foreground">
                {health.node.reasons.map((r) => (
                  <li key={r}>{r}</li>
                ))}
              </ul>
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Dimensions</CardTitle>
          <CardDescription>Weighted by relevance; missing dimensions (e.g. no node/Lightning) redistribute weight.</CardDescription>
        </CardHeader>
        <CardContent className="grid grid-cols-1 md:grid-cols-2 gap-x-8 gap-y-4">
          {(health?.dimensions ?? []).map((d) => (
            <div key={d.key} className="space-y-1">
              <div className="flex justify-between text-sm">
                <span>{DIMENSION_LABELS[d.key] ?? d.key}</span>
                <span className="mono-nums text-muted-foreground">
                  {d.score} · weight {d.weight.toFixed(0)}%
                </span>
              </div>
              <Progress value={d.score} />
            </div>
          ))}
        </CardContent>
      </Card>
    </div>
  );
}
