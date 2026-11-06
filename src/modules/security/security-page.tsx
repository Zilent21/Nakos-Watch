import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ShieldCheck, AlertTriangle } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { PageHeader } from "@/components/ui/page-header";
import * as api from "@/bindings/commands";
import { cn } from "@/lib/utils";
import { toast } from "@/hooks/use-toast";
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

export function SecurityPage() {
  const queryClient = useQueryClient();
  const { data: score } = useQuery({ queryKey: ["security-score"], queryFn: api.getSecurityScore });
  const { data: wallets } = useQuery({ queryKey: ["wallets"], queryFn: () => api.listWallets() });
  const [selectedWallet, setSelectedWallet] = useState<string>("global");

  const [hwModel, setHwModel] = useState("");
  const [firmwareOk, setFirmwareOk] = useState(false);
  const [passphrase, setPassphrase] = useState(false);
  const [geoLocations, setGeoLocations] = useState(0);
  const [backupTested, setBackupTested] = useState(false);
  const [exchange2fa, setExchange2fa] = useState("none");

  const mutation = useMutation({
    mutationFn: () =>
      api.updateSecurityConfig({
        wallet_id: selectedWallet === "global" ? null : selectedWallet,
        hardware_wallet_model: hwModel || null,
        firmware_up_to_date: firmwareOk,
        passphrase_enabled: passphrase,
        geographic_redundancy_locations: geoLocations,
        backup_tested: backupTested,
        exchange_2fa_type: exchange2fa,
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["security-score"] });
      toast({ title: "Saved", description: "Security settings updated", variant: "success" });
    },
    onError: (e) => toast({ title: "Could not save", description: String(e), variant: "destructive" }),
  });

  return (
    <div className="space-y-6">
      <PageHeader title="Security" description="Scored from your actual setup. Nothing here is guessed." />

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <Card className="col-span-1">
          <CardHeader>
            <CardTitle>Security Score</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex items-center gap-4">
              <div className={cn("text-4xl font-bold mono-nums", scoreColor(score?.score ?? 0))}>{score?.score ?? "—"}</div>
              <ShieldCheck className={cn("h-8 w-8", scoreColor(score?.score ?? 0))} />
            </div>
            <Progress value={score?.score ?? 0} className="mt-3" />
          </CardContent>
        </Card>

        <Card className="md:col-span-2">
          <CardHeader>
            <CardTitle>Factors</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            {(score?.factors ?? []).map((f) => (
              <div key={f.key} className="flex items-center justify-between text-sm">
                <span className="text-muted-foreground">{f.label}</span>
                <span className="mono-nums">
                  {f.points}/{f.max_points}
                </span>
              </div>
            ))}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Recommended actions</CardTitle>
          <CardDescription>Ordered by priority and points available.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {(score?.recommended_actions ?? []).length === 0 && (
            <p className="text-sm text-muted-foreground">Nothing outstanding — great work.</p>
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
              <span className="mono-nums text-xs text-muted-foreground">+{a.points_available} pts</span>
            </div>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Update setup details</CardTitle>
          <CardDescription>Report your real configuration to keep the score accurate.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-1.5">
            <Label>Applies to</Label>
            <Select value={selectedWallet} onValueChange={setSelectedWallet}>
              <SelectTrigger className="w-64">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="global">Global default</SelectItem>
                {(wallets ?? []).map((w) => (
                  <SelectItem key={w.id} value={w.id}>
                    {w.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-1.5">
              <Label>Hardware wallet model</Label>
              <Input value={hwModel} onChange={(e) => setHwModel(e.target.value)} placeholder="e.g. Coldcard Mk4" />
            </div>
            <div className="space-y-1.5">
              <Label>Geographic backup locations</Label>
              <Input type="number" min={0} value={geoLocations} onChange={(e) => setGeoLocations(Number(e.target.value))} />
            </div>
          </div>

          <div className="space-y-1.5">
            <Label>Exchange 2FA type</Label>
            <Select value={exchange2fa} onValueChange={setExchange2fa}>
              <SelectTrigger className="w-64">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="none">None</SelectItem>
                <SelectItem value="sms">SMS</SelectItem>
                <SelectItem value="totp">Authenticator app (TOTP)</SelectItem>
                <SelectItem value="hardware_key">Hardware security key</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="flex flex-wrap gap-6">
            <label className="flex items-center gap-2 text-sm">
              <Switch checked={firmwareOk} onCheckedChange={setFirmwareOk} />
              Firmware up to date
            </label>
            <label className="flex items-center gap-2 text-sm">
              <Switch checked={passphrase} onCheckedChange={setPassphrase} />
              BIP-39 passphrase enabled
            </label>
            <label className="flex items-center gap-2 text-sm">
              <Switch checked={backupTested} onCheckedChange={setBackupTested} />
              Backup restore tested
            </label>
          </div>

          <Button onClick={() => mutation.mutate()} disabled={mutation.isPending}>
            Save
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}
