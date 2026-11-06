-- Cookie-file auth support for Bitcoin Core / Knots (the default auth mode
-- when no rpcuser/rpcpassword is set in bitcoin.conf). The cookie file path
-- itself is not a secret (Core/Knots regenerates the file's contents on
-- every restart), so it is stored in plaintext unlike rpc_password.
ALTER TABLE node_configs ADD COLUMN cookie_path TEXT;
