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
    TransferWitness, WithdrawWitness,
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
    Withdraw,
    Split,
    EscrowContribute,
    EscrowPayout,
    ChannelClose,
}

impl Circuit {
    fn name(self) -> &'static str {
        match self {
            Circuit::Deposit => "deposit",
            Circuit::Transfer => "transfer",
            Circuit::Withdraw => "withdraw",
            Circuit::Split => "split",
            Circuit::EscrowContribute => "escrow_contribute",
            Circuit::EscrowPayout => "escrow_payout",
            Circuit::ChannelClose => "channel_close",
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

/// Generate (and verify) a proof for `circuit` from a `Prover.toml` body.
fn prove(circuit: Circuit, prover_toml: &str) -> Result<ProofBundle, CoreError> {
    let name = circuit.name();
    let root = repo_root();
    let circuit_dir = root.join("circuits").join(name);
    if !circuit_dir.exists() {
        return Err(CoreError::Proving(format!(
            "circuit dir not found: {} (set OZKY_REPO_ROOT)",
            circuit_dir.display()
        )));
    }
    std::fs::write(circuit_dir.join("Prover.toml"), prover_toml)
        .map_err(|e| CoreError::Proving(format!("write Prover.toml: {e}")))?;

    match sidecar_bin() {
        Some(bin) => prove_via_sidecar(&bin, &circuit_dir, &root, name)?,
        None => prove_via_docker(&root, name)?,
    }

    let target = circuit_dir.join("target");
    let proof = std::fs::read(target.join("proof"))
        .map_err(|e| CoreError::Proving(format!("read proof: {e}")))?;
    let public_inputs = std::fs::read(target.join("public_inputs"))
        .map_err(|e| CoreError::Proving(format!("read public_inputs: {e}")))?;
    Ok(ProofBundle { proof, public_inputs })
}

/// The native prover sidecar binary, if configured. `OZKY_PROVER_BIN` points at the
/// `ozky-prover` SEA executable (its WASM blobs ship beside it). Unset ⇒ Docker fallback.
fn sidecar_bin() -> Option<PathBuf> {
    super::config::cfg_var("OZKY_PROVER_BIN")
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

/// Prove + verify via the native sidecar (no Docker). It reads `<circuit>/Prover.toml`
/// and `target/<name>.json`, writes `target/{proof,public_inputs}`, and exits non-zero
/// unless the proof verified AND its VK matches the frozen one.
fn prove_via_sidecar(bin: &Path, circuit_dir: &Path, root: &Path, name: &str) -> Result<(), CoreError> {
    let frozen_vk = root.join("contracts").join("frozen_vks").join(name).join("vk");
    // The WASM blobs ship beside the binary; point the sidecar at them explicitly.
    let assets_dir = bin.parent().unwrap_or_else(|| Path::new("."));
    let out = Command::new(bin)
        .arg(circuit_dir)
        .arg(&frozen_vk)
        .env("OZKY_PROVER_ASSETS", assets_dir)
        .output()
        .map_err(|e| CoreError::Proving(format!("spawn prover sidecar: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: String = stderr.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(CoreError::Proving(format!("prover sidecar failed for {name}:\n{tail}")));
    }
    Ok(())
}

/// Prove + verify in the ZK Docker container. One run: solve the witness, prove (keccak
/// oracle = on-chain format), verify against the frozen VK. `set -e` ⇒ a failed verify
/// fails the run.
fn prove_via_docker(root: &Path, name: &str) -> Result<(), CoreError> {
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
    Ok(())
}

/// Prove a transfer from a fully-built witness, verifying against the frozen VK.
pub fn prove_transfer_witness(w: &TransferWitness) -> Result<ProofBundle, CoreError> {
    prove(Circuit::Transfer, &w.to_prover_toml())
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
}
