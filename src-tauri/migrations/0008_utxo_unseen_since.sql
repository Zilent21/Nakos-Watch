-- A scan that succeeds but does not report a coin used to DELETE its row, taking the user's lot,
-- label, note and freeze flag with it. Sync now keeps any row that carries user data and marks it
-- not-live instead (spent = 1). `unseen_since` records when a scan first failed to report a coin
-- that was live, so "the server omitted it" can be told apart from "known spent" (spent = 1 with
-- this column empty). A coin reported live again clears it. Existing rows start empty.
ALTER TABLE utxos ADD COLUMN unseen_since TEXT;
