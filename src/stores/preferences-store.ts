import { create } from "zustand";
import * as api from "@/bindings/commands";
import { PHONE_TABS_PREF, parsePhoneTabs } from "@/lib/phone-tabs";

interface PreferencesState {
  fiatCurrency: string;
  fiatPricePerBtc: number | null;
  priceSource: "manual" | "last_synced_node";
  displayUnit: "sats" | "btc";
  /** The four route paths in the phone's bottom bar. */
  phoneTabs: string[];
  /** Receive: at or over this many sats an address must be checked on the device first. 0 = every address. */
  receiveCheckSats: number;
  /** Check for payments every N minutes while Nakos is open; 0 = off. */
  autoCheckMinutes: number;
  loaded: boolean;
  load: () => Promise<void>;
  setFiatCurrency: (currency: string) => Promise<void>;
  setFiatPrice: (price: number) => Promise<void>;
  setDisplayUnit: (unit: "sats" | "btc") => Promise<void>;
  setPhoneTabs: (tabs: string[]) => Promise<void>;
  setReceiveCheckSats: (sats: number) => Promise<void>;
  setAutoCheckMinutes: (minutes: number) => Promise<void>;
}

export const DEFAULT_RECEIVE_CHECK_SATS = 500_000;
export const AUTO_CHECK_CHOICES = [0, 15, 30, 60];

/** A stored threshold, or the default when it is missing or not a whole number of sats. */
export function parseReceiveCheckSats(raw: string | undefined): number {
  if (raw === undefined || !/^[0-9]{1,16}$/.test(raw)) return DEFAULT_RECEIVE_CHECK_SATS;
  return Number(raw);
}

export function parseAutoCheckMinutes(raw: string | undefined): number {
  const n = Number(raw);
  return AUTO_CHECK_CHOICES.includes(n) ? n : 0;
}

export const usePreferencesStore = create<PreferencesState>((set) => ({
  fiatCurrency: "USD",
  fiatPricePerBtc: null,
  priceSource: "manual",
  // BTC unless sats was chosen: every screen showed BTC before the setting was wired up.
  displayUnit: "btc",
  phoneTabs: parsePhoneTabs(null),
  receiveCheckSats: DEFAULT_RECEIVE_CHECK_SATS,
  autoCheckMinutes: 0,
  loaded: false,

  load: async () => {
    const all = await api.getAllPreferences();
    const map = Object.fromEntries(all);
    set({
      fiatCurrency: map.fiat_currency ?? "USD",
      fiatPricePerBtc: map.fiat_price_per_btc ? Number(map.fiat_price_per_btc) : null,
      displayUnit: map.display_unit === "sats" ? "sats" : "btc",
      phoneTabs: parsePhoneTabs(map[PHONE_TABS_PREF]),
      receiveCheckSats: parseReceiveCheckSats(map.receive_check_sats),
      autoCheckMinutes: parseAutoCheckMinutes(map.auto_check_minutes),
      loaded: true,
    });
  },

  setFiatCurrency: async (currency) => {
    await api.setPreference("fiat_currency", currency);
    set({ fiatCurrency: currency });
  },

  setFiatPrice: async (price) => {
    await api.setPreference("fiat_price_per_btc", String(price));
    set({ fiatPricePerBtc: price });
  },

  setDisplayUnit: async (unit) => {
    await api.setPreference("display_unit", unit);
    set({ displayUnit: unit });
  },

  setPhoneTabs: async (tabs) => {
    // Stored as given only if it is a valid choice; parse it back so the bar shows what was saved.
    const saved = JSON.stringify(parsePhoneTabs(JSON.stringify(tabs)));
    await api.setPreference(PHONE_TABS_PREF, saved);
    set({ phoneTabs: parsePhoneTabs(saved) });
  },

  setReceiveCheckSats: async (sats) => {
    const v = Math.max(0, Math.floor(sats));
    await api.setPreference("receive_check_sats", String(v));
    set({ receiveCheckSats: v });
  },

  setAutoCheckMinutes: async (minutes) => {
    const v = AUTO_CHECK_CHOICES.includes(minutes) ? minutes : 0;
    await api.setPreference("auto_check_minutes", String(v));
    set({ autoCheckMinutes: v });
  },
}));
