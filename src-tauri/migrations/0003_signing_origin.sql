-- Signing-origin metadata for singlesig xpub/ypub/zpub wallets, so PSBTs
-- built later can carry bip32_derivation origin info. Both are supplied by
-- the user at Add Wallet time (never invented/guessed by Nakos) — e.g. the
-- fingerprint and derivation path printed on a hardware wallet's descriptor
-- export. Multisig cosigner origin already lives on multisig_cosigners
-- (fingerprint, derivation_path columns from 0001_init.sql).
ALTER TABLE wallets ADD COLUMN master_fingerprint TEXT;
ALTER TABLE wallets ADD COLUMN derivation_path TEXT;
