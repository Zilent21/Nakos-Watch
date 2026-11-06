-- Cost-basis lots live on the UTXO row itself (acquisition_cost_fiat and
-- acquisition_date already exist from 0001). A coin has a usable lot when it
-- has BOTH. This adds the optional free-text note.
ALTER TABLE utxos ADD COLUMN acquisition_note TEXT;
