// Run with `npm run test:ui`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { secretKind } from "./key-guard.ts";

const XPUB = "xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8";
const XPRV = "xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi";

test("secrets are spotted", () => {
  assert.equal(secretKind("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"), "seed");
  assert.equal(secretKind(Array(24).fill("Zoo").join("  ")), "seed");
  assert.equal(secretKind(XPRV), "xprv");
  assert.equal(secretKind(`wpkh([d34db33f/84h/0h/0h]${XPRV}/0/*)`), "xprv");
  assert.equal(secretKind("5HueCGU8rMjxEXxiPuD5BDku4MkFqeZyd4dZ1jvhTVqvbTLvyTJ"), "wif");
  assert.equal(secretKind("KwdMAjGmerYanjeui5SHS7JkmpZvVipYvB2LJGU1ZxJwYvP98617"), "wif");
});

test("public material is never mistaken for a secret", () => {
  for (const ok of [
    "",
    XPUB,
    `wpkh([d34db33f/84h/0h/0h]${XPUB}/0/*)`,
    "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq",
    "1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2",
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon",
  ]) {
    assert.equal(secretKind(ok), null, ok);
  }
});
