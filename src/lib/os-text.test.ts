// Run with `npm run test:ui` (node --test; Node strips the types itself).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { coreCookieExample, coreDataFolderHint, keychainNote, paletteShortcut } from "./os-text.ts";

test("the palette shortcut is labelled as the keyboard has it", () => {
  assert.equal(paletteShortcut("macos"), "⌘K");
  assert.equal(paletteShortcut("windows"), "Ctrl+K");
  assert.equal(paletteShortcut("linux"), "Ctrl+K");
});

test("the Core cookie example is a path for this OS", () => {
  assert.equal(coreCookieExample("windows"), "C:\\Users\\you\\AppData\\Local\\Bitcoin\\.cookie");
  assert.equal(coreCookieExample("macos"), "~/Library/Application Support/Bitcoin/.cookie");
  assert.equal(coreCookieExample("linux"), "~/.bitcoin/.cookie");
});

test("the Core data folder hint is the folder for this OS", () => {
  assert.equal(coreDataFolderHint("windows"), "C:\\Users\\you\\AppData\\Local\\Bitcoin (older installs: AppData\\Roaming\\Bitcoin)");
  assert.equal(coreDataFolderHint("macos"), "~/Library/Application Support/Bitcoin");
  assert.equal(coreDataFolderHint("linux"), "~/.bitcoin");
});

test("the keychain note says what this OS does", () => {
  const win = keychainNote("windows");
  assert.match(win, /Credential Manager/);
  assert.match(win, /does not ask for a password/);
  assert.match(win, /roaming profile/);
  assert.doesNotMatch(win, /login password/);
  assert.equal(keychainNote("macos"), "macOS may ask for your login password.");
  assert.match(keychainNote("linux"), /keyring/);
});

/** Every file under `dir` with one of the extensions. */
function files(dir: string, exts: string[]): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return files(path, exts);
    return exts.some((e) => name.endsWith(e)) ? [path] : [];
  });
}

test("no screen hard-codes macOS words; the screens ask os-text for them", () => {
  const src = join(import.meta.dirname, "..");
  for (const path of files(src, [".ts", ".tsx"])) {
    if (path.endsWith("os-text.ts") || path.endsWith(".test.ts")) continue;
    const text = readFileSync(path, "utf8");
    for (const word of ["⌘K", "login password", "Library/Application Support", "~/.bitcoin"]) {
      assert.ok(!text.includes(word), `${path} says "${word}" itself`);
    }
  }
  const read = (rel: string) => readFileSync(join(src, rel), "utf8");
  assert.ok(read("components/layout/app-shell.tsx").includes("{paletteShortcut(os)}"));
  assert.ok(read("modules/node-ops/node-ops-page.tsx").includes("placeholder={coreCookieExample(os)}"));
  assert.ok(read("modules/node-ops/node-ops-page.tsx").includes("<code>{coreDataFolderHint(os)}</code>"));
  assert.ok(read("modules/onboarding/welcome-screen.tsx").includes("{keychainNote(os)}"));
  assert.ok(read("modules/settings-page.tsx").includes("{keychainNote(os)}"));
  assert.ok(read("stores/vault-store.ts").includes("os: status.os,"), "the OS comes from vault_status");
});
