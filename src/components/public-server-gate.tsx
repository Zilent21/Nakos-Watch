import { useState } from "react";
import { create } from "zustand";
import { ShieldAlert, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from "@/components/ui/dialog";
import * as api from "@/bindings/commands";
import { toast } from "@/hooks/use-toast";

/** Error prefix from Rust (`node::PUBLIC_SERVER_GATE`): sync wants someone else's server and the warning was not accepted yet. */
const GATE = "PUBLIC_SERVER_NOT_ACKNOWLEDGED";

interface Pending {
  host: string;
  port: number;
  retry: () => void;
}

const useGate = create<{ pending: Pending | null; set: (p: Pending | null) => void }>((set) => ({
  pending: null,
  set: (pending) => set({ pending }),
}));

/**
 * Call from a sync's onError. If the error is the privacy gate, show the warning (and retry the
 * sync once the person accepts) and return true so the caller shows no error toast.
 */
export function handlePublicServerGate(error: unknown, retry: () => void): boolean {
  const parts = String(error).split("|");
  if (parts[0] !== GATE || parts.length < 3) return false;
  useGate.getState().set({ host: parts[1], port: Number(parts[2]), retry });
  return true;
}

/** What someone else's Electrum server learns when Nakos syncs through it. Shown before the first sync, and on the Node page. */
export function PublicServerFacts({ host }: { host: string }) {
  return (
    <ul className="list-disc space-y-1.5 pl-5 text-sm">
      <li>
        <span className="font-semibold">{host}</span> is run by someone else. To sync, Nakos asks it about every address in your wallets. For a wallet added from an xpub that is dozens of addresses at a time.
      </li>
      <li>
        <span className="font-semibold">The operator can see all of those addresses, link them together as one person's, and see your IP address and when you sync.</span> They may keep that record.
      </li>
      <li>The server tells Nakos your balance and history. Nakos cannot double-check it.</li>
      <li>It cannot spend your bitcoin. Nakos never has your keys.</li>
      <li>For full privacy, run your own node and Electrum server and connect Nakos to that instead. A VPN hides your IP address, not your addresses.</li>
    </ul>
  );
}

/** Mounted once. Opens when a sync hits the privacy gate. */
export function PublicServerGateDialog() {
  const { pending, set } = useGate();
  const [understood, setUnderstood] = useState(false);
  const [busy, setBusy] = useState(false);

  const close = () => {
    set(null);
    setUnderstood(false);
  };

  const accept = async () => {
    if (!pending) return;
    setBusy(true);
    try {
      await api.acknowledgePublicElectrum(pending.host, pending.port);
      const retry = pending.retry;
      close();
      retry();
    } catch (e) {
      toast({ title: "Couldn't save your choice", description: String(e), variant: "destructive" });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={pending !== null} onOpenChange={(v) => !v && close()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldAlert className="h-5 w-5 text-warning" />
            Sync through a server that isn't yours?
          </DialogTitle>
          <DialogDescription>Read this before the first sync through {pending?.host}. You only need to agree once for this server.</DialogDescription>
        </DialogHeader>
        {pending && <PublicServerFacts host={pending.host} />}
        <label className="flex items-start gap-3 rounded-md border border-border p-3 text-sm">
          <Checkbox id="public-server-understood" checked={understood} onCheckedChange={(v) => setUnderstood(v === true)} className="mt-0.5" />
          <span>I understand that {pending?.host} will see my wallet addresses and my IP address.</span>
        </label>
        <DialogFooter>
          <Button variant="outline" onClick={close}>
            Cancel
          </Button>
          <Button disabled={!understood || busy} onClick={accept}>
            {busy && <Loader2 className="h-4 w-4 animate-spin" />}
            Sync through this server
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
