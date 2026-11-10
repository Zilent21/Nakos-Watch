import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, ClipboardList, ListPlus, Loader2, Plus, RefreshCw, Trash2 } from "lucide-react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { EmptyState } from "@/components/ui/empty-state";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { handlePublicServerGate } from "@/components/public-server-gate";
import * as api from "@/bindings/commands";
import type { AddressList, SavedAddress } from "@/bindings/types";
import { toast } from "@/hooks/use-toast";
import { formatRelativeTime } from "@/lib/format";
import { useFormatAmount } from "@/lib/use-amount";
import { PayPanel, textareaClass } from "./shared";

const MAX_UNUSED = 20;

function useRefresh() {
  const qc = useQueryClient();
  return () => {
    qc.invalidateQueries({ queryKey: ["address-lists"] });
    qc.invalidateQueries({ queryKey: ["alerts"] });
  };
}

function AddDialog({ list, open, onOpenChange }: { list: AddressList; open: boolean; onOpenChange: (o: boolean) => void }) {
  const refresh = useRefresh();
  const [text, setText] = useState("");
  const add = useMutation({
    mutationFn: () => api.addSavedAddresses(list.id, text),
    onSuccess: (r) => {
      refresh();
      if (r.skipped.length === 0) {
        toast({ title: `${r.added} address${r.added === 1 ? "" : "es"} added`, variant: "success" });
        setText("");
        onOpenChange(false);
      } else {
        toast({
          title: `${r.added} added, ${r.skipped.length} skipped`,
          description: r.skipped.slice(0, 4).map((s) => `${s.text.slice(0, 18)}…: ${s.reason}`).join("\n"),
          variant: r.added ? "default" : "destructive",
        });
        if (r.added) setText(r.skipped.map((s) => s.text).join("\n"));
      }
    },
    onError: (e) => toast({ title: "Nothing added", description: String(e), variant: "destructive" }),
  });
  const unused = list.fresh + list.given_out;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add addresses to {list.name}</DialogTitle>
          <DialogDescription>
            Copy receiving addresses from that wallet's Receive screen and paste them here, one per line. Room for {Math.max(0, MAX_UNUSED - unused)} more.
          </DialogDescription>
        </DialogHeader>
        <textarea
          className={textareaClass}
          rows={8}
          value={text}
          spellCheck={false}
          autoComplete="off"
          placeholder={"bc1q…\nbc1q…"}
          onChange={(e) => setText(e.target.value)}
        />
        <p className="text-xs text-muted-foreground">
          Addresses only. Nakos refuses a seed phrase or a private key, and never stores either. Paste straight from the wallet app and check that
          the first and last characters match: clipboard malware swaps addresses.
        </p>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button onClick={() => add.mutate()} disabled={!text.trim() || add.isPending}>
            {add.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
            Add
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function AddressRow({ a, onShow }: { a: SavedAddress; onShow: () => void }) {
  const refresh = useRefresh();
  const amount = useFormatAmount();
  const [confirm, setConfirm] = useState(false);
  const del = useMutation({
    mutationFn: () => api.deleteSavedAddress(a.id),
    onSuccess: refresh,
    onError: (e) => toast({ title: "Couldn't delete", description: String(e), variant: "destructive" }),
  });
  const badge =
    a.state === "used" ? (
      <Badge variant="success">{a.received_sats ? amount(a.received_sats) : "Used"}</Badge>
    ) : a.state === "given_out" ? (
      <Badge variant="warning">Given out</Badge>
    ) : (
      <Badge variant="muted">Fresh</Badge>
    );
  return (
    <div className="flex flex-wrap items-center justify-between gap-2 px-6 py-2.5 text-sm">
      <button type="button" className="min-w-0 text-left hover:underline" onClick={onShow}>
        <span className="font-mono">{a.address.slice(0, 12)}…{a.address.slice(-6)}</span>
        {a.label && <span className="ml-2 text-muted-foreground">{a.label}</span>}
      </button>
      <span className="flex items-center gap-2">
        {badge}
        {a.device_checked_at && <Badge variant="outline">Checked</Badge>}
        {confirm ? (
          <>
            <span className="text-xs text-muted-foreground">Stop watching it?</span>
            <Button size="sm" variant="destructive" onClick={() => del.mutate()} disabled={del.isPending}>Delete</Button>
            <Button size="sm" variant="ghost" onClick={() => setConfirm(false)}>Keep</Button>
          </>
        ) : (
          <Button size="sm" variant="ghost" aria-label={`Delete ${a.address}`} onClick={() => setConfirm(true)}>
            <Trash2 className="h-4 w-4" />
          </Button>
        )}
      </span>
    </div>
  );
}

function ListCard({ list }: { list: AddressList }) {
  const refresh = useRefresh();
  const [adding, setAdding] = useState(false);
  const [shownId, setShownId] = useState<string | null>(null);
  const [settings, setSettings] = useState(false);
  const [name, setName] = useState(list.name);
  const [lowAt, setLowAt] = useState(String(list.low_at));
  const [confirmDelete, setConfirmDelete] = useState(false);

  const next = useMutation({
    mutationFn: () => api.giveOutNextAddress(list.id),
    onSuccess: (a) => {
      setShownId(a.id);
      refresh();
    },
    onError: (e) => toast({ title: "No address", description: String(e), variant: "destructive" }),
  });
  const check = useMutation({
    mutationFn: ({ id, checked }: { id: string; checked: boolean }) => api.setSavedChecked(id, checked),
    onSuccess: refresh,
  });
  const save = useMutation({
    mutationFn: () => api.updateAddressList(list.id, name, Number(lowAt)),
    onSuccess: () => {
      refresh();
      setSettings(false);
    },
    onError: (e) => toast({ title: "Couldn't save", description: String(e), variant: "destructive" }),
  });
  const remove = useMutation({
    mutationFn: () => api.deleteAddressList(list.id),
    onSuccess: refresh,
    onError: (e) => toast({ title: "Couldn't delete", description: String(e), variant: "destructive" }),
  });

  const shown = list.addresses.find((a) => a.id === shownId) ?? null;

  return (
    <Card>
      <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3 space-y-0">
        <div className="space-y-1">
          <CardTitle>{list.name}</CardTitle>
          <CardDescription>
            {list.fresh} fresh · {list.given_out} given out · {list.used} used
            {list.last_checked_at ? ` · checked ${formatRelativeTime(list.last_checked_at)}` : " · not checked yet"}
          </CardDescription>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" onClick={() => next.mutate()} disabled={next.isPending || list.fresh === 0}>
            Use next address
          </Button>
          <Button size="sm" variant="outline" onClick={() => setAdding(true)}>
            <Plus className="h-4 w-4" />
            Add
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setSettings((s) => !s)}>Settings</Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-4 p-0">
        {list.low && (
          <div role="status" className="mx-6 flex items-start gap-2 rounded-md border border-warning/40 bg-warning/10 p-3 text-sm">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-warning" />
            <p>
              {list.fresh === 0 ? "No fresh addresses left." : `Only ${list.fresh} fresh address${list.fresh === 1 ? "" : "es"} left.`} Copy a few more from
              this wallet the next time you open it.
            </p>
          </div>
        )}

        {settings && (
          <div className="mx-6 grid gap-3 rounded-md border border-border p-3 sm:grid-cols-[1fr_auto_auto] sm:items-end">
            <div className="space-y-1">
              <Label htmlFor={`n-${list.id}`}>Name</Label>
              <Input id={`n-${list.id}`} value={name} maxLength={60} onChange={(e) => setName(e.target.value)} />
            </div>
            <div className="space-y-1">
              <Label htmlFor={`l-${list.id}`}>Running low at</Label>
              <Input id={`l-${list.id}`} className="w-24" inputMode="numeric" value={lowAt} onChange={(e) => setLowAt(e.target.value.replace(/\D/g, ""))} />
            </div>
            <Button size="sm" onClick={() => save.mutate()} disabled={save.isPending}>Save</Button>
            <div className="sm:col-span-3">
              {confirmDelete ? (
                <span className="flex flex-wrap items-center gap-2 text-sm">
                  Delete the list and stop watching its {list.addresses.length} addresses?
                  <Button size="sm" variant="destructive" onClick={() => remove.mutate()}>Delete list</Button>
                  <Button size="sm" variant="ghost" onClick={() => setConfirmDelete(false)}>Keep</Button>
                </span>
              ) : (
                <Button size="sm" variant="ghost" className="text-destructive" onClick={() => setConfirmDelete(true)}>
                  <Trash2 className="h-4 w-4" />
                  Delete list
                </Button>
              )}
            </div>
          </div>
        )}

        {shown && (
          <div className="mx-6 rounded-lg border border-border p-4">
            <PayPanel
              address={shown.address}
              amountSats={null}
              label={shown.label}
              checkedAt={shown.device_checked_at}
              where="in your wallet"
              howToCheck={<p>Open {list.name} and find this address in its receive or address list. Compare every character.</p>}
              onChecked={(checked) => check.mutate({ id: shown.id, checked })}
              footer={
                shown.state === "used" ? (
                  <p className="text-sm text-muted-foreground">This address was already used. Use the next one for a new payment.</p>
                ) : (
                  <p className="text-xs text-muted-foreground">Marked as given out, so "Use next address" won't hand it to anyone else.</p>
                )
              }
            />
          </div>
        )}

        {list.addresses.length === 0 ? (
          <p className="px-6 pb-6 text-sm text-muted-foreground">No addresses yet. Add up to {MAX_UNUSED} from this wallet's Receive screen.</p>
        ) : (
          <div className="divide-y divide-border border-t border-border">
            {list.addresses.map((a) => (
              <AddressRow key={a.id} a={a} onShow={() => setShownId(a.id)} />
            ))}
          </div>
        )}
      </CardContent>
      <AddDialog list={list} open={adding} onOpenChange={setAdding} />
    </Card>
  );
}

export function SavedAddresses() {
  const refresh = useRefresh();
  const { data: lists = [], isLoading } = useQuery({ queryKey: ["address-lists"], queryFn: api.listAddressLists });
  const [name, setName] = useState("");

  const create = useMutation({
    mutationFn: () => api.createAddressList(name),
    onSuccess: () => {
      setName("");
      refresh();
    },
    onError: (e) => toast({ title: "Couldn't create the list", description: String(e), variant: "destructive" }),
  });
  const checkAll = useMutation({
    mutationFn: api.checkAddressLists,
    onSuccess: (r) => {
      refresh();
      toast({
        title: r.newly_used ? `${r.newly_used} address${r.newly_used === 1 ? "" : "es"} newly used` : "No new payments",
        description: `Checked ${r.checked} address${r.checked === 1 ? "" : "es"}.`,
        variant: "success",
      });
    },
    onError: (e) => {
      if (handlePublicServerGate(e, () => checkAll.mutate())) return;
      toast({ title: "Check failed", description: String(e), variant: "destructive" });
    },
  });

  const newList = (
    <form
      className="flex flex-wrap items-end gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (name.trim()) create.mutate();
      }}
    >
      <div className="space-y-1">
        <Label htmlFor="new-list">New list</Label>
        <Input id="new-list" className="w-56" value={name} maxLength={60} placeholder="Cake wallet" onChange={(e) => setName(e.target.value)} />
      </div>
      <Button type="submit" variant="outline" disabled={!name.trim() || create.isPending}>
        <ListPlus className="h-4 w-4" />
        Create
      </Button>
    </form>
  );

  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <CardTitle>Addresses from your own wallets</CardTitle>
          <CardDescription>
            Copy up to {MAX_UNUSED} receiving addresses from a hot or cold wallet (Cake, Sparrow, a Jade…) into a list. Hand them out from here, and open
            that wallet again only when the list runs low. Nakos only watches these addresses: it can't send from them.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap items-end justify-between gap-4">
          {newList}
          {lists.length > 0 && (
            <Button variant="outline" onClick={() => checkAll.mutate()} disabled={checkAll.isPending}>
              {checkAll.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}
              Check for payments
            </Button>
          )}
        </CardContent>
      </Card>
      {isLoading ? null : lists.length === 0 ? (
        <Card>
          <EmptyState icon={ClipboardList} title="No lists yet" helper="Create a list named after the wallet the addresses come from." />
        </Card>
      ) : (
        lists.map((l) => <ListCard key={l.id} list={l} />)
      )}
    </div>
  );
}
