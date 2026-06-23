//! Escrow client flows (building block B, phase E4): open / contribute / release / refund, tying
//! the escrow circuits ([`super::witness`] contribute/payout), proving ([`super::proving`]) and
//! the pool's escrow entrypoints ([`super::chain`]) into the hidden-sum invoice flow.
//!
//! Privacy recap (claude-docs/escrow_interface.md): contribution amounts are hidden on-chain
//! (folded into a running Pedersen commitment); each contributor encrypts `(amount, r)` to the
//! payee so ONLY the payee can open the total to prove `S >= target` at release. Contributor
//! identities stay hidden from everyone.
//!
//! Persistence: the opener remembers its escrow's `payee_salt` (to release); each contributor
//! remembers its `(amount, r, contrib_salt)` (to refund). Encrypted at rest with the wallet key
//! (same scheme as the notes/payroll store). Change notes go to the local notes store (the pool
//! publishes no ciphertext for them, like withdraw change).

use super::config::PoolConfig;
use super::encrypt::{self, NotePlaintext};
use super::keys::WalletKeys;
use super::notes::{self, data_dir};
use super::pedersen;
use super::poseidon::{Fr, Hasher, DOMAIN_ESCROW_PAYEE, SELECTOR_ESCROW_CONTRIBUTE, SELECTOR_ESCROW_PAYOUT};
use super::scan::{self, WalletIdentity};
use super::witness::{ContributeInputs, ContributeWitness, PayoutInputs, PayoutWitness};
use super::{chain, proving, CoreError};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use rand_core::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub const MODE_ALL_OR_NOTHING: u32 = 0;
pub const MODE_KEEP_WHAT_YOU_RAISE: u32 = 1;

// ----------------------------- persisted records -----------------------------

/// An escrow this wallet opened (kept so the payee can later release it).
#[derive(Clone, Serialize, Deserialize)]
pub struct OpenedEscrow {
    pub escrow_id: u64,
    /// Hex of the `payee_salt` bound into `payee_bind` at open.
    pub payee_salt: String,
}

/// A contribution this wallet made (kept so it can refund if the escrow fails).
#[derive(Clone, Serialize, Deserialize)]
pub struct ContributionRecord {
    pub escrow_id: u64,
    pub contrib_index: u32,
    pub asset: String,
    pub amount: u64,
    /// Hex of the Pedersen blinding `r` used for this contribution's commitment.
    pub blinding_r: String,
    /// Hex of the `contrib_salt` bound into `refund_bind`.
    pub contrib_salt: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Store {
    opened: Vec<OpenedEscrow>,
    contributions: Vec<ContributionRecord>,
}

fn store_path(wallet: &WalletKeys) -> PathBuf {
    let digest = Sha256::digest(wallet.stellar_address().as_bytes());
    data_dir().join(format!("escrow-{}.enc", hex::encode(&digest[..8])))
}

fn cipher(wallet: &WalletKeys) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(Key::from_slice(&wallet.notes_key()))
}

fn load_store(wallet: &WalletKeys) -> Result<Store, CoreError> {
    let path = store_path(wallet);
    let blob = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Store::default()),
        Err(e) => return Err(CoreError::Crypto(format!("read escrow store: {e}"))),
    };
    if blob.len() < 12 {
        return Err(CoreError::Crypto("escrow store too short".into()));
    }
    let (nonce, ct) = blob.split_at(12);
    let plain = cipher(wallet)
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| CoreError::Crypto("escrow store decrypt failed".into()))?;
    serde_json::from_slice(&plain).map_err(|e| CoreError::Crypto(format!("escrow decode: {e}")))
}

fn save_store(wallet: &WalletKeys, s: &Store) -> Result<(), CoreError> {
    let plain = serde_json::to_vec(s).map_err(|e| CoreError::Crypto(format!("escrow encode: {e}")))?;
    let mut nonce = [0u8; 12];
    rand_core::OsRng.fill_bytes(&mut nonce);
    let ct = cipher(wallet)
        .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
        .map_err(|_| CoreError::Crypto("escrow store encrypt failed".into()))?;
    std::fs::create_dir_all(data_dir()).map_err(|e| CoreError::Crypto(format!("mkdir escrow dir: {e}")))?;
    let mut blob = Vec::with_capacity(12 + ct.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    std::fs::write(store_path(wallet), blob).map_err(|e| CoreError::Crypto(format!("write escrow store: {e}")))
}

pub fn list_opened(wallet: &WalletKeys) -> Result<Vec<OpenedEscrow>, CoreError> {
    Ok(load_store(wallet)?.opened)
}

pub fn list_contributions(wallet: &WalletKeys) -> Result<Vec<ContributionRecord>, CoreError> {
    Ok(load_store(wallet)?.contributions)
}

// ----------------------------- payee blob (amount, r) -----------------------------

/// Serialize the `(amount, r)` a contributor sends to the payee: 8-byte amount BE + 32-byte r.
fn serialize_payee_blob(amount: u64, r: &Fr) -> Vec<u8> {
    let mut v = Vec::with_capacity(40);
    v.extend_from_slice(&amount.to_be_bytes());
    v.extend_from_slice(&r.0);
    v
}

/// The on-chain `payee_enc` blob: `ephemeral_pub(32) || enc_note`. The contract emits this opaquely
/// (spec §5 `payee_enc: Bytes`), so the ephemeral key the payee needs to decrypt rides along inside
/// the blob rather than as a separate event field.
fn pack_payee_blob(ephemeral_pub: &[u8; 32], enc_note: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(32 + enc_note.len());
    v.extend_from_slice(ephemeral_pub);
    v.extend_from_slice(enc_note);
    v
}

/// Decrypt an on-chain `payee_enc` blob (`ephemeral_pub || enc_note`) with the payee's transmission
/// key, returning `(amount, r)`. The payee accumulates these across contributions to learn the
/// total `(S, R_sum)` to open at release (see [`scan_total`]).
pub fn decrypt_payee_blob(id: &WalletIdentity, blob: &[u8]) -> Option<(u64, Fr)> {
    if blob.len() < 32 {
        return None;
    }
    let mut ephemeral_pub = [0u8; 32];
    ephemeral_pub.copy_from_slice(&blob[0..32]);
    let pt = encrypt::decrypt_note(&blob[32..], &ephemeral_pub, &id.transmission_sk).ok()?;
    if pt.len() != 40 {
        return None;
    }
    let amount = u64::from_be_bytes(pt[0..8].try_into().ok()?);
    let mut r = [0u8; 32];
    r.copy_from_slice(&pt[8..40]);
    Some((amount, Fr(r)))
}

/// Scan an escrow's contribution blobs as the payee and accumulate the running total it must open
/// at release: `S = Σ amountᵢ`, `R = Σ rᵢ` (field sum). Blobs that don't decrypt to this wallet are
/// skipped (a non-payee scanning gets `(0, 0)`). The homomorphism `Σ Commit(vᵢ, rᵢ) = Commit(S, R)`
/// is exactly what the payout circuit opens against the stored `c_raised`.
pub fn scan_total(wallet: &WalletKeys, cfg: &PoolConfig, escrow_id: u64) -> Result<(u64, Fr), CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let blobs = chain::escrow_contributions(cfg, escrow_id)?;
    let mut total: u64 = 0;
    let mut rs: Vec<Fr> = Vec::new();
    for blob in &blobs {
        if let Some((amount, r)) = decrypt_payee_blob(&id, blob) {
            total = total
                .checked_add(amount)
                .ok_or_else(|| CoreError::Proving("escrow total overflow".into()))?;
            rs.push(r);
        }
    }
    Ok((total, pedersen::sum_blindings(&rs)))
}

// ----------------------------- flows -----------------------------

/// Open a hidden-sum escrow as the payee. `target`/`deadline` (ledger seq) public; `mode` is
/// all-or-nothing or keep-what-you-raise. Returns the assigned escrow id.
pub fn open(
    wallet: &WalletKeys,
    cfg: &PoolConfig,
    target: u64,
    deadline: u64,
    mode: u32,
) -> Result<u64, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let h = Hasher::new();
    let payee_salt = Fr::random();
    let payee_bind = h.escrow_bind(DOMAIN_ESCROW_PAYEE, &id.owner_pk, &payee_salt);

    // The id the open will assign (the wallet submits one open at a time).
    let escrow_id = chain::escrow_next_id(cfg)?;
    chain::submit_open_escrow(cfg, cfg.submit_source(wallet.stellar_secret()), target, deadline, mode, &payee_bind)?;

    let mut s = load_store(wallet)?;
    s.opened.push(OpenedEscrow { escrow_id, payee_salt: payee_salt.to_hex() });
    save_store(wallet, &s)?;
    Ok(escrow_id)
}

/// Contribute `amount` to `escrow_id`, hidden. `payee_code` is the payee's shielded payment code
/// (so the contributor can encrypt `(amount, r)` to them). Spends one owned note; change returns
/// to self. Returns the assigned contribution index.
pub fn contribute(
    wallet: &WalletKeys,
    cfg_base: &PoolConfig,
    escrow_id: u64,
    payee_code: &str,
    amount: u64,
) -> Result<u32, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let h = Hasher::new();
    let (_payee_pk, payee_transmission_pub) = super::send::parse_payment_code(payee_code)?;

    let st = chain::read_escrow(cfg_base, escrow_id)?;
    let cfg = cfg_base.with_asset_tag(st.asset_tag)?;
    let epoch = chain::current_epoch(&cfg.rpc_url)?;
    let pool = chain::pool_state(&cfg)?;
    let commitment_leaves = chain::commitment_leaves_from(&pool.commits)?;
    let asp_leaves = chain::approved_set(&cfg)?;
    let local = notes::load(wallet)?;

    let note = scan::owned_notes(&id, &pool, &local, 0)?
        .into_iter()
        .find(|n| n.value >= amount && n.asset_tag == cfg.asset_tag)
        .ok_or_else(|| CoreError::Proving(format!("no single owned note covers {amount}")))?;
    if !asp_leaves.contains(&id.owner_pk) {
        return Err(CoreError::Proving("wallet not enrolled in this pool's ASP set".into()));
    }

    // The escrow's running commitment point (identity until the first contribution).
    let p_old = pedersen::point_from_coords(&st.raised_x, &st.raised_y, st.n_contrib == 0);
    let blinding_r = Fr::random();
    let contrib_salt = Fr::random();
    let change_blinding = Fr::random();
    let change_rho = Fr::random();

    let witness = ContributeWitness::build(
        &h,
        ContributeInputs {
            owner_sk: id.owner_sk,
            asset_tag: cfg.asset_tag,
            epoch: Fr::from_u64(epoch as u64),
            note_epoch: Fr::from_u64(note.epoch as u64),
            domain_sep: h.domain_sep(&cfg.pool_id, &cfg.network_id, SELECTOR_ESCROW_CONTRIBUTE),
            note_value: note.value,
            note_blinding: note.blinding,
            note_rho: note.rho,
            note_leaf_index: note.leaf_index as usize,
            commitment_leaves: &commitment_leaves,
            asp_leaves: &asp_leaves,
            prior_nullifiers: &pool.nullifiers,
            dummy_rho: Fr::random(),
            amount,
            blinding_r,
            contrib_salt,
            change_blinding,
            change_rho,
            p_old,
        },
    );
    let bundle = proving::prove_escrow_contribute_witness(&witness)?;

    // The new running point (for the contract to cache so the next contributor can fold).
    let c = pedersen::commit(&Fr::from_u64(amount), &blinding_r);
    let p_new = pedersen::add(&pedersen::point_from_coords(&st.raised_x, &st.raised_y, st.n_contrib == 0), &c);
    let (raised_x, raised_y) = pedersen::coords(&p_new);

    // Encrypt the change note to self (the pool publishes no ciphertext for escrow change).
    let change = NotePlaintext { value: note.value - amount, asset_tag: cfg.asset_tag, blinding: change_blinding, epoch, rho: change_rho };
    let change_enc = encrypt::encrypt_note(&change.serialize(), &id.transmission_pub)?;
    let change_payload = chain::OutputPayload { enc_note: change_enc.enc_note, ephemeral_pub: change_enc.ephemeral_pub, view_tag: change_enc.view_tag };

    // Encrypt (amount, r) to the payee so only they can open the running total. The ephemeral key
    // rides inside the published blob (ephemeral_pub || enc_note) so the payee can decrypt it.
    let payee_enc = encrypt::encrypt_note(&serialize_payee_blob(amount, &blinding_r), &payee_transmission_pub)?;
    let payee_blob = pack_payee_blob(&payee_enc.ephemeral_pub, &payee_enc.enc_note);

    chain::submit_escrow_contribute(
        &cfg,
        cfg.submit_source(wallet.stellar_secret()),
        escrow_id,
        &bundle.public_inputs,
        &bundle.proof,
        &change_payload,
        &payee_blob,
        &raised_x,
        &raised_y,
    )?;

    // Persist the change opening (so a later scan can spend it) + the refund record.
    if change.value > 0 {
        notes::add(wallet, change.clone())?;
    }
    let mut s = load_store(wallet)?;
    let contrib_index = st.n_contrib;
    s.contributions.push(ContributionRecord {
        escrow_id,
        contrib_index,
        asset: cfg_asset_code(&cfg),
        amount,
        blinding_r: blinding_r.to_hex(),
        contrib_salt: contrib_salt.to_hex(),
    });
    save_store(wallet, &s)?;
    Ok(contrib_index)
}

/// Release the escrow to the payee (this wallet). `total_value` (S) and `total_r` (R_sum) are the
/// payee's accumulated opening of the running commitment, decrypted from the contributors' blobs
/// (see [`decrypt_payee_blob`]). Mints one shielded note of `total_value` to the payee.
pub fn release(
    wallet: &WalletKeys,
    cfg_base: &PoolConfig,
    escrow_id: u64,
    total_value: u64,
    total_r: Fr,
) -> Result<String, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let h = Hasher::new();
    let st = chain::read_escrow(cfg_base, escrow_id)?;
    let cfg = cfg_base.with_asset_tag(st.asset_tag)?;
    let epoch = chain::current_epoch(&cfg.rpc_url)?;

    let opened = list_opened(wallet)?
        .into_iter()
        .find(|o| o.escrow_id == escrow_id)
        .ok_or_else(|| CoreError::Proving("no stored payee_salt for this escrow".into()))?;
    let payee_salt = Fr::from_hex(&opened.payee_salt).ok_or_else(|| CoreError::Crypto("payee_salt hex".into()))?;
    let floor = if st.mode == MODE_KEEP_WHAT_YOU_RAISE { 0 } else { st.target };

    let out_blinding = Fr::random();
    let out_rho = Fr::random();
    let witness = PayoutWitness::build(
        &h,
        PayoutInputs {
            domain_sep: h.domain_sep(&cfg.pool_id, &cfg.network_id, SELECTOR_ESCROW_PAYOUT),
            asset_tag: cfg.asset_tag,
            epoch: Fr::from_u64(epoch as u64),
            floor,
            domain_bind: DOMAIN_ESCROW_PAYEE,
            recipient_sk: id.owner_sk,
            value: total_value,
            blinding_r: total_r,
            out_blinding,
            out_rho,
            salt: payee_salt,
        },
    );
    let bundle = proving::prove_escrow_payout_witness(&witness)?;

    let out = NotePlaintext { value: total_value, asset_tag: cfg.asset_tag, blinding: out_blinding, epoch, rho: out_rho };
    let enc = encrypt::encrypt_note(&out.serialize(), &id.transmission_pub)?;
    let payload = chain::OutputPayload { enc_note: enc.enc_note, ephemeral_pub: enc.ephemeral_pub, view_tag: enc.view_tag };
    let hash = chain::submit_escrow_release(&cfg, cfg.submit_source(wallet.stellar_secret()), escrow_id, &bundle.public_inputs, &bundle.proof, &payload)?;
    notes::add(wallet, out)?;
    Ok(hash)
}

/// Refund this wallet's contribution `contrib_index` to `escrow_id` (all-or-nothing fail path).
/// Uses the stored contribution record; mints the contribution amount back to this wallet.
pub fn refund(wallet: &WalletKeys, cfg_base: &PoolConfig, escrow_id: u64, contrib_index: u32) -> Result<String, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let h = Hasher::new();
    let rec = list_contributions(wallet)?
        .into_iter()
        .find(|c| c.escrow_id == escrow_id && c.contrib_index == contrib_index)
        .ok_or_else(|| CoreError::Proving("no stored contribution record".into()))?;
    let cfg = cfg_base.with_asset(&rec.asset)?;
    let epoch = chain::current_epoch(&cfg.rpc_url)?;
    let blinding_r = Fr::from_hex(&rec.blinding_r).ok_or_else(|| CoreError::Crypto("blinding_r hex".into()))?;
    let contrib_salt = Fr::from_hex(&rec.contrib_salt).ok_or_else(|| CoreError::Crypto("contrib_salt hex".into()))?;

    let out_blinding = Fr::random();
    let out_rho = Fr::random();
    let witness = PayoutWitness::build(
        &h,
        PayoutInputs {
            domain_sep: h.domain_sep(&cfg.pool_id, &cfg.network_id, SELECTOR_ESCROW_PAYOUT),
            asset_tag: cfg.asset_tag,
            epoch: Fr::from_u64(epoch as u64),
            floor: 0,
            domain_bind: super::poseidon::DOMAIN_ESCROW_REFUND,
            recipient_sk: id.owner_sk,
            value: rec.amount,
            blinding_r,
            out_blinding,
            out_rho,
            salt: contrib_salt,
        },
    );
    let bundle = proving::prove_escrow_payout_witness(&witness)?;

    let out = NotePlaintext { value: rec.amount, asset_tag: cfg.asset_tag, blinding: out_blinding, epoch, rho: out_rho };
    let enc = encrypt::encrypt_note(&out.serialize(), &id.transmission_pub)?;
    let payload = chain::OutputPayload { enc_note: enc.enc_note, ephemeral_pub: enc.ephemeral_pub, view_tag: enc.view_tag };
    let hash = chain::submit_escrow_refund(&cfg, cfg.submit_source(wallet.stellar_secret()), escrow_id, contrib_index, &bundle.public_inputs, &bundle.proof, &payload)?;
    notes::add(wallet, out)?;
    Ok(hash)
}

/// The v1 asset code for a config (reverse of `with_asset`), for the contribution record.
fn cfg_asset_code(cfg: &PoolConfig) -> String {
    super::config::ASSETS
        .iter()
        .find(|a| Fr::from_u64(a.tag) == cfg.asset_tag)
        .map(|a| a.code.to_string())
        .unwrap_or_else(|| "XLM".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payee_blob_roundtrips() {
        // (amount, r) survives serialize -> encrypt -> decrypt to the payee's transmission key.
        let ivk = [9u8; 32];
        let id = WalletIdentity {
            owner_sk: Fr::from_u64(7),
            owner_pk: Fr::from_u64(0),
            transmission_sk: encrypt::transmission_secret(&ivk),
            transmission_pub: encrypt::transmission_public(&ivk),
        };
        let amount = 1234u64;
        let r = Fr::from_hex("0xb11d").unwrap();
        let enc = encrypt::encrypt_note(&serialize_payee_blob(amount, &r), &id.transmission_pub).unwrap();
        let blob = pack_payee_blob(&enc.ephemeral_pub, &enc.enc_note);
        let (got_amount, got_r) = decrypt_payee_blob(&id, &blob).unwrap();
        assert_eq!(got_amount, amount);
        assert_eq!(got_r, r);
    }

    #[test]
    fn store_roundtrips_encrypted() {
        let _g = super::super::notes::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("ozky-escrow-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("OZKY_NOTES_DIR", &dir);
        let wallet = super::super::keys::derive_from_mnemonic(
            "illness spike retreat truth genius clock brain pass fit cave bargain toe",
        )
        .unwrap();

        assert!(list_opened(&wallet).unwrap().is_empty());
        let mut s = load_store(&wallet).unwrap();
        s.opened.push(OpenedEscrow { escrow_id: 3, payee_salt: "0xabcd".into() });
        s.contributions.push(ContributionRecord {
            escrow_id: 3,
            contrib_index: 0,
            asset: "USDC".into(),
            amount: 500,
            blinding_r: "0xb11d".into(),
            contrib_salt: "0x5a17".into(),
        });
        save_store(&wallet, &s).unwrap();

        assert_eq!(list_opened(&wallet).unwrap()[0].escrow_id, 3);
        let c = &list_contributions(&wallet).unwrap()[0];
        assert_eq!(c.amount, 500);
        assert_eq!(c.contrib_salt, "0x5a17");
        // Encrypted at rest (the salt is not cleartext).
        let raw = std::fs::read(store_path(&wallet)).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"0x5a17"), "salt must not be cleartext");
        std::env::remove_var("OZKY_NOTES_DIR");
    }
}
