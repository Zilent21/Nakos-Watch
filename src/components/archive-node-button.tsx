import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Archive, ArchiveRestore, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from "@/components/ui/dialog";
import * as api from "@/bindings/commands";
import type { NodeConfig } from "@/bindings/types";

/// Same idea as archiving a wallet: nothing is deleted. An archived connection is stored as
/// `enabled = 0`, which status, fee estimates, the health score and sync already ignore, and it
/// can no longer be the primary. Unarchiving brings it back as an ordinary connection; it does
/// not take over as primary.
export function ArchiveNodeButton({ node }: { node: NodeConfig }) {
  const queryClient = useQueryClient();
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const isArchived = !node.enabled;

  const mutation = useMutation({
    mutationFn: () => api.archiveNodeConfig(node.id, !isArchived),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["node-configs"] });
      queryClient.invalidateQueries({ queryKey: ["node-status"] });
      queryClient.invalidateQueries({ queryKey: ["fee-estimates"] });
      setConfirmOpen(false);
      setError(null);
    },
    onError: (e) => setError(String(e)),
  });

  return (
    <>
      <Button
        size="sm"
        variant="ghost"
        onClick={(e) => {
          e.stopPropagation();
          setError(null);
          setConfirmOpen(true);
        }}
        title={isArchived ? "Unarchive connection" : "Archive connection"}
      >
        {isArchived ? <ArchiveRestore className="h-3.5 w-3.5" /> : <Archive className="h-3.5 w-3.5" />}
        {isArchived ? "Unarchive" : "Archive"}
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
              {isArchived ? "Unarchive" : "Archive"} &ldquo;{node.label}&rdquo;?
            </DialogTitle>
            <DialogDescription>
              {isArchived
                ? "This connection will appear in the list again and can be tested and chosen as primary. It will not become primary on its own."
                : "Nakos will stop using this connection for status, fee estimates, the health score and sync, and it can no longer be the primary. Nothing is deleted, and you can bring it back anytime with the \"Show archived\" switch."}
            </DialogDescription>
          </DialogHeader>

          {error && <p className="text-sm text-destructive">{error}</p>}

          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmOpen(false)}>
              Cancel
            </Button>
            <Button variant={isArchived ? "default" : "destructive"} onClick={() => mutation.mutate()} disabled={mutation.isPending}>
              {mutation.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
              {isArchived ? "Unarchive" : "Archive"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
