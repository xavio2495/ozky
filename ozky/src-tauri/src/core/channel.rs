//! Merchant-pull subscription channel client flows (building block B, phase 2 / CH4): open / close /
//! reclaim, tying the channel close circuit ([`super::witness::ChannelCloseWitness`]), the reused
//! escrow contribute/payout circuits, the Schnorr signer ([`super::pedersen`]) and the pool's channel
//! entrypoints ([`super::chain`]) into the one-way shielded payment channel.
//!
//! Flow (claude-docs/channel_interface.md): the SUBSCRIBER opens a channel by spending one owned note
//! of a hidden `cap` (reusing the escrow_contribute proof, whose `c_contrib` IS the cap commitment),
//! derives a per-channel signing key, and pre-signs a RAMP of cumulative authorizations off-chain.
//! The MERCHANT draws while the subscriber is offline and CLOSES once with the highest elapsed
//! authorization: the close proof opens the cap + the signed cumulative commitment, verifies the
//! Schnorr signature in-circuit, and mints drawn -> merchant, (cap - drawn) -> subscriber. If the
//! merchant never closes, after `expiry` the subscriber RECLAIMS the full cap (reused escrow_payout).
//!
//! Persistence: the opener persists the full [`ChannelRecord`] (cap/r_cap, sk_chan, salts, the ramp,
//! both parties' pubkeys) encrypted at rest with the wallet key. A real cross-wallet merchant imports
//! the same record from the on-chain `chanopen` event's sealed `merchant_enc` blob.

use super::config::PoolConfig;
use super::encrypt::{self, NotePlaintext};
use super::keys::WalletKeys;
use super::notes::{self, data_dir};
use super::pedersen::{self, Signature};
use super::poseidon::{
    Fr, Hasher, DOMAIN_CHANNEL_MERCHANT, DOMAIN_ESCROW_REFUND, SELECTOR_CHANNEL_CLOSE,
    SELECTOR_ESCROW_CONTRIBUTE, SELECTOR_ESCROW_PAYOUT,
};
use super::scan::{self, WalletIdentity};
use super::witness::{
    ChannelCloseInputs, ChannelCloseWitness, ContributeInputs, ContributeWitness, PayoutInputs,
    PayoutWitness,
};
use super::{chain, proving, send, CoreError};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use rand_core::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

// ----------------------------- persisted records -----------------------------

/// One pre-signed cumulative authorization in a channel's ramp.
#[derive(Clone, Serialize, Deserialize)]
pub struct RampEntry {
    pub period: u32,
    /// Cumulative amount the merchant may have drawn by this period (== `period * amount_per_period`).
    pub cum_amount: u64,
    /// Hex of the Pedersen blinding `r_k` for this period's commitment `C_k = Commit(cum_amount, r_k)`.
    pub r_k: String,
    /// Ledger sequence after which this authorization is valid (the merchant may close at it).
    pub valid_after_ledger: u64,
    pub sig_r_x: String,
    pub sig_r_y: String,
    pub s_lo: String,
    pub s_hi: String,
}

/// Everything a closer (subscriber in self-test, or a real merchant after import) needs to close or
/// reclaim a channel. Encrypted at rest with the wallet key.
#[derive(Clone, Serialize, Deserialize)]
pub struct ChannelRecord {
    pub channel_id: u64,
    pub asset: String,
    pub cap: u64,
    pub r_cap: String,
    /// Hex of the per-channel Schnorr signing key (subscriber side; never spends, only authorizes).
    pub sk_chan: String,
    /// Subscriber binding salt (== the open proof's refund salt -> subscriber_bind).
    pub s_salt: String,
    /// Merchant binding salt.
    pub m_salt: String,
    pub merchant_owner_pk: String,
    pub merchant_transmission_pub: String,
    pub subscriber_owner_pk: String,
    pub subscriber_transmission_pub: String,
    pub expiry: u64,
    pub amount_per_period: u64,
    pub ramp: Vec<RampEntry>,
    /// Local flag set once this wallet closed/reclaimed it (on-chain status is canonical).
    pub closed: bool,
}

/// The sealed `merchant_enc` blob (subscriber -> merchant): the channel secrets + ramp, so a real
/// merchant can close while the subscriber is offline. JSON, encrypted to the merchant.
#[derive(Clone, Serialize, Deserialize)]
struct MerchantBlob {
    cap: u64,
    r_cap: String,
    s_salt: String,
    m_salt: String,
    subscriber_owner_pk: String,
    subscriber_transmission_pub: String,
    sk_chan: String,
    expiry: u64,
    amount_per_period: u64,
    ramp: Vec<RampEntry>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Store {
    channels: Vec<ChannelRecord>,
}

fn store_path(wallet: &WalletKeys) -> PathBuf {
    let digest = Sha256::digest(wallet.stellar_address().as_bytes());
    data_dir().join(format!("channel-{}.enc", hex::encode(&digest[..8])))
}

fn cipher(wallet: &WalletKeys) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(Key::from_slice(&wallet.notes_key()))
}

fn load_store(wallet: &WalletKeys) -> Result<Store, CoreError> {
    let path = store_path(wallet);
    let blob = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Store::default()),
        Err(e) => return Err(CoreError::Crypto(format!("read channel store: {e}"))),
    };
    if blob.len() < 12 {
        return Err(CoreError::Crypto("channel store too short".into()));
    }
    let (nonce, ct) = blob.split_at(12);
    let plain = cipher(wallet)
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| CoreError::Crypto("channel store decrypt failed".into()))?;
    serde_json::from_slice(&plain).map_err(|e| CoreError::Crypto(format!("channel decode: {e}")))
}

fn save_store(wallet: &WalletKeys, s: &Store) -> Result<(), CoreError> {
    let plain = serde_json::to_vec(s).map_err(|e| CoreError::Crypto(format!("channel encode: {e}")))?;
    let mut nonce = [0u8; 12];
    rand_core::OsRng.fill_bytes(&mut nonce);
    let ct = cipher(wallet)
        .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
        .map_err(|_| CoreError::Crypto("channel store encrypt failed".into()))?;
    std::fs::create_dir_all(data_dir()).map_err(|e| CoreError::Crypto(format!("mkdir channel dir: {e}")))?;
    let mut blob = Vec::with_capacity(12 + ct.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    std::fs::write(store_path(wallet), blob).map_err(|e| CoreError::Crypto(format!("write channel store: {e}")))
}

pub fn list_records(wallet: &WalletKeys) -> Result<Vec<ChannelRecord>, CoreError> {
    Ok(load_store(wallet)?.channels)
}

fn upsert_record(wallet: &WalletKeys, rec: ChannelRecord) -> Result<(), CoreError> {
    let mut s = load_store(wallet)?;
    match s.channels.iter_mut().find(|c| c.channel_id == rec.channel_id) {
        Some(slot) => *slot = rec,
        None => s.channels.push(rec),
    }
    save_store(wallet, &s)
}

fn get_record(wallet: &WalletKeys, channel_id: u64) -> Result<ChannelRecord, CoreError> {
    list_records(wallet)?
        .into_iter()
        .find(|c| c.channel_id == channel_id)
        .ok_or_else(|| CoreError::Proving("no stored channel record (open or import it first)".into()))
}

fn fr_hex(s: &str) -> Result<Fr, CoreError> {
    Fr::from_hex(s).ok_or_else(|| CoreError::Crypto(format!("bad hex field: {s}")))
}

fn pub32(s: &str) -> Result<[u8; 32], CoreError> {
    let b = hex::decode(s.strip_prefix("0x").unwrap_or(s)).map_err(|_| CoreError::Crypto("bad 32-byte hex".into()))?;
    b.try_into().map_err(|_| CoreError::Crypto("expected 32 bytes".into()))
}

/// A non-zero random field (a zero Schnorr nonce would put R at the identity, which the prover rejects).
fn nonzero_random() -> Fr {
    loop {
        let f = Fr::random();
        if !f.is_zero() {
            return f;
        }
    }
}

// ----------------------------- merchant blob (cross-wallet transport) -----------------------------

/// Pack `ephemeral_pub(32) || enc_note` (the same self-describing layout the escrow payee blob uses),
/// so the ephemeral key the merchant needs to decrypt rides inside the on-chain `merchant_enc` blob.
fn pack_blob(ephemeral_pub: &[u8; 32], enc_note: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(32 + enc_note.len());
    v.extend_from_slice(ephemeral_pub);
    v.extend_from_slice(enc_note);
    v
}

fn seal_merchant_blob(blob: &MerchantBlob, merchant_transmission_pub: &[u8; 32]) -> Result<Vec<u8>, CoreError> {
    let plain = serde_json::to_vec(blob).map_err(|e| CoreError::Crypto(format!("merchant blob encode: {e}")))?;
    let enc = encrypt::encrypt_note(&plain, merchant_transmission_pub)?;
    Ok(pack_blob(&enc.ephemeral_pub, &enc.enc_note))
}

/// Decrypt a `chanopen` `merchant_enc` blob with the merchant's transmission key.
fn open_merchant_blob(id: &WalletIdentity, blob: &[u8]) -> Option<MerchantBlob> {
    if blob.len() < 32 {
        return None;
    }
    let mut eph = [0u8; 32];
    eph.copy_from_slice(&blob[0..32]);
    let pt = encrypt::decrypt_note(&blob[32..], &eph, &id.transmission_sk).ok()?;
    serde_json::from_slice(&pt).ok()
}

// ----------------------------- flows -----------------------------

/// Open a subscription channel as the subscriber. Spends one owned note of `cap` (hidden), derives a
/// per-channel signing key, pre-signs the ramp `[(period, cum, valid_after)]` for `n_periods`
/// (`amount_per_period` each), seals it to the merchant, and persists the record. Returns the id.
#[allow(clippy::too_many_arguments)]
pub fn open(
    wallet: &WalletKeys,
    cfg_base: &PoolConfig,
    asset: &str,
    cap: u64,
    merchant_code: &str,
    amount_per_period: u64,
    n_periods: u32,
    ledgers_per_period: u64,
) -> Result<u64, CoreError> {
    if amount_per_period == 0 || n_periods == 0 {
        return Err(CoreError::Proving("amount_per_period and n_periods must be non-zero".into()));
    }
    let max_draw = amount_per_period
        .checked_mul(n_periods as u64)
        .ok_or_else(|| CoreError::Proving("ramp overflows u64".into()))?;
    if max_draw > cap {
        return Err(CoreError::Proving(format!("ramp total {max_draw} exceeds cap {cap}")));
    }

    let id = scan::wallet_identity(wallet)?;
    let h = Hasher::new();
    let cfg = cfg_base.with_asset(asset)?;
    let epoch = chain::current_epoch(&cfg.rpc_url)?;
    let open_ledger = chain::latest_ledger(&cfg.rpc_url)? as u64;
    let (merchant_owner_pk, merchant_transmission_pub) = send::parse_payment_code(merchant_code)?;

    // --- spend the cap (escrow_contribute-shaped: amount = cap, p_old = the G1 seed) ---
    let pool = chain::pool_state(&cfg)?;
    let commitment_leaves = chain::commitment_leaves_from(&pool.commits)?;
    let asp_leaves = chain::approved_set(&cfg)?;
    let local = notes::load(wallet)?;
    let note = scan::owned_notes(&id, &pool, &local, 0)?
        .into_iter()
        .find(|n| n.value >= cap && n.asset_tag == cfg.asset_tag)
        .ok_or_else(|| CoreError::Proving(format!("no single owned note covers cap {cap}")))?;
    if !asp_leaves.contains(&id.owner_pk) {
        return Err(CoreError::Proving("wallet not enrolled in this pool's ASP set".into()));
    }

    let r_cap = Fr::random(); // the cap commitment blinding (== contribute fold blinding)
    let s_salt = Fr::random(); // subscriber binding salt (-> refund_bind -> subscriber_bind)
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
            amount: cap,
            blinding_r: r_cap,
            contrib_salt: s_salt,
            change_blinding,
            change_rho,
            p_old: pedersen::seed_point(),
        },
    );
    let bundle = proving::prove_escrow_contribute_witness(&witness)?;

    // --- channel params + pre-signed ramp ---
    let sk_chan = nonzero_random();
    let pk_chan = pedersen::schnorr_pubkey(&sk_chan);
    let auth_key = pedersen::point_hash(&h, &pk_chan);
    let m_salt = Fr::random();
    let merchant_bind = h.escrow_bind(DOMAIN_CHANNEL_MERCHANT, &merchant_owner_pk, &m_salt);
    let expiry = open_ledger + (n_periods as u64 + 1) * ledgers_per_period; // term end + 1 period buffer

    let channel_id = chain::channel_next_id(&cfg)?;
    let mut ramp = Vec::with_capacity(n_periods as usize);
    for period in 1..=n_periods {
        let cum = amount_per_period * period as u64;
        let r_k = Fr::random();
        let valid_after = open_ledger + period as u64 * ledgers_per_period;
        let c_k = pedersen::commit(&Fr::from_u64(cum), &r_k);
        let (ckx, cky) = pedersen::coords(&c_k);
        let msg = h.hash(&[Fr::from_u64(channel_id), Fr::from_u64(valid_after), ckx, cky]);
        let sig = pedersen::schnorr_sign(&h, &sk_chan, &nonzero_random(), &msg);
        let (rx, ry) = pedersen::coords(&sig.r);
        ramp.push(RampEntry {
            period,
            cum_amount: cum,
            r_k: r_k.to_hex(),
            valid_after_ledger: valid_after,
            sig_r_x: rx.to_hex(),
            sig_r_y: ry.to_hex(),
            s_lo: sig.s_lo.to_hex(),
            s_hi: sig.s_hi.to_hex(),
        });
    }

    // --- change note (to self) + sealed merchant blob ---
    let change = NotePlaintext { value: note.value - cap, asset_tag: cfg.asset_tag, blinding: change_blinding, epoch, rho: change_rho };
    let change_enc = encrypt::encrypt_note(&change.serialize(), &id.transmission_pub)?;
    let change_payload = chain::OutputPayload { enc_note: change_enc.enc_note, ephemeral_pub: change_enc.ephemeral_pub, view_tag: change_enc.view_tag };

    let merchant_blob = MerchantBlob {
        cap,
        r_cap: r_cap.to_hex(),
        s_salt: s_salt.to_hex(),
        m_salt: m_salt.to_hex(),
        subscriber_owner_pk: id.owner_pk.to_hex(),
        subscriber_transmission_pub: hex::encode(id.transmission_pub),
        sk_chan: sk_chan.to_hex(),
        expiry,
        amount_per_period,
        ramp: ramp.clone(),
    };
    let sealed = seal_merchant_blob(&merchant_blob, &merchant_transmission_pub)?;

    chain::submit_open_channel(
        &cfg,
        cfg.submit_source(wallet.stellar_secret()),
        &bundle.public_inputs,
        &bundle.proof,
        &change_payload,
        &merchant_bind,
        &auth_key,
        expiry,
        &sealed,
    )?;

    if change.value > 0 {
        notes::add(wallet, change)?;
    }
    upsert_record(
        wallet,
        ChannelRecord {
            channel_id,
            asset: asset.to_string(),
            cap,
            r_cap: r_cap.to_hex(),
            sk_chan: sk_chan.to_hex(),
            s_salt: s_salt.to_hex(),
            m_salt: m_salt.to_hex(),
            merchant_owner_pk: merchant_owner_pk.to_hex(),
            merchant_transmission_pub: hex::encode(merchant_transmission_pub),
            subscriber_owner_pk: id.owner_pk.to_hex(),
            subscriber_transmission_pub: hex::encode(id.transmission_pub),
            expiry,
            amount_per_period,
            ramp,
            closed: false,
        },
    )?;
    Ok(channel_id)
}

/// Close a channel (merchant) at the highest elapsed authorization. Mints drawn -> merchant and
/// remainder -> subscriber. Returns the tx hash.
pub fn close(wallet: &WalletKeys, cfg_base: &PoolConfig, channel_id: u64) -> Result<String, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let h = Hasher::new();
    let mut rec = get_record(wallet, channel_id)?;
    let st = chain::read_channel(cfg_base, channel_id)?;
    let cfg = cfg_base.with_asset_tag(st.asset_tag)?;
    let epoch = chain::current_epoch(&cfg.rpc_url)?;
    let now_ledger = chain::latest_ledger(&cfg.rpc_url)? as u64;

    // Highest elapsed authorization (the merchant always draws the most they're entitled to).
    let entry = rec
        .ramp
        .iter()
        .filter(|e| e.valid_after_ledger <= now_ledger)
        .max_by_key(|e| e.cum_amount)
        .cloned()
        .ok_or_else(|| CoreError::Proving("no elapsed authorization to close at yet".into()))?;

    let drawn = entry.cum_amount;
    let cap = rec.cap;
    let sig = Signature {
        r: pedersen::point_from_coords(&fr_hex(&entry.sig_r_x)?, &fr_hex(&entry.sig_r_y)?, false),
        s_lo: fr_hex(&entry.s_lo)?,
        s_hi: fr_hex(&entry.s_hi)?,
    };
    let pk_chan = pedersen::schnorr_pubkey(&fr_hex(&rec.sk_chan)?);
    let merchant_owner_pk = fr_hex(&rec.merchant_owner_pk)?;
    let subscriber_owner_pk = fr_hex(&rec.subscriber_owner_pk)?;

    let merchant_blinding = Fr::random();
    let merchant_rho = Fr::random();
    let subscriber_blinding = Fr::random();
    let subscriber_rho = Fr::random();

    let witness = ChannelCloseWitness::build(
        &h,
        ChannelCloseInputs {
            domain_sep: h.domain_sep(&cfg.pool_id, &cfg.network_id, SELECTOR_CHANNEL_CLOSE),
            asset_tag: cfg.asset_tag,
            epoch: Fr::from_u64(epoch as u64),
            valid_after_ledger: entry.valid_after_ledger,
            channel_id,
            cap,
            r_cap: fr_hex(&rec.r_cap)?,
            drawn,
            r_k: fr_hex(&entry.r_k)?,
            pk: pk_chan,
            sig,
            merchant_pk: merchant_owner_pk,
            m_salt: fr_hex(&rec.m_salt)?,
            merchant_blinding,
            merchant_rho,
            subscriber_pk: subscriber_owner_pk,
            s_salt: fr_hex(&rec.s_salt)?,
            subscriber_blinding,
            subscriber_rho,
        },
    );
    let bundle = proving::prove_channel_close_witness(&witness)?;

    // Mint payloads (encrypted to each party's transmission key).
    let merchant_t = pub32(&rec.merchant_transmission_pub)?;
    let subscriber_t = pub32(&rec.subscriber_transmission_pub)?;
    let merchant_note = NotePlaintext { value: drawn, asset_tag: cfg.asset_tag, blinding: merchant_blinding, epoch, rho: merchant_rho };
    let subscriber_note = NotePlaintext { value: cap - drawn, asset_tag: cfg.asset_tag, blinding: subscriber_blinding, epoch, rho: subscriber_rho };
    let m_enc = encrypt::encrypt_note(&merchant_note.serialize(), &merchant_t)?;
    let s_enc = encrypt::encrypt_note(&subscriber_note.serialize(), &subscriber_t)?;
    let merchant_payload = chain::OutputPayload { enc_note: m_enc.enc_note, ephemeral_pub: m_enc.ephemeral_pub, view_tag: m_enc.view_tag };
    let subscriber_payload = chain::OutputPayload { enc_note: s_enc.enc_note, ephemeral_pub: s_enc.ephemeral_pub, view_tag: s_enc.view_tag };

    let hash = chain::submit_close_channel(
        &cfg,
        cfg.submit_source(wallet.stellar_secret()),
        channel_id,
        &bundle.public_inputs,
        &bundle.proof,
        &merchant_payload,
        &subscriber_payload,
    )?;

    // Discover whichever minted notes this wallet owns (both, in a self-test).
    if merchant_owner_pk == id.owner_pk && drawn > 0 {
        notes::add(wallet, merchant_note)?;
    }
    if subscriber_owner_pk == id.owner_pk && cap - drawn > 0 {
        notes::add(wallet, subscriber_note)?;
    }
    rec.closed = true;
    upsert_record(wallet, rec)?;
    Ok(hash)
}

/// Reclaim the full cap (subscriber, expiry path). Opens the cap commitment with floor 0 and mints
/// the cap back to this wallet (must be the subscriber). Returns the tx hash.
pub fn reclaim(wallet: &WalletKeys, cfg_base: &PoolConfig, channel_id: u64) -> Result<String, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let h = Hasher::new();
    let mut rec = get_record(wallet, channel_id)?;
    let st = chain::read_channel(cfg_base, channel_id)?;
    let cfg = cfg_base.with_asset_tag(st.asset_tag)?;
    let epoch = chain::current_epoch(&cfg.rpc_url)?;

    let out_blinding = Fr::random();
    let out_rho = Fr::random();
    let witness = PayoutWitness::build(
        &h,
        PayoutInputs {
            domain_sep: h.domain_sep(&cfg.pool_id, &cfg.network_id, SELECTOR_ESCROW_PAYOUT),
            asset_tag: cfg.asset_tag,
            epoch: Fr::from_u64(epoch as u64),
            floor: 0,
            domain_bind: DOMAIN_ESCROW_REFUND,
            recipient_sk: id.owner_sk,
            value: rec.cap,
            blinding_r: fr_hex(&rec.r_cap)?,
            out_blinding,
            out_rho,
            salt: fr_hex(&rec.s_salt)?,
        },
    );
    let bundle = proving::prove_escrow_payout_witness(&witness)?;

    let out = NotePlaintext { value: rec.cap, asset_tag: cfg.asset_tag, blinding: out_blinding, epoch, rho: out_rho };
    let enc = encrypt::encrypt_note(&out.serialize(), &id.transmission_pub)?;
    let payload = chain::OutputPayload { enc_note: enc.enc_note, ephemeral_pub: enc.ephemeral_pub, view_tag: enc.view_tag };
    let hash = chain::submit_channel_reclaim(&cfg, cfg.submit_source(wallet.stellar_secret()), channel_id, &bundle.public_inputs, &bundle.proof, &payload)?;
    notes::add(wallet, out)?;
    rec.closed = true;
    upsert_record(wallet, rec)?;
    Ok(hash)
}

/// Import a channel this wallet is the MERCHANT for: scan the `chanopen` blob, decrypt `merchant_enc`,
/// and persist a [`ChannelRecord`] so [`close`] can run. (The self-test opener already has the record;
/// this is the genuine cross-wallet path.)
pub fn import_from_chain(wallet: &WalletKeys, cfg_base: &PoolConfig, channel_id: u64) -> Result<(), CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let st = chain::read_channel(cfg_base, channel_id)?;
    let cfg = cfg_base.with_asset_tag(st.asset_tag)?;
    let blob = chain::channel_open_blob(&cfg, channel_id)?
        .ok_or_else(|| CoreError::Chain("no chanopen blob for this channel".into()))?;
    let mb = open_merchant_blob(&id, &blob)
        .ok_or_else(|| CoreError::Crypto("merchant_enc did not decrypt to this wallet".into()))?;
    let asset = super::config::ASSETS
        .iter()
        .find(|a| Fr::from_u64(a.tag) == st.asset_tag)
        .map(|a| a.code.to_string())
        .unwrap_or_else(|| "XLM".to_string());
    upsert_record(
        wallet,
        ChannelRecord {
            channel_id,
            asset,
            cap: mb.cap,
            r_cap: mb.r_cap,
            sk_chan: mb.sk_chan,
            s_salt: mb.s_salt,
            m_salt: mb.m_salt,
            merchant_owner_pk: id.owner_pk.to_hex(),
            merchant_transmission_pub: hex::encode(id.transmission_pub),
            subscriber_owner_pk: mb.subscriber_owner_pk,
            subscriber_transmission_pub: mb.subscriber_transmission_pub,
            expiry: mb.expiry,
            amount_per_period: mb.amount_per_period,
            ramp: mb.ramp,
            closed: false,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merchant_blob_roundtrips() {
        // The sealed merchant blob survives serialize -> encrypt -> decrypt to the merchant key.
        let ivk = [11u8; 32];
        let id = WalletIdentity {
            owner_sk: Fr::from_u64(7),
            owner_pk: Fr::from_u64(0),
            transmission_sk: encrypt::transmission_secret(&ivk),
            transmission_pub: encrypt::transmission_public(&ivk),
        };
        let mb = MerchantBlob {
            cap: 1000,
            r_cap: "0xca9".into(),
            s_salt: "0x5b17".into(),
            m_salt: "0x3e17".into(),
            subscriber_owner_pk: "0xabc".into(),
            subscriber_transmission_pub: hex::encode([3u8; 32]),
            sk_chan: "0x1234567".into(),
            expiry: 500,
            amount_per_period: 100,
            ramp: vec![RampEntry {
                period: 1,
                cum_amount: 100,
                r_k: "0xd4a".into(),
                valid_after_ledger: 50,
                sig_r_x: "0x1".into(),
                sig_r_y: "0x2".into(),
                s_lo: "0x3".into(),
                s_hi: "0x4".into(),
            }],
        };
        let sealed = seal_merchant_blob(&mb, &id.transmission_pub).unwrap();
        let got = open_merchant_blob(&id, &sealed).expect("decrypts to merchant");
        assert_eq!(got.cap, 1000);
        assert_eq!(got.ramp.len(), 1);
        assert_eq!(got.ramp[0].cum_amount, 100);
        assert_eq!(got.sk_chan, "0x1234567");
    }

    #[test]
    fn store_roundtrips_encrypted() {
        let _g = super::super::notes::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("ozky-channel-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("OZKY_NOTES_DIR", &dir);
        let wallet = super::super::keys::derive_from_mnemonic(
            "illness spike retreat truth genius clock brain pass fit cave bargain toe",
        )
        .unwrap();

        assert!(list_records(&wallet).unwrap().is_empty());
        upsert_record(
            &wallet,
            ChannelRecord {
                channel_id: 5,
                asset: "XLM".into(),
                cap: 1000,
                r_cap: "0xca9".into(),
                sk_chan: "0x1234567".into(),
                s_salt: "0x5b17".into(),
                m_salt: "0x3e17".into(),
                merchant_owner_pk: "0xaa".into(),
                merchant_transmission_pub: hex::encode([1u8; 32]),
                subscriber_owner_pk: "0xbb".into(),
                subscriber_transmission_pub: hex::encode([2u8; 32]),
                expiry: 500,
                amount_per_period: 100,
                ramp: vec![],
                closed: false,
            },
        )
        .unwrap();

        let got = list_records(&wallet).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].channel_id, 5);
        assert_eq!(got[0].cap, 1000);
        // Encrypted at rest (the signing key is not cleartext).
        let raw = std::fs::read(store_path(&wallet)).unwrap();
        assert!(!raw.windows(9).any(|w| w == b"0x1234567"), "sk_chan must not be cleartext");
        std::env::remove_var("OZKY_NOTES_DIR");
    }
}
