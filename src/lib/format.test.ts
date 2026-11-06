// Run with `npm run test:ui` (node --test; Node strips the types itself).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { formatAmount, formatBtc } from "./format.ts";

test("sats are whole numbers with thousands separators", () => {
  assert.equal(formatAmount(1234567, "sats"), "1,234,567 sats");
  assert.equal(formatAmount(0, "sats"), "0 sats");
  assert.equal(formatAmount(546, "sats", 4), "546 sats", "btcDecimals does not change sats");
  assert.equal(formatAmount(-2100, "sats"), "-2,100 sats");
});

test("BTC has eight decimals unless a screen asks for fewer", () => {
  assert.equal(formatAmount(1234567, "btc"), "0.01234567 BTC");
  assert.equal(formatAmount(1234567, "btc", 4), "0.0123 BTC");
  assert.equal(formatAmount(100_000_000, "btc"), "1.00000000 BTC");
  assert.equal(formatBtc(1234567), "0.01234567");
});

/** Every file under `dir` with one of the extensions. */
function files(dir: string, exts: string[]): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return files(path, exts);
    return exts.some((e) => name.endsWith(e)) ? [path] : [];
  });
}

test("screens show amounts only through the Display unit setting", () => {
  const src = join(import.meta.dirname, "..");
  const offenders = files(src, [".ts", ".tsx"])
    .filter((f) => !f.endsWith(join("lib", "format.ts")) && !f.endsWith(".test.ts"))
    .filter((f) => /\bformatBtc\(|[\d}] BTC\b|["'`] BTC\b/.test(readFileSync(f, "utf8")));
  assert.deepEqual(offenders, [], "use useFormatAmount() instead of formatBtc or a hard-coded BTC");
});
