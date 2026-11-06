//! Builds a singlesig and a 2-of-3 multisig fixture PSBT (fixed BIP-32
//! test-vector xpubs, no network) and prints a decoded field list for each
//! input, so the presence of bip32_derivation / witness_script /
//! redeem_script can be inspected directly rather than taken on faith.
//!
//! Usage: cargo run --example dump_psbt

use bitcoin::base64::engine::general_purpose::STANDARD;
use bitcoin::base64::Engine;
use bitcoin::psbt::Psbt;
use bitcoin::Network;
use nakos_lib::bitcoin::descriptors;
use nakos_lib::bitcoin::psbt::{build_unsigned_psbt, PsbtInput, PsbtInputOrigin, PsbtOutput};
use nakos_lib::db::models::Cosigner;

const TEST_XPUB_1: &str = "xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8";
const TEST_XPUB_2: &str = "xpub661MyMwAqRbcFW31YEwpkMuc5THy2PSt5bDMsktWQcFF8syAmRUapSCGu8ED9W6oDMSgv6Zz8idoc4a6mr8BDzTJY47LJhkJ8UB7WEGuduB";
const TEST_XPUB_3: &str = "xpub661MyMwAqRbcEZVB4dScxMAdx6d4nFc9nvyvH3v4gJL378CSRZiYmhRoP7mBy6gSPSCYk6SzXPTf3ND1cZAceL7SfJ1Z3GC8vBgp2epUt13";

fn dump(label: &str, base64: &str) {
    let bytes = STANDARD.decode(base64).expect("valid base64");
    let psbt = Psbt::deserialize(&bytes).expect("valid psbt");

    println!("== {label} ==");
    println!("psbt version fields present: {}", 1); // BIP174 unsigned tx always present
    for (i, input) in psbt.inputs.iter().enumerate() {
        println!("-- input {i} --");
        println!("  witness_utxo:      {}", input.witness_utxo.is_some());
        println!("  non_witness_utxo:  {}", input.non_witness_utxo.is_some());
        println!("  witness_script:    {}", input.witness_script.is_some());
        println!("  redeem_script:     {}", input.redeem_script.is_some());
        println!("  bip32_derivation:  {} entries", input.bip32_derivation.len());
        for (pubkey, (fingerprint, path)) in &input.bip32_derivation {
            println!("    pubkey={pubkey} fingerprint={fingerprint} path={path}");
        }
    }
    println!("base64: {base64}");
    println!();
}

fn main() {
    let network = Network::Bitcoin;

    // -- singlesig p2wpkh fixture --------------------------------------
    let pair = descriptors::descriptors_from_extended_key(TEST_XPUB_1, network, Some(("aabbccdd", "84'/0'/0'")))
        .expect("valid xpub with origin");
    let singlesig_inputs = vec![PsbtInput {
        txid: "11".repeat(32),
        vout: 0,
        amount_sats: 100_000,
        address: derive_address(&pair.external, 5, network),
        origin: Some(PsbtInputOrigin { wildcard_descriptor: pair.external.clone(), derivation_index: 5 }),
    }];
    let singlesig_outputs = vec![PsbtOutput { address: derive_address(&pair.internal, 0, network), amount_sats: 90_000, origin: None }];
    let singlesig_built = build_unsigned_psbt(&singlesig_inputs, &singlesig_outputs, network, "Singlesig Fixture Wallet")
        .expect("singlesig psbt should build");
    dump("SINGLESIG P2WPKH (84'/0'/0')", &singlesig_built.base64);

    // -- multisig 2-of-3 p2wsh fixture -----------------------------------
    let cosigners = vec![
        Cosigner { id: String::new(), label: "A".into(), xpub: TEST_XPUB_1.into(), fingerprint: Some("aaaaaaaa".into()), derivation_path: Some("48'/0'/0'/2'".into()), is_mine: false, hardware_model: None },
        Cosigner { id: String::new(), label: "B".into(), xpub: TEST_XPUB_2.into(), fingerprint: Some("bbbbbbbb".into()), derivation_path: Some("48'/0'/0'/2'".into()), is_mine: false, hardware_model: None },
        Cosigner { id: String::new(), label: "C".into(), xpub: TEST_XPUB_3.into(), fingerprint: Some("cccccccc".into()), derivation_path: Some("48'/0'/0'/2'".into()), is_mine: false, hardware_model: None },
    ];
    let multisig_pair = descriptors::descriptors_from_multisig(2, &cosigners, network, "p2wsh").expect("valid multisig");
    let multisig_inputs = vec![PsbtInput {
        txid: "22".repeat(32),
        vout: 1,
        amount_sats: 500_000,
        address: derive_address(&multisig_pair.external, 0, network),
        origin: Some(PsbtInputOrigin { wildcard_descriptor: multisig_pair.external.clone(), derivation_index: 0 }),
    }];
    let multisig_outputs = vec![PsbtOutput { address: derive_address(&multisig_pair.internal, 0, network), amount_sats: 480_000, origin: None }];
    let multisig_built = build_unsigned_psbt(&multisig_inputs, &multisig_outputs, network, "Multisig Fixture Vault")
        .expect("multisig psbt should build");
    dump("MULTISIG 2-of-3 P2WSH (48'/0'/0'/2')", &multisig_built.base64);
}

fn derive_address(wildcard_descriptor: &str, index: u32, network: Network) -> String {
    use bdk_wallet::miniscript::{Descriptor, DescriptorPublicKey};
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let (descriptor, _keymap) =
        Descriptor::<DescriptorPublicKey>::parse_descriptor(&secp, wildcard_descriptor).expect("valid descriptor");
    let definite = descriptor.at_derivation_index(index).expect("valid index");
    definite.address(network).expect("script has an address form").to_string()
}
