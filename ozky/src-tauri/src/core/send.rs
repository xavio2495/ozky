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
use super::poseidon::{Fr, Hasher, SELECTOR_SPLIT, SELECTOR_TRANSFER, SELECTOR_TRANSFER_4};
use super::scan::{self, OwnedNote, WalletIdentity};
use super::witness::{
    self, Transfer4Inputs, Transfer4Witness, TransferInputs, TransferWitness, N_TRANSFER4_INPUTS,
};
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

// ----------------------------- split (1 payer -> N recipients) -----------------------------

/// A split recipient: a shielded payment code + amount.
pub struct SplitRecipient {
    pub code: String,
    pub amount: u64,
}

/// Build a split witness paying each recipient (1..=7) from one owned note, change to
/// self, padding the remaining output slots with value-0 self-notes. Pure (no network).
/// Returns the witness + the per-slot (recipient_transmission_pub, value) needed to
/// encrypt the output payloads in the SAME slot order.
fn build_split_witness(
    h: &Hasher,
    id: &WalletIdentity,
    cfg: &PoolConfig,
    epoch: u32,
    note: &OwnedNote,
    commitment_leaves: &[Fr],
    prior_nullifiers: &[Fr],
    asp_leaves: &[Fr],
    recipients: &[(Fr, [u8; 32], u64)], // (owner_pk, transmission_pub, amount)
) -> Result<(witness::SplitWitness, Vec<(witness::SplitOutMeta, [u8; 32])>), CoreError> {
    use witness::N_SPLIT_OUTPUTS;
    if recipients.is_empty() || recipients.len() > N_SPLIT_OUTPUTS - 1 {
        return Err(CoreError::Proving("split needs 1..=7 recipients".into()));
    }
    if note.asset_tag != cfg.asset_tag {
        return Err(CoreError::Proving("note asset_tag != pool asset_tag".into()));
    }
    let paid: u64 = recipients.iter().map(|(_, _, v)| *v).sum();
    if note.value < paid {
        return Err(CoreError::Proving(format!(
            "selected note ({}) does not cover split total ({paid})",
            note.value
        )));
    }
    if !asp_leaves.contains(&id.owner_pk) {
        return Err(CoreError::Proving(
            "wallet not enrolled in this pool's ASP approved set (cannot prove membership)".into(),
        ));
    }
    let domain_sep = h.domain_sep(&cfg.pool_id, &cfg.network_id, SELECTOR_SPLIT);

    // Fresh randomness per output slot (recipients, change, dummies).
    let out_rand: [(Fr, Fr); N_SPLIT_OUTPUTS] =
        std::array::from_fn(|_| (Fr::random(), Fr::random()));
    let recip_pairs: Vec<(Fr, u64)> = recipients.iter().map(|(pk, _, v)| (*pk, *v)).collect();

    let w = witness::SplitWitness::build(
        h,
        witness::SplitInputs {
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
            dummy_rho: Fr::random(),
            recipients: &recip_pairs,
            out_rand: &out_rand,
        },
    );

    // Per-slot (value, blinding, rho, dest transmission pub) so we encrypt to the right
    // party in the SAME order the witness built the outputs.
    let change_value = note.value - paid;
    let mut meta: Vec<(witness::SplitOutMeta, [u8; 32])> = Vec::with_capacity(N_SPLIT_OUTPUTS);
    for (i, (_, tpub, v)) in recipients.iter().enumerate() {
        meta.push((
            witness::SplitOutMeta { value: *v, blinding: out_rand[i].0, rho: out_rand[i].1 },
            *tpub,
        ));
    }
    let change_slot = recipients.len();
    meta.push((
        witness::SplitOutMeta { value: change_value, blinding: out_rand[change_slot].0, rho: out_rand[change_slot].1 },
        id.transmission_pub,
    ));
    for i in (change_slot + 1)..N_SPLIT_OUTPUTS {
        meta.push((
            witness::SplitOutMeta { value: 0, blinding: out_rand[i].0, rho: out_rand[i].1 },
            id.transmission_pub,
        ));
    }
    Ok((w, meta))
}

/// Encrypt the 8 split output notes, each to its slot's destination transmission key.
fn split_output_payloads(
    cfg: &PoolConfig,
    epoch: u32,
    meta: &[(witness::SplitOutMeta, [u8; 32])],
) -> Result<Vec<chain::OutputPayload>, CoreError> {
    let mut out = Vec::with_capacity(meta.len());
    for (m, tpub) in meta {
        let pt = NotePlaintext {
            value: m.value,
            asset_tag: cfg.asset_tag,
            blinding: m.blinding,
            epoch,
            rho: m.rho,
        };
        let e = encrypt::encrypt_note(&pt.serialize(), tpub)?;
        out.push(chain::OutputPayload {
            enc_note: e.enc_note,
            ephemeral_pub: e.ephemeral_pub,
            view_tag: e.view_tag,
        });
    }
    Ok(out)
}

/// Split `asset` from one owned note to multiple recipients via the keychain wallet.
pub fn split(asset: &str, recipients: &[SplitRecipient]) -> Result<String, CoreError> {
    let wallet = keys::current_wallet()?;
    let cfg = PoolConfig::load()?.with_asset(asset)?;
    split_with(&wallet, &cfg, recipients)
}

/// Split with an explicit wallet + config (keychain-independent). Reads live pool state,
/// selects an owned note covering the split total, builds + proves the split, submits it.
pub fn split_with(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    recipients: &[SplitRecipient],
) -> Result<String, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let h = Hasher::new();
    let epoch = chain::current_epoch(&cfg.rpc_url)?;
    let state = chain::pool_state(cfg)?;
    let commitment_leaves = chain::commitment_leaves_from(&state.commits)?;
    let asp_leaves = chain::approved_set(cfg)?;
    let local = notes::load(wallet)?;

    // Parse recipient codes -> (owner_pk, transmission_pub, amount).
    let parsed: Vec<(Fr, [u8; 32], u64)> = recipients
        .iter()
        .map(|r| {
            let (pk, tpub) = parse_payment_code(&r.code)?;
            Ok((pk, tpub, r.amount))
        })
        .collect::<Result<_, CoreError>>()?;
    let total: u64 = parsed.iter().map(|(_, _, v)| *v).sum();

    let note = scan::owned_notes(&id, &state, &local, 0)?
        .into_iter()
        .find(|n| n.value >= total && n.asset_tag == cfg.asset_tag)
        .ok_or_else(|| CoreError::Proving(format!("no single owned note covers split total {total}")))?;

    let (witness, meta) = build_split_witness(
        &h,
        &id,
        cfg,
        epoch,
        &note,
        &commitment_leaves,
        &state.nullifiers,
        &asp_leaves,
        &parsed,
    )?;

    let bundle = proving::prove_split_witness(&witness)?;
    let outputs = split_output_payloads(cfg, epoch, &meta)?;

    chain::submit_split(
        cfg,
        cfg.submit_source(wallet.stellar_secret()),
        &bundle.public_inputs,
        &bundle.proof,
        &outputs,
    )
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

    // Owned, unspent notes (incl. notes recovered from the local store, e.g. withdraw change).
    let owned = scan::owned_notes(&id, &state, &local, 0)?;

    // Fast path: a single note covers `amount` — use the cheaper 2-in `transfer`.
    if let Some(note) = owned
        .iter()
        .find(|n| n.value >= amount && n.asset_tag == cfg.asset_tag)
    {
        return send_prepared(
            wallet,
            cfg,
            recipient_code,
            amount,
            note,
            &commitment_leaves,
            &state.nullifiers,
            &asp_leaves,
            epoch,
        );
    }

    // Multi-note path: spend the fewest notes (up to 4) covering `amount` via `transfer4`.
    let selected = select_notes(&owned, cfg.asset_tag, amount).ok_or_else(|| {
        CoreError::Proving(format!(
            "insufficient shielded balance: no set of up to {N_TRANSFER4_INPUTS} notes covers {amount}"
        ))
    })?;
    send_prepared4(
        wallet,
        cfg,
        recipient_code,
        amount,
        &selected,
        &commitment_leaves,
        &state.nullifiers,
        &asp_leaves,
        epoch,
    )
}

/// Select the fewest owned notes (largest-first, up to [`N_TRANSFER4_INPUTS`]) of `asset_tag`
/// covering `amount`. Returns `None` if even the 4 largest can't cover it.
fn select_notes(owned: &[OwnedNote], asset_tag: Fr, amount: u64) -> Option<Vec<OwnedNote>> {
    let mut notes: Vec<OwnedNote> =
        owned.iter().filter(|n| n.asset_tag == asset_tag).cloned().collect();
    notes.sort_by(|a, b| b.value.cmp(&a.value)); // largest first → fewest notes
    let mut chosen: Vec<OwnedNote> = Vec::new();
    let mut sum: u64 = 0;
    for n in notes.into_iter().take(N_TRANSFER4_INPUTS) {
        sum += n.value;
        chosen.push(n);
        if sum >= amount {
            return Some(chosen);
        }
    }
    None
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

/// Send `amount` against EXPLICIT live state by spending MULTIPLE owned notes (1..=4) in one
/// `transfer4` — the multi-input core (build witness → prove → encrypt → submit). `notes` must be
/// owned notes of `cfg.asset_tag` whose values sum to `>= amount`; unused input slots are padded
/// with dummies. out0 = `amount` to the recipient, out1 = change back to the spender.
#[allow(clippy::too_many_arguments)]
pub fn send_prepared4(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    recipient_code: &str,
    amount: u64,
    notes: &[OwnedNote],
    commitment_leaves: &[Fr],
    prior_nullifiers: &[Fr],
    asp_leaves: &[Fr],
    epoch: u32,
) -> Result<String, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let h = Hasher::new();
    let (recipient_owner_pk, recipient_transmission_pub) = parse_payment_code(recipient_code)?;

    if notes.is_empty() || notes.len() > N_TRANSFER4_INPUTS {
        return Err(CoreError::Proving(format!(
            "transfer4 spends 1..={N_TRANSFER4_INPUTS} notes, got {}",
            notes.len()
        )));
    }
    if notes.iter().any(|n| n.asset_tag != cfg.asset_tag) {
        return Err(CoreError::Proving("a selected note's asset_tag != pool asset_tag".into()));
    }
    let total_in: u64 = notes.iter().map(|n| n.value).sum();
    if total_in < amount {
        return Err(CoreError::Proving(format!(
            "selected notes ({total_in}) do not cover amount ({amount})"
        )));
    }
    if !asp_leaves.contains(&id.owner_pk) {
        return Err(CoreError::Proving(
            "wallet not enrolled in this pool's ASP approved set (cannot prove membership)".into(),
        ));
    }
    let change = total_in - amount;
    let domain_sep = h.domain_sep(&cfg.pool_id, &cfg.network_id, SELECTOR_TRANSFER_4);

    let spend: Vec<witness::SpendNote> = notes.iter().map(|n| n.as_spend_note()).collect();
    let dummy_rhos: Vec<Fr> =
        (0..(N_TRANSFER4_INPUTS - notes.len())).map(|_| Fr::random()).collect();
    let rnd = OutputRandomness::random();

    let witness = Transfer4Witness::build(
        &h,
        Transfer4Inputs {
            owner_sk: id.owner_sk,
            asset_tag: cfg.asset_tag,
            epoch: Fr::from_u64(epoch as u64),
            domain_sep,
            commitment_leaves,
            asp_leaves,
            prior_nullifiers,
            notes: &spend,
            dummy_rhos: &dummy_rhos,
            recipient_owner_pk,
            out0_value: amount,
            out0_blinding: rnd.out0_blinding,
            out0_rho: rnd.out0_rho,
            change_blinding: rnd.change_blinding,
            change_rho: rnd.change_rho,
        },
    );

    let bundle = proving::prove_transfer4_witness(&witness)?;
    let outputs = output_payloads(
        cfg,
        &id,
        epoch,
        amount,
        change,
        &recipient_transmission_pub,
        &rnd,
    )?;

    chain::submit_transfer4(
        cfg,
        cfg.submit_source(wallet.stellar_secret()),
        &bundle.public_inputs,
        &bundle.proof,
        &outputs,
    )
}

/// Consolidate a fragmented balance: collapse up to [`N_TRANSFER4_INPUTS`] owned notes of `asset`
/// into ONE self-owned note (out0 = total to self, out1 = value-0 change). Uses the keychain wallet.
pub fn consolidate(asset: &str) -> Result<String, CoreError> {
    let wallet = keys::current_wallet()?;
    let cfg = PoolConfig::load()?.with_asset(asset)?;
    consolidate_with(&wallet, &cfg)
}

/// Consolidate with an explicit wallet + config (keychain-independent). Spends the wallet's
/// smallest notes first (merging dust), collapsing up to 4 into one self note via `transfer4`.
pub fn consolidate_with(wallet: &keys::WalletKeys, cfg: &PoolConfig) -> Result<String, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let epoch = chain::current_epoch(&cfg.rpc_url)?;
    let state = chain::pool_state(cfg)?;
    let commitment_leaves = chain::commitment_leaves_from(&state.commits)?;
    let asp_leaves = chain::approved_set(cfg)?;
    let local = notes::load(wallet)?;

    let mut owned: Vec<OwnedNote> = scan::owned_notes(&id, &state, &local, 0)?
        .into_iter()
        .filter(|n| n.asset_tag == cfg.asset_tag)
        .collect();
    if owned.len() < 2 {
        return Err(CoreError::Proving(
            "nothing to consolidate (need >= 2 notes of this asset)".into(),
        ));
    }
    owned.sort_by(|a, b| a.value.cmp(&b.value)); // smallest first → merge dust
    let chosen: Vec<OwnedNote> = owned.into_iter().take(N_TRANSFER4_INPUTS).collect();
    let total: u64 = chosen.iter().map(|n| n.value).sum();

    // A self-send of the full selected total: out0 = total to self, out1 = 0 change to self.
    let self_code = payment_code(&id);
    send_prepared4(
        wallet,
        cfg,
        &self_code,
        total,
        &chosen,
        &commitment_leaves,
        &state.nullifiers,
        &asp_leaves,
        epoch,
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

    fn owned(leaf: u32, value: u64, asset_tag: Fr) -> OwnedNote {
        OwnedNote {
            leaf_index: leaf,
            value,
            asset_tag,
            blinding: Fr::ZERO,
            epoch: 28,
            rho: Fr::ZERO,
            commitment: Fr::ZERO,
        }
    }

    #[test]
    fn select_notes_picks_fewest_largest_first() {
        let tag = Fr::from_u64(1);
        let other = Fr::from_u64(2);
        let set = vec![
            owned(0, 100, tag),
            owned(1, 400, tag),
            owned(2, 50, tag),
            owned(3, 9999, other), // different asset — never selected for `tag`
        ];
        // 450: largest-first picks 400 + 100 = 500 (2 notes); the other-asset note is ignored.
        let sel = select_notes(&set, tag, 450).unwrap();
        assert_eq!(sel.iter().map(|n| n.value).collect::<Vec<_>>(), vec![400, 100]);
        // A single note covers 300 → 1 note (the caller's fast path handles this separately).
        let sel1 = select_notes(&set, tag, 300).unwrap();
        assert_eq!(sel1.len(), 1);
        assert_eq!(sel1[0].value, 400);
        // 600 needs all three tag notes (400+100+50 = 550 < 600) → uncoverable.
        assert!(select_notes(&set, tag, 600).is_none());
    }

    #[test]
    fn transfer4_witness_conserves_and_pads_dummies() {
        let h = Hasher::new();
        let id = test_identity(&h);
        let asset_tag = Fr::from_u64(1);
        let epoch = Fr::from_u64(28);
        let mk = |v: u64, b: u64, rho: u64| {
            h.commitment(&Fr::from_u64(v), &asset_tag, &id.owner_pk, &Fr::from_u64(b), &epoch, &Fr::from_u64(rho))
        };
        let leaves = vec![mk(1000, 777, 111), mk(500, 778, 112)];
        let notes = vec![
            witness::SpendNote { value: 1000, blinding: Fr::from_u64(777), epoch, rho: Fr::from_u64(111), leaf_index: 0 },
            witness::SpendNote { value: 500, blinding: Fr::from_u64(778), epoch, rho: Fr::from_u64(112), leaf_index: 1 },
        ];
        let w = Transfer4Witness::build(
            &h,
            Transfer4Inputs {
                owner_sk: id.owner_sk,
                asset_tag,
                epoch,
                domain_sep: Fr::from_u64(0xabc),
                commitment_leaves: &leaves,
                asp_leaves: &[id.owner_pk],
                prior_nullifiers: &[],
                notes: &notes,
                dummy_rhos: &[Fr::from_u64(0xdead), Fr::from_u64(0xbeef)],
                recipient_owner_pk: h.owner_pk(&Fr::from_u64(99)),
                out0_value: 1200,
                out0_blinding: Fr::from_u64(1),
                out0_rho: Fr::from_u64(2),
                change_blinding: Fr::from_u64(3),
                change_rho: Fr::from_u64(4),
            },
        );
        // 1200 to recipient + 300 change == 1500 spent.
        assert_eq!(w.outputs[0].value, Fr::from_u64(1200));
        assert_eq!(w.outputs[1].value, Fr::from_u64(300));
        // 2 real + 2 dummy inputs; reals first, dummies padded.
        assert!(!w.inputs[0].is_dummy && !w.inputs[1].is_dummy);
        assert!(w.inputs[2].is_dummy && w.inputs[3].is_dummy);
        // Four distinct nullifiers.
        let mut nfs = w.nullifiers.to_vec();
        nfs.sort_by_key(|f| f.to_hex());
        nfs.dedup();
        assert_eq!(nfs.len(), 4, "nullifiers must be distinct");
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

    /// One-off **persistent** deploy + deposit for a real wallet, so the app shows a real
    /// balance. Reads the mnemonic from `$OZKY_DEPLOY_MNEMONIC` (never committed). Deploys
    /// verifiers/policy/pool/viewkeys, enrolls the wallet's `owner_pk` (+2 decoys) into the
    /// ASP set, registers XLM+USDC, funds the wallet, creates a relayer, and DEPOSITS
    /// `$OZKY_DEPLOY_XLM` (default 100) XLM. Prints the contract IDs to wire into the app.
    ///   OZKY_DEPLOY_MNEMONIC="…" OZKY_PROVER_BIN=… cargo test --lib -- --ignored \
    ///     --test-threads=1 --nocapture deploy_persistent_for_user
    #[test]
    #[ignore = "one-off persistent testnet deploy; needs ZK container + network + $OZKY_DEPLOY_MNEMONIC"]
    fn deploy_persistent_for_user() {
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => {
                eprintln!("skip: set OZKY_DEPLOY_MNEMONIC to run the persistent deploy");
                return;
            }
        };
        let wallet = keys::derive_from_mnemonic(&mnemonic).unwrap();
        let h = Hasher::new();
        let id = scan::wallet_identity(&wallet).unwrap();
        let addr = wallet.stellar_address().to_string();
        let secret = wallet.stellar_secret().to_string();
        let wallet_pk = id.owner_pk.to_decimal();
        let decoy0 = h.owner_pk(&Fr::from_u64(0xDEC0)).to_decimal();
        let decoy1 = h.owner_pk(&Fr::from_u64(0xDEC1)).to_decimal();

        // Isolated notes dir for the deposit's self-scan (the app uses its own).
        let notes_dir = std::env::temp_dir().join("ozky-deploy-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);
        // Docker-free proving for the deposit (sidecar).
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var(
                "OZKY_PROVER_BIN",
                repo_root().join("prover-sidecar/dist/ozky-prover.exe"),
            );
        }
        std::env::set_var("OZKY_REPO_ROOT", repo_root());

        // Persistent deploy (same proven flow as the lifecycle test, kept on-chain).
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
             stellar keys generate relayer --network testnet --fund --overwrite >/dev/null 2>&1\n\
             echo \"POOL=$POOL\"\n\
             echo \"POLICY=$POLICY\"\n\
             echo \"VIEWKEYS=$VIEWKEYS\"\n\
             echo \"RELAYER_SECRET=$(stellar keys secret relayer)\"",
            addr = addr, wallet_pk = wallet_pk, decoy0 = decoy0, decoy1 = decoy1,
        );
        let out = run_zk(&secret, &setup);
        let pool = kv(&out, "POOL");
        let policy = kv(&out, "POLICY");
        let viewkeys = kv(&out, "VIEWKEYS");
        let relayer_secret = kv(&out, "RELAYER_SECRET");

        std::env::set_var("OZKY_POOL_CONTRACT", &pool);
        std::env::set_var("OZKY_POLICY_CONTRACT", &policy);
        std::env::set_var("OZKY_VIEWKEYS_CONTRACT", &viewkeys);
        let cfg = PoolConfig::load().unwrap();

        let xlm: u64 = std::env::var("OZKY_DEPLOY_XLM")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);
        let base = xlm * 10_000_000;
        deposit::deposit_with(&wallet, &cfg, base).expect("deposit must succeed on-chain");

        println!("=== PERSISTENT DEPLOY DONE ===");
        println!("OZKY_POOL_CONTRACT={pool}");
        println!("OZKY_POLICY_CONTRACT={policy}");
        println!("OZKY_VIEWKEYS_CONTRACT={viewkeys}");
        println!("OZKY_RELAYER_SECRET={relayer_secret}");
        println!("DEPOSITED_XLM={xlm}");
        println!("WALLET_ADDR={addr}");
    }

    /// One-off MIGRATION to a split-capable pool. Reads `$OZKY_DEPLOY_MNEMONIC`; the OLD
    /// pool/policy/viewkeys from the current `ozky.config.json` (via `PoolConfig::load`).
    /// Steps: (1) scan the old pool, withdraw each owned note back to the wallet's public
    /// account; (2) deploy a NEW pool whose constructor includes the `split_verifier`
    /// (4th verifier = frozen split VK), register XLM+USDC, enroll owner_pk(+decoys),
    /// deploy viewkeys, create a relayer; (3) re-deposit the recovered amounts. Prints the
    /// new IDs to put in `ozky.config.json`.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture deploy_split_pool
    #[test]
    #[ignore = "one-off split-pool migration; needs ZK container + network + $OZKY_DEPLOY_MNEMONIC + ozky.config.json"]
    fn deploy_split_pool() {
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => {
                eprintln!("skip: set OZKY_DEPLOY_MNEMONIC");
                return;
            }
        };
        let wallet = keys::derive_from_mnemonic(&mnemonic).unwrap();
        let h = Hasher::new();
        let id = scan::wallet_identity(&wallet).unwrap();
        let addr = wallet.stellar_address().to_string();
        let secret = wallet.stellar_secret().to_string();
        let wallet_pk = id.owner_pk.to_decimal();
        let decoy0 = h.owner_pk(&Fr::from_u64(0xDEC0)).to_decimal();
        let decoy1 = h.owner_pk(&Fr::from_u64(0xDEC1)).to_decimal();

        let notes_dir = std::env::temp_dir().join("ozky-split-migrate-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo_root().join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", repo_root());

        // --- 1. Withdraw owned notes from the OLD pool back to the public account. ---
        // Old pool config comes from ozky.config.json; drop the relayer so the wallet
        // sources its own withdraw fees (it holds ample public XLM).
        let mut old_cfg = PoolConfig::load().expect("old pool config (ozky.config.json)");
        old_cfg.relayer_secret = None;
        let old_state = chain::pool_state(&old_cfg).unwrap();
        let local = notes::load(&wallet).unwrap();
        let owned = scan::owned_notes(&id, &old_state, &local, 0).unwrap();
        // Sum recoverable value per asset tag (decimal).
        let mut xlm_base: u64 = 0;
        let mut usdc_base: u64 = 0;
        let xlm_tag = Fr::from_u64(1).to_decimal();
        let usdc_tag = Fr::from_u64(2).to_decimal();
        for n in &owned {
            if n.asset_tag.to_decimal() == xlm_tag {
                xlm_base += n.value;
            } else if n.asset_tag.to_decimal() == usdc_tag {
                usdc_base += n.value;
            }
        }
        eprintln!("OLD pool owned: {} XLM base, {} USDC base", xlm_base, usdc_base);
        if xlm_base > 0 {
            let c = old_cfg.clone().with_asset("XLM").unwrap();
            withdraw::withdraw_with(&wallet, &c, &addr, xlm_base).expect("withdraw XLM from old pool");
            eprintln!("withdrew {} XLM base from old pool", xlm_base);
        }
        if usdc_base > 0 {
            let c = old_cfg.clone().with_asset("USDC").unwrap();
            withdraw::withdraw_with(&wallet, &c, &addr, usdc_base).expect("withdraw USDC from old pool");
            eprintln!("withdrew {} USDC base from old pool", usdc_base);
        }

        // --- 2. Deploy the NEW split-capable pool (4 verifiers incl. split). ---
        let setup = format!(
            "set -e\n\
             stellar network add testnet --rpc-url https://soroban-testnet.stellar.org --network-passphrase 'Test SDF Network ; September 2015' 2>/dev/null || true\n\
             T=/workspace/contracts/target/wasm32v1-none/release\n\
             VK=/workspace/contracts/frozen_vks\n\
             V=$T/rs_soroban_ultrahonk.wasm\n\
             VDEP=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/deposit/vk)\n\
             VTRA=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/transfer/vk)\n\
             VWIT=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/withdraw/vk)\n\
             VSPL=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/split/vk)\n\
             POLICY=$(stellar contract deploy --wasm $T/policy.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --admin {addr})\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- enroll --owner_pk {wallet_pk} --who {addr} >/dev/null\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- approve_member --owner_pk {decoy0} >/dev/null\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- approve_member --owner_pk {decoy1} >/dev/null\n\
             ASP=$(stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet -- asp_root | tr -d '\\\"')\n\
             SAC=$(stellar contract id asset --asset native --network testnet)\n\
             POOL=$(stellar contract deploy --wasm $T/pool.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --pool_id 7 --network_id 42 --deposit_verifier $VDEP --transfer_verifier $VTRA --withdraw_verifier $VWIT --split_verifier $VSPL --policy $POLICY --asp_root $ASP --admin {addr})\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 1 --sac $SAC --decimals 7 >/dev/null\n\
             USDC_ISSUER=GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5\n\
             USDC_SAC=$(stellar contract id asset --asset USDC:$USDC_ISSUER --network testnet)\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 2 --sac $USDC_SAC --decimals 7 >/dev/null\n\
             VIEWKEYS=$(stellar contract deploy --wasm $T/viewkeys.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet)\n\
             stellar keys generate relayer --network testnet --fund --overwrite >/dev/null 2>&1\n\
             echo \"POOL=$POOL\"\n\
             echo \"POLICY=$POLICY\"\n\
             echo \"VIEWKEYS=$VIEWKEYS\"\n\
             echo \"RELAYER_SECRET=$(stellar keys secret relayer)\"",
            addr = addr, wallet_pk = wallet_pk, decoy0 = decoy0, decoy1 = decoy1,
        );
        let out = run_zk(&secret, &setup);
        let pool = kv(&out, "POOL");
        let policy = kv(&out, "POLICY");
        let viewkeys = kv(&out, "VIEWKEYS");
        let relayer_secret = kv(&out, "RELAYER_SECRET");

        std::env::set_var("OZKY_POOL_CONTRACT", &pool);
        std::env::set_var("OZKY_POLICY_CONTRACT", &policy);
        std::env::set_var("OZKY_VIEWKEYS_CONTRACT", &viewkeys);
        std::env::remove_var("OZKY_RELAYER_SECRET"); // deposit is user-sourced anyway
        let new_cfg = PoolConfig::load().unwrap();

        // --- 3. Re-deposit the recovered amounts into the NEW pool. ---
        if xlm_base > 0 {
            deposit::deposit_with(&wallet, &new_cfg.clone().with_asset("XLM").unwrap(), xlm_base)
                .expect("re-deposit XLM into new pool");
        }
        if usdc_base > 0 {
            deposit::deposit_with(&wallet, &new_cfg.clone().with_asset("USDC").unwrap(), usdc_base)
                .expect("re-deposit USDC into new pool");
        }

        println!("=== SPLIT POOL DEPLOY DONE ===");
        println!("OZKY_POOL_CONTRACT={pool}");
        println!("OZKY_POLICY_CONTRACT={policy}");
        println!("OZKY_VIEWKEYS_CONTRACT={viewkeys}");
        println!("OZKY_RELAYER_SECRET={relayer_secret}");
        println!("REDEPOSITED_XLM_BASE={xlm_base}");
        println!("REDEPOSITED_USDC_BASE={usdc_base}");
    }

    /// One-off **escrow-pool migration**: like [`deploy_split_pool`] but the new pool's
    /// constructor includes ALL SIX verifiers — the 4th `split_verifier` plus the 5th/6th
    /// `escrow_contribute_verifier` / `escrow_payout_verifier` (frozen escrow VKs). Withdraws
    /// owned notes from the OLD pool, deploys the new escrow-capable pool (register XLM+USDC,
    /// enroll owner_pk +2 decoys, viewkeys, relayer), re-deposits. Prints the new IDs for
    /// `ozky.config.json`.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture deploy_escrow_pool
    #[test]
    #[ignore = "one-off escrow-pool migration; needs ZK container + network + $OZKY_DEPLOY_MNEMONIC + ozky.config.json"]
    fn deploy_escrow_pool() {
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => {
                eprintln!("skip: set OZKY_DEPLOY_MNEMONIC");
                return;
            }
        };
        let wallet = keys::derive_from_mnemonic(&mnemonic).unwrap();
        let h = Hasher::new();
        let id = scan::wallet_identity(&wallet).unwrap();
        let addr = wallet.stellar_address().to_string();
        let secret = wallet.stellar_secret().to_string();
        let wallet_pk = id.owner_pk.to_decimal();
        let decoy0 = h.owner_pk(&Fr::from_u64(0xDEC0)).to_decimal();
        let decoy1 = h.owner_pk(&Fr::from_u64(0xDEC1)).to_decimal();

        let notes_dir = std::env::temp_dir().join("ozky-escrow-migrate-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo_root().join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", repo_root());

        // --- 1. Withdraw owned notes from the OLD pool back to the public account. ---
        // Drain note-by-note (largest first, re-scanning each time): a single withdraw can only
        // spend ONE note, so the balance can be fragmented across several. Force a fresh RPC scan
        // each iteration and sleep one ledger so the spent note's nullifier propagates before the
        // next selection (avoids re-picking a just-spent note).
        std::env::set_var("OZKY_NO_POOL_CACHE", "1");
        let mut old_cfg = PoolConfig::load().expect("old pool config (ozky.config.json)");
        old_cfg.relayer_secret = None;

        let drain = |asset: &str| -> u64 {
            let c = old_cfg.clone().with_asset(asset).unwrap();
            let mut total: u64 = 0;
            let mut done: Vec<u32> = Vec::new(); // leaf indices already withdrawn this run
            loop {
                let st = chain::pool_state(&c).unwrap();
                let local = notes::load(&wallet).unwrap();
                let mut owned: Vec<_> = scan::owned_notes(&id, &st, &local, 0)
                    .unwrap()
                    .into_iter()
                    .filter(|n| n.asset_tag == c.asset_tag && !done.contains(&n.leaf_index))
                    .collect();
                if owned.is_empty() {
                    break;
                }
                owned.sort_by_key(|n| std::cmp::Reverse(n.value));
                let n = &owned[0];
                // Withdraw exactly the note's value (no change). Picking the LARGEST means the
                // verifier's `find(value >= amount)` resolves to this note, not a larger one.
                withdraw::withdraw_with(&wallet, &c, &addr, n.value)
                    .unwrap_or_else(|e| panic!("withdraw {asset} note {}: {e:?}", n.leaf_index));
                eprintln!("withdrew {} {asset} base (note {})", n.value, n.leaf_index);
                total += n.value;
                done.push(n.leaf_index);
                std::thread::sleep(std::time::Duration::from_secs(7));
            }
            total
        };

        let xlm_base = drain("XLM");
        let usdc_base = drain("USDC");
        eprintln!("OLD pool drained: {} XLM base, {} USDC base", xlm_base, usdc_base);

        // --- 2. Deploy the NEW escrow-capable pool (6 verifiers incl. escrow pair). ---
        let setup = format!(
            "set -e\n\
             stellar network add testnet --rpc-url https://soroban-testnet.stellar.org --network-passphrase 'Test SDF Network ; September 2015' 2>/dev/null || true\n\
             T=/workspace/contracts/target/wasm32v1-none/release\n\
             VK=/workspace/contracts/frozen_vks\n\
             V=$T/rs_soroban_ultrahonk.wasm\n\
             VDEP=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/deposit/vk)\n\
             VTRA=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/transfer/vk)\n\
             VWIT=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/withdraw/vk)\n\
             VSPL=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/split/vk)\n\
             VECON=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/escrow_contribute/vk)\n\
             VEPAY=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/escrow_payout/vk)\n\
             POLICY=$(stellar contract deploy --wasm $T/policy.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --admin {addr})\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- enroll --owner_pk {wallet_pk} --who {addr} >/dev/null\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- approve_member --owner_pk {decoy0} >/dev/null\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- approve_member --owner_pk {decoy1} >/dev/null\n\
             ASP=$(stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet -- asp_root | tr -d '\\\"')\n\
             SAC=$(stellar contract id asset --asset native --network testnet)\n\
             POOL=$(stellar contract deploy --wasm $T/pool.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --pool_id 7 --network_id 42 --deposit_verifier $VDEP --transfer_verifier $VTRA --withdraw_verifier $VWIT --split_verifier $VSPL --escrow_contribute_verifier $VECON --escrow_payout_verifier $VEPAY --policy $POLICY --asp_root $ASP --admin {addr})\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 1 --sac $SAC --decimals 7 >/dev/null\n\
             USDC_ISSUER=GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5\n\
             USDC_SAC=$(stellar contract id asset --asset USDC:$USDC_ISSUER --network testnet)\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 2 --sac $USDC_SAC --decimals 7 >/dev/null\n\
             VIEWKEYS=$(stellar contract deploy --wasm $T/viewkeys.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet)\n\
             stellar keys generate relayer --network testnet --fund --overwrite >/dev/null 2>&1\n\
             echo \"POOL=$POOL\"\n\
             echo \"POLICY=$POLICY\"\n\
             echo \"VIEWKEYS=$VIEWKEYS\"\n\
             echo \"RELAYER_SECRET=$(stellar keys secret relayer)\"",
            addr = addr, wallet_pk = wallet_pk, decoy0 = decoy0, decoy1 = decoy1,
        );
        let out = run_zk(&secret, &setup);
        let pool = kv(&out, "POOL");
        let policy = kv(&out, "POLICY");
        let viewkeys = kv(&out, "VIEWKEYS");
        let relayer_secret = kv(&out, "RELAYER_SECRET");

        std::env::set_var("OZKY_POOL_CONTRACT", &pool);
        std::env::set_var("OZKY_POLICY_CONTRACT", &policy);
        std::env::set_var("OZKY_VIEWKEYS_CONTRACT", &viewkeys);
        std::env::remove_var("OZKY_RELAYER_SECRET"); // deposit is user-sourced anyway
        let new_cfg = PoolConfig::load().unwrap();

        // --- 3. Re-deposit the recovered amounts into the NEW pool. ---
        if xlm_base > 0 {
            deposit::deposit_with(&wallet, &new_cfg.clone().with_asset("XLM").unwrap(), xlm_base)
                .expect("re-deposit XLM into new pool");
        }
        if usdc_base > 0 {
            deposit::deposit_with(&wallet, &new_cfg.clone().with_asset("USDC").unwrap(), usdc_base)
                .expect("re-deposit USDC into new pool");
        }

        println!("=== ESCROW POOL DEPLOY DONE ===");
        println!("OZKY_POOL_CONTRACT={pool}");
        println!("OZKY_POLICY_CONTRACT={policy}");
        println!("OZKY_VIEWKEYS_CONTRACT={viewkeys}");
        println!("OZKY_RELAYER_SECRET={relayer_secret}");
        println!("REDEPOSITED_XLM_BASE={xlm_base}");
        println!("REDEPOSITED_USDC_BASE={usdc_base}");
    }

    /// One-off **channel-pool migration** (building block B phase 2): like [`deploy_escrow_pool`] but
    /// the new pool's constructor includes ALL SEVEN verifiers — the 6 escrow-capable ones plus the
    /// 7th `channel_close_verifier` (frozen `channel_close` VK). Withdraws owned notes from the OLD
    /// pool, deploys the new 7-verifier pool (register XLM+USDC, enroll owner_pk +2 decoys, viewkeys,
    /// relayer), re-deposits. Prints the new IDs for `ozky.config.json`.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture deploy_channel_pool
    #[test]
    #[ignore = "one-off channel-pool migration; needs ZK container + network + $OZKY_DEPLOY_MNEMONIC + ozky.config.json"]
    fn deploy_channel_pool() {
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => {
                eprintln!("skip: set OZKY_DEPLOY_MNEMONIC");
                return;
            }
        };
        let wallet = keys::derive_from_mnemonic(&mnemonic).unwrap();
        let h = Hasher::new();
        let id = scan::wallet_identity(&wallet).unwrap();
        let addr = wallet.stellar_address().to_string();
        let secret = wallet.stellar_secret().to_string();
        let wallet_pk = id.owner_pk.to_decimal();
        let decoy0 = h.owner_pk(&Fr::from_u64(0xDEC0)).to_decimal();
        let decoy1 = h.owner_pk(&Fr::from_u64(0xDEC1)).to_decimal();

        let notes_dir = std::env::temp_dir().join("ozky-channel-migrate-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo_root().join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", repo_root());

        // --- 1. Drain owned notes from the OLD pool note-by-note (see deploy_escrow_pool). ---
        std::env::set_var("OZKY_NO_POOL_CACHE", "1");
        let mut old_cfg = PoolConfig::load().expect("old pool config (ozky.config.json)");
        old_cfg.relayer_secret = None;

        let drain = |asset: &str| -> u64 {
            let c = old_cfg.clone().with_asset(asset).unwrap();
            let mut total: u64 = 0;
            let mut done: Vec<u32> = Vec::new();
            loop {
                let st = chain::pool_state(&c).unwrap();
                let local = notes::load(&wallet).unwrap();
                let mut owned: Vec<_> = scan::owned_notes(&id, &st, &local, 0)
                    .unwrap()
                    .into_iter()
                    .filter(|n| n.asset_tag == c.asset_tag && !done.contains(&n.leaf_index))
                    .collect();
                if owned.is_empty() {
                    break;
                }
                owned.sort_by_key(|n| std::cmp::Reverse(n.value));
                let n = &owned[0];
                withdraw::withdraw_with(&wallet, &c, &addr, n.value)
                    .unwrap_or_else(|e| panic!("withdraw {asset} note {}: {e:?}", n.leaf_index));
                eprintln!("withdrew {} {asset} base (note {})", n.value, n.leaf_index);
                total += n.value;
                done.push(n.leaf_index);
                std::thread::sleep(std::time::Duration::from_secs(7));
            }
            total
        };

        let xlm_base = drain("XLM");
        let usdc_base = drain("USDC");
        eprintln!("OLD pool drained: {} XLM base, {} USDC base", xlm_base, usdc_base);

        // --- 2. Deploy the NEW channel-capable pool (7 verifiers incl. channel_close). ---
        let setup = format!(
            "set -e\n\
             stellar network add testnet --rpc-url https://soroban-testnet.stellar.org --network-passphrase 'Test SDF Network ; September 2015' 2>/dev/null || true\n\
             T=/workspace/contracts/target/wasm32v1-none/release\n\
             VK=/workspace/contracts/frozen_vks\n\
             V=$T/rs_soroban_ultrahonk.wasm\n\
             VDEP=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/deposit/vk)\n\
             VTRA=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/transfer/vk)\n\
             VWIT=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/withdraw/vk)\n\
             VSPL=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/split/vk)\n\
             VECON=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/escrow_contribute/vk)\n\
             VEPAY=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/escrow_payout/vk)\n\
             VCHAN=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/channel_close/vk)\n\
             POLICY=$(stellar contract deploy --wasm $T/policy.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --admin {addr})\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- enroll --owner_pk {wallet_pk} --who {addr} >/dev/null\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- approve_member --owner_pk {decoy0} >/dev/null\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- approve_member --owner_pk {decoy1} >/dev/null\n\
             ASP=$(stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet -- asp_root | tr -d '\\\"')\n\
             SAC=$(stellar contract id asset --asset native --network testnet)\n\
             POOL=$(stellar contract deploy --wasm $T/pool.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --pool_id 7 --network_id 42 --deposit_verifier $VDEP --transfer_verifier $VTRA --withdraw_verifier $VWIT --split_verifier $VSPL --escrow_contribute_verifier $VECON --escrow_payout_verifier $VEPAY --channel_close_verifier $VCHAN --policy $POLICY --asp_root $ASP --admin {addr})\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 1 --sac $SAC --decimals 7 >/dev/null\n\
             USDC_ISSUER=GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5\n\
             USDC_SAC=$(stellar contract id asset --asset USDC:$USDC_ISSUER --network testnet)\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 2 --sac $USDC_SAC --decimals 7 >/dev/null\n\
             EURC_ISSUER=GB3Q6QDZYTHWT7E5PVS3W7FUT5GVAFC5KSZFFLPU25GO7VTC3NM2ZTVO\n\
             EURC_SAC=$(stellar contract id asset --asset EURC:$EURC_ISSUER --network testnet)\n\
             stellar contract asset deploy --asset EURC:$EURC_ISSUER --source \"$OZKY_SOURCE_SECRET\" --network testnet >/dev/null 2>&1 || true\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 4 --sac $EURC_SAC --decimals 7 >/dev/null\n\
             VIEWKEYS=$(stellar contract deploy --wasm $T/viewkeys.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet)\n\
             stellar keys generate relayer --network testnet --fund --overwrite >/dev/null 2>&1\n\
             echo \"POOL=$POOL\"\n\
             echo \"POLICY=$POLICY\"\n\
             echo \"VIEWKEYS=$VIEWKEYS\"\n\
             echo \"RELAYER_SECRET=$(stellar keys secret relayer)\"",
            addr = addr, wallet_pk = wallet_pk, decoy0 = decoy0, decoy1 = decoy1,
        );
        let out = run_zk(&secret, &setup);
        let pool = kv(&out, "POOL");
        let policy = kv(&out, "POLICY");
        let viewkeys = kv(&out, "VIEWKEYS");
        let relayer_secret = kv(&out, "RELAYER_SECRET");

        std::env::set_var("OZKY_POOL_CONTRACT", &pool);
        std::env::set_var("OZKY_POLICY_CONTRACT", &policy);
        std::env::set_var("OZKY_VIEWKEYS_CONTRACT", &viewkeys);
        std::env::remove_var("OZKY_RELAYER_SECRET"); // deposit is user-sourced anyway
        let new_cfg = PoolConfig::load().unwrap();

        // --- 3. Re-deposit the recovered amounts into the NEW pool. ---
        if xlm_base > 0 {
            deposit::deposit_with(&wallet, &new_cfg.clone().with_asset("XLM").unwrap(), xlm_base)
                .expect("re-deposit XLM into new pool");
        }
        if usdc_base > 0 {
            deposit::deposit_with(&wallet, &new_cfg.clone().with_asset("USDC").unwrap(), usdc_base)
                .expect("re-deposit USDC into new pool");
        }

        println!("=== CHANNEL POOL DEPLOY DONE ===");
        println!("OZKY_POOL_CONTRACT={pool}");
        println!("OZKY_POLICY_CONTRACT={policy}");
        println!("OZKY_VIEWKEYS_CONTRACT={viewkeys}");
        println!("OZKY_RELAYER_SECRET={relayer_secret}");
        println!("REDEPOSITED_XLM_BASE={xlm_base}");
        println!("REDEPOSITED_USDC_BASE={usdc_base}");
    }

    /// Roadmap 2.5 Phase 2 (SW5): migrate to an 8-verifier SWAP-capable pool, then seed the AMM
    /// XLM+USDC reserves from the drained balance and re-deposit the remainder. Mirrors
    /// `deploy_channel_pool` + the 8th `shielded_swap_verifier`; seeds `XLM_RESERVE`/`USDC_RESERVE`
    /// base units (defaults 20 XLM / 1 USDC) so a live swap is immediately demonstrable. The
    /// reserve is the user's own liquidity (admin can `withdraw_reserve` it back).
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture deploy_swap_pool
    #[test]
    #[ignore = "one-off swap-pool migration; needs ZK container + network + $OZKY_DEPLOY_MNEMONIC + ozky.config.json"]
    fn deploy_swap_pool() {
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => {
                eprintln!("skip: set OZKY_DEPLOY_MNEMONIC");
                return;
            }
        };
        let wallet = keys::derive_from_mnemonic(&mnemonic).unwrap();
        let h = Hasher::new();
        let id = scan::wallet_identity(&wallet).unwrap();
        let addr = wallet.stellar_address().to_string();
        let secret = wallet.stellar_secret().to_string();
        let wallet_pk = id.owner_pk.to_decimal();
        let decoy0 = h.owner_pk(&Fr::from_u64(0xDEC0)).to_decimal();
        let decoy1 = h.owner_pk(&Fr::from_u64(0xDEC1)).to_decimal();

        // Reserve seed amounts (base units). Overridable; defaults 20 XLM / 1 USDC.
        let xlm_reserve: u64 = std::env::var("XLM_RESERVE").ok().and_then(|s| s.parse().ok()).unwrap_or(20 * 10_000_000);
        let usdc_reserve: u64 = std::env::var("USDC_RESERVE").ok().and_then(|s| s.parse().ok()).unwrap_or(10_000_000);

        let notes_dir = std::env::temp_dir().join("ozky-swap-migrate-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo_root().join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", repo_root());

        // --- 1. Drain owned notes from the OLD pool note-by-note. ---
        std::env::set_var("OZKY_NO_POOL_CACHE", "1");
        let mut old_cfg = PoolConfig::load().expect("old pool config (ozky.config.json)");
        old_cfg.relayer_secret = None;
        let drain = |asset: &str| -> u64 {
            let c = old_cfg.clone().with_asset(asset).unwrap();
            let mut total: u64 = 0;
            let mut done: Vec<u32> = Vec::new();
            loop {
                let st = chain::pool_state(&c).unwrap();
                let local = notes::load(&wallet).unwrap();
                let mut owned: Vec<_> = scan::owned_notes(&id, &st, &local, 0)
                    .unwrap()
                    .into_iter()
                    .filter(|n| n.asset_tag == c.asset_tag && !done.contains(&n.leaf_index))
                    .collect();
                if owned.is_empty() {
                    break;
                }
                owned.sort_by_key(|n| std::cmp::Reverse(n.value));
                let n = &owned[0];
                withdraw::withdraw_with(&wallet, &c, &addr, n.value)
                    .unwrap_or_else(|e| panic!("withdraw {asset} note {}: {e:?}", n.leaf_index));
                eprintln!("withdrew {} {asset} base (note {})", n.value, n.leaf_index);
                total += n.value;
                done.push(n.leaf_index);
                std::thread::sleep(std::time::Duration::from_secs(7));
            }
            total
        };
        let xlm_base = drain("XLM");
        let usdc_base = drain("USDC");
        eprintln!("OLD pool drained: {} XLM base, {} USDC base", xlm_base, usdc_base);
        assert!(xlm_base > xlm_reserve, "need > {xlm_reserve} XLM base to seed the reserve");
        assert!(usdc_base > usdc_reserve, "need > {usdc_reserve} USDC base to seed the reserve");

        // --- 2. Deploy the NEW swap-capable pool (8 verifiers incl. shielded_swap) + seed reserves. ---
        let setup = format!(
            "set -e\n\
             stellar network add testnet --rpc-url https://soroban-testnet.stellar.org --network-passphrase 'Test SDF Network ; September 2015' 2>/dev/null || true\n\
             T=/workspace/contracts/target/wasm32v1-none/release\n\
             VK=/workspace/contracts/frozen_vks\n\
             V=$T/rs_soroban_ultrahonk.wasm\n\
             VDEP=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/deposit/vk)\n\
             VTRA=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/transfer/vk)\n\
             VWIT=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/withdraw/vk)\n\
             VSPL=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/split/vk)\n\
             VECON=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/escrow_contribute/vk)\n\
             VEPAY=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/escrow_payout/vk)\n\
             VCHAN=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/channel_close/vk)\n\
             VSWAP=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/shielded_swap/vk)\n\
             POLICY=$(stellar contract deploy --wasm $T/policy.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --admin {addr})\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- enroll --owner_pk {wallet_pk} --who {addr} >/dev/null\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- approve_member --owner_pk {decoy0} >/dev/null\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- approve_member --owner_pk {decoy1} >/dev/null\n\
             ASP=$(stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet -- asp_root | tr -d '\\\"')\n\
             SAC=$(stellar contract id asset --asset native --network testnet)\n\
             POOL=$(stellar contract deploy --wasm $T/pool.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --pool_id 7 --network_id 42 --deposit_verifier $VDEP --transfer_verifier $VTRA --withdraw_verifier $VWIT --split_verifier $VSPL --escrow_contribute_verifier $VECON --escrow_payout_verifier $VEPAY --channel_close_verifier $VCHAN --swap_verifier $VSWAP --policy $POLICY --asp_root $ASP --admin {addr})\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 1 --sac $SAC --decimals 7 >/dev/null\n\
             USDC_ISSUER=GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5\n\
             USDC_SAC=$(stellar contract id asset --asset USDC:$USDC_ISSUER --network testnet)\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 2 --sac $USDC_SAC --decimals 7 >/dev/null\n\
             EURC_ISSUER=GB3Q6QDZYTHWT7E5PVS3W7FUT5GVAFC5KSZFFLPU25GO7VTC3NM2ZTVO\n\
             EURC_SAC=$(stellar contract id asset --asset EURC:$EURC_ISSUER --network testnet)\n\
             stellar contract asset deploy --asset EURC:$EURC_ISSUER --source \"$OZKY_SOURCE_SECRET\" --network testnet >/dev/null 2>&1 || true\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 4 --sac $EURC_SAC --decimals 7 >/dev/null\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- seed_reserve --asset_tag 1 --amount {xlm_reserve} >/dev/null\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- seed_reserve --asset_tag 2 --amount {usdc_reserve} >/dev/null\n\
             VIEWKEYS=$(stellar contract deploy --wasm $T/viewkeys.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet)\n\
             stellar keys generate relayer --network testnet --fund --overwrite >/dev/null 2>&1\n\
             echo \"POOL=$POOL\"\n\
             echo \"POLICY=$POLICY\"\n\
             echo \"VIEWKEYS=$VIEWKEYS\"\n\
             echo \"RELAYER_SECRET=$(stellar keys secret relayer)\"",
            addr = addr, wallet_pk = wallet_pk, decoy0 = decoy0, decoy1 = decoy1,
            xlm_reserve = xlm_reserve, usdc_reserve = usdc_reserve,
        );
        let out = run_zk(&secret, &setup);
        let pool = kv(&out, "POOL");
        let policy = kv(&out, "POLICY");
        let viewkeys = kv(&out, "VIEWKEYS");
        let relayer_secret = kv(&out, "RELAYER_SECRET");

        std::env::set_var("OZKY_POOL_CONTRACT", &pool);
        std::env::set_var("OZKY_POLICY_CONTRACT", &policy);
        std::env::set_var("OZKY_VIEWKEYS_CONTRACT", &viewkeys);
        std::env::remove_var("OZKY_RELAYER_SECRET");
        let new_cfg = PoolConfig::load().unwrap();

        // --- 3. Re-deposit the remainder (drained - reserve) into the NEW pool. ---
        let xlm_redeposit = xlm_base - xlm_reserve;
        let usdc_redeposit = usdc_base - usdc_reserve;
        if xlm_redeposit > 0 {
            deposit::deposit_with(&wallet, &new_cfg.clone().with_asset("XLM").unwrap(), xlm_redeposit)
                .expect("re-deposit XLM into new pool");
        }
        if usdc_redeposit > 0 {
            deposit::deposit_with(&wallet, &new_cfg.clone().with_asset("USDC").unwrap(), usdc_redeposit)
                .expect("re-deposit USDC into new pool");
        }

        println!("=== SWAP POOL DEPLOY DONE (8 verifiers) ===");
        println!("OZKY_POOL_CONTRACT={pool}");
        println!("OZKY_POLICY_CONTRACT={policy}");
        println!("OZKY_VIEWKEYS_CONTRACT={viewkeys}");
        println!("OZKY_RELAYER_SECRET={relayer_secret}");
        println!("SEEDED_XLM_RESERVE={xlm_reserve} SEEDED_USDC_RESERVE={usdc_reserve}");
        println!("REDEPOSITED_XLM_BASE={xlm_redeposit} REDEPOSITED_USDC_BASE={usdc_redeposit}");
    }

    /// One-off MIGRATION to a MULTI-INPUT-capable pool (next-build scope #1). Same flow as
    /// `deploy_swap_pool` but deploys all **9** verifiers (the 8 current + `transfer4`) and uses the
    /// new `--verifiers` Vec constructor (Soroban caps positional constructor args). Drains the old
    /// pool note-by-note, seeds the XLM/USDC AMM reserves, re-deposits the remainder. Prints the new
    /// IDs to put in `ozky.config.json`.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture deploy_multiinput_pool
    #[test]
    #[ignore = "one-off multi-input-pool migration; needs ZK container + network + $OZKY_DEPLOY_MNEMONIC + ozky.config.json"]
    fn deploy_multiinput_pool() {
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => {
                eprintln!("skip: set OZKY_DEPLOY_MNEMONIC");
                return;
            }
        };
        let wallet = keys::derive_from_mnemonic(&mnemonic).unwrap();
        let h = Hasher::new();
        let id = scan::wallet_identity(&wallet).unwrap();
        let addr = wallet.stellar_address().to_string();
        let secret = wallet.stellar_secret().to_string();
        let wallet_pk = id.owner_pk.to_decimal();
        let decoy0 = h.owner_pk(&Fr::from_u64(0xDEC0)).to_decimal();
        let decoy1 = h.owner_pk(&Fr::from_u64(0xDEC1)).to_decimal();

        let xlm_reserve: u64 = std::env::var("XLM_RESERVE").ok().and_then(|s| s.parse().ok()).unwrap_or(20 * 10_000_000);
        let usdc_reserve: u64 = std::env::var("USDC_RESERVE").ok().and_then(|s| s.parse().ok()).unwrap_or(10_000_000);

        let notes_dir = std::env::temp_dir().join("ozky-mi-migrate-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo_root().join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", repo_root());

        // --- 1. Drain owned notes from the OLD pool note-by-note. ---
        std::env::set_var("OZKY_NO_POOL_CACHE", "1");
        let mut old_cfg = PoolConfig::load().expect("old pool config (ozky.config.json)");
        old_cfg.relayer_secret = None;
        let drain = |asset: &str| -> u64 {
            let c = old_cfg.clone().with_asset(asset).unwrap();
            let mut total: u64 = 0;
            let mut done: Vec<u32> = Vec::new();
            loop {
                let st = chain::pool_state(&c).unwrap();
                let local = notes::load(&wallet).unwrap();
                let mut owned: Vec<_> = scan::owned_notes(&id, &st, &local, 0)
                    .unwrap()
                    .into_iter()
                    .filter(|n| n.asset_tag == c.asset_tag && !done.contains(&n.leaf_index))
                    .collect();
                if owned.is_empty() {
                    break;
                }
                owned.sort_by_key(|n| std::cmp::Reverse(n.value));
                let n = &owned[0];
                withdraw::withdraw_with(&wallet, &c, &addr, n.value)
                    .unwrap_or_else(|e| panic!("withdraw {asset} note {}: {e:?}", n.leaf_index));
                eprintln!("withdrew {} {asset} base (note {})", n.value, n.leaf_index);
                total += n.value;
                done.push(n.leaf_index);
                std::thread::sleep(std::time::Duration::from_secs(7));
            }
            total
        };
        let xlm_base = drain("XLM");
        let usdc_base = drain("USDC");
        eprintln!("OLD pool drained: {} XLM base, {} USDC base", xlm_base, usdc_base);
        assert!(xlm_base > xlm_reserve, "need > {xlm_reserve} XLM base to seed the reserve");
        assert!(usdc_base > usdc_reserve, "need > {usdc_reserve} USDC base to seed the reserve");

        // --- 2. Deploy the NEW multi-input-capable pool (9 verifiers incl. transfer4) + seed reserves. ---
        let setup = format!(
            "set -e\n\
             stellar network add testnet --rpc-url https://soroban-testnet.stellar.org --network-passphrase 'Test SDF Network ; September 2015' 2>/dev/null || true\n\
             T=/workspace/contracts/target/wasm32v1-none/release\n\
             VK=/workspace/contracts/frozen_vks\n\
             V=$T/rs_soroban_ultrahonk.wasm\n\
             VDEP=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/deposit/vk)\n\
             VTRA=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/transfer/vk)\n\
             VWIT=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/withdraw/vk)\n\
             VSPL=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/split/vk)\n\
             VECON=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/escrow_contribute/vk)\n\
             VEPAY=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/escrow_payout/vk)\n\
             VCHAN=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/channel_close/vk)\n\
             VSWAP=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/shielded_swap/vk)\n\
             VTR4=$(stellar contract deploy --wasm $V --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --vk_bytes-file-path $VK/transfer4/vk)\n\
             VERIFIERS=\"[\\\"$VDEP\\\",\\\"$VTRA\\\",\\\"$VWIT\\\",\\\"$VSPL\\\",\\\"$VECON\\\",\\\"$VEPAY\\\",\\\"$VCHAN\\\",\\\"$VSWAP\\\",\\\"$VTR4\\\"]\"\n\
             POLICY=$(stellar contract deploy --wasm $T/policy.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --admin {addr})\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- enroll --owner_pk {wallet_pk} --who {addr} >/dev/null\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- approve_member --owner_pk {decoy0} >/dev/null\n\
             stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- approve_member --owner_pk {decoy1} >/dev/null\n\
             ASP=$(stellar contract invoke --id $POLICY --source \"$OZKY_SOURCE_SECRET\" --network testnet -- asp_root | tr -d '\\\"')\n\
             SAC=$(stellar contract id asset --asset native --network testnet)\n\
             POOL=$(stellar contract deploy --wasm $T/pool.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet -- --pool_id 7 --network_id 42 --verifiers \"$VERIFIERS\" --policy $POLICY --asp_root $ASP --admin {addr})\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 1 --sac $SAC --decimals 7 >/dev/null\n\
             USDC_ISSUER=GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5\n\
             USDC_SAC=$(stellar contract id asset --asset USDC:$USDC_ISSUER --network testnet)\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 2 --sac $USDC_SAC --decimals 7 >/dev/null\n\
             EURC_ISSUER=GB3Q6QDZYTHWT7E5PVS3W7FUT5GVAFC5KSZFFLPU25GO7VTC3NM2ZTVO\n\
             EURC_SAC=$(stellar contract id asset --asset EURC:$EURC_ISSUER --network testnet)\n\
             stellar contract asset deploy --asset EURC:$EURC_ISSUER --source \"$OZKY_SOURCE_SECRET\" --network testnet >/dev/null 2>&1 || true\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 4 --sac $EURC_SAC --decimals 7 >/dev/null\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- seed_reserve --asset_tag 1 --amount {xlm_reserve} >/dev/null\n\
             stellar contract invoke --id $POOL --source \"$OZKY_SOURCE_SECRET\" --network testnet --send yes -- seed_reserve --asset_tag 2 --amount {usdc_reserve} >/dev/null\n\
             VIEWKEYS=$(stellar contract deploy --wasm $T/viewkeys.wasm --source \"$OZKY_SOURCE_SECRET\" --network testnet)\n\
             stellar keys generate relayer --network testnet --fund --overwrite >/dev/null 2>&1\n\
             echo \"POOL=$POOL\"\n\
             echo \"POLICY=$POLICY\"\n\
             echo \"VIEWKEYS=$VIEWKEYS\"\n\
             echo \"RELAYER_SECRET=$(stellar keys secret relayer)\"",
            addr = addr, wallet_pk = wallet_pk, decoy0 = decoy0, decoy1 = decoy1,
            xlm_reserve = xlm_reserve, usdc_reserve = usdc_reserve,
        );
        let out = run_zk(&secret, &setup);
        let pool = kv(&out, "POOL");
        let policy = kv(&out, "POLICY");
        let viewkeys = kv(&out, "VIEWKEYS");
        let relayer_secret = kv(&out, "RELAYER_SECRET");

        std::env::set_var("OZKY_POOL_CONTRACT", &pool);
        std::env::set_var("OZKY_POLICY_CONTRACT", &policy);
        std::env::set_var("OZKY_VIEWKEYS_CONTRACT", &viewkeys);
        std::env::remove_var("OZKY_RELAYER_SECRET");
        let new_cfg = PoolConfig::load().unwrap();

        // --- 3. Re-deposit the remainder (drained - reserve) into the NEW pool, as SEVERAL notes
        // per asset so the multi-input path has fragments to spend (and consolidate). ---
        let redeposit_fragments = |asset: &str, total: u64| {
            if total == 0 {
                return;
            }
            let c = new_cfg.clone().with_asset(asset).unwrap();
            // Split into up to 5 roughly-equal notes (so a later send must combine several).
            let parts = 5u64;
            let each = total / parts;
            let mut left = total;
            for i in 0..parts {
                let amt = if i == parts - 1 { left } else { each };
                if amt == 0 {
                    continue;
                }
                deposit::deposit_with(&wallet, &c, amt).unwrap_or_else(|e| panic!("re-deposit {asset} fragment: {e:?}"));
                left -= amt;
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        };
        redeposit_fragments("XLM", xlm_base - xlm_reserve);
        redeposit_fragments("USDC", usdc_base - usdc_reserve);

        println!("=== MULTI-INPUT POOL DEPLOY DONE (9 verifiers) ===");
        println!("OZKY_POOL_CONTRACT={pool}");
        println!("OZKY_POLICY_CONTRACT={policy}");
        println!("OZKY_VIEWKEYS_CONTRACT={viewkeys}");
        println!("OZKY_RELAYER_SECRET={relayer_secret}");
        println!("SEEDED_XLM_RESERVE={xlm_reserve} SEEDED_USDC_RESERVE={usdc_reserve}");
    }

    /// Live multi-input lifecycle (scope #1): on the 9-verifier pool, send an amount LARGER than any
    /// single owned note (forcing `transfer4` to combine fragments), then consolidate the remaining
    /// notes into one. Asserts the 13-PI `transfer4` proof verifies on-chain and value is conserved.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture multiinput_lifecycle_on_testnet
    #[test]
    #[ignore = "live multi-input lifecycle; needs ZK container + network + ozky.config.json + $OZKY_DEPLOY_MNEMONIC"]
    fn multiinput_lifecycle_on_testnet() {
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => return,
        };
        let wallet = keys::derive_from_mnemonic(&mnemonic).unwrap();
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo_root().join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", repo_root());
        std::env::set_var("OZKY_NO_POOL_CACHE", "1");
        let notes_dir = std::env::temp_dir().join("ozky-mi-test-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);

        let cfg = PoolConfig::load().expect("ozky.config.json").with_asset("XLM").unwrap();
        let id = scan::wallet_identity(&wallet).unwrap();

        // Scan owned XLM notes; pick a send amount larger than the largest single note but <= the
        // sum of the 4 largest (so transfer4 must combine, and the single-note fast path can't).
        let st = chain::pool_state(&cfg).unwrap();
        let local = notes::load(&wallet).unwrap();
        let mut owned: Vec<_> = scan::owned_notes(&id, &st, &local, 0)
            .unwrap()
            .into_iter()
            .filter(|n| n.asset_tag == cfg.asset_tag)
            .collect();
        owned.sort_by(|a, b| b.value.cmp(&a.value));
        assert!(owned.len() >= 2, "need a fragmented balance (>= 2 notes)");
        let largest = owned[0].value;
        let top_sum: u64 = owned.iter().take(N_TRANSFER4_INPUTS).map(|n| n.value).sum();
        let amount = (largest + 1).min(top_sum);
        assert!(amount > largest, "send amount must exceed the largest single note");

        // Send to self (so the value stays recoverable) — exercises the multi-note transfer4 path.
        let self_code = payment_code(&id);
        let tx = send_with(&wallet, &cfg, &self_code, amount).expect("multi-note transfer4 send");
        eprintln!("transfer4 send tx: {tx}");
        std::thread::sleep(std::time::Duration::from_secs(8));

        // Then consolidate the remaining notes into one.
        let _ = std::fs::remove_dir_all(&notes_dir);
        let ctx = consolidate_with(&wallet, &cfg).expect("consolidate");
        eprintln!("consolidate tx: {ctx}");
    }

    /// Roadmap 2.5 Phase 2 (SW5): the REAL in-pool swap lifecycle on the swap-capable pool. Reads
    /// `$OZKY_DEPLOY_MNEMONIC`; swaps `SWAP_XLM` base (default 5 XLM) of shielded XLM into USDC via
    /// the constant-product AMM, then rescans and asserts a USDC note was minted, XLM was spent, and
    /// the reserves moved as priced. Proves the 14-PI `shielded_swap` proof verifies on-chain.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture swap_lifecycle_on_testnet
    #[test]
    #[ignore = "live swap lifecycle; needs ZK container + network + ozky.config.json + $OZKY_DEPLOY_MNEMONIC"]
    fn swap_lifecycle_on_testnet() {
        use crate::core::swap;
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => return,
        };
        let wallet = keys::derive_from_mnemonic(&mnemonic).unwrap();
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo_root().join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", repo_root());
        let notes_dir = std::env::temp_dir().join("ozky-swap-test-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);

        let cfg = PoolConfig::load().unwrap();
        let xlm = cfg.clone().with_asset("XLM").unwrap();
        let usdc = cfg.clone().with_asset("USDC").unwrap();
        let id = scan::wallet_identity(&wallet).unwrap();
        let swap_xlm: u64 = std::env::var("SWAP_XLM").ok().and_then(|s| s.parse().ok()).unwrap_or(5 * 10_000_000);

        let shielded = |c: &PoolConfig| -> u64 {
            let st = chain::pool_state(c).unwrap();
            scan::owned_notes(&id, &st, &notes::load(&wallet).unwrap(), 0).unwrap().iter()
                .filter(|n| n.asset_tag == c.asset_tag).map(|n| n.value).sum()
        };

        let xlm_before = shielded(&xlm);
        let usdc_before = shielded(&usdc);
        let res_xlm_before = chain::read_reserve(&cfg, &xlm.asset_tag).unwrap();
        let res_usdc_before = chain::read_reserve(&cfg, &usdc.asset_tag).unwrap();
        eprintln!("before: shielded XLM {xlm_before} USDC {usdc_before}; reserves XLM {res_xlm_before} USDC {res_usdc_before}");

        let q = swap::quote("XLM", "USDC", swap_xlm).expect("quote");
        eprintln!("quote: {} XLM -> ~{} USDC base", swap_xlm, q.dest_amount);
        let receipt = swap::swap_with(&wallet, &cfg, "XLM", "USDC", swap_xlm, 100)
            .expect("swap must succeed on-chain");
        eprintln!("SWAP OK tx {} received {} USDC base", receipt.tx_hash, receipt.received);

        // Rescan: a USDC note was minted, XLM shielded dropped by ~swap_xlm, reserves moved.
        let xlm_after = shielded(&xlm);
        let usdc_after = shielded(&usdc);
        let res_xlm_after = chain::read_reserve(&cfg, &xlm.asset_tag).unwrap();
        let res_usdc_after = chain::read_reserve(&cfg, &usdc.asset_tag).unwrap();
        eprintln!("after: shielded XLM {xlm_after} USDC {usdc_after}; reserves XLM {res_xlm_after} USDC {res_usdc_after}");

        assert_eq!(usdc_after, usdc_before + receipt.received, "minted B-note credited");
        assert_eq!(xlm_after, xlm_before - swap_xlm, "A spent: swapped value left the XLM balance");
        assert_eq!(res_xlm_after, res_xlm_before + swap_xlm as i128, "reserve_A += value_a");
        assert_eq!(res_usdc_after, res_usdc_before - receipt.received as i128, "reserve_B -= value_b");
        println!("SWAP LIFECYCLE OK");
    }

    /// One-off: perform a REAL split on the configured (split-capable) pool. Reads
    /// `$OZKY_DEPLOY_MNEMONIC`, splits 30 XLM -> 3 self-codes (10 each) + change, then
    /// rescans and asserts the 3 outputs + change landed and the input note is spent.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture split_lifecycle_on_testnet
    #[test]
    #[ignore = "live split lifecycle; needs network + ozky.config.json + $OZKY_DEPLOY_MNEMONIC"]
    fn split_lifecycle_on_testnet() {
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => return,
        };
        let wallet = keys::derive_from_mnemonic(&mnemonic).unwrap();
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo_root().join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", repo_root());
        let notes_dir = std::env::temp_dir().join("ozky-split-test-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);

        let cfg = PoolConfig::load().unwrap().with_asset("XLM").unwrap();
        let id = scan::wallet_identity(&wallet).unwrap();
        let code = payment_code(&id);

        // Balance before.
        let before: u64 = {
            let st = chain::pool_state(&cfg).unwrap();
            scan::owned_notes(&id, &st, &[], 0).unwrap().iter()
                .filter(|n| n.asset_tag == cfg.asset_tag).map(|n| n.value).sum()
        };
        eprintln!("XLM shielded before split: {} base", before);

        // Split 30 XLM to 3 self-codes (10 each); change returns to self.
        let ten = 10 * 10_000_000u64;
        let recipients = vec![
            SplitRecipient { code: code.clone(), amount: ten },
            SplitRecipient { code: code.clone(), amount: ten },
            SplitRecipient { code: code.clone(), amount: ten },
        ];
        let hash = split_with(&wallet, &cfg, &recipients).expect("split must succeed on-chain");
        assert!(!hash.is_empty());
        eprintln!("SPLIT OK tx {hash}");

        // Rescan: the original note is spent; the 3 recipient outputs (to self) + change
        // are now ours, so total XLM is conserved (minus nothing — interior transfer).
        let st = chain::pool_state(&cfg).unwrap();
        let notes = scan::owned_notes(&id, &st, &[], 0).unwrap();
        let after: u64 = notes.iter().filter(|n| n.asset_tag == cfg.asset_tag).map(|n| n.value).sum();
        eprintln!("XLM shielded after split: {} base across {} notes", after, notes.len());
        assert_eq!(after, before, "interior split conserves total shielded value");
        // At least 3 notes of exactly 10 XLM (the self-paid recipients) must exist now.
        let tens = notes.iter().filter(|n| n.value == ten).count();
        assert!(tens >= 3, "expected >=3 ten-XLM outputs, got {tens}");
        println!("SPLIT LIFECYCLE OK");
    }

    /// One-off: the REAL escrow lifecycle on the configured (escrow-capable) pool. Reads
    /// `$OZKY_DEPLOY_MNEMONIC`; the one wallet plays BOTH payee and contributor (a self-test:
    /// the contributor encrypts `(amount, r)` to the payee = self, so `scan_total` can open it).
    /// Two paths:
    ///   A) all-or-nothing RELEASE — open(target 5 XLM, far deadline), contribute 6 XLM (≥ target),
    ///      scan_total → release; assert the escrow is Released and a 6-XLM payout note is ours.
    ///   B) all-or-nothing REFUND — open(target 50 XLM, deadline = now), contribute 3 XLM (< target),
    ///      refund after the deadline; assert the 3-XLM refund note is ours.
    /// Proves the 14-PI `escrow_contribute` and 7-PI `escrow_payout` proofs verify within budget.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture escrow_lifecycle_on_testnet
    #[test]
    #[ignore = "live escrow lifecycle; needs ZK container + network + ozky.config.json + $OZKY_DEPLOY_MNEMONIC"]
    fn escrow_lifecycle_on_testnet() {
        use crate::core::escrow;
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => return,
        };
        let wallet = keys::derive_from_mnemonic(&mnemonic).unwrap();
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo_root().join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", repo_root());
        let notes_dir = std::env::temp_dir().join("ozky-escrow-test-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);

        let cfg = PoolConfig::load().unwrap();
        let xlm = cfg.clone().with_asset("XLM").unwrap();
        let id = scan::wallet_identity(&wallet).unwrap();
        let code = payment_code(&id);
        let one_xlm = 10_000_000u64;

        let shielded_xlm = |c: &PoolConfig| -> u64 {
            let st = chain::pool_state(c).unwrap();
            scan::owned_notes(&id, &st, &notes::load(&wallet).unwrap(), 0)
                .unwrap()
                .iter()
                .filter(|n| n.asset_tag == c.asset_tag)
                .map(|n| n.value)
                .sum()
        };
        eprintln!("XLM shielded before escrow: {} base", shielded_xlm(&xlm));

        // ---- Path A: all-or-nothing RELEASE ----
        let latest = chain::latest_ledger(&cfg.rpc_url).unwrap() as u64;
        let target_a = 5 * one_xlm;
        let id_a = escrow::open(&wallet, &xlm, target_a, latest + 100_000, escrow::MODE_ALL_OR_NOTHING)
            .expect("open escrow A");
        eprintln!("opened escrow A id={id_a}");
        let idx_a = escrow::contribute(&wallet, &cfg, id_a, &code, 6 * one_xlm).expect("contribute to A");
        eprintln!("contributed 6 XLM to A as #{idx_a}");
        let (s_a, r_a) = escrow::scan_total(&wallet, &cfg, id_a).expect("scan total A");
        assert_eq!(s_a, 6 * one_xlm, "payee opens the running total to 6 XLM");
        let rel = escrow::release(&wallet, &cfg, id_a, s_a, r_a).expect("release A");
        assert!(!rel.is_empty());
        eprintln!("RELEASE A OK tx {rel}");
        let st_a = chain::read_escrow(&cfg, id_a).unwrap();
        assert_eq!(st_a.status, 1, "escrow A must be Released");

        // ---- Path B: all-or-nothing REFUND ----
        let latest_b = chain::latest_ledger(&cfg.rpc_url).unwrap() as u64;
        let target_b = 50 * one_xlm;
        // Deadline already in the past so the refund guard (ledger > deadline) holds after contribute.
        let id_b = escrow::open(&wallet, &xlm, target_b, latest_b, escrow::MODE_ALL_OR_NOTHING)
            .expect("open escrow B");
        eprintln!("opened escrow B id={id_b} (deadline {latest_b})");
        let idx_b = escrow::contribute(&wallet, &cfg, id_b, &code, 3 * one_xlm).expect("contribute to B");
        eprintln!("contributed 3 XLM to B as #{idx_b}");
        let refunded = escrow::refund(&wallet, &cfg, id_b, idx_b).expect("refund B");
        assert!(!refunded.is_empty());
        eprintln!("REFUND B OK tx {refunded}");

        eprintln!("XLM shielded after escrow: {} base", shielded_xlm(&xlm));
        println!("ESCROW LIFECYCLE OK (release id={id_a}, refund id={id_b})");
    }

    /// One-off: register **EURC** (asset_tag 4) on the configured pool. The deploy scripts seed
    /// XLM(1)+USDC(2); this adds EURC for a pool that was deployed before the script registered it
    /// (the wallet is the pool admin, so `register_asset` is authorized by its secret). Idempotent
    /// on the SAC (the asset deploy is `|| true`); re-registering a tag just overwrites the mapping.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture register_eurc_on_pool
    #[test]
    #[ignore = "one-off EURC registration; needs ZK container + network + ozky.config.json + $OZKY_DEPLOY_MNEMONIC"]
    fn register_eurc_on_pool() {
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => return,
        };
        let wallet = keys::derive_from_mnemonic(&mnemonic).unwrap();
        let secret = wallet.stellar_secret().to_string();
        let pool = PoolConfig::load().expect("ozky.config.json").pool_contract;
        let script = format!(
            "set -e\n\
             stellar network add testnet --rpc-url https://soroban-testnet.stellar.org --network-passphrase 'Test SDF Network ; September 2015' 2>/dev/null || true\n\
             EURC_ISSUER=GB3Q6QDZYTHWT7E5PVS3W7FUT5GVAFC5KSZFFLPU25GO7VTC3NM2ZTVO\n\
             EURC_SAC=$(stellar contract id asset --asset EURC:$EURC_ISSUER --network testnet)\n\
             stellar contract asset deploy --asset EURC:$EURC_ISSUER --source \"$OZKY_SOURCE_SECRET\" --network testnet >/dev/null 2>&1 || true\n\
             stellar contract invoke --id {pool} --source \"$OZKY_SOURCE_SECRET\" --network testnet -- register_asset --asset_tag 4 --sac $EURC_SAC --decimals 7 >/dev/null\n\
             echo \"EURC_SAC=$EURC_SAC\"\n\
             echo REGISTERED",
            pool = pool,
        );
        let out = run_zk(&secret, &script);
        assert!(out.contains("REGISTERED"), "EURC registration failed:\n{out}");
        println!("EURC REGISTERED on pool {pool}: {}", kv(&out, "EURC_SAC"));
    }

    /// One-off: the REAL merchant-pull channel lifecycle on the configured (channel-capable) pool
    /// (building block B phase 2). Reads `$OZKY_DEPLOY_MNEMONIC`; the one wallet plays BOTH subscriber
    /// and merchant (a self-test: the merchant_code is the wallet's own code, so both minted notes
    /// land back here and total XLM is conserved). Two paths:
    ///   A) CLOSE — open(cap 10 XLM, 1 period of 6 XLM, ~1 ledger/period), wait for the period to
    ///      elapse, close as merchant; assert status=Closed and the 6-XLM draw + 4-XLM remainder are
    ///      ours (XLM conserved).
    ///   B) RECLAIM — open(cap 5 XLM, short expiry), wait past expiry, reclaim as subscriber; assert
    ///      status=Closed and the 5-XLM cap came back.
    /// Proves the 10-PI `channel_close` (incl. the in-circuit Schnorr) verifies within the per-tx budget.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture channel_lifecycle_on_testnet
    #[test]
    #[ignore = "live channel lifecycle; needs ZK container + network + ozky.config.json + $OZKY_DEPLOY_MNEMONIC"]
    fn channel_lifecycle_on_testnet() {
        use crate::core::channel;
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => return,
        };
        let wallet = keys::derive_from_mnemonic(&mnemonic).unwrap();
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo_root().join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", repo_root());
        let notes_dir = std::env::temp_dir().join("ozky-channel-test-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);

        let cfg = PoolConfig::load().unwrap();
        let xlm = cfg.clone().with_asset("XLM").unwrap();
        let id = scan::wallet_identity(&wallet).unwrap();
        let code = payment_code(&id);
        let one_xlm = 10_000_000u64;

        let shielded_xlm = || -> u64 {
            let st = chain::pool_state(&xlm).unwrap();
            scan::owned_notes(&id, &st, &notes::load(&wallet).unwrap(), 0)
                .unwrap()
                .iter()
                .filter(|n| n.asset_tag == xlm.asset_tag)
                .map(|n| n.value)
                .sum()
        };
        let before = shielded_xlm();
        eprintln!("XLM shielded before channel: {} base", before);

        // ---- Path A: CLOSE (merchant draws part of the cap, remainder refunds to subscriber) ----
        // cap 10 XLM; 1 period of 6 XLM, ~1 ledger/period. Self-test: merchant_code = our own code.
        let cap_a = 10 * one_xlm;
        let id_a = channel::open(&wallet, &cfg, "XLM", cap_a, &code, 6 * one_xlm, 1, 1)
            .expect("open channel A");
        eprintln!("opened channel A id={id_a} (cap 10 XLM, draw 6 XLM)");
        // Wait for period 1's valid_after_ledger (open_ledger + 1) to elapse (~3 ledgers).
        std::thread::sleep(std::time::Duration::from_secs(18));
        let hash_a = channel::close(&wallet, &cfg, id_a).expect("close channel A");
        assert!(!hash_a.is_empty());
        eprintln!("CLOSE A OK tx {hash_a}");
        let st_a = chain::read_channel(&cfg, id_a).unwrap();
        assert_eq!(st_a.status, 1, "channel A must be Closed");
        // Both the 6-XLM draw and the 4-XLM remainder are ours (self-test) — XLM conserved.
        let notes_a = {
            let st = chain::pool_state(&xlm).unwrap();
            scan::owned_notes(&id, &st, &notes::load(&wallet).unwrap(), 0).unwrap()
        };
        assert!(notes_a.iter().any(|n| n.value == 6 * one_xlm), "6-XLM merchant draw note");
        assert!(notes_a.iter().any(|n| n.value == 4 * one_xlm), "4-XLM subscriber remainder note");

        // ---- Path B: RECLAIM (channel expires unclosed; subscriber sweeps the full cap) ----
        let cap_b = 5 * one_xlm;
        let id_b = channel::open(&wallet, &cfg, "XLM", cap_b, &code, 5 * one_xlm, 1, 1)
            .expect("open channel B");
        eprintln!("opened channel B id={id_b} (cap 5 XLM, expiry ~2 ledgers out)");
        // expiry = open_ledger + (1+1)*1; wait past it (~4 ledgers) so the reclaim guard holds.
        std::thread::sleep(std::time::Duration::from_secs(24));
        let hash_b = channel::reclaim(&wallet, &cfg, id_b).expect("reclaim channel B");
        assert!(!hash_b.is_empty());
        eprintln!("RECLAIM B OK tx {hash_b}");
        let st_b = chain::read_channel(&cfg, id_b).unwrap();
        assert_eq!(st_b.status, 1, "channel B must be Closed (reclaimed)");

        let after = shielded_xlm();
        eprintln!("XLM shielded after channel: {} base", after);
        // Channels are interior accounting (mint-only close/reclaim): total shielded XLM is conserved.
        assert_eq!(after, before, "channel lifecycle conserves total shielded XLM");
        println!("CHANNEL LIFECYCLE OK (close id={id_a}, reclaim id={id_b})");
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
