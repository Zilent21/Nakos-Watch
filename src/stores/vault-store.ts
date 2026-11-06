import { create } from "zustand";
import * as api from "@/bindings/commands";

interface VaultState {
  initialized: boolean;
  unlocked: boolean;
  keychainRemembered: boolean;
  keychainSupported: boolean;
  platform: "android" | "ios" | "desktop";
  /** "macos", "windows", "linux", ... (see `os-text.ts`). */
  os: string;
  loading: boolean;
  refresh: () => Promise<void>;
  init: (passphrase: string, remember: boolean) => Promise<void>;
  unlock: (passphrase: string, remember: boolean) => Promise<void>;
  /** Locks now. A passphrase remembered in the OS keychain is forgotten too. */
  lock: () => Promise<void>;
  tryKeychainUnlock: () => Promise<boolean>;
}

export const useVaultStore = create<VaultState>((set, get) => ({
  initialized: false,
  unlocked: false,
  keychainRemembered: false,
  keychainSupported: false,
  platform: "desktop",
  os: "macos",
  loading: true,

  refresh: async () => {
    const status = await api.vaultStatus();
    set({
      initialized: status.initialized,
      unlocked: status.unlocked,
      keychainRemembered: status.keychain_remembered,
      keychainSupported: status.keychain_supported,
      platform: status.platform,
      os: status.os,
      loading: false,
    });
  },

  init: async (passphrase, remember) => {
    await api.vaultInit(passphrase, remember);
    await get().refresh();
  },

  unlock: async (passphrase, remember) => {
    await api.vaultUnlock(passphrase, remember);
    await get().refresh();
  },

  lock: async () => {
    // The key is dropped before the keychain is touched, so refresh even when the lock reports a
    // keychain error: the unlock screen must show either way.
    try {
      await api.vaultLock();
    } finally {
      await get().refresh();
    }
  },

  tryKeychainUnlock: async () => {
    const ok = await api.vaultTryKeychainUnlock();
    await get().refresh();
    return ok;
  },
}));
