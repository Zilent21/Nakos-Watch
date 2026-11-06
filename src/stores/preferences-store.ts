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
  loaded: boolean;
  load: () => Promise<void>;
  setFiatCurrency: (currency: string) => Promise<void>;
  setFiatPrice: (price: number) => Promise<void>;
  setDisplayUnit: (unit: "sats" | "btc") => Promise<void>;
  setPhoneTabs: (tabs: string[]) => Promise<void>;
}

export const usePreferencesStore = create<PreferencesState>((set) => ({
  fiatCurrency: "USD",
  fiatPricePerBtc: null,
  priceSource: "manual",
  // BTC unless sats was chosen: every screen showed BTC before the setting was wired up.
  displayUnit: "btc",
  phoneTabs: parsePhoneTabs(null),
  loaded: false,

  load: async () => {
    const all = await api.getAllPreferences();
    const map = Object.fromEntries(all);
    set({
      fiatCurrency: map.fiat_currency ?? "USD",
      fiatPricePerBtc: map.fiat_price_per_btc ? Number(map.fiat_price_per_btc) : null,
      displayUnit: map.display_unit === "sats" ? "sats" : "btc",
      phoneTabs: parsePhoneTabs(map[PHONE_TABS_PREF]),
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
}));
