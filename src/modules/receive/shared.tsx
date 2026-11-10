import { useState, type ReactNode } from "react";
import { Copy, Info, ShieldAlert, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { QrCode } from "@/components/qr-code";
import { toast } from "@/hooks/use-toast";
import { useFormatAmount } from "@/lib/use-amount";
import { bip21, mustCheckFirst } from "@/lib/receive";
import { usePreferencesStore } from "@/stores/preferences-store";

export const textareaClass =
  "w-full rounded-md border border-border bg-secondary/40 p-3 font-mono text-sm outline-hidden focus-visible:ring-1 focus-visible:ring-[hsl(var(--ring))]";

/** Which kinds of wallet give a new address each time, and which give only what they have. */
export function AddressSourcesNote() {
  return (
    <div role="note" className="flex gap-3 rounded-md border border-border bg-secondary/40 p-3 text-sm">
      <Info className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
      <div className="space-y-1">
        <p className="font-medium">Not every wallet gives new addresses</p>
        <ul className="list-disc space-y-0.5 pl-5 text-muted-foreground">
          <li>
            <span className="text-foreground">xpub, or a descriptor ending in /*:</span> a fresh address every time you press New address, worked out
            from the public key.
          </li>
          <li>
            <span className="text-foreground">A single address, or a descriptor without /*:</span> just that one address. Nakos can't make more from it.
          </li>
          <li>
            <span className="text-foreground">Saved addresses:</span> only the ones you pasted, each handed out once. Add more from your wallet when the list
            runs low.
          </li>
        </ul>
      </div>
    </div>
  );
}

export function copy(text: string, what: string) {
  navigator.clipboard
    .writeText(text)
    .then(() => toast({ title: `${what} copied`, variant: "success" }))
    .catch((e) => toast({ title: "Couldn't copy", description: String(e), variant: "destructive" }));
}

export function CopyButton({ text, what, label }: { text: string; what: string; label?: string }) {
  return (
    <Button variant="outline" size="sm" onClick={() => copy(text, what)}>
      <Copy className="h-4 w-4" />
      {label ?? `Copy ${what.toLowerCase()}`}
    </Button>
  );
}

/** The address in groups of four, so it is easy to compare character by character with a device screen. */
export function GroupedAddress({ address }: { address: string }) {
  const groups = address.match(/.{1,4}/g) ?? [address];
  return (
    <p className="break-all font-mono text-sm leading-relaxed tracking-wide" aria-label={address}>
      {groups.map((g, i) => (
        <span key={i} className={i % 2 ? "text-muted-foreground" : undefined}>
          {g}
          {i < groups.length - 1 ? " " : ""}
        </span>
      ))}
    </p>
  );
}

export function CheckBadge({ checkedAt, where }: { checkedAt: string | null; where: string }) {
  return checkedAt ? (
    <Badge variant="success" className="gap-1">
      <ShieldCheck className="h-3 w-3" />
      Checked {where} · {new Date(checkedAt).toLocaleDateString()}
    </Badge>
  ) : (
    <Badge variant="warning" className="gap-1">
      <ShieldAlert className="h-3 w-3" />
      Not checked {where}
    </Badge>
  );
}

/**
 * The QR code and the address, behind the device check when the amount calls for it. `where` is
 * "on device" for Receive and "in your wallet" for saved addresses.
 */
export function PayPanel({
  address,
  amountSats,
  label,
  checkedAt,
  where,
  howToCheck,
  onChecked,
  footer,
}: {
  address: string;
  amountSats: number | null;
  label: string | null;
  checkedAt: string | null;
  where: string;
  howToCheck: ReactNode;
  onChecked: (checked: boolean) => void;
  footer?: ReactNode;
}) {
  const amount = useFormatAmount();
  const threshold = usePreferencesStore((s) => s.receiveCheckSats);
  const [shownAnyway, setShownAnyway] = useState<string | null>(null);
  const link = bip21(address, amountSats, label);
  const covered = mustCheckFirst(amountSats, threshold, checkedAt !== null) && shownAnyway !== address;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <CheckBadge checkedAt={checkedAt} where={where} />
        {amountSats !== null && <Badge variant="outline">{amount(amountSats)}</Badge>}
        {label && <Badge variant="muted">{label}</Badge>}
      </div>

      {covered ? (
        <div role="alert" className="space-y-3 rounded-lg border border-warning/50 bg-warning/10 p-4 text-sm">
          <p className="font-medium">
            {threshold === 0 ? "Check every address before you use it." : `This is ${amount(amountSats ?? 0)}: check the address ${where} first.`}
          </p>
          <p className="text-muted-foreground">
            If this computer or the vault were tampered with, the address on screen could belong to someone else. Your device can't be fooled that way.
          </p>
          <div className="rounded-md border border-border bg-background/60 p-3">
            <GroupedAddress address={address} />
          </div>
          <div className="text-muted-foreground">{howToCheck}</div>
          <div className="flex flex-wrap gap-2">
            <Button size="sm" onClick={() => onChecked(true)}>
              <ShieldCheck className="h-4 w-4" />
              It matches: mark as checked
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setShownAnyway(address)}>
              Show without checking
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex flex-col gap-4 sm:flex-row sm:items-start">
          <QrCode text={link} className="shrink-0 self-center sm:self-start" />
          <div className="min-w-0 flex-1 space-y-3">
            <GroupedAddress address={address} />
            <div className="flex flex-wrap gap-2">
              <CopyButton text={address} what="Address" />
              <CopyButton text={link} what="Payment link" label="Copy link" />
            </div>
            {!checkedAt && amountSats === null && threshold > 0 && (
              <p className="text-xs text-muted-foreground">Expecting {amount(threshold)} or more? Check this address {where} first.</p>
            )}
            {checkedAt ? (
              <Button variant="ghost" size="sm" onClick={() => onChecked(false)}>
                Mark as not checked
              </Button>
            ) : (
              <Button variant="outline" size="sm" onClick={() => onChecked(true)}>
                <ShieldCheck className="h-4 w-4" />
                I checked it {where}
              </Button>
            )}
            {footer}
          </div>
        </div>
      )}
    </div>
  );
}
