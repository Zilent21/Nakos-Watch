-- Bitcoin Core RPC has no TLS in Nakos. The RPC transport speaks plain HTTP and ignored the
-- flag, so a Core row saved with "TLS" on was never encrypted. Clear the flag so nothing claims
-- otherwise (the connection itself is unchanged: it was always plain HTTP). Electrum rows keep
-- their TLS flag; that one is real.
UPDATE node_configs SET use_tls = 0 WHERE kind = 'bitcoin_core_rpc' AND use_tls <> 0;
