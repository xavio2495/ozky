//! Client-side proving (Phase A2). Turns a natively-generated witness (see
//! [`super::witness`]) into an UltraHonk proof the on-chain verifier accepts.
//!
//! Proving: (1) builds the witness in native Rust, (2) writes the circuit's
//! `Prover.toml`, (3) runs the prover, (4) reads back the `proof` + `public_inputs`
//! bytes. A proof is only returned if it verified against the FROZEN VK.
//!
//! Two prover backends, selected at step (3):
//! - **Native sidecar** (`OZKY_PROVER_BIN` → the `ozky-prover` SEA binary): solves the
//!   witness (noir_js) and proves (bb.js, keccak oracle) entirely in WASM, no Docker.
//!   This is the shippable path — output is byte-identical to the container's `bb`.
//! - **Docker fallback** (when `OZKY_PROVER_BIN` is unset): `nargo execute` + `bb prove`
//!   + `bb verify` in the ZK container (the toolchain that froze the VKs).

use super::witness::{
    ChannelCloseWitness, ContributeWitness, DepositWitness, PayoutWitness, SplitWitness,
    SwapWitness, Transfer4Witness, TransferWitness, WithdrawWitness,
};
use super::CoreError;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A proof + its public-input vector, ready to submit to the pool contract.
pub struct ProofBundle {
    pub proof: Vec<u8>,
    pub public_inputs: Vec<u8>,
}

#[derive(Clone, Copy)]
pub enum Circuit {
    Deposit,
    Transfer,
    Transfer4,
    Withdraw,
    Split,
    EscrowContribute,
    EscrowPayout,
    ChannelClose,
    ShieldedSwap,
}

impl Circuit {
    fn name(self) -> &'static str {
        match self {
            Circuit::Deposit => "deposit",
            Circuit::Transfer => "transfer",
            Circuit::Transfer4 => "transfer4",
            Circuit::Withdraw => "withdraw",
            Circuit::Split => "split",
            Circuit::EscrowContribute => "escrow_contribute",
            Circuit::EscrowPayout => "escrow_payout",
            Circuit::ChannelClose => "channel_close",
            Circuit::ShieldedSwap => "shielded_swap",
        }
    }
}

/// Repo root (holds `compose.zk.yaml`, `circuits/`, `contracts/`). Overridable via
/// `OZKY_REPO_ROOT`; otherwise the compile-time location (`src-tauri/../..`).
fn repo_root() -> PathBuf {
    if let Some(p) = super::config::cfg_var("OZKY_REPO_ROOT") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Generate (and verify) a proof for `circuit` from a `Prover.toml` body. The native
/// sidecar (the shippable path) is preferred; Docker is the dev-only fallback.
fn prove(circuit: Circuit, prover_toml: &str) -> Result<ProofBundle, CoreError> {
    let name = circuit.name();
    let root = repo_root();
    match sidecar_bin() {
        Some(bin) => prove_via_sidecar(&bin, &root, name, prover_toml),
        None => prove_via_docker(&root, name, prover_toml),
    }
}

/// The native prover sidecar binary, if configured. `OZKY_PROVER_BIN` points at the
/// `ozky-prover` SEA executable (its WASM blobs ship beside it). Unset ⇒ Docker fallback.
fn sidecar_bin() -> Option<PathBuf> {
    super::config::cfg_var("OZKY_PROVER_BIN")
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

/// The dir holding the sidecar's WASM/worker blobs: `OZKY_PROVER_ASSETS` if set (the
/// bundled app points it at the prover resource dir), else beside the binary.
fn sidecar_assets(bin: &Path) -> PathBuf {
    super::config::cfg_var("OZKY_PROVER_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| bin.parent().unwrap_or_else(|| Path::new(".")).to_path_buf())
}

/// A unique writable scratch dir for one prove. The sidecar reads `Prover.toml` +
/// `target/<name>.json` and writes `target/{proof,public_inputs}` here, so the source
/// circuit artifacts can live in a read-only bundle (Program Files / the app bundle).
/// The work dir's *last segment* must be the circuit `name` — the sidecar derives the
/// circuit name from it — so the circuit dir is nested under a unique parent.
fn staging_dir(name: &str) -> Result<PathBuf, CoreError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let unique = format!("ozky-prove-{}-{}", std::process::id(), nanos as u64 ^ SEQ.fetch_add(1, Ordering::Relaxed));
    let work = std::env::temp_dir().join(unique).join(name);
    std::fs::create_dir_all(work.join("target"))
        .map_err(|e| CoreError::Proving(format!("create staging dir: {e}")))?;
    Ok(work)
}

/// Remove a [`staging_dir`] (and its unique parent) — best-effort cleanup.
fn cleanup_staging(work: &Path) {
    let _ = std::fs::remove_dir_all(work.parent().unwrap_or(work));
}

/// Prove + verify via the native sidecar (no Docker). Inputs/outputs are staged in a
/// writable temp dir so the compiled circuit + frozen VK can be read-only bundle
/// resources; the sidecar exits non-zero unless the proof verified AND its VK matches
/// the frozen one.
fn prove_via_sidecar(bin: &Path, root: &Path, name: &str, prover_toml: &str) -> Result<ProofBundle, CoreError> {
    let circuit_json = root.join("circuits").join(name).join("target").join(format!("{name}.json"));
    if !circuit_json.exists() {
        return Err(CoreError::Proving(format!(
            "compiled circuit not found: {} (set OZKY_REPO_ROOT)",
            circuit_json.display()
        )));
    }
    let frozen_vk = root.join("contracts").join("frozen_vks").join(name).join("vk");

    let work = staging_dir(name)?;
    let target = work.join("target");
    std::fs::copy(&circuit_json, target.join(format!("{name}.json")))
        .map_err(|e| CoreError::Proving(format!("stage circuit json: {e}")))?;
    std::fs::write(work.join("Prover.toml"), prover_toml)
        .map_err(|e| CoreError::Proving(format!("write Prover.toml: {e}")))?;

    let out = Command::new(bin)
        .arg(&work)
        .arg(&frozen_vk)
        .env("OZKY_PROVER_ASSETS", sidecar_assets(bin))
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => {
            cleanup_staging(&work);
            return Err(CoreError::Proving(format!("spawn prover sidecar: {e}")));
        }
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: String = stderr.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        cleanup_staging(&work);
        return Err(CoreError::Proving(format!("prover sidecar failed for {name}:\n{tail}")));
    }

    let bundle = read_bundle(&target);
    let _ = std::fs::remove_dir_all(&work);
    bundle
}

/// Read the `proof` + `public_inputs` files a prover backend wrote into `target`.
fn read_bundle(target: &Path) -> Result<ProofBundle, CoreError> {
    let proof = std::fs::read(target.join("proof"))
        .map_err(|e| CoreError::Proving(format!("read proof: {e}")))?;
    let public_inputs = std::fs::read(target.join("public_inputs"))
        .map_err(|e| CoreError::Proving(format!("read public_inputs: {e}")))?;
    Ok(ProofBundle { proof, public_inputs })
}

/// Prove + verify in the ZK Docker container (dev-only fallback). One run: solve the
/// witness, prove (keccak oracle = on-chain format), verify against the frozen VK.
/// `set -e` ⇒ a failed verify fails the run. Writes in-place under the repo's circuit
/// dir (the container needs the Noir source to `nargo compile`).
fn prove_via_docker(root: &Path, name: &str, prover_toml: &str) -> Result<ProofBundle, CoreError> {
    let circuit_dir = root.join("circuits").join(name);
    if !circuit_dir.exists() {
        return Err(CoreError::Proving(format!(
            "circuit dir not found: {} (set OZKY_REPO_ROOT)",
            circuit_dir.display()
        )));
    }
    std::fs::write(circuit_dir.join("Prover.toml"), prover_toml)
        .map_err(|e| CoreError::Proving(format!("write Prover.toml: {e}")))?;
    let script = format!(
        "set -e; cd circuits/{name}; nargo compile; nargo execute; \
         bb prove --scheme ultra_honk --oracle_hash keccak \
            --bytecode_path target/{name}.json --witness_path target/{name}.gz \
            --output_path target --output_format bytes_and_fields; \
         bb verify --scheme ultra_honk --oracle_hash keccak \
            -k /workspace/contracts/frozen_vks/{name}/vk \
            -p target/proof -i target/public_inputs"
    );
    let compose = root.join("compose.zk.yaml");
    let out = Command::new("docker")
        .args(["compose", "-f"])
        .arg(&compose)
        .args(["run", "--rm", "zk", "bash", "-c", &script])
        .output()
        .map_err(|e| CoreError::Proving(format!("spawn docker: {e}")))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: String = stderr.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(CoreError::Proving(format!("prove/verify failed for {name}:\n{tail}")));
    }
    read_bundle(&circuit_dir.join("target"))
}

/// Prove a transfer from a fully-built witness, verifying against the frozen VK.
pub fn prove_transfer_witness(w: &TransferWitness) -> Result<ProofBundle, CoreError> {
    prove(Circuit::Transfer, &w.to_prover_toml())
}

/// Prove a 4-input transfer (spend up to 4 owned notes) against the frozen `transfer4` VK.
pub fn prove_transfer4_witness(w: &Transfer4Witness) -> Result<ProofBundle, CoreError> {
    prove(Circuit::Transfer4, &w.to_prover_toml())
}

pub fn prove_deposit_witness(w: &DepositWitness) -> Result<ProofBundle, CoreError> {
    prove(Circuit::Deposit, &w.to_prover_toml())
}

pub fn prove_withdraw_witness(w: &WithdrawWitness) -> Result<ProofBundle, CoreError> {
    prove(Circuit::Withdraw, &w.to_prover_toml())
}

/// Prove a split (2-in / 8-out) from a fully-built witness, verifying against the frozen VK.
pub fn prove_split_witness(w: &SplitWitness) -> Result<ProofBundle, CoreError> {
    prove(Circuit::Split, &w.to_prover_toml())
}

/// Prove an escrow contribute (withdraw-shaped spend + Pedersen fold) against the frozen VK.
pub fn prove_escrow_contribute_witness(w: &ContributeWitness) -> Result<ProofBundle, CoreError> {
    prove(Circuit::EscrowContribute, &w.to_prover_toml())
}

/// Prove an escrow payout (release/refund: open commitment, V>=floor, mint) against the frozen VK.
pub fn prove_escrow_payout_witness(w: &PayoutWitness) -> Result<ProofBundle, CoreError> {
    prove(Circuit::EscrowPayout, &w.to_prover_toml())
}

/// Prove a channel close (open cap + cumulative, verify Schnorr, mint two notes) against the frozen VK.
pub fn prove_channel_close_witness(w: &ChannelCloseWitness) -> Result<ProofBundle, CoreError> {
    prove(Circuit::ChannelClose, &w.to_prover_toml())
}

/// Prove an in-pool swap (spend A-note, mint B-note + A change, conserve A) against the frozen VK.
pub fn prove_swap_witness(w: &SwapWitness) -> Result<ProofBundle, CoreError> {
    prove(Circuit::ShieldedSwap, &w.to_prover_toml())
}

// --- Command-facing entrypoints -------------------------------------------------
//
// The transfer (send) flow is wired in `super::send` (it builds the witness from live
// state, calls `prove_transfer_witness`, and submits). The deposit/withdraw command
// flows are still pending; their proving engine is ready via the `*_witness` fns.

/// Build a `deposit` proof binding `amount` to a fresh note. (deposit flow, pending)
pub fn prove_deposit(_amount: u64) -> Result<ProofBundle, CoreError> {
    Err(CoreError::not_implemented(
        "proving::prove_deposit (A3): needs live epoch/pool config; \
         engine ready via prove_deposit_witness",
    ))
}

/// Build a `withdraw` proof releasing `amount` to `dest`. (A3 send)
pub fn prove_withdraw(_dest: &str, _amount: u64) -> Result<ProofBundle, CoreError> {
    Err(CoreError::not_implemented(
        "proving::prove_withdraw (A3): needs live epoch/pool config + dest binding; \
         engine ready via prove_withdraw_witness",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::poseidon::{Fr, Hasher};
    use crate::core::witness::{TransferInputs, TransferWitness, WithdrawWitness};

    // Docker-backed proving tests (slow, need the ZK container). Run with:
    //   cargo test -- --ignored
    // The testnet epoch 28 / pool 7 / net 42 domain_seps the frozen VKs were exercised
    // with (DEPOSIT=1/TRANSFER=2/WITHDRAW=3 selectors).
    const DSEP_TRANSFER_28: &str =
        "0x2eae4c361f605c06c766cb126a391a0f916308610ae8128f7e615e5e6b6c67ff";

    #[test]
    #[ignore = "needs the ZK Docker container; run with --ignored"]
    fn transfer_demo_proof_verifies_against_frozen_vk() {
        let h = Hasher::new();
        let w = TransferWitness::demo(&h, 28, Fr::from_hex(DSEP_TRANSFER_28).unwrap());
        let bundle = prove_transfer_witness(&w).expect("proof must verify against frozen VK");
        // keccak transfer proof = 14592 bytes / public inputs = 11 fields * 32 = 352 bytes.
        assert_eq!(bundle.proof.len(), 14592, "proof byte length");
        assert_eq!(bundle.public_inputs.len(), 352, "11 public inputs");
    }

    #[test]
    #[ignore = "needs the ZK Docker container; run with --ignored"]
    fn stateful_multileaf_transfer_proof_verifies() {
        // The genuine stateful path: our note is leaf 2 of a 4-leaf commitment tree,
        // the spender is one of 4 approved keys, and 2 nullifiers are already spent.
        // If this proof verifies against the frozen VK, the native stateful witness
        // generator is correct for live multi-leaf / non-empty-accumulator state.
        let h = Hasher::new();
        let owner_sk = Fr::from_u64(12345);
        let owner_pk = h.owner_pk(&owner_sk);
        let asset_tag = Fr::from_u64(1);
        let epoch = Fr::from_u64(28);

        let in_commitment =
            h.commitment(&Fr::from_u64(1000), &asset_tag, &owner_pk, &Fr::from_u64(777), &epoch, &Fr::from_u64(111));
        // Our note at index 2; the other leaves are arbitrary prior commitments.
        let commitment_leaves = vec![
            Fr::from_u64(0xaaaa),
            Fr::from_u64(0xbbbb),
            in_commitment,
            Fr::from_u64(0xcccc),
        ];
        // 4-key approved set with our owner_pk among them.
        let asp_leaves = vec![h.owner_pk(&Fr::from_u64(1)), owner_pk, h.owner_pk(&Fr::from_u64(3)), h.owner_pk(&Fr::from_u64(4))];
        // Two unrelated nullifiers already in the accumulator.
        let prior = vec![h.nullifier(&Fr::from_u64(7), &Fr::from_u64(8)), h.nullifier(&Fr::from_u64(9), &Fr::from_u64(10))];

        let w = TransferWitness::build(
            &h,
            TransferInputs {
                owner_sk,
                asset_tag,
                epoch,
                note_epoch: epoch,
                domain_sep: Fr::from_hex(DSEP_TRANSFER_28).unwrap(),
                note_value: 1000,
                note_blinding: Fr::from_u64(777),
                note_rho: Fr::from_u64(111),
                note_leaf_index: 2,
                commitment_leaves: &commitment_leaves,
                asp_leaves: &asp_leaves,
                prior_nullifiers: &prior,
                dummy_rho: Fr::from_u64(0xdead),
                recipient_owner_pk: h.owner_pk(&Fr::from_u64(99)),
                out0_value: 600,
                out0_blinding: Fr::from_u64(222),
                out0_rho: Fr::from_u64(333),
                change_blinding: Fr::from_u64(444),
                change_rho: Fr::from_u64(555),
            },
        );
        prove_transfer_witness(&w).expect("stateful multi-leaf transfer must verify");
    }

    /// The Rust core's `ChannelCloseWitness` -> Prover.toml -> prove path produces a proof the FROZEN
    /// channel_close VK accepts (the `close_demo` vector, signed by the native signer). This is the
    /// end-to-end CH4 check that the witness serialization matches the circuit's ABI.
    #[test]
    #[ignore = "needs the ZK Docker container; run with --ignored"]
    fn channel_close_demo_proof_verifies_against_frozen_vk() {
        use crate::core::pedersen;
        use crate::core::witness::{ChannelCloseInputs, ChannelCloseWitness};
        let h = Hasher::new();
        let sk = Fr::from_hex("0x1234567").unwrap();
        let k = Fr::from_hex("0x89abcdef").unwrap();
        let r_k = Fr::from_hex("0xd4a").unwrap();
        let c_k = pedersen::commit(&Fr::from_u64(600), &r_k);
        let (ckx, cky) = pedersen::coords(&c_k);
        let msg = h.hash(&[Fr::from_u64(1), Fr::from_u64(50), ckx, cky]);
        let pk = pedersen::schnorr_pubkey(&sk);
        let sig = pedersen::schnorr_sign(&h, &sk, &k, &msg);
        let w = ChannelCloseWitness::build(
            &h,
            ChannelCloseInputs {
                domain_sep: Fr::from_u64(0xabc),
                asset_tag: Fr::from_u64(1),
                epoch: Fr::from_u64(5),
                valid_after_ledger: 50,
                channel_id: 1,
                cap: 1000,
                r_cap: Fr::from_hex("0xca9").unwrap(),
                drawn: 600,
                r_k,
                pk,
                sig,
                merchant_pk: h.owner_pk(&Fr::from_hex("0x3e").unwrap()),
                m_salt: Fr::from_hex("0x3e17").unwrap(),
                merchant_blinding: Fr::from_u64(222),
                merchant_rho: Fr::from_u64(333),
                subscriber_pk: h.owner_pk(&Fr::from_hex("0x5b").unwrap()),
                s_salt: Fr::from_hex("0x5b17").unwrap(),
                subscriber_blinding: Fr::from_u64(444),
                subscriber_rho: Fr::from_u64(555),
            },
        );
        let bundle = prove_channel_close_witness(&w).expect("channel close must verify against frozen VK");
        // keccak proof = 14592 bytes; 10 public inputs * 32 = 320 bytes.
        assert_eq!(bundle.public_inputs.len(), 320, "10 public inputs");
    }

    #[test]
    #[ignore = "needs the ZK Docker container; run with --ignored"]
    fn withdraw_demo_proof_verifies_against_frozen_vk() {
        let h = Hasher::new();
        // WITHDRAW selector=3 domain_sep for the same pool/net/epoch.
        let dsep = h.domain_sep(&Fr::from_u64(7), &Fr::from_u64(42), crate::core::poseidon::SELECTOR_WITHDRAW);
        let w = WithdrawWitness::demo(&h, 28, dsep);
        prove_withdraw_witness(&w).expect("withdraw demo must verify against frozen VK");
    }

    /// The Rust core's `SwapWitness` -> Prover.toml -> prove path produces a proof the FROZEN
    /// shielded_swap VK accepts (SW4↔SW2 ABI check): spend a 1000 A-note, swap 800 into asset 2,
    /// keep 200 A change, mint a 750 B-note.
    #[test]
    #[ignore = "needs the ZK Docker container; run with --ignored"]
    fn swap_demo_proof_verifies_against_frozen_vk() {
        use crate::core::witness::{SwapInputs, SwapWitness};
        let h = Hasher::new();
        let dsep = h.domain_sep(&Fr::from_u64(7), &Fr::from_u64(42), crate::core::poseidon::SELECTOR_SWAP);
        let asset_a = Fr::from_u64(1);
        let owner_sk = Fr::from_u64(12345);
        let owner_pk = h.owner_pk(&owner_sk);
        let epoch = Fr::from_u64(28);
        let in_commitment = h.commitment(
            &Fr::from_u64(1000), &asset_a, &owner_pk, &Fr::from_u64(777), &epoch, &Fr::from_u64(111),
        );
        let w = SwapWitness::build(
            &h,
            SwapInputs {
                owner_sk,
                asset_a_tag: asset_a,
                asset_b_tag: Fr::from_u64(2),
                epoch,
                note_epoch: epoch,
                domain_sep: dsep,
                note_value: 1000,
                note_blinding: Fr::from_u64(777),
                note_rho: Fr::from_u64(111),
                note_leaf_index: 0,
                commitment_leaves: &[in_commitment],
                asp_leaves: &[owner_pk],
                prior_nullifiers: &[],
                dummy_rho: Fr::from_u64(0xdead),
                value_a: 800,
                value_b: 750,
                change_blinding: Fr::from_u64(444),
                change_rho: Fr::from_u64(555),
                out_owner_pk: owner_pk,
                out_blinding: Fr::from_u64(888),
                out_rho: Fr::from_u64(999),
            },
        );
        let bundle = prove_swap_witness(&w).expect("swap demo must verify against frozen VK");
        assert_eq!(bundle.public_inputs.len(), 448, "14 public inputs");
    }

    /// The Rust core's `Transfer4Witness` -> Prover.toml -> prove path produces a proof the FROZEN
    /// transfer4 VK accepts (the SW-style ABI check): spend three owned notes (1000 + 500 + 300) at
    /// leaves 0,1,2 plus one dummy, pay 1500 to a recipient with 300 change.
    #[test]
    #[ignore = "needs the ZK Docker container; run with --ignored"]
    fn transfer4_demo_proof_verifies_against_frozen_vk() {
        use crate::core::witness::{SpendNote, Transfer4Inputs, Transfer4Witness};
        let h = Hasher::new();
        let dsep =
            h.domain_sep(&Fr::from_u64(7), &Fr::from_u64(42), crate::core::poseidon::SELECTOR_TRANSFER_4);
        let asset_tag = Fr::from_u64(1);
        let owner_sk = Fr::from_u64(12345);
        let owner_pk = h.owner_pk(&owner_sk);
        let epoch = Fr::from_u64(28);

        // Three owned notes at leaves 0,1,2 of the commitment tree.
        let mk = |v: u64, b: u64, rho: u64| {
            h.commitment(&Fr::from_u64(v), &asset_tag, &owner_pk, &Fr::from_u64(b), &epoch, &Fr::from_u64(rho))
        };
        let commitment_leaves = vec![mk(1000, 777, 111), mk(500, 778, 112), mk(300, 779, 113)];
        let notes = vec![
            SpendNote { value: 1000, blinding: Fr::from_u64(777), epoch, rho: Fr::from_u64(111), leaf_index: 0 },
            SpendNote { value: 500, blinding: Fr::from_u64(778), epoch, rho: Fr::from_u64(112), leaf_index: 1 },
            SpendNote { value: 300, blinding: Fr::from_u64(779), epoch, rho: Fr::from_u64(113), leaf_index: 2 },
        ];

        let w = Transfer4Witness::build(
            &h,
            Transfer4Inputs {
                owner_sk,
                asset_tag,
                epoch,
                domain_sep: dsep,
                commitment_leaves: &commitment_leaves,
                asp_leaves: &[owner_pk],
                prior_nullifiers: &[],
                notes: &notes,
                dummy_rhos: &[Fr::from_u64(0xdead)],
                recipient_owner_pk: h.owner_pk(&Fr::from_u64(99)),
                out0_value: 1500,
                out0_blinding: Fr::from_u64(222),
                out0_rho: Fr::from_u64(333),
                change_blinding: Fr::from_u64(444),
                change_rho: Fr::from_u64(555),
            },
        );
        let bundle = prove_transfer4_witness(&w).expect("transfer4 demo must verify against frozen VK");
        assert_eq!(bundle.public_inputs.len(), 416, "13 public inputs");
    }
}
