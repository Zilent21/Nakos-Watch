import { create } from "zustand";

/** Shell-level UI state that more than one screen needs to drive. */
interface ShellState {
  paletteOpen: boolean;
  setPaletteOpen: (open: boolean) => void;
  addWalletOpen: boolean;
  setAddWalletOpen: (open: boolean) => void;
}

export const useShellStore = create<ShellState>((set) => ({
  paletteOpen: false,
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
  addWalletOpen: false,
  setAddWalletOpen: (addWalletOpen) => set({ addWalletOpen }),
}));
