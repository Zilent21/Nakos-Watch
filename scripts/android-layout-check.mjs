#!/usr/bin/env node
// Layout check for the Android debug build, run against the live WebView over the Chrome DevTools
// Protocol. It needs a running emulator or device with a debug build open and an unlocked vault.
//
//   (with ANDROID_HOME and adb on the PATH) node scripts/android-layout-check.mjs [--keyboard]
//
// The viewport decides what is expected: below 768 CSS px the phone layout (no sidebar, bottom
// bar, no ⌘K hint, header below the status bar, nothing past the screen edge), from 1024 the
// desktop one. --keyboard also opens the Add connection and Add wallet dialogs, taps their last
// field so the real keyboard opens, and checks every field and button can still be reached.
// --tabs saves bottom-bar choices (a custom one, a damaged one, one picked in Settings with real
// taps, then Reset) and checks what the bar and More show; it leaves the default in place.
// --units saves Sats and BTC as the Display unit (and picks Sats with real taps in Settings)
// and checks the amounts on Today and Portfolio follow; it puts back the unit it found.
// --lockscreen locks the vault, checks the unlock screen's passphrase eye (hidden by default, a
// real tap shows it with suggestions off and Android's keyboard reports NoSuggestion, a second
// tap hides it), the auto-lock note, unlocks through the field with PASSPHRASE (env), and
// checks the eye in the restore dialog and the note in Settings.
// With more than one device attached, set ANDROID_SERIAL. Exit code 1 on failure.
import { execFileSync } from "node:child_process";

const PORT = 9223;
const ROUTES = ["#/", "#/portfolio", "#/health", "#/security", "#/privacy", "#/inheritance", "#/utxos", "#/spend", "#/node", "#/settings"];
const KEYBOARD = process.argv.includes("--keyboard");
const TABS = process.argv.includes("--tabs");
const UNITS = process.argv.includes("--units");
const LOCKSCREEN = process.argv.includes("--lockscreen");

function adb(...args) {
  return execFileSync("adb", args, { encoding: "utf8" });
}

const pid = adb("shell", "pidof", "org.nakos.app").trim();
if (!pid) throw new Error("org.nakos.app is not running");
const socket = adb("shell", "cat", "/proc/net/unix").match(new RegExp(`webview_devtools_remote_${pid}\\b`));
if (!socket) throw new Error("no WebView devtools socket; is this a debug build?");
adb("forward", `tcp:${PORT}`, `localabstract:${socket[0]}`);

const page = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json()).find((t) => t.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.onopen = resolve;
  ws.onerror = reject;
});
let nextId = 1;
const pending = new Map();
ws.onmessage = (m) => {
  const d = JSON.parse(m.data);
  pending.get(d.id)?.(d);
};
async function evaluate(expression) {
  const id = nextId++;
  const reply = new Promise((resolve) => pending.set(id, resolve));
  ws.send(JSON.stringify({ id, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
  const d = await reply;
  if (d.result.exceptionDetails) throw new Error(JSON.stringify(d.result.exceptionDetails));
  return d.result.result.value;
}

// Runs in the page. Returns the list of failed expectations for the current route.
const CHECK = `(() => {
  const fail = [];
  const shown = (el) => !!el && getComputedStyle(el).display !== "none" && el.getClientRects().length > 0;
  const W = innerWidth, H = innerHeight;
  const phone = W < 768;
  if (W >= 768 && W < 1024) return ["viewport " + W + " is between the phone and desktop layouts; use < 768 or >= 1024"];
  const probe = document.createElement("div");
  probe.style.cssText = "position:fixed;padding-top:env(safe-area-inset-top,0px)";
  document.body.appendChild(probe);
  const insetTop = parseFloat(getComputedStyle(probe).paddingTop);
  probe.remove();

  const main = document.querySelector("main");
  const aside = document.querySelector("aside");
  const nav = document.querySelector("[data-testid=bottom-nav]");
  const kbd = [...document.querySelectorAll("header kbd")].find((k) => k.textContent.includes("⌘K"));
  const header = document.querySelector("header");
  if (!main || !header) return ["no app shell (is the vault unlocked?)"];

  if (document.documentElement.scrollWidth > W + 1) fail.push("page scrolls sideways: " + document.documentElement.scrollWidth + " > " + W);
  if (main.scrollWidth > main.clientWidth + 1) fail.push("main scrolls sideways: " + main.scrollWidth + " > " + main.clientWidth);

  if (phone) {
    if (shown(aside)) fail.push("sidebar shown on a phone");
    if (!shown(nav)) fail.push("bottom bar missing on a phone");
    else {
      const items = [...nav.querySelectorAll("a,button")].filter(shown);
      if (items.length !== 5) fail.push("bottom bar has " + items.length + " items, expected 5");
      if (nav.getBoundingClientRect().bottom > H + 1) fail.push("bottom bar runs off the screen");
      // Android draws its gesture handle over the last ~16px; labels must stay above it.
      const lowest = Math.max(...items.map((i) => i.getBoundingClientRect().bottom));
      if (lowest > H - 16) fail.push("bottom bar labels reach the gesture handle: " + lowest + " > " + (H - 16));
      if (items.some((i) => i.getBoundingClientRect().height < 44)) fail.push("a bottom bar target is shorter than 44px");
    }
    if (shown(kbd)) fail.push("⌘K hint shown on a phone");
    if (header.getBoundingClientRect().top < insetTop - 0.5) fail.push("header under the status bar: top " + header.getBoundingClientRect().top + " < inset " + insetTop);
    if (main.clientWidth < W - 1) fail.push("content narrower than the screen: " + main.clientWidth);
    const content = main.firstElementChild;
    const pad = parseFloat(getComputedStyle(content).paddingLeft);
    if (pad > 16) fail.push("side padding " + pad + "px, at most 16 on a phone");
    const inner = content.clientWidth - 2 * pad;
    // Top-level cards only: a card inside a card may be a deliberate small tile.
    const cards = [...content.querySelectorAll(".rounded-lg.border.bg-card")].filter((c) => shown(c) && !c.parentElement.closest(".rounded-lg.border.bg-card"));
    const squeezed = cards.filter((c) => c.getBoundingClientRect().width < inner - 1);
    if (squeezed.length) fail.push(squeezed.length + " card(s) narrower than the content (" + Math.round(squeezed[0].getBoundingClientRect().width) + " < " + inner + ")");
    const title = content.querySelector("h1");
    const actions = title?.parentElement?.nextElementSibling;
    if (title && actions && actions.getBoundingClientRect().top < title.getBoundingClientRect().bottom) fail.push("page header actions sit beside the title");
    // Nothing may reach past the screen edge unless a box scrolls it sideways (a table) or clips
    // it (a progress bar's fill slides out of its track).
    const scrollsSideways = (el) => {
      for (let a = el.parentElement; a && a !== main; a = a.parentElement) {
        if (getComputedStyle(a).overflowX !== "visible") return true;
      }
      return false;
    };
    const past = [...content.querySelectorAll("*")].filter((el) => {
      if (!shown(el) || scrollsSideways(el)) return false;
      const r = el.getBoundingClientRect();
      return r.width > 0 && (r.right > W + 1 || r.left < -1);
    });
    if (past.length) fail.push(past.length + " element(s) past the screen edge, first: <" + past[0].tagName.toLowerCase() + "> " + JSON.stringify((past[0].textContent || "").trim().slice(0, 40)));
    // A button, tab or dropdown whose text is wider than itself spills over its neighbours.
    // Nothing may stick out of the bordered box it sits in (a button pushed past its row's edge).
    const boxOf = (el) => {
      for (let a = el.parentElement; a && a !== main; a = a.parentElement) {
        const cs = getComputedStyle(a);
        if (cs.overflowX !== "visible") return null;
        if (parseFloat(cs.borderRightWidth) > 0 && parseFloat(cs.borderLeftWidth) > 0) return a;
      }
      return null;
    };
    const outside = [...content.querySelectorAll("*")].filter((el) => {
      if (!shown(el)) return false;
      const box = boxOf(el);
      if (!box) return false;
      const r = el.getBoundingClientRect(), b = box.getBoundingClientRect();
      return r.width > 0 && (r.right > b.right + 1 || r.left < b.left - 1);
    });
    if (outside.length) fail.push(outside.length + " element(s) outside their box, first: <" + outside[0].tagName.toLowerCase() + "> " + JSON.stringify((outside[0].textContent || "").trim().slice(0, 40)));
    const spills = [...content.querySelectorAll("button,[role=tab],[role=combobox]")].filter((el) => shown(el) && el.scrollWidth > el.clientWidth + 1);
    if (spills.length) fail.push(spills.length + " control(s) with text wider than the control, first: " + JSON.stringify((spills[0].textContent || "").trim().slice(0, 40)));
  } else {
    if (!shown(aside)) fail.push("sidebar missing on desktop");
    if (shown(nav)) fail.push("bottom bar shown on desktop");
    if (!shown(kbd)) fail.push("⌘K hint missing on desktop");
  }

  if (location.hash === "#/settings") {
    // The card follows the platform, not the width: on Android it shows even at a wide size.
    // (Hidden on desktop is not testable here; the emulator is always Android.)
    const card = document.querySelector("[data-testid=phone-tabs-card]");
    if (__PLATFORM__ !== "desktop" && !shown(card)) fail.push("no Bottom bar card in Settings on " + __PLATFORM__);
  }

  if (location.hash === "#/portfolio") {
    const list = document.querySelector("[data-testid=wallet-list]");
    const table = main.querySelector("table");
    if (!list && !table) fail.push("portfolio shows no wallets (add a test wallet first)");
    else if (phone ? !shown(list) || shown(table) : shown(list) || !shown(table)) fail.push("portfolio shows the " + (phone ? "table" : "card list") + " layout");
  }
  return fail;
})()`;

let failures = 0;
const width = await evaluate("innerWidth");
const platform = (await evaluate(`window.__TAURI_INTERNALS__.invoke("vault_status")`)).platform;
for (const route of ROUTES) {
  await evaluate(`(location.hash = ${JSON.stringify(route)}, new Promise((r) => setTimeout(r, 1500)))`);
  const fail = await evaluate(CHECK.replaceAll("__PLATFORM__", JSON.stringify(platform)));
  console.log(`${width}px ${route}: ${fail.length ? "FAIL\n  - " + fail.join("\n  - ") : "ok"}`);
  failures += fail.length;
}

// Open a dialog, tap its last text field with the real keyboard, and check that the WebView
// shrank, the field is visible, and the dialog scrolls down to its last button.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function keyboardCheck(route, buttonText) {
  // Start each check from a fresh page so one dialog's leftovers cannot affect the next.
  await evaluate(`(location.hash = ${JSON.stringify(route)}, 1)`);
  await evaluate("(location.reload(), 1)").catch(() => {});
  await sleep(5000);
  const before = await evaluate("innerHeight");
  const opened = await evaluate(`(() => { const b = [...document.querySelectorAll("main button")].find((b) => b.textContent.trim().includes(${JSON.stringify(buttonText)})); if (b) b.click(); return !!b; })()`);
  if (!opened) return [`no "${buttonText}" button`];
  // Wait for the open animation to finish: a tap measured mid-zoom lands on the backdrop.
  let last = "";
  for (let i = 0; i < 20; i++) {
    await sleep(250);
    const now = await evaluate(`JSON.stringify(document.querySelector("[role=dialog]")?.getBoundingClientRect() ?? null)`);
    if (now !== "null" && now === last) break;
    last = now;
  }
  const at = await evaluate(`(() => { const d = document.querySelector("[role=dialog]"); const f = [...d.querySelectorAll("input:not([type=checkbox]):not([type=hidden]),textarea")].filter((e) => e.getClientRects().length).pop(); f.scrollIntoView({ block: "center" }); const r = f.getBoundingClientRect(); return [Math.round((r.left + r.width / 2) * devicePixelRatio), Math.round((r.top + r.height / 2) * devicePixelRatio)]; })()`);
  adb("shell", "input", "tap", String(at[0]), String(at[1]));
  // The first keyboard after a cold boot can take several seconds to appear.
  for (let i = 0; i < 16; i++) {
    await sleep(500);
    if ((await evaluate("innerHeight")) < before - 100) break;
  }
  await sleep(500);
  const fail = await evaluate(`(() => {
    const fail = [];
    const d = document.querySelector("[role=dialog]");
    if (!d) return ["the dialog closed when its field was tapped"];
    const H = innerHeight;
    if (H > ${before} - 100) fail.push("the page did not shrink for the keyboard (" + H + " of " + ${before} + ")");
    const a = document.activeElement, r = a.getBoundingClientRect();
    if (!/INPUT|TEXTAREA/.test(a.tagName)) fail.push("the tapped field did not get focus");
    else if (r.bottom > H + 1 || r.top < 0) fail.push("the focused field is outside the visible area");
    d.scrollTop = d.scrollHeight;
    const last = [...d.querySelectorAll("button")].filter((b) => b.getClientRects().length).pop();
    if (last.getBoundingClientRect().bottom > H + 1) fail.push("the dialog's last button cannot be scrolled into view");
    return fail;
  })()`);
  adb("shell", "input", "keyevent", "4"); // hide the keyboard
  await sleep(800);
  await evaluate(`(document.querySelector("[role=dialog] button[class*=absolute]")?.click(), 1)`);
  await sleep(800);
  return fail;
}
// What the bar and the More list show right now.
const BAR = `(async () => {
  const nav = document.querySelector("[data-testid=bottom-nav]");
  const bar = [...nav.querySelectorAll("a,button")].map((e) => e.textContent.trim());
  nav.querySelector("button").click();
  await new Promise((r) => setTimeout(r, 600));
  const more = [...document.querySelectorAll("[role=dialog] .grid button")].map((e) => e.textContent.trim());
  document.querySelector("[role=dialog] button[class*=absolute]")?.click();
  await new Promise((r) => setTimeout(r, 400));
  return { bar, more };
})()`;
const savePref = (value) => evaluate(`window.__TAURI_INTERNALS__.invoke("set_preference", { key: "phone_tabs", value: ${JSON.stringify(value)} })`);
const reload = async () => {
  await evaluate("(location.reload(), 1)").catch(() => {});
  await sleep(5000);
};
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const DEFAULT_BAR = ["Today", "Portfolio", "Coins", "Node", "More"];
async function tabsCheck() {
  const fail = [];
  await evaluate(`(location.hash = "#/", 1)`);
  await savePref(JSON.stringify(["/settings", "/", "/spend", "/health"]));
  await reload();
  let got = await evaluate(BAR);
  if (!same(got.bar, ["Settings", "Today", "Spend", "Health", "More"])) fail.push("saved choice not shown: " + got.bar.join(", "));
  if (!same(got.more, ["Portfolio", "Security", "Privacy", "Inheritance", "Coins", "Node"])) fail.push("More list wrong: " + got.more.join(", "));
  await savePref(JSON.stringify(["/", "/", "/asset-map", 7]));
  await reload();
  got = await evaluate(BAR);
  if (!same(got.bar, DEFAULT_BAR)) fail.push("a damaged choice did not fall back to the default: " + got.bar.join(", "));
  // In Settings, with real taps: Tab 1 becomes Spend, then Reset.
  await evaluate(`(location.hash = "#/settings", new Promise((r) => setTimeout(r, 1500)))`);
  const tapAt = async (expr) => {
    const at = await evaluate(`(() => { const e = ${expr}; if (!e) return null; e.scrollIntoView({ block: "center" }); const r = e.getBoundingClientRect(); return [Math.round((r.left + r.width / 2) * devicePixelRatio), Math.round((r.top + r.height / 2) * devicePixelRatio)]; })()`);
    if (!at) return false;
    await sleep(400);
    const again = await evaluate(`(() => { const e = ${expr}; const r = e.getBoundingClientRect(); return [Math.round((r.left + r.width / 2) * devicePixelRatio), Math.round((r.top + r.height / 2) * devicePixelRatio)]; })()`);
    adb("shell", "input", "tap", String(again[0]), String(again[1]));
    await sleep(1200);
    return true;
  };
  if (!(await tapAt(`document.querySelector('[data-testid=phone-tabs-card] [aria-label="Tab 1"]')`))) fail.push("no Tab 1 picker in Settings");
  else if (!(await tapAt(`[...document.querySelectorAll("[role=option]")].find((o) => o.textContent.trim() === "Spend")`))) fail.push("Spend is not offered for Tab 1");
  else {
    const saved = await evaluate(`window.__TAURI_INTERNALS__.invoke("get_preference", { key: "phone_tabs" })`);
    got = await evaluate(BAR);
    if (!same(got.bar, ["Spend", "Portfolio", "Coins", "Node", "More"])) fail.push("choosing Spend for Tab 1 gave: " + got.bar.join(", "));
    if (saved !== JSON.stringify(["/spend", "/portfolio", "/utxos", "/node"])) fail.push("saved preference is " + saved);
  }
  await evaluate(`([...document.querySelectorAll("[data-testid=phone-tabs-card] button")].find((b) => b.textContent.startsWith("Reset"))?.click(), new Promise((r) => setTimeout(r, 1000)))`);
  got = await evaluate(BAR);
  if (!same(got.bar, DEFAULT_BAR)) fail.push("Reset did not restore the default: " + got.bar.join(", "));
  await savePref(JSON.stringify(["/", "/portfolio", "/utxos", "/node"]));
  return fail;
}
// The tallest dialog (Add connection with the public-server warning open) must start below the
// status bar, and its close button must be a comfortable tap target.
async function dialogCheck() {
  await evaluate(`(location.hash = "#/node", new Promise((r) => setTimeout(r, 1500)))`);
  const opened = await evaluate(`(() => { const b = [...document.querySelectorAll("main button")].find((b) => b.textContent.trim().includes("Add connection")); b?.click(); return !!b; })()`);
  if (!opened) return ["no Add connection button"];
  await sleep(1000);
  await evaluate(`([...document.querySelectorAll("[role=dialog] button")].find((b) => b.textContent.includes("Use a public server"))?.click(), 1)`);
  await sleep(1500);
  const fail = await evaluate(`(() => {
    const fail = [];
    const probe = document.createElement("div");
    probe.style.cssText = "position:fixed;padding-top:env(safe-area-inset-top,0px)";
    document.body.appendChild(probe);
    const inset = parseFloat(getComputedStyle(probe).paddingTop);
    probe.remove();
    const d = document.querySelector("[role=dialog]");
    const r = d.getBoundingClientRect();
    if (r.top < inset + 8) fail.push("the dialog starts at " + Math.round(r.top) + "px, under or against the status bar (" + inset + "px)");
    const close = d.querySelector("button[class*=absolute]");
    const c = close.getBoundingClientRect();
    if (c.width < 40 || c.height < 40) fail.push("the close button is " + Math.round(c.width) + "x" + Math.round(c.height) + "px, under 40x40");
    if (c.top < r.top - 1 || c.right > r.right + 1) fail.push("the close button sits outside the dialog");
    close.click();
    return fail;
  })()`);
  await sleep(600);
  return fail;
}
if (width < 768) {
  const fail = await dialogCheck();
  console.log(`dialog: ${fail.length ? "FAIL\n  - " + fail.join("\n  - ") : "ok"}`);
  failures += fail.length;
}

// What Today and Portfolio show as amounts right now.
const AMOUNTS = `(async () => {
  const pick = () => [...document.querySelectorAll("main .mono-nums")].map((e) => e.textContent.trim()).filter((t) => /\\d/.test(t) && /(BTC|sats)/.test(t));
  location.hash = "#/";
  await new Promise((r) => setTimeout(r, 1500));
  const today = pick();
  location.hash = "#/portfolio";
  await new Promise((r) => setTimeout(r, 1500));
  return { today, portfolio: pick() };
})()`;
async function unitsCheck() {
  const fail = [];
  const before = await evaluate(`window.__TAURI_INTERNALS__.invoke("get_preference", { key: "display_unit" })`);
  const setUnit = (u) => evaluate(`window.__TAURI_INTERNALS__.invoke("set_preference", { key: "display_unit", value: ${JSON.stringify(u)} })`);
  // A missing or unknown saved unit shows BTC, as every screen did before the setting worked.
  for (const [unit, word] of [["sats", "sats"], ["btc", "BTC"], ["nonsense", "BTC"]]) {
    await setUnit(unit);
    await reload();
    const got = await evaluate(AMOUNTS);
    const all = [...got.today, ...got.portfolio];
    if (!got.today.length || !got.portfolio.length) fail.push(`no amounts found with ${unit} (today ${got.today.length}, portfolio ${got.portfolio.length})`);
    const wrong = all.filter((t) => !t.includes(word));
    if (wrong.length) fail.push(`with ${unit}, ${wrong.length} amount(s) not in ${word}: ${JSON.stringify(wrong.slice(0, 3))}`);
  }
  // With BTC saved, pick Sats in Settings with real taps; Today must follow without a reload.
  await evaluate(`(location.hash = "#/settings", new Promise((r) => setTimeout(r, 1500)))`);
  const tap = async (expr) => {
    const at = await evaluate(`(() => { const e = ${expr}; if (!e) return null; e.scrollIntoView({ block: "center" }); return 1; })()`);
    if (!at) return false;
    await sleep(500);
    const xy = await evaluate(`(() => { const r = (${expr}).getBoundingClientRect(); return [Math.round((r.left + r.width / 2) * devicePixelRatio), Math.round((r.top + r.height / 2) * devicePixelRatio)]; })()`);
    adb("shell", "input", "tap", String(xy[0]), String(xy[1]));
    await sleep(1200);
    return true;
  };
  const trigger = `[...document.querySelectorAll("label")].find((l) => l.textContent.trim() === "Display unit")?.parentElement.querySelector("[role=combobox]")`;
  if (!(await tap(trigger))) fail.push("no Display unit picker in Settings");
  else if (!(await tap(`[...document.querySelectorAll("[role=option]")].find((o) => o.textContent.trim() === "Sats")`))) fail.push("Sats is not offered");
  else {
    const saved = await evaluate(`window.__TAURI_INTERNALS__.invoke("get_preference", { key: "display_unit" })`);
    if (saved !== "sats") fail.push("choosing Sats saved " + saved);
    const got = await evaluate(AMOUNTS);
    if (!got.today.length || got.today.some((t) => !t.includes("sats"))) fail.push("Today did not switch to sats: " + JSON.stringify(got.today));
  }
  await setUnit(before ?? "btc");
  await reload();
  return fail;
}
if (UNITS) {
  const fail = await unitsCheck();
  console.log(`units: ${fail.length ? "FAIL\n  - " + fail.join("\n  - ") : "ok"}`);
  failures += fail.length;
}

async function tapEl(expr) {
  const xy = await evaluate(`(() => { const e = ${expr}; if (!e) return null; const r = e.getBoundingClientRect(); return [Math.round((r.left + r.width / 2) * devicePixelRatio), Math.round((r.top + r.height / 2) * devicePixelRatio)]; })()`);
  if (!xy) return false;
  adb("shell", "input", "tap", String(xy[0]), String(xy[1]));
  await sleep(1500);
  return true;
}
async function lockscreenCheck() {
  const fail = [];
  const pass = process.env.PASSPHRASE;
  if (!pass) return ["set PASSPHRASE to the test vault's passphrase"];
  await evaluate(`window.__TAURI_INTERNALS__.invoke("vault_lock", { forgetKeychain: false })`);
  await reload();
  const field = `document.querySelector("#passphrase")`;
  const eye = `document.querySelector("[data-testid=toggle-passphrase]")`;
  const state = () => evaluate(`(() => { const f = ${field}, e = ${eye}; if (!f || !e) return null; const r = e.getBoundingClientRect(); return { type: f.type, spell: f.spellcheck, correct: f.getAttribute("autocorrect"), caps: f.getAttribute("autocapitalize"), w: r.width, h: r.height, label: e.getAttribute("aria-label"), note: document.querySelector("[data-testid=stays-unlocked]")?.textContent ?? "" }; })()`);
  let st = await state();
  if (!st) return ["no unlock screen with a passphrase field and eye button"];
  if (st.type !== "password") fail.push("the passphrase is not hidden by default");
  if (st.w < 40 || st.h < 40) fail.push(`the eye button is ${Math.round(st.w)}x${Math.round(st.h)}px, under 40x40`);
  if (st.label !== "Show passphrase") fail.push("the eye button is labelled " + st.label);
  if (!/^Nakos locks /.test(st.note)) fail.push("no auto-lock note on the unlock screen: " + JSON.stringify(st.note));
  if (platform === "android" && !/30 seconds after it leaves the screen/.test(st.note)) fail.push("the note does not say Nakos locks when it leaves the screen");
  await tapEl(eye);
  st = await state();
  if (st.type !== "text" || st.label !== "Hide passphrase") fail.push(`one tap on the eye left the field as ${st.type}, ${st.label}`);
  if (st.spell !== false || st.correct !== "off" || st.caps !== "off") fail.push("the shown passphrase still allows suggestions");
  if (platform === "android") {
    adb("logcat", "-c");
    await tapEl(field);
    await sleep(1500);
    const log = adb("logcat", "-d");
    const types = [...log.matchAll(/packageName=org\.nakos\.app, inputType=[0-9a-f]+, inputTypeString=([^,]*)/g)].map((m) => m[1]);
    const last = types.pop() ?? "none";
    // Chrome reports a shown passphrase field as VisiblePassword (still a password to the keyboard);
    // a field without a password autocomplete hint gets NoSuggestion. Anything else may be learned.
    if (!/VisiblePassword|NoSuggestion/.test(last)) fail.push("Android's keyboard sees the shown passphrase as " + last);
  }
  await tapEl(eye);
  st = await state();
  if (st.type !== "password") fail.push("a second tap did not hide the passphrase");
  // Unlock through the real field: type it, then the Unlock button.
  await tapEl(field);
  adb("shell", "input", "text", pass);
  await sleep(1000);
  const typed = await evaluate(`${field}.value.length`);
  if (typed !== pass.length) fail.push(`typed ${typed} of ${pass.length} characters`);
  await evaluate(`([...document.querySelectorAll("button[type=submit]")].find((b) => b.textContent.includes("Unlock"))?.click(), 1)`);
  await sleep(4000);
  if (!(await evaluate(`!!document.querySelector("main")`))) fail.push("the vault did not unlock through the field");
  // Settings: the note next to Lock vault now, and the restore dialog's passphrase has an eye.
  await evaluate(`(location.hash = "#/settings", new Promise((r) => setTimeout(r, 1500)))`);
  if (!(await evaluate(`/^Nakos locks /.test(document.querySelector("main [data-testid=stays-unlocked]")?.textContent ?? "")`))) fail.push("no stays-unlocked note in Settings");
  await evaluate(`([...document.querySelectorAll("main button")].find((b) => /Restore from encrypted backup/i.test(b.textContent))?.click(), new Promise((r) => setTimeout(r, 1200)))`);
  const restoreEye = await evaluate(`!!document.querySelector("[role=dialog] #restore-pass") && !!document.querySelector("[role=dialog] [data-testid=toggle-passphrase]")`);
  if (!restoreEye) fail.push("the restore dialog's passphrase has no eye");
  await evaluate(`(document.querySelector("[role=dialog] button[class*=absolute]")?.click(), 1)`);
  await sleep(600);
  return fail;
}
if (LOCKSCREEN) {
  const fail = await lockscreenCheck();
  console.log(`lockscreen: ${fail.length ? "FAIL\n  - " + fail.join("\n  - ") : "ok"}`);
  failures += fail.length;
}

if (TABS) {
  const fail = await tabsCheck();
  console.log(`tabs: ${fail.length ? "FAIL\n  - " + fail.join("\n  - ") : "ok"}`);
  failures += fail.length;
}

if (KEYBOARD) {
  for (const [route, button] of [["#/node", "Add connection"], ["#/portfolio", "Add wallet"]]) {
    const fail = await keyboardCheck(route, button);
    console.log(`keyboard ${route} "${button}": ${fail.length ? "FAIL\n  - " + fail.join("\n  - ") : "ok"}`);
    failures += fail.length;
  }
}
ws.close();
adb("forward", "--remove", `tcp:${PORT}`);
process.exit(failures ? 1 : 0);
