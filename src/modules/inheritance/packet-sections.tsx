import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, Download, ListChecks, CalendarClock, Upload, FileText } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useDeviceWord } from "@/lib/platform";
import * as api from "@/bindings/commands";
import { toast } from "@/hooks/use-toast";
import type { ImportSummary, PacketWallet } from "@/bindings/types";

const KIND_LABEL: Record<PacketWallet["kind"], string> = {
  descriptor: "Descriptor",
  xpub: "Extended public key",
  multisig: "Multisig",
  address_only: "Address only",
  manual: "Manual entry",
};

const textareaClass =
  "w-full rounded-md border border-border bg-secondary/40 p-3 text-sm outline-hidden focus-visible:ring-1 focus-visible:ring-[hsl(var(--ring))]";

function Field({ label, value }: { label: string; value: string | null }) {
  if (!value) return null;
  return (
    <div className="text-xs">
      <span className="text-muted-foreground">{label}: </span>
      <span className="break-all font-mono">{value}</span>
    </div>
  );
}

/** Per-wallet note, encrypted with the vault key (same path as the global letter). */
function WalletNote({ walletId }: { walletId: string }) {
  const queryClient = useQueryClient();
  const { data, error } = useQuery({
    queryKey: ["inheritance-instructions", walletId],
    queryFn: () => api.getInheritanceInstructions(walletId),
    retry: false,
  });
  const [text, setText] = useState("");
  useEffect(() => setText(data ?? ""), [data]);

  const saveMutation = useMutation({
    mutationFn: () => api.saveInheritanceInstructions(walletId, text),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["inheritance-instructions", walletId] });
      toast({ title: "Note saved", description: "Encrypted with your vault key.", variant: "success" });
    },
    onError: (e) => toast({ title: "Couldn't save note", description: String(e), variant: "destructive" }),
  });

  return (
    <div className="space-y-2">
      <Label className="text-xs">Note for this wallet (encrypted)</Label>
      <textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        rows={3}
        className={textareaClass}
        placeholder="Where the signing devices are, passphrase hints (never the passphrase), who to call…"
      />
      {error ? <p className="text-xs text-destructive">{String(error)}</p> : null}
      <Button size="sm" variant="outline" onClick={() => saveMutation.mutate()} disabled={saveMutation.isPending}>
        Save note
      </Button>
    </div>
  );
}

export function WalletsSection({ showArchived, onShowArchived }: { showArchived: boolean; onShowArchived: (v: boolean) => void }) {
  const device = useDeviceWord();
  const { data: packet, error } = useQuery({
    queryKey: ["inheritance-packet", showArchived],
    queryFn: () => api.getInheritancePacket(showArchived),
  });

  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between space-y-0">
        <div>
          <CardTitle>Wallets an heir would import</CardTitle>
          <CardDescription>Exactly what is stored on this {device}, unencrypted. Nakos never invents descriptors, and no secrets are shown.</CardDescription>
        </div>
        <label className="flex items-center gap-2 text-sm text-muted-foreground">
          <Switch checked={showArchived} onCheckedChange={onShowArchived} />
          Show archived
        </label>
      </CardHeader>
      <CardContent className="space-y-4">
        {error ? <p className="text-sm text-destructive">Couldn't load wallets: {String(error)}</p> : null}
        {packet && packet.wallets.length === 0 && (
          <p className="text-sm text-muted-foreground">No wallets yet. Add a watch-only wallet in Portfolio.</p>
        )}
        {(packet?.wallets ?? []).map((w) => (
          <div key={w.id} className="space-y-2 rounded-md border border-border p-3">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-sm font-medium">{w.name}</span>
              <Badge variant="outline">{KIND_LABEL[w.kind]}</Badge>
              {w.archived && <Badge variant="muted">archived</Badge>}
              {w.kind === "address_only" && (
                <Badge variant="warning">heir cannot reconstruct without extra info you store in notes</Badge>
              )}
            </div>
            {w.heir_warning && w.kind !== "address_only" && (
              <p className="flex items-start gap-1.5 text-xs text-warning">
                <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" /> {w.heir_warning}
              </p>
            )}
            <Field label="Master fingerprint" value={w.master_fingerprint} />
            <Field label="Derivation path" value={w.derivation_path} />
            <Field label="Descriptor" value={w.descriptor} />
            <Field label="Change descriptor" value={w.change_descriptor} />
            <Field label="Extended public key" value={w.xpub} />
            {w.multisig && (
              <div className="space-y-1">
                <div className="text-xs">
                  Multisig {w.multisig.threshold_m}-of-{w.multisig.total_n} ({w.multisig.script_type})
                </div>
                {w.multisig.cosigners.map((c) => (
                  <div key={c.label + c.xpub} className="ml-3 space-y-0.5 border-l border-border pl-3">
                    <div className="text-xs font-medium">
                      {c.label}
                      {c.hardware_model ? ` · ${c.hardware_model}` : ""}
                    </div>
                    <Field label="Fingerprint" value={c.fingerprint} />
                    <Field label="Path" value={c.derivation_path} />
                    <Field label="xpub" value={c.xpub} />
                  </div>
                ))}
              </div>
            )}
            {w.addresses.length > 0 && <Field label="Watched addresses" value={w.addresses.join(", ")} />}
            <WalletNote walletId={w.id} />
          </div>
        ))}
      </CardContent>
    </Card>
  );
}

export function ChecklistCard() {
  const { data: packet } = useQuery({ queryKey: ["inheritance-packet", false], queryFn: () => api.getInheritancePacket(false) });
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <ListChecks className="h-4 w-4" /> Heir checklist
        </CardTitle>
        <CardDescription>What someone needs to do to watch and plan spends from these wallets.</CardDescription>
      </CardHeader>
      <CardContent>
        <ol className="list-decimal space-y-2 pl-5 text-sm">
          {(packet?.checklist ?? []).map((c) => (
            <li key={c}>{c}</li>
          ))}
        </ol>
      </CardContent>
    </Card>
  );
}

export function ReminderCard() {
  const device = useDeviceWord("machine");
  const queryClient = useQueryClient();
  const { data, error } = useQuery({ queryKey: ["review-reminder"], queryFn: api.getReviewReminder });
  const [date, setDate] = useState("");
  const [note, setNote] = useState("");
  useEffect(() => {
    setDate(data?.review_by ?? "");
    setNote(data?.note ?? "");
  }, [data]);

  const saveMutation = useMutation({
    mutationFn: () => api.saveReviewReminder(date || null, note || null),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["review-reminder"] });
      toast({ title: date ? "Reminder saved" : "Reminder cleared", variant: "success" });
    },
    onError: (e) => toast({ title: "Couldn't save reminder", description: String(e), variant: "destructive" }),
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <CalendarClock className="h-4 w-4" /> Review reminder
        </CardTitle>
        <CardDescription>A date and note kept on this {device} only. Nothing is sent anywhere.</CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        {data?.overdue && (
          <p className="flex items-center gap-1.5 text-sm text-warning">
            <AlertTriangle className="h-4 w-4" /> Review was due {data.review_by}.
          </p>
        )}
        {error ? <p className="text-xs text-destructive">{String(error)}</p> : null}
        <div className="space-y-1.5">
          <Label>Review this packet by</Label>
          <Input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
        </div>
        <div className="space-y-1.5">
          <Label>Note (encrypted)</Label>
          <Input value={note} onChange={(e) => setNote(e.target.value)} placeholder="e.g. re-check descriptors after adding a device" />
        </div>
        <Button size="sm" onClick={() => saveMutation.mutate()} disabled={saveMutation.isPending}>
          Save reminder
        </Button>
      </CardContent>
    </Card>
  );
}

type ExportFormat = "json" | "markdown" | "markdown_pdf" | "letter" | "letter_pdf";
type LetterTemplate = "family" | "single_sig_cold" | "multisig";

const TEMPLATES: { value: LetterTemplate; label: string; hint: string }[] = [
  { value: "family", label: "Simple family", hint: "Plain steps for someone with no bitcoin background." },
  { value: "single_sig_cold", label: "Single-sig cold storage", hint: "One hardware wallet and one written backup of its words." },
  { value: "multisig", label: "Technical multisig", hint: "Several signing devices, some needed together. Lists each one." },
];

function useExport(format: ExportFormat, includeNotes: boolean, includeArchived: boolean, includeBasis = false, template: LetterTemplate = "family") {
  return useMutation({
    mutationFn: async () => {
      // Rust opens the save dialog; null means the person cancelled.
      return api.exportInheritancePacket({ format, includeNotes, includeArchived, includeBasis: format === "json" && includeBasis, template });
    },
    onSuccess: (path) => {
      if (path) toast({ title: "File saved", description: path, variant: "success" });
    },
    onError: (e) => toast({ title: "Export failed", description: String(e), variant: "destructive" }),
  });
}

/** Two paths: a plain letter for family, and the full packet for a helper. */
export function ExportCard({ includeArchivedDefault }: { includeArchivedDefault: boolean }) {
  const [advFormat, setAdvFormat] = useState<"json" | "markdown">("markdown");
  const [includeNotes, setIncludeNotes] = useState(false);
  const [includeArchived, setIncludeArchived] = useState(includeArchivedDefault);
  const [template, setTemplate] = useState<LetterTemplate>("family");
  const letterExport = useExport("letter", includeNotes, includeArchived, false, template);
  const letterPdf = useExport("letter_pdf", includeNotes, includeArchived, false, template);
  const [includeBasis, setIncludeBasis] = useState(false);
  const advancedExport = useExport(advFormat, includeNotes, includeArchived, includeBasis);
  const advancedPdf = useExport("markdown_pdf", includeNotes, includeArchived);

  return (
    <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <FileText className="h-4 w-4" /> Simple letter (for family)
          </CardTitle>
          <CardDescription>
            Plain English steps and what to expect. Family can open it in Nakos to see the bitcoin. Spending still needs the hardware wallet or seed backup you
            already keep. The technical data sits at the very bottom of the file. This is not a backup of your vault; for that, use Settings, Encrypted vault backup.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="space-y-1">
            <Select value={template} onValueChange={(v) => setTemplate(v as LetterTemplate)}>
              <SelectTrigger className="w-64" aria-label="Letter template">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {TEMPLATES.map((t) => (
                  <SelectItem key={t.value} value={t.value}>
                    {t.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">{TEMPLATES.find((t) => t.value === template)?.hint}</p>
          </div>
          <Button className="h-auto w-full whitespace-normal py-2.5" onClick={() => letterExport.mutate()} disabled={letterExport.isPending}>
            <Download className="h-4 w-4" /> Download simple letter (for family)
          </Button>
          <Button variant="outline" className="h-auto w-full whitespace-normal py-2.5" onClick={() => letterPdf.mutate()} disabled={letterPdf.isPending}>
            <FileText className="h-4 w-4" /> Printable PDF, with blanks to fill in by hand
          </Button>
          <WhatItLacks />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Download className="h-4 w-4" /> Advanced packet (Sparrow / helper)
          </CardTitle>
          <CardDescription>
            Full wallet details for someone technical. Never RPC passwords, your vault passphrase, or seeds. A cookie file is listed by path only. Also not a vault backup.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <Select value={advFormat} onValueChange={(v) => setAdvFormat(v as "json" | "markdown")}>
            <SelectTrigger className="w-56">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="markdown">Markdown (readable)</SelectItem>
              <SelectItem value="json">JSON (importable)</SelectItem>
            </SelectContent>
          </Select>
          <label className={`flex items-center gap-2 text-xs ${advFormat === "json" ? "" : "text-muted-foreground"}`}>
            <Checkbox checked={includeBasis && advFormat === "json"} disabled={advFormat !== "json"} onCheckedChange={(v) => setIncludeBasis(v === true)} />
            Include cost basis (JSON only; never in the family letter)
          </label>
          <Button variant="outline" className="h-auto w-full whitespace-normal py-2.5" onClick={() => advancedExport.mutate()} disabled={advancedExport.isPending}>
            <Download className="h-4 w-4" /> Download advanced packet (Sparrow / helper)
          </Button>
          <Button variant="outline" className="h-auto w-full whitespace-normal py-2.5" onClick={() => advancedPdf.mutate()} disabled={advancedPdf.isPending}>
            <FileText className="h-4 w-4" /> Printable recovery packet (PDF)
          </Button>
          <p className="text-xs text-muted-foreground">Each wallet gets structured recovery details: type, where its public keys are, the hardware model, and next steps.</p>
        </CardContent>
      </Card>

      <div className="flex flex-wrap items-center gap-6 text-sm md:col-span-2">
        <label className="flex items-center gap-2">
          <Checkbox checked={includeNotes} onCheckedChange={(v) => setIncludeNotes(v === true)} />
          Include my notes and letter in either file (the vault must be unlocked)
        </label>
        <label className="flex items-center gap-2">
          <Checkbox checked={includeArchived} onCheckedChange={(v) => setIncludeArchived(v === true)} />
          Include archived wallets
        </label>
      </div>
    </div>
  );
}

/** Said next to both exports, so nobody mistakes the letter for a backup of the keys. */
function WhatItLacks() {
  return (
    <div role="note" className="rounded-md border border-border bg-secondary/40 p-3 text-xs text-muted-foreground">
      <p className="font-medium text-foreground">What this packet does not contain</p>
      <p>
        No seed words, no PINs or passwords, no vault passphrase, no private keys. It can't move bitcoin. It tells your family what exists and where you keep the real keys, which you
        fill in by hand on the printed copy.
      </p>
    </div>
  );
}

export function ImportCard() {
  const queryClient = useQueryClient();
  const importMutation = useMutation({
    mutationFn: async () => {
      return api.importInheritancePacket(); // Rust opens the file dialog; null if cancelled
    },
    onSuccess: (r) => {
      if (!r) return;
      queryClient.invalidateQueries({ queryKey: ["wallets"] });
      queryClient.invalidateQueries({ queryKey: ["portfolio-summary"] });
      queryClient.invalidateQueries({ queryKey: ["inheritance-packet"] });
      queryClient.invalidateQueries({ queryKey: ["snapshot-wallets"] });
      toast({
        title: r.created.length ? `Imported ${r.created.length} wallet${r.created.length === 1 ? "" : "s"}` : "Nothing new imported",
        description:
          [r.skipped.length ? `${r.skipped.length} skipped. See the list below.` : "", r.letter_saved ? "The letter was saved to your notes." : ""]
            .filter(Boolean)
            .join(" ") || undefined,
        variant: r.created.length ? "success" : "default",
      });
      setLast(r);
    },
    onError: (e) => toast({ title: "Import failed", description: String(e), variant: "destructive" }),
  });
  const [last, setLast] = useState<ImportSummary | null>(null);

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Upload className="h-4 w-4" /> Import a letter or packet
        </CardTitle>
        <CardDescription>
          Creates watch-only wallets from the file, the same way Add Wallet does. No private keys, and no node passwords are ever read or filled in. Balances in the file
          show as "last known" until you sync.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <Button onClick={() => importMutation.mutate()} disabled={importMutation.isPending}>
          <Upload className="h-4 w-4" /> Import a letter or packet…
        </Button>
        {last && (
          <div className="space-y-1 text-sm">
            {last.created.length > 0 && <p className="text-success">Imported: {last.created.join(", ")}</p>}
            {last.letter_saved && <p className="text-success">The letter from the file was saved (encrypted) under "Letter / notes".</p>}
            {last.letter_note && <p className="text-muted-foreground">{last.letter_note}</p>}
            {last.skipped.map(([name, reason]) => (
              <p key={name} className="text-muted-foreground">
                Skipped {name}: {reason}
              </p>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
