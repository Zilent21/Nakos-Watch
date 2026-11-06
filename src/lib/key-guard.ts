/**
 * Spots secrets pasted where a public key belongs, before anything leaves the input box. Rust
 * refuses the same things (`refuse_private_key_material`); this only warns sooner and says why.
 */

export type SecretKind = "seed" | "xprv" | "wif" | null;

/** 12, 15, 18, 21 or 24 words of letters only: the shape of a seed phrase (BIP-39). */
export function looksLikeSeedPhrase(text: string): boolean {
  const words = text.trim().split(/\s+/).filter(Boolean);
  return [12, 15, 18, 21, 24].includes(words.length) && words.every((w) => /^[a-z]{3,8}$/i.test(w));
}

/** xprv, yprv, zprv, tprv, uprv, vprv and their multisig capitals, anywhere in the text. */
export function containsExtendedPrivateKey(text: string): boolean {
  return /\b[xyztuvYZUV]prv[1-9A-HJ-NP-Za-km-z]{20,}/.test(text);
}

/** A single private key in WIF: 51 characters starting with 5, or 52 starting with K or L (testnet 9/c). */
export function looksLikeWif(text: string): boolean {
  const t = text.trim();
  return /^[5][1-9A-HJ-NP-Za-km-z]{50}$/.test(t) || /^[KLc][1-9A-HJ-NP-Za-km-z]{51}$/.test(t) || /^9[1-9A-HJ-NP-Za-km-z]{50}$/.test(t);
}

export function secretKind(text: string): SecretKind {
  if (!text.trim()) return null;
  if (containsExtendedPrivateKey(text)) return "xprv";
  if (looksLikeSeedPhrase(text)) return "seed";
  if (looksLikeWif(text)) return "wif";
  return null;
}

export const SECRET_WARNING: Record<Exclude<SecretKind, null>, string> = {
  seed: "This looks like a seed phrase. Never type or paste it into any app, Nakos included. Clear this box now. If it was ever on a computer or phone, think about moving your coins to a new seed.",
  xprv: "This is an extended private key (it says \"prv\"). It can spend your coins. Nakos won't take it: export the public key (xpub, zpub) or a descriptor instead.",
  wif: "This looks like a single private key. It can spend coins. Nakos won't take it: use the address or a descriptor instead.",
};
