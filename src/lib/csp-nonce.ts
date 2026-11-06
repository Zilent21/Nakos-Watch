import { setNonce } from "get-nonce";

/**
 * The nonce Tauri gives the empty `<style id="csp-nonce">` in index.html, fresh on every load. The
 * release CSP allows no other inline style, so a component that adds a `<style>` element (Radix
 * ScrollArea and Select, the dialogs' scroll lock) must carry it. Undefined under `vite dev`, whose
 * CSP (`devCsp`) still allows inline styles.
 */
export const styleNonce: string | undefined = (document.getElementById("csp-nonce") as HTMLStyleElement | null)?.nonce || undefined;

// react-remove-scroll-bar (under Radix Dialog, Select and DropdownMenu) reads it from here.
if (styleNonce) setNonce(styleNonce);
