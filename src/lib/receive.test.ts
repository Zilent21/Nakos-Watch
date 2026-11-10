import { test } from "node:test";
import assert from "node:assert/strict";
import { bip21, mustCheckFirst, parseSats, satsToBtcString } from "./receive.ts";

test("sats become an exact BTC string", () => {
  assert.equal(satsToBtcString(250_000), "0.0025");
  assert.equal(satsToBtcString(100_000_000), "1");
  assert.equal(satsToBtcString(1), "0.00000001");
  assert.equal(satsToBtcString(2_100_000_000_000_000), "21000000");
  assert.equal(satsToBtcString(123_456_789), "1.23456789");
});

test("payment links carry the amount and an encoded label only when given", () => {
  const a = "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu";
  assert.equal(bip21(a), `bitcoin:${a}`);
  assert.equal(bip21(a, 250_000), `bitcoin:${a}?amount=0.0025`);
  assert.equal(bip21(a, null, "from Kraken & co"), `bitcoin:${a}?label=from%20Kraken%20%26%20co`);
  assert.equal(bip21(a, 0, "  "), `bitcoin:${a}`);
});

test("typed amounts are read forgivingly and refused when not whole sats", () => {
  assert.equal(parseSats("50,000"), 50_000);
  assert.equal(parseSats(" 1 000 000 "), 1_000_000);
  for (const bad of ["", "0", "-5", "1e5", "0.5", "abc", "99999999999999999"]) assert.equal(parseSats(bad), null, bad);
});

test("the QR is covered for amounts at or over the threshold until checked", () => {
  assert.equal(mustCheckFirst(500_000, 500_000, false), true);
  assert.equal(mustCheckFirst(499_999, 500_000, false), false);
  assert.equal(mustCheckFirst(5_000_000, 500_000, true), false);
  assert.equal(mustCheckFirst(null, 500_000, false), false, "no amount: shown, with a note");
  assert.equal(mustCheckFirst(null, 0, false), true, "every address");
  assert.equal(mustCheckFirst(1, 0, true), false);
});
