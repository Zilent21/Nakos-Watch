import { useVaultStore } from "@/stores/vault-store";

/** Reported by the backend in `vault_status`. */
export type Platform = "android" | "ios" | "desktop";

/** "phone" on Android and iOS; on desktop, the word the screen already used. */
export function useDeviceWord(desktopWord: "computer" | "machine" = "computer"): string {
  const platform = useVaultStore((s) => s.platform);
  return platform === "desktop" ? desktopWord : "phone";
}

/** The OS name from `vault_status`, for the words in `os-text.ts`. */
export function useOs(): string {
  return useVaultStore((s) => s.os);
}

export function useIsMobile(): boolean {
  return useVaultStore((s) => s.platform) !== "desktop";
}

/** When the vault key leaves memory. Said where the passphrase is entered and next to the lock
 * button. The timeout itself is chosen in Settings (default 10 min on desktop, 5 on a phone). */
export function useStaysUnlockedNote(): string {
  return useIsMobile()
    ? "Nakos locks 30 seconds after it leaves the screen, after a few minutes without use, or when you tap the lock at the top."
    : "Nakos locks after a period of inactivity (10 minutes unless you change it in Settings), when you click Lock, or when you quit. Locking also forgets a remembered passphrase.";
}
