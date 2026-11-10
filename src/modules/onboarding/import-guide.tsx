import { useState } from "react";
import { BookOpen, CheckCircle2, Circle, ShieldAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import * as api from "@/bindings/commands";
import { toast } from "@/hooks/use-toast";

const CHECKLIST = [
  "I'm copying from my wallet's export or settings screen, not from my seed backup.",
  "What I copied starts with xpub, ypub, zpub, Zpub or tpub, or with wpkh( / wsh( / tr( / sh(, or it is an address.",
  "It does not contain \"prv\" and it is not a list of 12 or 24 words.",
  "I know which wallet it came from, so I can name it here.",
  "If it's a multisig, I have the descriptor (or every cosigner's xpub, fingerprint and path).",
];

function openHelp() {
  api.openProjectLink("help_import").catch(() => toast({ title: "Open this in your browser", description: "nakos.org/help/import" }));
}

/** Where to find the watch-only key in common wallets, why descriptors are better, and how to be
 * sure the thing being pasted is not a secret. Shown on first run, in empty states, and from Add wallet. */
export function ImportGuide() {
  const [ticked, setTicked] = useState<boolean[]>(() => CHECKLIST.map(() => false));
  return (
    <div className="space-y-5 text-sm">
      <Tabs defaultValue="sparrow">
        <TabsList>
          <TabsTrigger value="sparrow">Sparrow</TabsTrigger>
          <TabsTrigger value="electrum">Electrum</TabsTrigger>
          <TabsTrigger value="hardware">Hardware wallet</TabsTrigger>
        </TabsList>
        <TabsContent value="sparrow" className="space-y-2 pt-3">
          <ol className="list-decimal space-y-1 pl-5 text-muted-foreground">
            <li>Open the wallet in Sparrow and go to its <b className="text-foreground">Settings</b> tab.</li>
            <li>
              Copy the <b className="text-foreground">Output Descriptor</b> shown there (or use <b className="text-foreground">Export…</b> and choose Output Descriptor).
            </li>
            <li>Paste it into Nakos under Add wallet. For a multisig, the one descriptor covers every cosigner.</li>
          </ol>
        </TabsContent>
        <TabsContent value="electrum" className="space-y-2 pt-3">
          <ol className="list-decimal space-y-1 pl-5 text-muted-foreground">
            <li>
              Open the wallet in Electrum and choose <b className="text-foreground">Wallet → Information</b>.
            </li>
            <li>
              Copy the <b className="text-foreground">Master Public Key</b> (it starts with zpub for a native segwit wallet).
            </li>
            <li>Paste it into Nakos. If Electrum shows the derivation path and fingerprint, add them too; they help with signing later.</li>
          </ol>
          <p className="text-xs text-muted-foreground">Never use Electrum's "Show seed" or "Export private keys" for this.</p>
        </TabsContent>
        <TabsContent value="hardware" className="space-y-2 pt-3">
          <p className="text-muted-foreground">
            The simplest way is to connect the hardware wallet to Sparrow (or the maker's own app), let it read the public key, and copy the descriptor from there as in the Sparrow tab.
            With a Trezor, open Trezor Suite, pick the account, and choose <b className="text-foreground">Details → Show public key</b> (confirm on the device). Copy that xpub or zpub.
          </p>
          <p className="text-xs text-muted-foreground">The device never shows its seed for this. If a screen asks you to type or reveal your seed, stop: that's not the export.</p>
        </TabsContent>
      </Tabs>

      <div className="space-y-1">
        <p className="font-medium">Why a descriptor is better than an xpub</p>
        <p className="text-muted-foreground">
          An xpub alone doesn't say which address type or path it uses, so a tool may have to guess and can show the wrong addresses. A descriptor says it all: the script type, the key,
          its fingerprint and path, and for a multisig every cosigner and the threshold. It is still public: it can watch coins but never spend them.
        </p>
      </div>

      <div role="note" className="flex gap-3 rounded-md border border-destructive/50 bg-destructive/10 p-3">
        <ShieldAlert className="mt-0.5 h-5 w-5 shrink-0 text-destructive" />
        <div className="space-y-1">
          <p className="font-medium">How to know it isn't a secret</p>
          <p className="text-muted-foreground">
            Public: starts with <code>xpub</code>, <code>ypub</code>, <code>zpub</code>, <code>Zpub</code> or <code>tpub</code>, or is a descriptor like <code>wpkh([…]xpub…/0/*)</code>, or an address
            like <code>bc1q…</code>. Secret, never paste: anything with <code>prv</code> in it (xprv, zprv), a list of 12 or 24 words, or a 51–52 character key starting with 5, K or L. Nakos refuses those, but the
            safest secret is one that is never typed anywhere.
          </p>
        </div>
      </div>

      <div className="space-y-2">
        <p className="font-medium">Import checklist</p>
        <ul className="space-y-1.5">
          {CHECKLIST.map((item, i) => (
            <li key={i}>
              <button
                type="button"
                className="flex w-full items-start gap-2 text-left text-muted-foreground hover:text-foreground"
                aria-pressed={ticked[i]}
                onClick={() => setTicked((t) => t.map((v, j) => (j === i ? !v : v)))}
              >
                {ticked[i] ? <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-primary" /> : <Circle className="mt-0.5 h-4 w-4 shrink-0" />}
                <span>{item}</span>
              </button>
            </li>
          ))}
        </ul>
      </div>

      <Button type="button" variant="link" className="h-auto p-0 text-xs" onClick={openHelp}>
        More help at nakos.org/help/import
      </Button>
    </div>
  );
}

export function ImportGuideDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>How to export a watch-only key</DialogTitle>
          <DialogDescription>Nakos needs only the public side of your wallet. Here's where to find it, and how to be sure it's not a secret.</DialogDescription>
        </DialogHeader>
        <ImportGuide />
      </DialogContent>
    </Dialog>
  );
}

/** The first-run / empty-state card: what to do first, with the guide inline. */
export function GettingStartedCard({ onAddWallet }: { onAddWallet: () => void }) {
  return (
    <Card data-testid="getting-started">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <BookOpen className="h-5 w-5" /> Add your first wallet
        </CardTitle>
        <CardDescription>
          Nakos watches your bitcoin from its public keys. Export a descriptor (best) or an xpub from the wallet you already use, paste it under Add wallet, and Nakos shows your coins.
          Then connect a node on the Node page, or use a public server.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-5">
        <Button onClick={onAddWallet}>Add wallet</Button>
        <ImportGuide />
      </CardContent>
    </Card>
  );
}
