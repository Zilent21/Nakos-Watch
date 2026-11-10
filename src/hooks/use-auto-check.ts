import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import * as api from "@/bindings/commands";
import { usePreferencesStore } from "@/stores/preferences-store";

/**
 * "Check for payments while Nakos is open": every N minutes, sync each wallet that can sync and
 * check the saved address lists, so alerts appear without anyone pressing Sync. Mounted in the
 * app shell, which only exists while the vault is unlocked. These calls are not user activity, so
 * auto-lock still locks on time (and the next tick then fails quietly and stops nothing).
 *
 * Errors are quiet on purpose: an unreachable server, or a public server whose warning was never
 * accepted, must not throw a dialog at someone every 15 minutes.
 */
export function useAutoCheck() {
  const minutes = usePreferencesStore((s) => s.autoCheckMinutes);
  const qc = useQueryClient();

  useEffect(() => {
    if (!minutes) return;
    let running = false;
    const tick = async () => {
      if (running) return;
      running = true;
      try {
        const wallets = await api.listWallets(false);
        for (const w of wallets.filter((w) => w.source_type !== "manual")) {
          try {
            await api.syncWallet(w.id);
          } catch {
            /* next wallet */
          }
        }
        try {
          const lists = await api.listAddressLists();
          if (lists.some((l) => l.addresses.length > 0)) await api.checkAddressLists();
        } catch {
          /* quiet */
        }
        for (const key of ["alerts", "receive", "address-lists", "wallet-balances", "portfolio-summary", "wallets", "balance-alerts"]) {
          qc.invalidateQueries({ queryKey: [key] });
        }
      } catch {
        /* locked or no database: try again next time */
      } finally {
        running = false;
      }
    };
    const id = window.setInterval(tick, minutes * 60_000);
    return () => window.clearInterval(id);
  }, [minutes, qc]);
}
