//! The Withdraw flow (Phase A3) — the PUBLIC off-ramp out of the shielded pool.
//! Spends one owned shielded note, releases `amount` of the real asset to a public
//! Stellar `dest`, and re-commits the remaining `note_value - amount` as shielded
//! change back to the spender.
//!
//! Builds the withdraw proof against live pool state (scan-selected note + raw-RPC
//! commitment/nullifier sets, like the send flow) and submits via the stellar CLI.
//!
//! Change-note caveat: the pool's `withdraw` entrypoint publishes NO ciphertext for
//! the change commitment, so [`super::scan`] cannot rediscover it from chain alone.
//! The change opening is returned in [`WithdrawReceipt`] so the caller can persist it;
//! a local notes store (so change is auto-recovered) is a follow-up.

use super::config::PoolConfig;
use super::encrypt::NotePlaintext;
use super::poseidon::{Fr, Hasher, DOMAIN_DEST, SELECTOR_WITHDRAW};
use super::witness::{WithdrawInputs, WithdrawWitness};
use super::{chain, keys, notes, proving, scan, CoreError};

const PROOF_PATH: &str = "/workspace/circuits/withdraw/target/proof";
const PUBLIC_INPUTS_PATH: &str = "/workspace/circuits/withdraw/target/public_inputs";

/// The result of a withdraw: the tx hash, plus the shielded change-note opening (which
/// the caller must persist — the contract publishes no ciphertext for it).
pub struct WithdrawReceipt {
    pub tx_hash: String,
    pub change_value: u64,
    pub change_blinding: Fr,
    pub change_rho: Fr,
    pub change_epoch: u32,
}

/// Withdraw `amount` of `asset` (a v1 code, e.g. "USDC") to the public Stellar `dest`,
/// using the wallet in the OS keychain. Returns the tx hash (change opening is in the
/// `_with` form).
pub fn withdraw(asset: &str, dest: &str, amount: u64) -> Result<String, CoreError> {
    let wallet = keys::current_wallet()?;
    let cfg = PoolConfig::load()?.with_asset(asset)?;
    Ok(withdraw_with(&wallet, &cfg, dest, amount)?.tx_hash)
}

/// Withdraw with an explicit wallet + config (keychain-independent). Selects an owned
/// note covering `amount`, proves the withdraw, releases to `dest`, returns the receipt.
pub fn withdraw_with(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    dest: &str,
    amount: u64,
) -> Result<WithdrawReceipt, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let h = Hasher::new();
    let epoch = chain::current_epoch(&cfg.rpc_url)?;

    // One RPC drain -> commitment leaves + nullifier set + owned notes (incl. notes
    // recovered from the local store, e.g. a prior withdraw change).
    let state = chain::pool_state(cfg)?;
    let commitment_leaves = chain::commitment_leaves_from(&state.commits)?;
    let asp_leaves = chain::approved_set(cfg)?;
    let local = notes::load(wallet)?;
    let note = scan::owned_notes(&id, &state, &local, 0)?
        .into_iter()
        .find(|n| n.value >= amount && n.asset_tag == cfg.asset_tag)
        .ok_or_else(|| CoreError::Proving(format!("no single owned note covers {amount}")))?;
    if !asp_leaves.contains(&id.owner_pk) {
        return Err(CoreError::Proving(
            "wallet not enrolled in this pool's ASP approved set (cannot prove membership)".into(),
        ));
    }

    // dest_bind = Poseidon(DOMAIN_DEST, dest_pubkey). The pool recomputes this from the
    // real `dest` and requires equality (G13), so the proof is cryptographically bound
    // to this destination and can't be redirected.
    let dest_field = dest_to_field(dest)?;
    let dest_bind = h.hash(&[Fr::from_u64(DOMAIN_DEST), dest_field]);

    let change_blinding = Fr::random();
    let change_rho = Fr::random();
    let witness = WithdrawWitness::build(
        &h,
        WithdrawInputs {
            owner_sk: id.owner_sk,
            asset_tag: cfg.asset_tag,
            epoch: Fr::from_u64(epoch as u64),
            note_epoch: Fr::from_u64(note.epoch as u64),
            domain_sep: h.domain_sep(&cfg.pool_id, &cfg.network_id, SELECTOR_WITHDRAW),
            note_value: note.value,
            note_blinding: note.blinding,
            note_rho: note.rho,
            note_leaf_index: note.leaf_index as usize,
            commitment_leaves: &commitment_leaves,
            asp_leaves: &asp_leaves,
            prior_nullifiers: &state.nullifiers,
            dummy_rho: Fr::random(),
            amount,
            dest_bind,
            change_blinding,
            change_rho,
        },
    );

    // Prove (writes proof + public_inputs to circuits/withdraw/target; verifies vs VK).
    proving::prove_withdraw_witness(&witness)?;

    // Relayer-submitted if configured (fee abstraction; withdraw is permissionless).
    let tx_hash = chain::submit_withdraw(
        cfg,
        cfg.submit_source(wallet.stellar_secret()),
        dest,
        amount,
        PUBLIC_INPUTS_PATH,
        PROOF_PATH,
    )?;

    // Persist the shielded change opening: the pool publishes NO ciphertext for it, so
    // the local store is the only way a later scan can rediscover (and spend) it.
    let change_value = note.value - amount;
    if change_value > 0 {
        notes::add(
            wallet,
            NotePlaintext {
                value: change_value,
                asset_tag: cfg.asset_tag,
                blinding: change_blinding,
                epoch,
                rho: change_rho,
            },
        )?;
    }

    Ok(WithdrawReceipt {
        tx_hash,
        change_value,
        change_blinding,
        change_rho,
        change_epoch: epoch,
    })
}

/// Decode a Stellar `G…` address to its 32-byte Ed25519 public key, as a field element.
fn dest_to_field(dest: &str) -> Result<Fr, CoreError> {
    let pk = stellar_strkey::ed25519::PublicKey::from_string(dest)
        .map_err(|e| CoreError::Chain(format!("invalid dest address {dest}: {e}")))?;
    Ok(Fr(pk.0))
}
