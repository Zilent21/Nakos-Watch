// Run with `npm run test:ui` (node --test; Node strips the types itself).
import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_PHONE_TABS, PHONE_SCREENS, chooseTab, moreScreens, parsePhoneTabs } from "./phone-tabs.ts";

test("the default is Today, Portfolio, Coins and Node", () => {
  assert.deepEqual(DEFAULT_PHONE_TABS, ["/", "/portfolio", "/utxos", "/node"]);
  assert.ok(!PHONE_SCREENS.includes("/asset-map"), "Map is not offered on phones");
  assert.equal(PHONE_SCREENS.length, 10);
});

test("a saved choice of four different known screens is used as saved", () => {
  const saved = JSON.stringify(["/settings", "/", "/spend", "/health"]);
  assert.deepEqual(parsePhoneTabs(saved), ["/settings", "/", "/spend", "/health"]);
});

test("anything else falls back to the default", () => {
  for (const bad of [
    null,
    undefined,
    "",
    "not json",
    "{}",
    JSON.stringify(["/", "/portfolio", "/utxos"]),
    JSON.stringify(["/", "/portfolio", "/utxos", "/node", "/spend"]),
    JSON.stringify(["/", "/", "/utxos", "/node"]),
    JSON.stringify(["/", "/", "/portfolio", "/utxos", "/node"]),
    JSON.stringify(["/", "/portfolio", "/utxos", "/asset-map"]),
    JSON.stringify(["/", "/portfolio", "/utxos", 4]),
  ]) {
    assert.deepEqual(parsePhoneTabs(bad), DEFAULT_PHONE_TABS, String(bad));
  }
  // The default is handed out as a copy, so a caller cannot change it.
  const a = parsePhoneTabs(null);
  a[0] = "/spend";
  assert.equal(DEFAULT_PHONE_TABS[0], "/");
});

test("choosing a screen already in another slot swaps the two", () => {
  const tabs = ["/", "/portfolio", "/utxos", "/node"];
  assert.deepEqual(chooseTab(tabs, 0, "/spend"), ["/spend", "/portfolio", "/utxos", "/node"]);
  assert.deepEqual(chooseTab(tabs, 0, "/node"), ["/node", "/portfolio", "/utxos", "/"]);
  assert.deepEqual(chooseTab(tabs, 2, "/utxos"), tabs);
  assert.deepEqual(tabs, ["/", "/portfolio", "/utxos", "/node"], "the input is not changed");
  // Out-of-range slots and unknown screens change nothing.
  assert.deepEqual(chooseTab(tabs, 4, "/spend"), tabs);
  assert.deepEqual(chooseTab(tabs, -1, "/spend"), tabs);
  assert.deepEqual(chooseTab(tabs, 1, "/asset-map"), tabs);
});

test("More lists every other screen in menu order", () => {
  assert.deepEqual(moreScreens(DEFAULT_PHONE_TABS), ["/health", "/security", "/privacy", "/inheritance", "/spend", "/settings"]);
  assert.deepEqual(moreScreens(["/settings", "/", "/spend", "/health"]), ["/portfolio", "/security", "/privacy", "/inheritance", "/utxos", "/node"]);
});
