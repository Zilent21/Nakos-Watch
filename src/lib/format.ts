const SATS_PER_BTC = 100_000_000;

export function satsToBtc(sats: number): number {
  return sats / SATS_PER_BTC;
}

export type DisplayUnit = "sats" | "btc";

/** An amount with its unit, as chosen in Settings, Display unit: "1,234,567 sats" or
 * "0.01234567 BTC". `btcDecimals` only shortens BTC (charts and badges use 4). */
export function formatAmount(sats: number, unit: DisplayUnit, btcDecimals = 8): string {
  if (unit === "sats") return `${sats.toLocaleString("en-US")} sats`;
  return `${formatBtc(sats, btcDecimals)} BTC`;
}

export function formatBtc(sats: number, decimals = 8): string {
  return satsToBtc(sats).toFixed(decimals);
}

export function formatFiat(amount: number, currency = "USD"): string {
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency,
    maximumFractionDigits: amount >= 1000 ? 0 : 2,
  }).format(amount);
}

export function satsToFiat(sats: number, priceFiatPerBtc: number): number {
  return satsToBtc(sats) * priceFiatPerBtc;
}

export function formatRelativeTime(iso: string | null | undefined): string {
  if (!iso) return "never";
  const date = new Date(iso);
  const diffMs = Date.now() - date.getTime();
  const diffSec = Math.round(diffMs / 1000);
  if (diffSec < 5) return "just now";
  if (diffSec < 60) return `${diffSec}s ago`;
  const diffMin = Math.round(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.round(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  const diffDay = Math.round(diffHr / 24);
  if (diffDay < 30) return `${diffDay}d ago`;
  return date.toLocaleDateString();
}

export function formatAddress(address: string, chars = 6): string {
  if (address.length <= chars * 2 + 3) return address;
  return `${address.slice(0, chars)}…${address.slice(-chars)}`;
}

export function formatTxid(txid: string): string {
  return formatAddress(txid, 8);
}
