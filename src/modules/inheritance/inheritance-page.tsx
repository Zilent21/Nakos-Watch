import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Users, Plus, AlertTriangle, CheckCircle2, KeyRound } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import * as api from "@/bindings/commands";
import { PageHeader } from "@/components/ui/page-header";
import { toast } from "@/hooks/use-toast";
import { ChecklistCard, ExportCard, ImportCard, ReminderCard, WalletsSection } from "./packet-sections";

export function InheritancePage() {
  const queryClient = useQueryClient();
  const [addOpen, setAddOpen] = useState(false);
  const [showArchived, setShowArchived] = useState(false);

  const { data: config } = useQuery({ queryKey: ["inheritance-config"], queryFn: () => api.getInheritanceConfig() });
  const { data: existingInstructions } = useQuery({
    queryKey: ["inheritance-instructions"],
    queryFn: () => api.getInheritanceInstructions(),
  });

  const [instructions, setInstructions] = useState("");
  useEffect(() => {
    if (existingInstructions) setInstructions(existingInstructions);
  }, [existingInstructions]);

  const [name, setName] = useState("");
  const [relationship, setRelationship] = useState("");
  const [contactMethod, setContactMethod] = useState("");
  const [hasKeyShare, setHasKeyShare] = useState(false);

  const addContactMutation = useMutation({
    mutationFn: () =>
      api.addRecoveryContact({ name, relationship, contact_method: contactMethod, has_key_share: hasKeyShare }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["inheritance-config"] });
      setAddOpen(false);
      setName("");
      setRelationship("");
      setContactMethod("");
      setHasKeyShare(false);
    },
    onError: (e) => toast({ title: "Couldn't add contact", description: String(e), variant: "destructive" }),
  });

  const saveInstructionsMutation = useMutation({
    mutationFn: () => api.saveInheritanceInstructions(undefined, instructions),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["inheritance-config"] });
      toast({ title: "Letter saved", description: "Encrypted with your vault key.", variant: "success" });
    },
    onError: (e) => toast({ title: "Couldn't save letter", description: String(e), variant: "destructive" }),
  });

  const markTestedMutation = useMutation({
    mutationFn: (passed: boolean) => api.markRecoveryTested(undefined, passed),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["inheritance-config"] }),
    onError: (e) => toast({ title: "Couldn't update drill status", description: String(e), variant: "destructive" }),
  });

  const neverTested = config?.test_status === "never_tested";

  return (
    <>
      <PageHeader
        title="Inheritance"
        description="Give your family a way to see your bitcoin. Nakos can't move funds, so they still need your hardware wallet or seed backup to spend."
      />

      {neverTested && (
        <div className="flex items-center gap-3 rounded-lg border border-warning/40 bg-warning/10 p-4 text-sm">
          <AlertTriangle className="h-5 w-5 shrink-0 text-warning" />
          <div>
            <span className="font-medium">Recovery has never been tested.</span> An heir who has never rehearsed
            recovery is likely to fail under stress. Run a supervised drill, then mark it tested below.
          </div>
        </div>
      )}

      <ExportCard includeArchivedDefault={showArchived} />

      <ImportCard />

      <WalletsSection showArchived={showArchived} onShowArchived={setShowArchived} />

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        <ChecklistCard />
        <ReminderCard />
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        <Card>
          <CardHeader className="flex-row items-center justify-between space-y-0">
            <div>
              <CardTitle>Recovery contacts</CardTitle>
              <CardDescription>People who can help execute your recovery plan.</CardDescription>
            </div>
            <Button size="sm" onClick={() => setAddOpen(true)}>
              <Plus className="h-4 w-4" /> Add
            </Button>
          </CardHeader>
          <CardContent className="space-y-2">
            {(config?.contacts ?? []).length === 0 && <p className="text-sm text-muted-foreground">No contacts added yet.</p>}
            {(config?.contacts ?? []).map((c) => (
              <div key={c.id} className="flex items-center justify-between rounded-md border border-border p-3">
                <div className="flex items-center gap-2">
                  <Users className="h-4 w-4 text-muted-foreground" />
                  <div>
                    <div className="text-sm font-medium">{c.name}</div>
                    <div className="text-xs text-muted-foreground">{c.relationship || "—"}</div>
                  </div>
                </div>
                {c.has_key_share && (
                  <Badge variant="outline">
                    <KeyRound className="mr-1 h-3 w-3" /> holds key share
                  </Badge>
                )}
              </div>
            ))}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Recovery testing</CardTitle>
            <CardDescription>Status: {config?.test_status.replace("_", " ") ?? "—"}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            <ul className="space-y-2 text-sm">
              <li className="flex items-center gap-2">
                <CheckCircle2 className={config?.contacts.length ? "h-4 w-4 text-success" : "h-4 w-4 text-muted-foreground"} />
                At least one recovery contact designated
              </li>
              <li className="flex items-center gap-2">
                <CheckCircle2 className={config?.has_instructions ? "h-4 w-4 text-success" : "h-4 w-4 text-muted-foreground"} />
                Written recovery instructions saved
              </li>
              <li className="flex items-center gap-2">
                <CheckCircle2 className={config?.test_status === "tested_pass" ? "h-4 w-4 text-success" : "h-4 w-4 text-muted-foreground"} />
                Full recovery drill completed successfully
              </li>
            </ul>
            <div className="flex gap-2 pt-2">
              <Button size="sm" variant="outline" onClick={() => markTestedMutation.mutate(true)}>
                Mark drill passed
              </Button>
              <Button size="sm" variant="ghost" onClick={() => markTestedMutation.mutate(false)}>
                Mark drill failed
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Letter / notes (encrypted)</CardTitle>
          <CardDescription>
            Write down where seeds/devices are stored, how many signatures are needed, and any passphrase hints
            (never the passphrase itself). Encrypted with your vault key.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <textarea
            value={instructions}
            onChange={(e) => setInstructions(e.target.value)}
            rows={8}
            className="w-full rounded-md border border-border bg-secondary/40 p-3 text-sm outline-hidden focus-visible:ring-1 focus-visible:ring-[hsl(var(--ring))]"
            placeholder="1. Trezor #1 is in the home safe...&#10;2. Trezor #2 is with [contact] in a bank deposit box...&#10;3. This is a 2-of-3 multisig — you need any 2 of the 3 devices..."
          />
          <Button onClick={() => saveInstructionsMutation.mutate()} disabled={saveInstructionsMutation.isPending}>
            Save instructions
          </Button>
        </CardContent>
      </Card>

      <Dialog open={addOpen} onOpenChange={setAddOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Add recovery contact</DialogTitle>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-1.5">
              <Label>Name</Label>
              <Input value={name} onChange={(e) => setName(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label>Relationship</Label>
              <Input value={relationship} onChange={(e) => setRelationship(e.target.value)} placeholder="e.g. spouse, attorney" />
            </div>
            <div className="space-y-1.5">
              <Label>Contact method</Label>
              <Input value={contactMethod} onChange={(e) => setContactMethod(e.target.value)} placeholder="email or phone" />
            </div>
            <label className="flex items-center gap-2 text-sm">
              <Checkbox checked={hasKeyShare} onCheckedChange={(v) => setHasKeyShare(v === true)} />
              Holds a key share / hardware device
            </label>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setAddOpen(false)}>
              Cancel
            </Button>
            <Button onClick={() => addContactMutation.mutate()} disabled={!name || addContactMutation.isPending}>
              Add contact
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
