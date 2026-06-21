//! The real Send flow (Phase A3): turns a user-initiated "send `amount` to
//! `recipient`" into an on-chain `transfer`. Ties together every A2 piece —
//! config + live epoch ([`super::config`]/[`super::chain`]), note selection
//! ([`super::scan`]), the stateful witness generator ([`super::witness`]),
//! client-side proving ([`super::proving`]), note encryption ([`super::encrypt`]) —
//! and submits via the native Rust submitter ([`super::chain::submit_transfer`], G14).
//!
//! v1 spends ONE owned note covering `amount` (2-in/2-out with a dummy second input):
//! output 0 = `amount` to the recipient, output 1 = change back to the sender.

use super::config::PoolConfig;
use super::encrypt::{self, NotePlaintext};
use super::poseidon::{Fr, Hasher, SELECTOR_TRANSFER};
use super::scan::{self, OwnedNote, WalletIdentity};
use super::witness::{TransferInputs, TransferWitness};
use super::{chain, keys, notes, proving, CoreError};

// ----------------------------- payment code -----------------------------

/// A shielded receive address: `owner_pk (32) ‖ transmission_pub (32)`, hex. The
/// sender needs both — `owner_pk` to form the output note's commitment, the
/// transmission key to encrypt the note so only the recipient can find/spend it.
pub fn payment_code(id: &WalletIdentity) -> String {
    let mut b = Vec::with_capacity(64);
    b.extend_from_slice(&id.owner_pk.0);
    b.extend_from_slice(&id.transmission_pub);
    format!("ozky{}", hex::encode(b))
}

/// Parse a payment code into (recipient `owner_pk`, recipient transmission pub).
pub fn parse_payment_code(code: &str) -> Result<(Fr, [u8; 32]), CoreError> {
    let hexpart = code.strip_prefix("ozky").unwrap_or(code);
    let bytes = hex::decode(hexpart)
        .map_err(|_| CoreError::Crypto("payment code is not valid hex".into()))?;
    if bytes.len() != 64 {
        return Err(CoreError::Crypto(format!(
            "payment code must be 64 bytes, got {}",
            bytes.len()
        )));
    }
    let mut owner_pk = [0u8; 32];
    owner_pk.copy_from_slice(&bytes[0..32]);
    let mut transmission_pub = [0u8; 32];
    transmission_pub.copy_from_slice(&bytes[32..64]);
    Ok((Fr(owner_pk), transmission_pub))
}

// ----------------------------- witness construction -----------------------------

/// Output-note randomness (blindings + rhos + the dummy input's rho). A distinct,
/// unpredictable set per send so output notes are unlinkable and nullifiers unique.
pub struct OutputRandomness {
    pub out0_blinding: Fr,
    pub out0_rho: Fr,
    pub change_blinding: Fr,
    pub change_rho: Fr,
    pub dummy_rho: Fr,
}

impl OutputRandomness {
    pub fn random() -> OutputRandomness {
        OutputRandomness {
            out0_blinding: Fr::random(),
            out0_rho: Fr::random(),
            change_blinding: Fr::random(),
            change_rho: Fr::random(),
            dummy_rho: Fr::random(),
        }
    }
}

/// Build the transfer witness for spending `note` against LIVE pool state, sending
/// `amount` to `recipient_owner_pk` with change back to the sender. Pure (no
/// network/keychain) so it is unit-testable; the public inputs bind to `cfg`'s
/// pool/network/asset and `epoch` (current). `note_epoch` is the note's own epoch.
#[allow(clippy::too_many_arguments)]
pub fn build_transfer_witness(
    h: &Hasher,
    id: &WalletIdentity,
    cfg: &PoolConfig,
    epoch: u32,
    note: &OwnedNote,
    commitment_leaves: &[Fr],
    prior_nullifiers: &[Fr],
    asp_leaves: &[Fr],
    recipient_owner_pk: Fr,
    amount: u64,
    rnd: &OutputRandomness,
) -> Result<TransferWitness, CoreError> {
    if note.value < amount {
        return Err(CoreError::Proving(format!(
            "selected note ({}) does not cover amount ({amount})",
            note.value
        )));
    }
    if note.asset_tag != cfg.asset_tag {
        return Err(CoreError::Proving("note asset_tag != pool asset_tag".into()));
    }
    // The spender must be in the pool's ASP approved set (anonymity set of size
    // `asp_leaves.len()`); proving `owner_pk ∈ asp_root` for a hidden index.
    if !asp_leaves.contains(&id.owner_pk) {
        return Err(CoreError::Proving(
            "wallet not enrolled in this pool's ASP approved set (cannot prove membership)".into(),
        ));
    }
    let domain_sep = h.domain_sep(&cfg.pool_id, &cfg.network_id, SELECTOR_TRANSFER);

    Ok(TransferWitness::build(
        h,
        TransferInputs {
            owner_sk: id.owner_sk,
            asset_tag: cfg.asset_tag,
            epoch: Fr::from_u64(epoch as u64),
            note_epoch: Fr::from_u64(note.epoch as u64),
            domain_sep,
            note_value: note.value,
            note_blinding: note.blinding,
            note_rho: note.rho,
            note_leaf_index: note.leaf_index as usize,
            commitment_leaves,
            asp_leaves,
            prior_nullifiers,
            dummy_rho: rnd.dummy_rho,
            recipient_owner_pk,
            out0_value: amount,
            out0_blinding: rnd.out0_blinding,
            out0_rho: rnd.out0_rho,
            change_blinding: rnd.change_blinding,
            change_rho: rnd.change_rho,
        },
    ))
}

/// Encrypt the two output notes: out0 to the recipient, change (out1) back to the
/// sender — each carrying the fields needed to recompute its commitment on scan.
fn output_payloads(
    cfg: &PoolConfig,
    id: &WalletIdentity,
    epoch: u32,
    amount: u64,
    change: u64,
    recipient_transmission_pub: &[u8; 32],
    rnd: &OutputRandomness,
) -> Result<Vec<chain::OutputPayload>, CoreError> {
    let to_recipient = NotePlaintext {
        value: amount,
        asset_tag: cfg.asset_tag,
        blinding: rnd.out0_blinding,
        epoch,
        rho: rnd.out0_rho,
    };
    let to_self = NotePlaintext {
        value: change,
        asset_tag: cfg.asset_tag,
        blinding: rnd.change_blinding,
        epoch,
        rho: rnd.change_rho,
    };
    let e0 = encrypt::encrypt_note(&to_recipient.serialize(), recipient_transmission_pub)?;
    let e1 = encrypt::encrypt_note(&to_self.serialize(), &id.transmission_pub)?;
    Ok(vec![
        chain::OutputPayload { enc_note: e0.enc_note, ephemeral_pub: e0.ephemeral_pub, view_tag: e0.view_tag },
        chain::OutputPayload { enc_note: e1.enc_note, ephemeral_pub: e1.ephemeral_pub, view_tag: e1.view_tag },
    ])
}

// ----------------------------- orchestration -----------------------------

/// Send `amount` of `asset` (a v1 code, e.g. "USDC") privately to the holder of
/// `recipient_code`, using the wallet stored in the OS keychain. Thin wrapper over
/// [`send_with`].
pub fn send(asset: &str, recipient_code: &str, amount: u64) -> Result<String, CoreError> {
    let wallet = keys::current_wallet()?;
    let cfg = PoolConfig::load()?.with_asset(asset)?;
    send_with(&wallet, &cfg, recipient_code, amount)
}

/// Send with an explicit wallet + config (keychain-independent). Reads live pool
/// state (epoch, commitment leaves, spent nullifiers) from the chain client, selects
/// an owned note covering `amount`, then delegates to [`send_prepared`].
pub fn send_with(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    recipient_code: &str,
    amount: u64,
) -> Result<String, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let epoch = chain::current_epoch(&cfg.rpc_url)?;
    // One RPC drain of the target pool -> commitment leaves + nullifier set + owned notes.
    let state = chain::pool_state(cfg)?;
    let commitment_leaves = chain::commitment_leaves_from(&state.commits)?;
    let asp_leaves = chain::approved_set(cfg)?;
    let local = notes::load(wallet)?;

    // Select an owned, unspent note that covers `amount` (single-input v1); includes
    // notes recovered from the local store (e.g. prior withdraw change).
    let note = scan::owned_notes(&id, &state, &local, 0)?
        .into_iter()
        .find(|n| n.value >= amount && n.asset_tag == cfg.asset_tag)
        .ok_or_else(|| CoreError::Proving(format!("no single owned note covers {amount}")))?;

    send_prepared(
        wallet,
        cfg,
        recipient_code,
        amount,
        &note,
        &commitment_leaves,
        &state.nullifiers,
        &asp_leaves,
        epoch,
    )
}

/// Send against EXPLICIT live state — the state-injected core of the send flow
/// (build witness -> prove -> encrypt -> submit). Separated from [`send_with`] so the
/// caller can supply pool state from any source (raw RPC, or, in the live-run driver,
/// ground truth it already holds). `asp_leaves` is the pool's approved set (the
/// anonymity set the spender proves membership in). Returns the transaction hash.
#[allow(clippy::too_many_arguments)]
pub fn send_prepared(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    recipient_code: &str,
    amount: u64,
    note: &OwnedNote,
    commitment_leaves: &[Fr],
    prior_nullifiers: &[Fr],
    asp_leaves: &[Fr],
    epoch: u32,
) -> Result<String, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let h = Hasher::new();
    let (recipient_owner_pk, recipient_transmission_pub) = parse_payment_code(recipient_code)?;
    let change = note.value - amount;

    let rnd = OutputRandomness::random();
    let witness = build_transfer_witness(
        &h,
        &id,
        cfg,
        epoch,
        note,
        commitment_leaves,
        prior_nullifiers,
        asp_leaves,
        recipient_owner_pk,
        amount,
        &rnd,
    )?;

    // Prove (verifies against the frozen VK before returning); the proof + public_inputs
    // bytes are submitted natively from memory (no in-container file paths — G14).
    let bundle = proving::prove_transfer_witness(&witness)?;

    let outputs = output_payloads(
        cfg,
        &id,
        epoch,
        amount,
        change,
        &recipient_transmission_pub,
        &rnd,
    )?;

    // Submit via the relayer if configured (fee abstraction: the user holds no XLM and
    // isn't linked as the fee-payer of this private transfer), else the wallet itself.
    chain::submit_transfer(
        cfg,
        cfg.submit_source(wallet.stellar_secret()),
        &bundle.public_inputs,
        &bundle.proof,
        &outputs,
    )
}

/// This wallet's shielded receive address (payment code).
pub fn receive_code() -> Result<String, CoreError> {
    let wallet = keys::current_wallet()?;
    Ok(payment_code(&scan::wallet_identity(&wallet)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg() -> PoolConfig {
        PoolConfig {
            pool_contract: "CTEST".into(),
            policy_contract: "CPOLICY".into(),
            viewkeys_contract: None,
            pool_id: Fr::from_u64(7),
            network_id: Fr::from_u64(42),
            asset_tag: Fr::from_u64(1),
            rpc_url: "http://localhost".into(),
            network: "testnet".into(),
            network_passphrase: "Test SDF Network ; September 2015".into(),
            relayer_secret: None,
        }
    }

    #[test]
    fn submit_source_prefers_relayer() {
        let mut cfg = test_cfg();
        // No relayer -> wallet pays its own fee.
        assert_eq!(cfg.submit_source("SWALLET"), "SWALLET");
        // Relayer configured -> the relayer is the fee-payer (user holds no XLM).
        cfg.relayer_secret = Some("SRELAYER".into());
        assert_eq!(cfg.submit_source("SWALLET"), "SRELAYER");
    }

    fn test_identity(h: &Hasher) -> WalletIdentity {
        let owner_sk = Fr::from_u64(12345);
        let ivk = [7u8; 32];
        WalletIdentity {
            owner_sk,
            owner_pk: h.owner_pk(&owner_sk),
            transmission_sk: encrypt::transmission_secret(&ivk),
            transmission_pub: encrypt::transmission_public(&ivk),
        }
    }

    #[test]
    fn payment_code_roundtrips() {
        let h = Hasher::new();
        let id = test_identity(&h);
        let code = payment_code(&id);
        assert!(code.starts_with("ozky"));
        let (pk, tx) = parse_payment_code(&code).unwrap();
        assert_eq!(pk, id.owner_pk);
        assert_eq!(tx, id.transmission_pub);
    }

    #[test]
    fn parse_rejects_bad_code() {
        assert!(parse_payment_code("ozkynothex").is_err());
        assert!(parse_payment_code("ozky00").is_err()); // wrong length
    }

    // Fresh-pool demo inputs (epoch 28). The witness this builds must match the FROZEN
    // Noir witgen public values — i.e. build_transfer_witness binds correctly to cfg.
    const DSEP_TRANSFER_28: &str =
        "0x2eae4c361f605c06c766cb126a391a0f916308610ae8128f7e615e5e6b6c67ff";
    const COMMITMENT_ROOT: &str =
        "0x16c6e766b9ecd7bcbaede4a371f17104130d1e65794c63cb3b91a5f1323b608e";
    const ASP_ROOT: &str =
        "0x1610446d123b3be5a338712bcf508007d94184a71cb8045dd351cbd68a52b8dd";

    fn demo_owned_note(h: &Hasher, id: &WalletIdentity) -> (OwnedNote, Vec<Fr>) {
        let commitment = h.commitment(
            &Fr::from_u64(1000),
            &Fr::from_u64(1),
            &id.owner_pk,
            &Fr::from_u64(777),
            &Fr::from_u64(28),
            &Fr::from_u64(111),
        );
        let note = OwnedNote {
            leaf_index: 0,
            value: 1000,
            asset_tag: Fr::from_u64(1),
            blinding: Fr::from_u64(777),
            epoch: 28,
            rho: Fr::from_u64(111),
            commitment,
        };
        (note, vec![commitment])
    }

    #[test]
    fn witness_binds_to_config_and_conserves_value() {
        let h = Hasher::new();
        let id = test_identity(&h);
        let cfg = test_cfg();
        let (note, leaves) = demo_owned_note(&h, &id);
        let rnd = OutputRandomness {
            out0_blinding: Fr::from_u64(222),
            out0_rho: Fr::from_u64(333),
            change_blinding: Fr::from_u64(444),
            change_rho: Fr::from_u64(555),
            dummy_rho: Fr::from_u64(0xdead),
        };
        let recipient = h.owner_pk(&Fr::from_u64(99));
        let asp = [id.owner_pk]; // single-member approved set (matches the frozen ASP_ROOT)
        let w = build_transfer_witness(&h, &id, &cfg, 28, &note, &leaves, &[], &asp, recipient, 600, &rnd)
            .unwrap();

        // domain_sep binds the pool/network/TRANSFER selector.
        assert_eq!(w.domain_sep.to_hex(), DSEP_TRANSFER_28, "domain_sep");
        // asp_root == single-leaf(owner_pk); commitment_root matches the frozen vector.
        assert_eq!(w.asp_root.to_hex(), ASP_ROOT, "asp_root");
        assert_eq!(w.commitment_root.to_hex(), COMMITMENT_ROOT, "commitment_root");
        // Value conservation: out0 (600) + change (400) == note (1000).
        assert_eq!(w.outputs[0].value, Fr::from_u64(600));
        assert_eq!(w.outputs[1].value, Fr::from_u64(400));
        // Recipient output carries the recipient's owner_pk; change returns to sender.
        assert_eq!(w.outputs[0].owner_pk, recipient);
        assert_eq!(w.outputs[1].owner_pk, id.owner_pk);
    }

    #[test]
    fn witness_rejects_insufficient_note() {
        let h = Hasher::new();
        let id = test_identity(&h);
        let cfg = test_cfg();
        let (note, leaves) = demo_owned_note(&h, &id);
        let rnd = OutputRandomness::random();
        // Note holds 1000; asking to send 2000 must fail.
        let asp = [id.owner_pk];
        let r = build_transfer_witness(&h, &id, &cfg, 28, &note, &leaves, &[], &asp, id.owner_pk, 2000, &rnd);
        assert!(r.is_err());
    }

    #[test]
    fn witness_rejects_unenrolled_spender() {
        let h = Hasher::new();
        let id = test_identity(&h);
        let cfg = test_cfg();
        let (note, leaves) = demo_owned_note(&h, &id);
        let rnd = OutputRandomness::random();
        // An approved set that does NOT contain our owner_pk -> can't prove membership.
        let asp = [h.owner_pk(&Fr::from_u64(1)), h.owner_pk(&Fr::from_u64(2))];
        let r = build_transfer_witness(&h, &id, &cfg, 28, &note, &leaves, &[], &asp, id.owner_pk, 600, &rnd);
        assert!(r.is_err(), "spender not in the approved set must be rejected");
    }

    #[test]
    fn witness_membership_in_set_of_three() {
        // The spender is one of THREE approved keys (a real anonymity set): the proof
        // reveals only asp_root, proving owner_pk ∈ set for a hidden index.
        let h = Hasher::new();
        let id = test_identity(&h);
        let cfg = test_cfg();
        let (note, leaves) = demo_owned_note(&h, &id);
        let rnd = OutputRandomness::random();
        let asp = [h.owner_pk(&Fr::from_u64(0xDEC0)), id.owner_pk, h.owner_pk(&Fr::from_u64(0xDEC1))];
        let w = build_transfer_witness(&h, &id, &cfg, 28, &note, &leaves, &[], &asp, h.owner_pk(&Fr::from_u64(99)), 600, &rnd)
            .expect("member of a 3-key set can build the witness");
        // asp_root is the 3-leaf root (NOT the single-leaf vector) — a real anon set.
        assert_ne!(w.asp_root.to_hex(), ASP_ROOT, "multi-member root differs from single-leaf");
    }

    #[test]
    fn change_output_decrypts_back_to_self() {
        // The change payload (out1) must be recoverable by the sender's own keys.
        let h = Hasher::new();
        let id = test_identity(&h);
        let cfg = test_cfg();
        let rnd = OutputRandomness::random();
        let outputs =
            output_payloads(&cfg, &id, 28, 600, 400, &id.transmission_pub, &rnd).unwrap();
        let change = &outputs[1];
        let pt = encrypt::decrypt_note(&change.enc_note, &change.ephemeral_pub, &id.transmission_sk)
            .unwrap();
        let note = NotePlaintext::deserialize(&pt).unwrap();
        assert_eq!(note.value, 400);
        assert_eq!(note.epoch, 28);
    }

    // ---------------------------------------------------------------------------
    // Live-run driver (the committed "script the live run" deliverable). Runs the
    // FULL Send lifecycle against testnet through the real app code path:
    //   fund -> deploy verifiers/policy/pool (asp_root bound to the test wallet's
    //   owner_pk) -> register asset -> deposit a wallet-owned note (proof built by
    //   the core) -> SEND via send_with -> scan and confirm the outputs landed.
    //
    // It is #[ignore]d (needs the ZK container, network, ~minutes) and uses a fixed
    // throwaway test wallet derived directly (never the user's keychain). Run with:
    //   cargo test --lib -- --ignored --test-threads=1 send_lifecycle_on_testnet
    // Prereq: pool/policy/verifier wasm built (contracts/target/wasm32v1-none/release)
    // and the CRS volume warmed (see ERRORS.md).
    use crate::core::{deposit, withdraw};
    use std::process::Command;

    /// A throwaway test wallet (the SEP-0005 vector phrase) — NOT the user's wallet.
    const TEST_MNEMONIC: &str =
        "illness spike retreat truth genius clock brain pass fit cave bargain toe";

    fn repo_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
    }

    /// Run a bash script in the ZK container with the wallet secret forwarded as
    /// `$OZKY_SOURCE_SECRET` (never argv). Returns stdout; panics with the stderr tail.
    fn run_zk(secret: &str, script: &str) -> String {
        let compose = repo_root().join("compose.zk.yaml");
        let out = Command::new("docker")
            .env("OZKY_SOURCE_SECRET", secret)
            .args(["compose", "-f"])
            .arg(&compose)
            .args(["run", "--rm", "-e", "OZKY_SOURCE_SECRET", "zk", "bash", "-c", script])
            .output()
            .expect("spawn docker");
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            panic!("zk script failed:\n{}", err.lines().rev().take(25).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"));
        }
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn kv(out: &str, key: &str) -> String {
        out.lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")))
            .unwrap_or_else(|| panic!("no {key} in output:\n{out}"))
            .trim()
            .to_string()
    }

    #[test]
    #[ignore = "live testnet lifecycle; needs ZK container + network. run with --ignored --test-threads=1"]
    fn send_lifecycle_on_testnet() {
        let wallet = keys::derive_from_mnemonic(TEST_MNEMONIC).unwrap();
        let h = Hasher::new();
        let id = scan::wallet_identity(&wallet).unwrap();
        let addr = wallet.stellar_address().to_string();
        let secret = wallet.stellar_secret().to_string();
        // Isolated, fresh local notes store for this run (withdraw change lands here).
        let notes_dir = std::env::temp_dir().join(format!("ozky-notes-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);
        // A REAL anonymity set: enroll the wallet + 2 decoy approved keys (set of 3).
        // owner_pk decimals for the policy `enroll`/`approve_member` U256 args.
        let wallet_pk = id.owner_pk.to_decimal();
        let decoy0 = h.owner_pk(&Fr::from_u64(0xDEC0)).to_decimal();
        let decoy1 = h.owner_pk(&Fr::from_u64(0xDEC1)).to_decimal();

        // --- 1. fund + deploy verifiers/policy, enroll a 3-key ASP set, deploy pool ---
        let setup = format!(
            "set -e\n\
             stellar network add testnet --rpc-url https://soroban-testnet.stellar.org --network-passphrase 'Test SDF Network ; September 2015' 2>/dev/null || true\n\
             curl -s 'https://friendbot.stellar.org/?addr={addr}' >/dev/null || true\n\
             T=/workspace/contracts/target/wasm32v1-none/release\n\
             VK=/workspace/contracts/frozen_vks\n\
             V=$T/rs_soroban_ultrahonk.wasm\n\
             VDEP=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/deposit/vk)\n\
             VTRA=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/transfer/vk)\n\
             VWIT=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/withdraw/vk)\n\
             POLICY=$(stellar contract deploy --wasm $T/policy.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --admin {addr})\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- enroll --owner_pk {wallet_pk} --who {addr} >/dev/null\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- approve_member --owner_pk {decoy0} >/dev/null\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- approve_member --owner_pk {decoy1} >/dev/null\n\
             ASP=$(stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet -- asp_root | tr -d '\\\"')\n\
             SAC=$(stellar contract id asset --asset native --network testnet)\n\
             stellar contract asset deploy --asset native --source \"$OZKY_SOURCE_SECRET\" --network testnet >/dev/null 2>&1 || true\n\
             POOL=$(stellar contract deploy --wasm $T/pool.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --pool_id 7 --network_id 42 --deposit_verifier $VDEP --transfer_verifier $VTRA --withdraw_verifier $VWIT --policy $POLICY --asp_root $ASP --admin {addr})\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 1 --sac $SAC --decimals 7 >/dev/null\n\
             USDC_ISSUER=GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5\n\
             USDC_SAC=$(stellar contract id asset --asset USDC:$USDC_ISSUER --network testnet)\n\
             stellar contract asset deploy --asset USDC:$USDC_ISSUER --source \"$OZKY_SOURCE_SECRET\" --network testnet >/dev/null 2>&1 || true\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 2 --sac $USDC_SAC --decimals 7 >/dev/null\n\
             VIEWKEYS=$(stellar contract deploy --wasm $T/viewkeys.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet)\n\
             stellar keys generate dest --network testnet --fund --overwrite >/dev/null 2>&1\n\
             stellar tx new change-trust --source-account dest --network testnet --line USDC:$USDC_ISSUER >/dev/null 2>&1\n\
             stellar keys generate relayer --network testnet --fund --overwrite >/dev/null 2>&1\n\
             stellar keys generate auditor --network testnet --fund --overwrite >/dev/null 2>&1\n\
             echo \"POOL=$POOL\"\n\
             echo \"POLICY=$POLICY\"\n\
             echo \"VIEWKEYS=$VIEWKEYS\"\n\
             echo \"SAC=$SAC\"\n\
             echo \"USDC_SAC=$USDC_SAC\"\n\
             echo \"DEST=$(stellar keys address dest)\"\n\
             echo \"AUDITOR=$(stellar keys address auditor)\"\n\
             echo \"RELAYER_ADDR=$(stellar keys address relayer)\"\n\
             echo \"RELAYER_SECRET=$(stellar keys secret relayer)\"",
            addr = addr, wallet_pk = wallet_pk, decoy0 = decoy0, decoy1 = decoy1,
        );
        let setup_out = run_zk(&secret, &setup);
        let pool = kv(&setup_out, "POOL");
        let policy = kv(&setup_out, "POLICY");
        let viewkeys = kv(&setup_out, "VIEWKEYS");
        let sac = kv(&setup_out, "SAC");
        let usdc_sac = kv(&setup_out, "USDC_SAC");
        let dest = kv(&setup_out, "DEST");
        let auditor = kv(&setup_out, "AUDITOR");
        let relayer_addr = kv(&setup_out, "RELAYER_ADDR");
        let relayer_secret = kv(&setup_out, "RELAYER_SECRET");
        eprintln!("SETUP OK — shared pool {pool}, ASP anonymity set = 3 (wallet + 2 decoys)");

        // Point the flows at the freshly-deployed pool + policy + viewkeys, and route
        // interior ops through a pre-funded RELAYER (fee abstraction: the wallet pays no
        // fee + isn't linked as the fee-payer of its private transfer).
        std::env::set_var("OZKY_POOL_CONTRACT", &pool);
        std::env::set_var("OZKY_POLICY_CONTRACT", &policy);
        std::env::set_var("OZKY_VIEWKEYS_CONTRACT", &viewkeys);
        std::env::set_var("OZKY_RELAYER_SECRET", &relayer_secret);
        let cfg = PoolConfig::load().unwrap();

        // Native XLM balance of an account via the SAC `balance` (read-only invoke).
        let bal = |acct: &str| -> i128 {
            let s = run_zk(
                &secret,
                &format!(
                    "stellar network add testnet --rpc-url https://soroban-testnet.stellar.org --network-passphrase 'Test SDF Network ; September 2015' 2>/dev/null || true; \
                     stellar contract invoke --id {sac} --source \"$OZKY_SOURCE_SECRET\" --network testnet -- balance --id {acct}",
                    sac = sac, acct = acct,
                ),
            );
            s.trim().trim_matches('"').trim().parse::<i128>().unwrap_or(0)
        };

        // --- 2. DEPOSIT 1000 into the pool via the core deposit flow (public on-ramp:
        // proof built by the core, tokens pulled from our Stellar account, note minted
        // + encrypted to self + published so scan rediscovers it). ---
        deposit::deposit_with(&wallet, &cfg, 1000)
            .expect("deposit_with must lock tokens, mint the note, and succeed on-chain");
        eprintln!("DEPOSIT OK — 1000 shielded via deposit_with");

        // --- 3. SEND 600 to ourselves through the FULL app path: `send_with` scans the
        // freshly-deployed pool from raw RPC (the scan-on-any-pool fix), rediscovers the
        // deposited note, then builds + proves + submits the transfer. The RELAYER pays
        // the fee, so the wallet's XLM must be UNCHANGED across the send (G4). ---
        let wallet_xlm_before = bal(&addr);
        let relayer_xlm_before = bal(&relayer_addr);
        let code = payment_code(&id);
        let txhash = send_with(&wallet, &cfg, &code, 600)
            .expect("send_with must scan the new pool, find the note, and succeed on-chain");
        assert!(!txhash.is_empty());
        assert_eq!(
            bal(&addr),
            wallet_xlm_before,
            "wallet XLM must be unchanged across a relayed send (relayer pays the fee)"
        );
        assert!(
            bal(&relayer_addr) < relayer_xlm_before,
            "the relayer's XLM must have decreased (it paid the transfer fee)"
        );
        eprintln!("SEND OK — relayer-paid transfer accepted; wallet XLM unchanged (fee abstraction, tx {txhash})");

        // --- 4. on-chain confirmation: a replay of the same transfer must be REJECTED
        // (the nullifier root advanced), proving the send truly mutated chain state. ---
        let replay = format!(
            "stellar network add testnet --rpc-url https://soroban-testnet.stellar.org --network-passphrase 'Test SDF Network ; September 2015' 2>/dev/null || true; \
             stellar contract invoke --id {pool} --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- \
               transfer --asset_tag 1 \
               --public_inputs-file-path /workspace/circuits/transfer/target/public_inputs \
               --proof-file-path /workspace/circuits/transfer/target/proof \
               --enc_notes '[]' --ephemeral_pubs '[]' --view_tags '[]'",
            pool = pool,
        );
        let compose = repo_root().join("compose.zk.yaml");
        let replay_out = Command::new("docker")
            .env("OZKY_SOURCE_SECRET", &secret)
            .args(["compose", "-f"])
            .arg(&compose)
            .args(["run", "--rm", "-e", "OZKY_SOURCE_SECRET", "zk", "bash", "-c", &replay])
            .output()
            .expect("spawn docker");
        assert!(
            !replay_out.status.success(),
            "double-spend replay MUST be rejected on-chain (nullifier root already advanced)"
        );
        eprintln!("DOUBLE-SPEND replay rejected");

        // --- 5. WITHDRAW 400 to a public dest through the core withdraw flow: scans the
        // pool (now holds our 600 + 400 outputs), proves the withdraw, releases 400 of
        // real XLM to `dest`. Confirm dest received exactly 400 (the off-ramp). ---
        let dest_before = bal(&dest);
        let receipt = withdraw::withdraw_with(&wallet, &cfg, &dest, 400)
            .expect("withdraw_with must scan, prove, and release to dest on-chain");
        let dest_after = bal(&dest);
        assert_eq!(
            dest_after - dest_before,
            400,
            "dest must receive exactly 400 base units from the withdraw"
        );
        eprintln!(
            "WITHDRAW OK — 400 released to {dest} (tx {tx}); change {chg} kept shielded",
            tx = receipt.tx_hash,
            chg = receipt.change_value,
        );

        // --- 6. LOCAL NOTES STORE proof: the withdraw change note has NO on-chain
        // ciphertext, so a chain-only scan can't see it, but the store can. ---
        let state = chain::pool_state(&cfg).unwrap();
        let chg = receipt.change_value;
        let chain_only = scan::scan_state(&id, &state, 0).unwrap();
        assert!(
            !chain_only.iter().any(|n| n.value == chg),
            "withdraw change must NOT be discoverable from chain alone (no ciphertext)"
        );
        let local = notes::load(&wallet).unwrap();
        let with_store = scan::owned_notes(&id, &state, &local, 0).unwrap();
        assert!(
            with_store.iter().any(|n| n.value == chg),
            "withdraw change MUST be recovered via the local notes store"
        );
        eprintln!("NOTES STORE OK — change {chg} invisible to chain scan, recovered from store");
        // (The recovered note carries a real leaf_index from chain, so it is spendable
        // through the identical owned_notes path send/withdraw already use.)

        // --- 7. SELECTIVE DISCLOSURE (G5): share a scoped read-only disclosure with an
        // auditor + record the on-chain grant; the auditor re-derives THIS wallet's
        // notes (verified against on-chain commitments) with no spend authority. ---
        use crate::core::disclose;
        let epoch = chain::current_epoch(&cfg.rpc_url).unwrap();
        let pkg = disclose::share_with_auditor_with(&wallet, &cfg, &auditor, epoch)
            .expect("share_with_auditor builds the package + records the on-chain grant");
        let disclosed = disclose::audit(&pkg).expect("auditor re-derives the disclosed notes");
        let total = disclose::disclosed_total(&disclosed);
        // The auditor sees the wallet's shielded outputs (the 600 self-output landed
        // back to us; the change 200 too). They must see >0 and only OUR notes.
        assert!(!disclosed.is_empty(), "auditor must recover at least one disclosed note");
        assert!(total > 0, "auditor sees the disclosed balance");
        // The package must NOT carry spend authority.
        assert!(!pkg.contains(wallet.owner_sk_hex().trim_start_matches("0x")), "no owner_sk leak");
        // On-chain grant recorded + provable.
        let granted = run_zk(&secret, &format!(
            "stellar network add testnet --rpc-url https://soroban-testnet.stellar.org --network-passphrase 'Test SDF Network ; September 2015' 2>/dev/null || true; \
             stellar contract invoke --id {vk} --source \"$OZKY_SOURCE_SECRET\" --network testnet -- \
               is_disclosed --owner {owner} --auditor {auditor} --scope '{{\"account\":0,\"asset_tag\":\"1\",\"epoch\":{epoch}}}'",
            vk = viewkeys, owner = addr, auditor = auditor, epoch = epoch,
        ));
        assert!(granted.trim().contains("true"), "on-chain disclosure grant must be provable");
        eprintln!("DISCLOSURE OK — auditor re-derived {} notes (total {total}); grant recorded on-chain, no spend leak", disclosed.len());

        // --- 8. MULTI-ASSET (G6): the SAME pool also holds USDC (asset_tag 2, a distinct
        // SAC vault). Run the full public->shielded->public lifecycle for a NON-NATIVE
        // asset: deposit USDC, send it privately to ourselves, withdraw to `dest`. Proves
        // `asset_tag` is carried correctly through the note commitment, scanning, and the
        // per-asset vault — the only change is `cfg.with_asset("USDC")`. (Needs the test
        // wallet pre-funded with testnet USDC; trustline added in setup.) ---
        let cfg_usdc = cfg.with_asset("USDC").unwrap();
        let usdc_bal = |acct: &str| -> i128 {
            let s = run_zk(
                &secret,
                &format!(
                    "stellar network add testnet --rpc-url https://soroban-testnet.stellar.org --network-passphrase 'Test SDF Network ; September 2015' 2>/dev/null || true; \
                     stellar contract invoke --id {sac} --source \"$OZKY_SOURCE_SECRET\" --network testnet -- balance --id {acct}",
                    sac = usdc_sac, acct = acct,
                ),
            );
            s.trim().trim_matches('"').trim().parse::<i128>().unwrap_or(0)
        };
        let wallet_usdc = usdc_bal(&addr);
        assert!(
            wallet_usdc >= 1000,
            "test wallet must hold >=1000 USDC base units for the G6 leg — faucet \
             GDRXE2BQUC3AZNPVFSCEZ76NJ3WWL25FYFK6RGZGIEKWE4SOOHSUJUJ6 with testnet USDC (have {wallet_usdc})"
        );
        deposit::deposit_with(&wallet, &cfg_usdc, 1000)
            .expect("USDC deposit must lock 1000 base units + mint the note");
        send_with(&wallet, &cfg_usdc, &code, 600)
            .expect("USDC send must scan the pool, find the USDC note, prove + submit");
        let dest_usdc_before = usdc_bal(&dest);
        withdraw::withdraw_with(&wallet, &cfg_usdc, &dest, 400)
            .expect("USDC withdraw must release 400 base units to dest");
        assert_eq!(
            usdc_bal(&dest) - dest_usdc_before,
            400,
            "dest must receive exactly 400 USDC base units (non-native asset off-ramp)"
        );
        eprintln!("MULTI-ASSET OK (G6) — USDC deposit 1000 -> send 600 -> withdraw 400 on the SAME pool (asset_tag 2); dest +400 USDC");

        // --- 9. INCREMENTAL SCAN CACHE (G9): the cached `pool_state` (resumed from the
        // per-pool cursor across all the calls above) must yield the SAME owned-note set
        // as a cache-bypassing full re-drain from the retention window. Correctness is
        // independent of the cache; the cache only changes how many events are fetched. ---
        let local_g9 = notes::load(&wallet).unwrap();
        let cached = scan::owned_notes(&id, &chain::pool_state(&cfg).unwrap(), &local_g9, 0).unwrap();
        std::env::set_var("OZKY_NO_POOL_CACHE", "1");
        let full = scan::owned_notes(&id, &chain::pool_state(&cfg).unwrap(), &local_g9, 0).unwrap();
        std::env::remove_var("OZKY_NO_POOL_CACHE");
        let mut a: Vec<String> = cached.iter().map(|n| n.commitment.to_hex()).collect();
        let mut b: Vec<String> = full.iter().map(|n| n.commitment.to_hex()).collect();
        a.sort();
        b.sort();
        assert_eq!(a, b, "incremental (cached) scan must equal a full re-drain (G9 correctness)");
        eprintln!("SCAN CACHE OK (G9) — incremental scan == full re-drain ({} owned notes)", a.len());

        eprintln!("A3 + G1/G4/G5/G6/G9 deposit -> enroll -> send(relayer) -> withdraw -> notes-store -> disclosure -> multi-asset(USDC) -> scan-cache lifecycle OK");
    }
}
