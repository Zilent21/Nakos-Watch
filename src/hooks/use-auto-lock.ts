import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import * as api from "@/bindings/commands";
import { useVaultStore } from "@/stores/vault-store";
import { toast } from "@/hooks/use-toast";

/** Activity is reported at most this often; the Rust watchdog owns the clock. */
const TOUCH_EVERY_MS = 10_000;
const ACTIVITY_EVENTS = ["pointerdown", "keydown", "wheel", "touchstart", "mousemove"] as const;

/**
 * Auto-lock, webview side. Rust decides when to lock (see `commands/autolock.rs`); this only reports
 * activity and visibility, and reacts to the lock: it drops every cached query so balances, labels
 * and notes leave the webview's memory too, then shows the unlock screen.
 */
export function useAutoLock(unlocked: boolean) {
  const queryClient = useQueryClient();
  const refresh = useVaultStore((s) => s.refresh);

  useEffect(() => {
    let disposed = false;
    const off = listen<string>("vault-locked", async (e) => {
      queryClient.clear();
      await refresh();
      if (disposed) return;
      toast({
        title: "Nakos locked",
        description: e.payload === "background" ? "It locks shortly after it leaves the screen." : "It locked after a period of inactivity. You can change this in Settings.",
      });
    });
    return () => {
      disposed = true;
      void off.then((f) => f());
    };
  }, [queryClient, refresh]);

  useEffect(() => {
    if (!unlocked) return;
    let last = 0;
    const onActivity = () => {
      const now = Date.now();
      if (now - last < TOUCH_EVERY_MS) return;
      last = now;
      void api.vaultTouch().catch(() => {});
    };
    const onVisibility = () => {
      void api.vaultAppHidden(document.visibilityState === "hidden").catch(() => {});
    };
    for (const ev of ACTIVITY_EVENTS) window.addEventListener(ev, onActivity, { passive: true });
    document.addEventListener("visibilitychange", onVisibility);
    onActivity();
    return () => {
      for (const ev of ACTIVITY_EVENTS) window.removeEventListener(ev, onActivity);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [unlocked]);
}
