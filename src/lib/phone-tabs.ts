// Which screens sit in the phone's bottom bar. Pure functions on route paths, no imports, so
// `node --test` can run the tests without a bundler (src/lib/phone-tabs.test.ts).

/** Preference key; the value is a JSON array of four route paths. */
export const PHONE_TABS_PREF = "phone_tabs";

/** Screens a phone may show, in menu order. Map is left out on phones (mobile v1). */
export const PHONE_SCREENS = ["/", "/portfolio", "/health", "/security", "/privacy", "/inheritance", "/utxos", "/spend", "/node", "/settings"];

export const TAB_COUNT = 4;

export const DEFAULT_PHONE_TABS = ["/", "/portfolio", "/utxos", "/node"];

/** The saved choice, or the default if it is missing or not exactly four different known screens. */
export function parsePhoneTabs(saved: string | null | undefined): string[] {
  if (!saved) return [...DEFAULT_PHONE_TABS];
  let value: unknown;
  try {
    value = JSON.parse(saved);
  } catch {
    return [...DEFAULT_PHONE_TABS];
  }
  const ok =
    Array.isArray(value) &&
    value.length === TAB_COUNT &&
    value.every((v) => typeof v === "string" && PHONE_SCREENS.includes(v)) &&
    new Set(value).size === TAB_COUNT;
  return ok ? [...(value as string[])] : [...DEFAULT_PHONE_TABS];
}

/** Put `screen` in `slot`. If it is already in another slot, the two swap, so no screen repeats. */
export function chooseTab(tabs: string[], slot: number, screen: string): string[] {
  if (slot < 0 || slot >= tabs.length || !PHONE_SCREENS.includes(screen)) return [...tabs];
  const next = [...tabs];
  const from = next.indexOf(screen);
  if (from !== -1) next[from] = next[slot];
  next[slot] = screen;
  return next;
}

/** The screens that are not tabs, in menu order: the "More" list. */
export function moreScreens(tabs: string[]): string[] {
  return PHONE_SCREENS.filter((s) => !tabs.includes(s));
}
