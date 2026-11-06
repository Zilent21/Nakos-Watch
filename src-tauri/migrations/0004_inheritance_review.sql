-- Local dead-man style reminder ("review this packet by DATE"). Lives on the
-- global inheritance config row (wallet_id IS NULL). The date is plaintext;
-- the free-text note is encrypted with the vault key like other notes.
-- Nothing here is transmitted anywhere.
ALTER TABLE inheritance_configs ADD COLUMN review_by TEXT;
ALTER TABLE inheritance_configs ADD COLUMN review_note_encrypted BLOB;
ALTER TABLE inheritance_configs ADD COLUMN review_note_nonce BLOB;
