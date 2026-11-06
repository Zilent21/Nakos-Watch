import type { ReactNode } from "react";
import { NakosMark } from "@/components/brand/nakos-mark";

/** Shared frame for the create-vault and unlock screens: mark, title, one card, one footer line. */
export function AuthFrame({ title, subtitle, children, footer }: { title: string; subtitle: string; children: ReactNode; footer: ReactNode }) {
  return (
    <div className="flex h-screen w-screen items-center justify-center overflow-y-auto bg-background px-6 pb-[env(safe-area-inset-bottom)] pt-[env(safe-area-inset-top)]" data-tauri-drag-region>
      <div className="w-full max-w-sm space-y-8">
        <div className="space-y-4 text-center">
          <NakosMark className="mx-auto h-14" />
          <div className="space-y-2">
            <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
            <p className="text-sm text-muted-foreground">{subtitle}</p>
          </div>
        </div>
        <div className="rounded-xl border border-border bg-card p-6">{children}</div>
        <div className="space-y-2 text-center text-xs text-muted-foreground">{footer}</div>
      </div>
    </div>
  );
}
