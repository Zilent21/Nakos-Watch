import { type ReactNode } from "react";
import { Search, Lock } from "lucide-react";
import { Sidebar } from "./sidebar";
import { BottomNav } from "./bottom-nav";
import { NakosMark } from "@/components/brand/nakos-mark";
import { CommandPalette } from "@/components/command-palette/command-palette";
import { AddWalletDialog } from "@/components/add-wallet-dialog";
import { Button } from "@/components/ui/button";
import { useVaultStore } from "@/stores/vault-store";
import { useOs } from "@/lib/platform";
import { paletteShortcut } from "@/lib/os-text";
import { useShellStore } from "@/stores/shell-store";

export function AppShell({ children }: { children: ReactNode }) {
  const lock = useVaultStore((s) => s.lock);
  const os = useOs();
  const setPaletteOpen = useShellStore((s) => s.setPaletteOpen);
  const addWalletOpen = useShellStore((s) => s.addWalletOpen);
  const setAddWalletOpen = useShellStore((s) => s.setAddWalletOpen);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background pt-[env(safe-area-inset-top)]">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center justify-between border-b border-border px-4 md:px-8" data-tauri-drag-region>
          <div className="flex items-center gap-2 md:hidden">
            <NakosMark className="h-6" />
            <span className="text-sm font-semibold tracking-tight">Nakos</span>
          </div>
          <button
            onClick={() => setPaletteOpen(true)}
            className="hidden h-9 w-80 items-center gap-2 rounded-md border border-border bg-secondary/40 px-3 text-sm text-muted-foreground transition-colors hover:bg-secondary/70 focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-[hsl(var(--ring))] md:flex"
          >
            <Search className="h-4 w-4" />
            <span>Search or run a command…</span>
            <kbd className="ml-auto rounded border border-border bg-background px-1.5 py-0.5 font-mono text-xs">{paletteShortcut(os)}</kbd>
          </button>

          <div className="flex items-center gap-1">
            <Button variant="ghost" size="sm" className="md:hidden" onClick={() => setPaletteOpen(true)} aria-label="Search or run a command">
              <Search className="h-4 w-4" />
            </Button>
            <Button variant="ghost" size="sm" onClick={() => lock()} title="Lock vault" aria-label="Lock vault">
              <Lock className="h-4 w-4" />
              <span className="hidden md:inline">Lock</span>
            </Button>
          </div>
        </header>
        <main className="min-h-0 flex-1 overflow-y-auto">
          <div className="mx-auto max-w-[1200px] space-y-6 px-4 py-5 md:space-y-8 md:px-8 md:py-8">{children}</div>
        </main>
        <BottomNav />
      </div>
      <CommandPalette />
      <AddWalletDialog open={addWalletOpen} onOpenChange={setAddWalletOpen} />
    </div>
  );
}
