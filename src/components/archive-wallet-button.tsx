import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Archive, ArchiveRestore, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from "@/components/ui/dialog";
import * as api from "@/bindings/commands";
import type { Wallet } from "@/bindings/types";

/// Archive is the only removal path Nakos exposes from the UI — there is no
/// delete. Archiving flips `wallets.archived`, which the backend already
/// excludes from `list_wallets` (default), `portfolio_summary`, and
/// `list_utxos`/`utxo_distribution` (default) — nothing is ever deleted
/// from the database, so unarchiving brings everything straight back.
export function ArchiveWalletButton({ wallet, size = "sm" }: { wallet: Wallet; size?: "sm" | "default" | "icon" }) {
  const queryClient = useQueryClient();
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const isArchived = wallet.archived;

  const mutation = useMutation({
    mutationFn: () => api.archiveWallet(wallet.id, !isArchived),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["wallets"] });
      queryClient.invalidateQueries({ queryKey: ["portfolio-summary"] });
      queryClient.invalidateQueries({ queryKey: ["utxos"] });
      queryClient.invalidateQueries({ queryKey: ["utxo-distribution"] });
      setConfirmOpen(false);
      setError(null);
    },
    onError: (e) => setError(String(e)),
  });

  return (
    <>
      <Button
        size={size}
        variant="ghost"
        onClick={(e) => {
          e.stopPropagation();
          setError(null);
          setConfirmOpen(true);
        }}
        title={isArchived ? "Unarchive wallet" : "Archive wallet"}
      >
        {isArchived ? <ArchiveRestore className="h-3.5 w-3.5" /> : <Archive className="h-3.5 w-3.5" />}
        {size !== "icon" && (isArchived ? "Unarchive" : "Archive")}
      </Button>

      <Dialog
        open={confirmOpen}
        onOpenChange={(v) => {
          setConfirmOpen(v);
          if (!v) setError(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {isArchived ? "Unarchive" : "Archive"} &ldquo;{wallet.name}&rdquo;?
            </DialogTitle>
            <DialogDescription>
              {isArchived
                ? "This wallet will reappear in Portfolio and Coins, and count toward your totals again."
                : "This hides the wallet from Portfolio and Coins and drops it from your totals. Nothing is deleted — its UTXOs, transactions, and labels stay in the database, and you can bring it back anytime with the \"Show archived\" toggle."}
            </DialogDescription>
          </DialogHeader>

          {error && <p className="text-sm text-destructive">{error}</p>}

          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmOpen(false)}>
              Cancel
            </Button>
            <Button
              variant={isArchived ? "default" : "destructive"}
              onClick={() => mutation.mutate()}
              disabled={mutation.isPending}
            >
              {mutation.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
              {isArchived ? "Unarchive" : "Archive"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
