import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Copy, Send, ShieldCheck } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { PageHeader } from "@/components/ui/page-header";
import { EmptyState } from "@/components/ui/empty-state";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import * as api from "@/bindings/commands";
import { formatFiat } from "@/lib/format";
import { useFormatAmount } from "@/lib/use-amount";
import { usePreferencesStore } from "@/stores/preferences-store";
import { toast } from "@/hooks/use-toast";
import type { SpendOptimization } from "@/bindings/types";

const OPTIMIZATIONS: { value: SpendOptimization; label: string; hint: string; disabled?: boolean }[] = [
  {
    value: "tax",
    label: "Lowest tax impact (unavailable)",
    hint: "Record a cost and date for your coins in Coins first. Until a coin has both, this can't optimize for anything real.",
    disabled: true,
  },
  { value: "privacy", label: "Highest privacy", hint: "One script type per tx, avoids spending your biggest coin, prefers a single input" },
  { value: "fee", label: "Lowest fees", hint: "Fewest inputs / vbytes: largest coins first. Uses heuristic input sizes, not a fee API" },
  { value: "fragmentation", label: "Least fragmentation", hint: "Cleans up dust by spending small UTXOs first" },
];

export function SpendingPlannerPage() {
  const amount = useFormatAmount();
  const { fiatCurrency, fiatPricePerBtc } = usePreferencesStore();
  const { data: wallets = [] } = useQuery({ queryKey: ["wallets"], queryFn: () => api.listWallets() });
  const { data: feeEstimates } = useQuery({ queryKey: ["fee-estimates"], queryFn: api.getFeeEstimates });
  const { data: readiness } = useQuery({ queryKey: ["tax-readiness"], queryFn: api.getTaxReadiness });

  // "Lowest tax impact" (HIFO) unlocks once any coin has a lot. The plan only
  // ever spends coins that have one; the backend rejects it otherwise.
  const taxAvailable = (readiness?.with_basis ?? 0) > 0;
  const options = OPTIMIZATIONS.map((o) =>
    o.value !== "tax"
      ? o
      : taxAvailable
        ? {
            ...o,
            label: "Lowest tax impact",
            disabled: false,
            hint: `HIFO: spends the coins with the highest cost per sat first, using only coins that have a cost and date recorded (${readiness?.with_basis} of ${readiness?.total_utxos}). Whole coins, no holding-period or wash-sale rules. Not tax advice.`,
          }
        : o,
  );

  const [walletId, setWalletId] = useState("");
  const [destination, setDestination] = useState("");
  const [amountBtc, setAmountBtc] = useState("");
  const [optimization, setOptimization] = useState<SpendOptimization>("privacy");
  const [feeRate, setFeeRate] = useState(15);

  const mutation = useMutation({
    mutationFn: () =>
      api.planSpend({
        walletId,
        destinationAddress: destination,
        amountSats: Math.round(Number(amountBtc) * 100_000_000),
        optimization,
        feeRateSatPerVb: feeRate,
        currentPriceFiatPerBtc: fiatPricePerBtc ?? undefined,
      }),
    onSuccess: (plan) =>
      toast({
        title: "Plan ready",
        description: `${plan.input_count} input${plan.input_count === 1 ? "" : "s"}, ~${plan.estimated_fee_sats.toLocaleString()} sat fee`,
        variant: "success",
      }),
    onError: (e) => toast({ title: "Plan failed", description: String(e), variant: "destructive" }),
  });

  return (
    <>
      <PageHeader title="Spend" description="Choose which coins to spend and build an unsigned transaction." />

      <div className="flex items-start gap-3 rounded-lg border border-border bg-secondary/40 p-4 text-sm">
        <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
        <p>Nakos cannot move funds. You will sign this on your hardware wallet.</p>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        <Card>
          <CardHeader>
            <CardTitle>Plan a spend</CardTitle>
          </CardHeader>
          <CardContent className="space-y-6">
            <div className="space-y-2">
              <Label>From wallet</Label>
              <Select value={walletId} onValueChange={setWalletId}>
                <SelectTrigger>
                  <SelectValue placeholder="Select wallet" />
                </SelectTrigger>
                <SelectContent>
                  {wallets
                    .filter((w) => w.source_type !== "manual")
                    .map((w) => (
                      <SelectItem key={w.id} value={w.id}>
                        {w.name}
                      </SelectItem>
                    ))}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-2">
              <Label>Destination address</Label>
              <Input value={destination} onChange={(e) => setDestination(e.target.value)} className="font-mono text-xs" placeholder="bc1q…" />
            </div>

            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label>Amount (BTC)</Label>
                <Input type="number" step="0.00000001" value={amountBtc} onChange={(e) => setAmountBtc(e.target.value)} className="mono-nums" />
              </div>
              <div className="space-y-2">
                <Label>Fee rate (sat/vB)</Label>
                <Input type="number" value={feeRate} onChange={(e) => setFeeRate(Number(e.target.value))} className="mono-nums" />
                {feeEstimates?.public_estimate ? (
                  <p className="text-xs text-muted-foreground">
                    Suggested: {feeEstimates.public_estimate.half_hour_fee} sat/vB (~30 min) · source: {feeEstimates.source}
                  </p>
                ) : (
                  (() => {
                    const core = feeEstimates?.core_estimates.find((e) => e.target_blocks === 3 && e.sat_per_vb !== null);
                    return core ? (
                      <p className="text-xs text-muted-foreground">
                        Suggested: {core.sat_per_vb?.toFixed(1)} sat/vB (~3 blocks) · source: {feeEstimates?.source === "electrum" ? "Electrum server" : "Bitcoin Core"}
                      </p>
                    ) : (
                      <p className="text-xs text-muted-foreground">No local fee estimate. Enter a rate.</p>
                    );
                  })()
                )}
              </div>
            </div>

            <div className="space-y-2">
              <Label>Optimize for</Label>
              <Tabs value={optimization} onValueChange={(v) => setOptimization(v as SpendOptimization)}>
                <TabsList className="grid h-auto w-full grid-cols-2 gap-1">
                  {options.map((o) => (
                    <TabsTrigger key={o.value} value={o.value} disabled={o.disabled} className="whitespace-normal text-xs disabled:opacity-40">
                      {o.label}
                    </TabsTrigger>
                  ))}
                </TabsList>
              </Tabs>
              <p className="text-xs text-muted-foreground">{options.find((o) => o.value === optimization)?.hint}</p>
            </div>

            <Button className="w-full" disabled={!walletId || !destination || !amountBtc || mutation.isPending} onClick={() => mutation.mutate()}>
              <Send className="h-4 w-4" />
              {mutation.isPending ? "Building plan…" : "Generate plan"}
            </Button>
            {mutation.isError && (
              <p role="alert" className="text-sm text-destructive">
                {String(mutation.error)}
              </p>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Plan result</CardTitle>
            <CardDescription>An unsigned transaction (PSBT). Open it in Sparrow or your signer.</CardDescription>
          </CardHeader>
          <CardContent className="space-y-6">
            {mutation.data ? (
              <>
                <dl className="grid grid-cols-2 gap-x-4 gap-y-6 text-sm">
                  <div className="space-y-1">
                    <dt className="text-xs text-muted-foreground">Inputs used</dt>
                    <dd className="mono-nums">{mutation.data.input_count}</dd>
                  </div>
                  <div className="space-y-1">
                    <dt className="text-xs text-muted-foreground">Estimated fee</dt>
                    <dd className="mono-nums">{mutation.data.estimated_fee_sats.toLocaleString()} sats</dd>
                  </div>
                  <div className="space-y-1">
                    <dt className="text-xs text-muted-foreground">Change</dt>
                    <dd className="mono-nums">{amount(mutation.data.change_sats)}</dd>
                  </div>
                  <div className="space-y-1">
                    <dt className="text-xs text-muted-foreground">Est. realized gain</dt>
                    <dd className={mutation.data.estimated_realized_gain_fiat !== null ? "mono-nums" : "text-xs text-muted-foreground"}>
                      {mutation.data.estimated_realized_gain_fiat !== null
                        ? formatFiat(mutation.data.estimated_realized_gain_fiat, fiatCurrency)
                        : fiatPricePerBtc
                          ? "no cost basis recorded for the coins this spends"
                          : "gain needs an exchange rate in Settings"}
                    </dd>
                    {mutation.data.estimated_realized_gain_fiat !== null && (
                      <dd className="text-xs text-muted-foreground">On the amount sent plus fee only. Change isn't a sale. Not tax advice.</dd>
                    )}
                  </div>
                </dl>
                <p className="text-xs text-muted-foreground">{mutation.data.selection_note}</p>
                <div className="space-y-2">
                  <textarea
                    readOnly
                    value={mutation.data.psbt_base64}
                    aria-label="Unsigned PSBT (base64)"
                    className="h-32 w-full resize-none rounded-md border border-border bg-secondary/40 p-3 font-mono text-xs"
                  />
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() =>
                      navigator.clipboard
                        .writeText(mutation.data!.psbt_base64)
                        .then(() => toast({ title: "PSBT copied", variant: "success" }))
                        .catch((e) => toast({ title: "Couldn't copy", description: String(e), variant: "destructive" }))
                    }
                  >
                    <Copy className="h-4 w-4" />
                    Copy PSBT
                  </Button>
                </div>
              </>
            ) : (
              <EmptyState title="No plan yet." helper="Fill in the form and generate one." className="py-10" />
            )}
          </CardContent>
        </Card>
      </div>
    </>
  );
}
