import { useState } from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { toast } from "@/hooks/use-toast";
import { CHECK_THRESHOLD_CHOICES, parseSats } from "@/lib/receive";
import { useFormatAmount } from "@/lib/use-amount";
import { AUTO_CHECK_CHOICES, usePreferencesStore } from "@/stores/preferences-store";

/** Settings → Receiving: the device-check threshold and regular payment checks. */
export function ReceivingSettingsCard() {
  const amount = useFormatAmount();
  const { receiveCheckSats, setReceiveCheckSats, autoCheckMinutes, setAutoCheckMinutes } = usePreferencesStore();
  const preset = CHECK_THRESHOLD_CHOICES.includes(receiveCheckSats);
  const [custom, setCustom] = useState(preset ? "" : receiveCheckSats.toLocaleString("en-US"));
  const [showCustom, setShowCustom] = useState(!preset);
  const customSats = parseSats(custom);

  const save = (sats: number) =>
    setReceiveCheckSats(sats)
      .then(() => toast({ title: sats === 0 ? "Every address must be checked first" : `Check on device from ${amount(sats)}`, variant: "success" }))
      .catch((e) => toast({ title: "Couldn't save", description: String(e), variant: "destructive" }));

  return (
    <Card>
      <CardHeader>
        <CardTitle>Receiving</CardTitle>
        <CardDescription>How careful Receive is, and how often Nakos looks for payments.</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-6 md:grid-cols-2">
        <div className="space-y-2">
          <Label>Check on the device first from</Label>
          <Select
            value={showCustom ? "custom" : String(receiveCheckSats)}
            onValueChange={(v) => {
              if (v === "custom") {
                setShowCustom(true);
              } else {
                setShowCustom(false);
                save(Number(v));
              }
            }}
          >
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              {CHECK_THRESHOLD_CHOICES.map((c) => (
                <SelectItem key={c} value={String(c)}>
                  {c === 0 ? "Every address" : `${amount(c)}${c === 500_000 ? " (default)" : ""}`}
                </SelectItem>
              ))}
              <SelectItem value="custom">Custom amount…</SelectItem>
            </SelectContent>
          </Select>
          {showCustom && (
            <div className="flex items-center gap-2">
              <Input
                inputMode="numeric"
                placeholder="sats"
                aria-label="Custom amount in sats"
                value={custom}
                aria-invalid={custom.trim() !== "" && customSats === null}
                onChange={(e) => setCustom(e.target.value)}
              />
              <Button size="sm" disabled={customSats === null} onClick={() => customSats !== null && save(customSats)}>Save</Button>
            </div>
          )}
          <p className="text-xs text-muted-foreground">
            For a payment of this much or more, Receive covers the QR code until you've compared the address with the one your hardware wallet shows.
            It's the one thing a screen can't prove by itself.
          </p>
        </div>
        <div className="space-y-2">
          <Label>Check for payments while Nakos is open</Label>
          <Select value={String(autoCheckMinutes)} onValueChange={(v) => setAutoCheckMinutes(Number(v))}>
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              {AUTO_CHECK_CHOICES.map((m) => (
                <SelectItem key={m} value={String(m)}>{m === 0 ? "Off (sync by hand)" : `Every ${m} minutes`}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">
            Syncs your wallets and checks saved addresses, then shows any alerts. It runs only while Nakos is open and unlocked, and doesn't keep it from
            auto-locking. Through someone else's Electrum server, each check tells that server about your addresses again.
          </p>
        </div>
      </CardContent>
    </Card>
  );
}
