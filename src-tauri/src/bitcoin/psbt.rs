//! Builds unsigned PSBTs from selected UTXOs. Nakos is watch-only: it can
//! construct and populate a PSBT's witness data, but never signs. The user
//! takes the resulting base64 PSBT to their hardware wallet / signer of
//! choice to complete and broadcast.

use bdk_wallet::miniscript::psbt::{PsbtInputExt, PsbtOutputExt};
use bdk_wallet::miniscript::{Descriptor, DescriptorPublicKey};
use bitcoin::psbt::{Input as PsbtInputData, Psbt};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::transaction::Version;
use bitcoin::{
    absolute::LockTime, base64::engine::general_purpose::STANDARD, base64::Engine, Address, Amount,
    Network, OutPoint, Sequence, Transaction, TxIn, TxOut, Txid, Witness,
};
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PsbtBuildError {
    #[error("invalid txid: {0}")]
    Txid(String),
    #[error("invalid address: {0}")]
    Address(String),
    #[error("psbt construction failed: {0}")]
    Psbt(String),
    #[error("no inputs selected")]
    NoInputs,
    #[error("no outputs")]
    NoOutputs,
    #[error(
        "wallet '{wallet}' has no known signing origin (master fingerprint + derivation path) \
         for input {txid}:{vout} — add the fingerprint and derivation path to this wallet \
         before building a spend; Nakos will not emit a PSBT it can't prove is signable"
    )]
    MissingOrigin { wallet: String, txid: String, vout: u32 },
    #[error("could not attach key origin to change output {address}: {reason}")]
    ChangeDerivation { address: String, reason: String },
    #[error("could not derive signing metadata for input {txid}:{vout}: {reason}")]
    DescriptorDerivation { txid: String, vout: u32, reason: String },
}

/// Per-input signing origin: the wildcard descriptor (external or internal,
/// as stored on the owning wallet) that covers this input, WITH a
/// `[fingerprint/path]` key origin already embedded — plus the concrete
/// derivation index for this specific address. `None` on `PsbtInput::origin`
/// means the wallet has no recorded origin for this input at all.
pub struct PsbtInputOrigin {
    pub wildcard_descriptor: String,
    pub derivation_index: u32,
}

pub struct PsbtInput {
    pub txid: String,
    pub vout: u32,
    pub amount_sats: u64,
    /// The address the UTXO currently sits at, used to build `witness_utxo`.
    pub address: String,
    pub origin: Option<PsbtInputOrigin>,
}

pub struct PsbtOutput {
    pub address: String,
    pub amount_sats: u64,
    /// Set only for our own change output: the descriptor + index that
    /// produced `address`, so `bip32_derivation` (and scripts) land on the
    /// PSBT output and signers/Sparrow recognise it as change. `None` for
    /// recipient outputs and for wallets with no recorded key origin.
    pub origin: Option<PsbtInputOrigin>,
}

#[derive(Debug)]
pub struct BuiltPsbt {
    pub base64: String,
    pub estimated_vbytes: f64,
    pub total_input_sats: u64,
    pub total_output_sats: u64,
    pub fee_sats: u64,
}

/// Rough vbyte weight per input by detected address type, used for fee estimation.
fn input_vbytes_estimate(address: &Address) -> f64 {
    use bitcoin::address::AddressType::*;
    match address.address_type() {
        Some(P2wpkh) => 68.0,
        // bitcoin::AddressType can't distinguish legacy P2SH from P2SH-wrapped
        // segwit; assume the common wrapped-segwit case.
        Some(P2sh) => 91.0,
        Some(P2pkh) => 148.0,
        Some(P2wsh) => 105.0, // rough 2-of-3 p2wsh estimate; varies with script complexity
        Some(P2tr) => 57.5,
        _ => 100.0,
    }
}

fn output_vbytes_estimate(address: &Address) -> f64 {
    use bitcoin::address::AddressType::*;
    match address.address_type() {
        Some(P2wpkh) => 31.0,
        Some(P2sh) => 32.0,
        Some(P2pkh) => 34.0,
        Some(P2wsh) => 43.0,
        Some(P2tr) => 43.0,
        _ => 40.0,
    }
}

pub fn estimate_vbytes(inputs: &[PsbtInput], outputs: &[PsbtOutput], network: Network) -> Result<f64, PsbtBuildError> {
    let mut total = 10.5; // version + locktime + segwit marker/flag overhead (approx)
    for i in inputs {
        let addr = parse_address(&i.address, network)?;
        total += input_vbytes_estimate(&addr);
    }
    for o in outputs {
        let addr = parse_address(&o.address, network)?;
        total += output_vbytes_estimate(&addr);
    }
    Ok(total)
}

fn parse_address(addr: &str, network: Network) -> Result<Address, PsbtBuildError> {
    let unchecked: Address<bitcoin::address::NetworkUnchecked> =
        addr.parse().map_err(|_| PsbtBuildError::Address(addr.to_string()))?;
    unchecked
        .require_network(network)
        .map_err(|_| PsbtBuildError::Address(addr.to_string()))
}

/// Build an unsigned, RBF-enabled PSBT spending `inputs` to `outputs`. The
/// caller is responsible for having already included a change output in
/// `outputs` if one is needed.
///
/// Every input must carry a known signing origin (see [`PsbtInputOrigin`]).
/// If any input's origin is missing, this returns
/// [`PsbtBuildError::MissingOrigin`] naming the wallet and the specific
/// input, rather than emitting a PSBT that looks complete but that no real
/// signer can actually sign. `wallet_label` is used only for that error
/// message (all inputs here are assumed to come from the same wallet).
pub fn build_unsigned_psbt(
    inputs: &[PsbtInput],
    outputs: &[PsbtOutput],
    network: Network,
    wallet_label: &str,
) -> Result<BuiltPsbt, PsbtBuildError> {
    if inputs.is_empty() {
        return Err(PsbtBuildError::NoInputs);
    }
    if outputs.is_empty() {
        return Err(PsbtBuildError::NoOutputs);
    }

    let tx_ins: Vec<TxIn> = inputs
        .iter()
        .map(|i| {
            let txid = Txid::from_str(&i.txid).map_err(|_| PsbtBuildError::Txid(i.txid.clone()))?;
            Ok(TxIn {
                previous_output: OutPoint { txid, vout: i.vout },
                script_sig: Default::default(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            })
        })
        .collect::<Result<_, PsbtBuildError>>()?;

    let tx_outs: Vec<TxOut> = outputs
        .iter()
        .map(|o| {
            let addr = parse_address(&o.address, network)?;
            Ok(TxOut {
                value: Amount::from_sat(o.amount_sats),
                script_pubkey: addr.script_pubkey(),
            })
        })
        .collect::<Result<_, PsbtBuildError>>()?;

    let unsigned_tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: tx_ins,
        output: tx_outs,
    };

    let mut psbt = Psbt::from_unsigned_tx(unsigned_tx).map_err(|e| PsbtBuildError::Psbt(e.to_string()))?;
    let secp = Secp256k1::new();

    for (i, input) in inputs.iter().enumerate() {
        let addr = parse_address(&input.address, network)?;
        psbt.inputs[i] = PsbtInputData {
            witness_utxo: Some(TxOut {
                value: Amount::from_sat(input.amount_sats),
                script_pubkey: addr.script_pubkey(),
            }),
            ..Default::default()
        };

        let origin = input.origin.as_ref().ok_or_else(|| PsbtBuildError::MissingOrigin {
            wallet: wallet_label.to_string(),
            txid: input.txid.clone(),
            vout: input.vout,
        })?;

        // Parse the wallet's wildcard descriptor (with [fingerprint/path]
        // origin already embedded) and resolve it to this exact input's
        // derivation index. `update_with_descriptor_unchecked` then fills
        // in bip32_derivation for every key, and witness_script/
        // redeem_script for P2WSH / P2SH-P2WSH multisig inputs — all
        // derived from real key material, nothing invented.
        let (descriptor, _keymap) =
            Descriptor::<DescriptorPublicKey>::parse_descriptor(&secp, &origin.wildcard_descriptor).map_err(|e| {
                PsbtBuildError::DescriptorDerivation { txid: input.txid.clone(), vout: input.vout, reason: e.to_string() }
            })?;
        let definite = descriptor.at_derivation_index(origin.derivation_index).map_err(|e| {
            PsbtBuildError::DescriptorDerivation { txid: input.txid.clone(), vout: input.vout, reason: e.to_string() }
        })?;
        psbt.inputs[i].update_with_descriptor_unchecked(&definite).map_err(|e| {
            PsbtBuildError::DescriptorDerivation { txid: input.txid.clone(), vout: input.vout, reason: e.to_string() }
        })?;
    }

    // Change origin: same rule as inputs (real descriptor with embedded
    // [fingerprint/path], nothing invented). Guard that the descriptor really
    // produces this output's script; a mismatch means we'd be labelling a
    // foreign script as ours, so fail rather than emit it.
    for (i, output) in outputs.iter().enumerate() {
        let Some(origin) = &output.origin else { continue };
        let derr = |reason: String| PsbtBuildError::ChangeDerivation { address: output.address.clone(), reason };
        let (descriptor, _keymap) = Descriptor::<DescriptorPublicKey>::parse_descriptor(&secp, &origin.wildcard_descriptor)
            .map_err(|e| derr(e.to_string()))?;
        let definite = descriptor.at_derivation_index(origin.derivation_index).map_err(|e| derr(e.to_string()))?;
        if definite.script_pubkey() != psbt.unsigned_tx.output[i].script_pubkey {
            return Err(derr("descriptor does not produce this change address".to_string()));
        }
        psbt.outputs[i].update_with_descriptor_unchecked(&definite).map_err(|e| derr(e.to_string()))?;
    }

    let total_input_sats: u64 = inputs.iter().map(|i| i.amount_sats).sum();
    let total_output_sats: u64 = outputs.iter().map(|o| o.amount_sats).sum();
    let estimated_vbytes = estimate_vbytes(inputs, outputs, network)?;

    let serialized = psbt.serialize();
    Ok(BuiltPsbt {
        base64: STANDARD.encode(serialized),
        estimated_vbytes,
        total_input_sats,
        total_output_sats,
        fee_sats: total_input_sats.saturating_sub(total_output_sats),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitcoin::descriptors;
    use crate::db::models::Cosigner;

    // Public master xpubs from the published BIP-32 test vectors
    // (https://github.com/bitcoin/bips/blob/master/bip-0032.mediawiki).
    // These are test-vector *public* keys only — no private keys, no
    // network access, fixed/deterministic across runs.
    const TEST_XPUB_1: &str = "xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8";
    const TEST_XPUB_2: &str = "xpub661MyMwAqRbcFW31YEwpkMuc5THy2PSt5bDMsktWQcFF8syAmRUapSCGu8ED9W6oDMSgv6Zz8idoc4a6mr8BDzTJY47LJhkJ8UB7WEGuduB";
    const TEST_XPUB_3: &str = "xpub661MyMwAqRbcEZVB4dScxMAdx6d4nFc9nvyvH3v4gJL378CSRZiYmhRoP7mBy6gSPSCYk6SzXPTf3ND1cZAceL7SfJ1Z3GC8vBgp2epUt13";

    fn fixed_txid(byte: u8) -> String {
        [byte; 32].iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Test-only helper: derive the actual receive address for a wildcard
    /// descriptor at a concrete index, so `PsbtInput::address` in the fixture
    /// is consistent with the descriptor doing the signing-metadata work.
    fn address_at(wildcard_descriptor: &str, index: u32, network: Network) -> String {
        let secp = Secp256k1::new();
        let (descriptor, _keymap) =
            Descriptor::<DescriptorPublicKey>::parse_descriptor(&secp, wildcard_descriptor).expect("valid descriptor");
        let definite = descriptor.at_derivation_index(index).expect("valid index");
        definite.address(network).expect("script has an address form").to_string()
    }

    fn cosigner(label: &str, xpub: &str, fingerprint: &str, path: &str) -> Cosigner {
        Cosigner {
            id: String::new(),
            label: label.to_string(),
            xpub: xpub.to_string(),
            fingerprint: Some(fingerprint.to_string()),
            derivation_path: Some(path.to_string()),
            is_mine: false,
            hardware_model: None,
        }
    }

    #[test]
    fn singlesig_wpkh_psbt_has_bip32_derivation_and_no_witness_script() {
        let network = Network::Bitcoin;
        let pair = descriptors::descriptors_from_extended_key(TEST_XPUB_1, network, Some(("aabbccdd", "84'/0'/0'")))
            .expect("valid xpub with origin");

        let input_addr = address_at(&pair.external, 5, network);
        let inputs = vec![PsbtInput {
            txid: fixed_txid(0x11),
            vout: 0,
            amount_sats: 100_000,
            address: input_addr,
            origin: Some(PsbtInputOrigin { wildcard_descriptor: pair.external.clone(), derivation_index: 5 }),
        }];
        let outputs = vec![PsbtOutput { address: address_at(&pair.internal, 0, network), amount_sats: 90_000, origin: None }];

        let built = build_unsigned_psbt(&inputs, &outputs, network, "Singlesig Test Wallet").expect("psbt should build");
        let bytes = STANDARD.decode(&built.base64).expect("valid base64");
        let psbt = Psbt::deserialize(&bytes).expect("valid psbt bytes");

        let input0 = &psbt.inputs[0];
        assert!(input0.witness_utxo.is_some(), "witness_utxo must be set");
        assert_eq!(input0.bip32_derivation.len(), 1, "exactly one signing key for p2wpkh");
        let (_, (fingerprint, path)) = input0.bip32_derivation.iter().next().unwrap();
        assert_eq!(fingerprint.to_string(), "aabbccdd");
        assert_eq!(path.to_string(), "84'/0'/0'/0/5");
        assert!(input0.witness_script.is_none(), "p2wpkh has no witness_script");
        assert!(input0.redeem_script.is_none(), "p2wpkh has no redeem_script");
    }

    #[test]
    fn multisig_wsh_2of3_psbt_has_bip32_derivation_and_witness_script() {
        let network = Network::Bitcoin;
        let cosigners = vec![
            cosigner("A", TEST_XPUB_1, "aaaaaaaa", "48'/0'/0'/2'"),
            cosigner("B", TEST_XPUB_2, "bbbbbbbb", "48'/0'/0'/2'"),
            cosigner("C", TEST_XPUB_3, "cccccccc", "48'/0'/0'/2'"),
        ];
        let pair = descriptors::descriptors_from_multisig(2, &cosigners, network, "p2wsh").expect("valid multisig");

        let input_addr = address_at(&pair.external, 0, network);
        let inputs = vec![PsbtInput {
            txid: fixed_txid(0x22),
            vout: 1,
            amount_sats: 500_000,
            address: input_addr,
            origin: Some(PsbtInputOrigin { wildcard_descriptor: pair.external.clone(), derivation_index: 0 }),
        }];
        let outputs = vec![PsbtOutput { address: address_at(&pair.internal, 0, network), amount_sats: 480_000, origin: None }];

        let built = build_unsigned_psbt(&inputs, &outputs, network, "Multisig Test Vault").expect("psbt should build");
        let bytes = STANDARD.decode(&built.base64).expect("valid base64");
        let psbt = Psbt::deserialize(&bytes).expect("valid psbt bytes");

        let input0 = &psbt.inputs[0];
        assert!(input0.witness_utxo.is_some(), "witness_utxo must be set");
        assert_eq!(input0.bip32_derivation.len(), 3, "one bip32_derivation entry per cosigner");
        assert!(input0.witness_script.is_some(), "native p2wsh multisig must carry witness_script");
        assert!(input0.redeem_script.is_none(), "native p2wsh (not wrapped) has no redeem_script");

        let fingerprints: std::collections::BTreeSet<String> =
            input0.bip32_derivation.values().map(|(fp, _)| fp.to_string()).collect();
        assert_eq!(
            fingerprints,
            ["aaaaaaaa", "bbbbbbbb", "cccccccc"].into_iter().map(String::from).collect()
        );
    }

    #[test]
    fn multisig_p2sh_p2wsh_2of3_psbt_has_witness_script_and_redeem_script() {
        let network = Network::Bitcoin;
        let cosigners = vec![
            cosigner("A", TEST_XPUB_1, "aaaaaaaa", "49'/0'/0'/2'"),
            cosigner("B", TEST_XPUB_2, "bbbbbbbb", "49'/0'/0'/2'"),
            cosigner("C", TEST_XPUB_3, "cccccccc", "49'/0'/0'/2'"),
        ];
        let pair =
            descriptors::descriptors_from_multisig(2, &cosigners, network, "p2sh-p2wsh").expect("valid multisig");

        let input_addr = address_at(&pair.external, 3, network);
        let inputs = vec![PsbtInput {
            txid: fixed_txid(0x33),
            vout: 0,
            amount_sats: 250_000,
            address: input_addr,
            origin: Some(PsbtInputOrigin { wildcard_descriptor: pair.external.clone(), derivation_index: 3 }),
        }];
        let outputs = vec![PsbtOutput { address: address_at(&pair.internal, 0, network), amount_sats: 240_000, origin: None }];

        let built = build_unsigned_psbt(&inputs, &outputs, network, "P2SH-P2WSH Test Vault").expect("psbt should build");
        let bytes = STANDARD.decode(&built.base64).expect("valid base64");
        let psbt = Psbt::deserialize(&bytes).expect("valid psbt bytes");

        let input0 = &psbt.inputs[0];
        assert_eq!(input0.bip32_derivation.len(), 3);
        assert!(input0.witness_script.is_some(), "p2sh-p2wsh multisig must carry witness_script");
        assert!(input0.redeem_script.is_some(), "p2sh-p2wsh multisig must carry redeem_script");
    }

    #[test]
    fn missing_origin_fails_build_instead_of_emitting_unsignable_psbt() {
        let network = Network::Bitcoin;
        // No `[fingerprint/path]` origin supplied — Nakos must refuse.
        let pair = descriptors::descriptors_from_extended_key(TEST_XPUB_1, network, None).expect("valid xpub");

        let inputs = vec![PsbtInput {
            txid: fixed_txid(0x44),
            vout: 0,
            amount_sats: 50_000,
            address: address_at(&pair.external, 0, network),
            origin: None,
        }];
        let outputs = vec![PsbtOutput { address: address_at(&pair.external, 1, network), amount_sats: 40_000, origin: None }];

        let err = build_unsigned_psbt(&inputs, &outputs, network, "Originless Wallet")
            .expect_err("build must fail rather than emit an unsignable psbt");
        match err {
            PsbtBuildError::MissingOrigin { wallet, txid, vout } => {
                assert_eq!(wallet, "Originless Wallet");
                assert_eq!(txid, fixed_txid(0x44));
                assert_eq!(vout, 0);
            }
            other => panic!("expected MissingOrigin, got {other:?}"),
        }
    }

    #[test]
    fn multisig_rejects_cosigner_missing_fingerprint() {
        let network = Network::Bitcoin;
        let mut c2 = cosigner("B", TEST_XPUB_2, "bbbbbbbb", "48'/0'/0'/2'");
        c2.fingerprint = None; // simulates a cosigner saved without an origin
        let cosigners = vec![cosigner("A", TEST_XPUB_1, "aaaaaaaa", "48'/0'/0'/2'"), c2];

        let err = descriptors::descriptors_from_multisig(2, &cosigners, network, "p2wsh")
            .expect_err("must reject a cosigner with no recorded origin");
        match err {
            descriptors::DescriptorError::CosignerMissingOrigin { label } => assert_eq!(label, "B"),
            other => panic!("expected CosignerMissingOrigin, got {other:?}"),
        }
    }

    fn change_fixture(network: Network) -> (descriptors::DescriptorPair, Vec<PsbtInput>) {
        let pair = descriptors::descriptors_from_extended_key(TEST_XPUB_1, network, Some(("aabbccdd", "84'/0'/0'")))
            .expect("valid xpub with origin");
        let inputs = vec![PsbtInput {
            txid: fixed_txid(0x33),
            vout: 0,
            amount_sats: 100_000,
            address: address_at(&pair.external, 5, network),
            origin: Some(PsbtInputOrigin { wildcard_descriptor: pair.external.clone(), derivation_index: 5 }),
        }];
        (pair, inputs)
    }

    #[test]
    fn change_output_with_origin_carries_bip32_derivation_like_inputs() {
        let network = Network::Bitcoin;
        let (pair, inputs) = change_fixture(network);
        let outputs = vec![
            PsbtOutput { address: address_at(&pair.external, 9, network), amount_sats: 40_000, origin: None },
            PsbtOutput {
                address: address_at(&pair.internal, 3, network),
                amount_sats: 59_000,
                origin: Some(PsbtInputOrigin { wildcard_descriptor: pair.internal.clone(), derivation_index: 3 }),
            },
        ];
        let built = build_unsigned_psbt(&inputs, &outputs, network, "W").expect("psbt builds");
        let psbt = Psbt::deserialize(&STANDARD.decode(&built.base64).unwrap()).unwrap();

        assert!(psbt.outputs[0].bip32_derivation.is_empty(), "recipient output must stay origin-free");
        assert_eq!(psbt.outputs[1].bip32_derivation.len(), 1, "change has one key, same structure as a p2wpkh input");
        let (_, (fp, path)) = psbt.outputs[1].bip32_derivation.iter().next().unwrap();
        assert_eq!(fp.to_string(), "aabbccdd");
        assert_eq!(path.to_string(), "84'/0'/0'/1/3");
    }

    #[test]
    fn change_without_origin_gets_no_derivation() {
        let network = Network::Bitcoin;
        let (pair, inputs) = change_fixture(network);
        let outputs = vec![PsbtOutput { address: address_at(&pair.internal, 0, network), amount_sats: 90_000, origin: None }];
        let built = build_unsigned_psbt(&inputs, &outputs, network, "W").expect("psbt builds");
        let psbt = Psbt::deserialize(&STANDARD.decode(&built.base64).unwrap()).unwrap();
        assert!(psbt.outputs[0].bip32_derivation.is_empty());
    }

    #[test]
    fn change_origin_that_does_not_match_the_address_fails_closed() {
        let network = Network::Bitcoin;
        let (pair, inputs) = change_fixture(network);
        // Address is internal/3 but the origin claims index 4.
        let outputs = vec![PsbtOutput {
            address: address_at(&pair.internal, 3, network),
            amount_sats: 90_000,
            origin: Some(PsbtInputOrigin { wildcard_descriptor: pair.internal.clone(), derivation_index: 4 }),
        }];
        let err = build_unsigned_psbt(&inputs, &outputs, network, "W").unwrap_err();
        assert!(matches!(err, PsbtBuildError::ChangeDerivation { .. }), "got {err:?}");
    }
}
