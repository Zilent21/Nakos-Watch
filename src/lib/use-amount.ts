import { useCallback } from "react";
import { usePreferencesStore } from "@/stores/preferences-store";
import { formatAmount } from "@/lib/format";

/** Format sats in the unit chosen in Settings. Screens use this, never formatBtc directly
 * (src/lib/format.test.ts checks). The function only changes when the unit does. */
export function useFormatAmount(): (sats: number, btcDecimals?: number) => string {
  const unit = usePreferencesStore((s) => s.displayUnit);
  return useCallback((sats: number, btcDecimals?: number) => formatAmount(sats, unit, btcDecimals), [unit]);
}
