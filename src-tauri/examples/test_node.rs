//! Standalone connectivity check against a real Bitcoin Core/Knots RPC and/or
//! Electrum server, using Nakos's actual client code — no GUI, no database.
//! Credentials are read from environment variables only, never hardcoded, so
//! nothing sensitive ends up committed to the repo.
//!
//! Usage:
//!   NAKOS_RPC_HOST=<host> NAKOS_RPC_PORT=<port> \
//!   NAKOS_RPC_USER=<user> NAKOS_RPC_PASS='...' \
//!   NAKOS_ELECTRUM_HOST=<host> NAKOS_ELECTRUM_PORT=<port> \
//!   cargo run --example test_node

use nakos_lib::rpc::{core_rpc, electrum};

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn main() {
    let mut any_ran = false;

    if let (Some(host), Some(port)) = (env("NAKOS_RPC_HOST"), env("NAKOS_RPC_PORT")) {
        any_ran = true;
        println!("== Bitcoin Core / Knots RPC: {host}:{port} ==");
        let cfg = core_rpc::CoreRpcConfig {
            host,
            port: port.parse().expect("NAKOS_RPC_PORT must be a number"),
            user: env("NAKOS_RPC_USER"),
            password: env("NAKOS_RPC_PASS"),
            cookie_path: env("NAKOS_RPC_COOKIE"),
            use_tls: env("NAKOS_RPC_TLS").as_deref() == Some("1"),
        };
        match cfg.connect().and_then(|c| core_rpc::get_node_status(&c)) {
            Ok(status) => {
                println!("  OK — chain={} blocks={} headers={} synced={} pruned={} peers_in={} peers_out={} subversion={}",
                    status.chain, status.blocks, status.headers, status.is_synced, status.is_pruned,
                    status.connections_in, status.connections_out, status.subversion);
                match cfg.connect().and_then(|c| core_rpc::estimate_fees(&c)) {
                    Ok(fees) => {
                        print!("  Fee estimates:");
                        for f in fees {
                            print!(" {}blk={:?}", f.target_blocks, f.sat_per_vb);
                        }
                        println!();
                    }
                    Err(e) => println!("  (fee estimate failed: {e})"),
                }
            }
            Err(e) => println!("  FAILED: {e}"),
        }
        println!();
    }

    if let (Some(host), Some(port)) = (env("NAKOS_ELECTRUM_HOST"), env("NAKOS_ELECTRUM_PORT")) {
        any_ran = true;
        println!("== Electrum: {host}:{port} ==");
        let cfg = electrum::ElectrumConfig {
            host,
            port: port.parse().expect("NAKOS_ELECTRUM_PORT must be a number"),
            use_tls: env("NAKOS_ELECTRUM_TLS").as_deref() == Some("1"),
        };
        match cfg.connect() {
            Ok(client) => match electrum::get_server_status(&client) {
                Ok(status) => println!(
                    "  OK — server={} protocol={} tip_height={}",
                    status.server_version, status.protocol_version, status.tip_height
                ),
                Err(e) => println!("  FAILED to fetch status: {e}"),
            },
            Err(e) => println!("  FAILED to connect: {e}"),
        }
    }

    if !any_ran {
        eprintln!("Set NAKOS_RPC_HOST/PORT and/or NAKOS_ELECTRUM_HOST/PORT env vars. See file header for usage.");
        std::process::exit(1);
    }
}
