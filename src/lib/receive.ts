/** Receiving helpers shared by the Receive screen and Settings. Pure, so they run under node --test. */

const SATS_PER_BTC = 100_000_000;

/** 250000 → "0.0025". Exact: integer maths, no floating point. */
export function satsToBtcString(sats: number): string {
  const whole = Math.floor(sats / SATS_PER_BTC);
  const frac = String(sats % SATS_PER_BTC).padStart(8, "0").replace(/0+$/, "");
  return frac ? `${whole}.${frac}` : String(whole);
}

/** A BIP21 payment link. The address goes in as given; amount and label are optional. */
export function bip21(address: string, amountSats?: number | null, label?: string | null): string {
  const q: string[] = [];
  if (amountSats && amountSats > 0) q.push(`amount=${satsToBtcString(amountSats)}`);
  if (label && label.trim()) q.push(`label=${encodeURIComponent(label.trim())}`);
  return `bitcoin:${address}${q.length ? `?${q.join("&")}` : ""}`;
}

/** "50,000" or "50 000" → 50000; anything that is not a positive whole number of sats → null. */
export function parseSats(text: string): number | null {
  const t = text.replace(/[\s,_]/g, "");
  if (!/^[0-9]{1,16}$/.test(t)) return null;
  const n = Number(t);
  return n > 0 && n <= 2_100_000_000_000_000 ? n : null;
}

/**
 * Whether the QR stays covered until the address is checked on the device: the amount is at or over
 * the threshold (0 means every address) and the address has not been checked. With no amount the
 * code shows, and the screen asks to check first if a large deposit is coming.
 */
export function mustCheckFirst(amountSats: number | null, thresholdSats: number, checked: boolean): boolean {
  if (checked) return false;
  if (thresholdSats === 0) return true;
  return amountSats !== null && amountSats >= thresholdSats;
}

/** Threshold choices in Settings, in sats. 0 = every address. */
export const CHECK_THRESHOLD_CHOICES = [100_000, 250_000, 500_000, 1_000_000, 5_000_000, 0];
