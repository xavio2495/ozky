//! Selective disclosure (Phase A3 / FEATURE_SET G5; spec D6). Lets a wallet hand an
//! auditor a SCOPED viewing capability: the auditor can re-derive exactly the owner's
//! notes for that scope (and verify each against its on-chain commitment), with no path
//! to spend and no path to other scopes.
//!
//! The disclosure is two parts:
//!  1. **Off-chain (the capability):** the scope's viewing secret + the owner's
//!     `owner_pk`, packaged for the auditor (out-of-band). With these the auditor runs
//!     the same view-tag match + decrypt + commitment re-derivation the wallet uses
//!     ([`scan::match_owned`]) — but cannot spend (no `owner_sk`).
//!  2. **On-chain (the trail):** `register_view_key` (publishes the scope's PUBLIC key
//!     halves) + `disclose` on the viewkeys contract — an auditable, revocable,
//!     timestamped grant (Z5).
//!
//! Scope granularity: disclosure is at `{account, asset}`. Notes are all encrypted to
//! the wallet's transmission key (derived at `{account 0, asset 1, epoch 0}`,
//! `scan::TRANSMISSION_EPOCH`), so the disclosed viewing secret decrypts every note for
//! that account/asset. Per-EPOCH cryptographic isolation would need per-epoch
//! transmission keys (a diversified-address change to the payment-code/send flow) — a
//! documented follow-up; the on-chain grant still records the epoch for the audit trail.

use super::config::PoolConfig;
use super::poseidon::{Fr, Hasher};
use super::scan::{self, OwnedNote, SCAN_ASSET_TAG};
use super::{chain, keys, CoreError};
use serde::Serialize;
use x25519_dalek::StaticSecret;

/// The portable disclosure package handed to an auditor (out-of-band). JSON, no spend
/// authority: the `viewing_secret` decrypts notes for the scope and `owner_pk` binds
/// re-derived notes to on-chain commitments, but neither can produce a nullifier.
#[derive(Serialize)]
pub struct DisclosurePackage {
    pub owner_stellar: String,
    /// The scope's viewing secret (X25519 static secret, hex) — decrypt capability.
    pub viewing_secret: String,
    /// The owner's `owner_pk` (hex) — to re-derive + match commitments.
    pub owner_pk: String,
    pub account: u32,
    pub asset_tag: String,
    pub epoch: u32,
    /// The pool the disclosed notes live in (so the auditor scans the right pool).
    pub pool_contract: String,
    pub note: &'static str,
}

/// A note revealed to an auditor by [`audit`] — value + identifying fields + the
/// on-chain commitment it was verified against.
#[derive(Serialize)]
pub struct DisclosedNote {
    pub leaf_index: u32,
    pub value: u64,
    pub asset_tag: String,
    pub epoch: u32,
    pub commitment: String,
}

/// Build the disclosure package for the keychain wallet at the default scope
/// (`{account 0, asset 1}`) and record the on-chain grant to `auditor` (a Stellar
/// `G…`). Returns the package (serialized JSON) the owner hands the auditor.
pub fn share_with_auditor(auditor: &str, epoch: u32) -> Result<String, CoreError> {
    let wallet = keys::current_wallet()?;
    let cfg = PoolConfig::load()?;
    share_with_auditor_with(&wallet, &cfg, auditor, epoch)
}

/// Keychain-independent disclosure (used by the live-run driver): build the package +
/// record the on-chain grant with an explicit wallet + config.
pub fn share_with_auditor_with(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    auditor: &str,
    epoch: u32,
) -> Result<String, CoreError> {
    let pkg = build_package(wallet, cfg, epoch)?;
    let json = serde_json::to_string_pretty(&pkg)
        .map_err(|e| CoreError::Crypto(format!("serialize disclosure: {e}")))?;
    // Record the auditable on-chain grant (requires a viewkeys contract configured).
    record_grant(wallet, cfg, auditor, epoch)?;
    Ok(json)
}

/// Build the off-chain disclosure package (no chain writes). The disclosed viewing
/// secret is the wallet's transmission secret for the scope — exactly the key the
/// owner's notes are encrypted to.
pub fn build_package(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    epoch: u32,
) -> Result<DisclosurePackage, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    // The transmission/viewing secret for the scope (same derivation scan uses).
    let view = wallet.scoped_view_key(wallet.account(), SCAN_ASSET_TAG, 0);
    Ok(DisclosurePackage {
        owner_stellar: wallet.stellar_address().to_string(),
        viewing_secret: format!("0x{}", hex::encode(view.viewing)),
        owner_pk: id.owner_pk.to_hex(),
        account: wallet.account(),
        asset_tag: cfg.asset_tag.to_decimal(),
        epoch,
        pool_contract: cfg.pool_contract.clone(),
        note: "ozky scoped disclosure: read-only. Decrypts this owner's notes for the \
               account/asset; cannot spend. Verify each note's commitment on-chain.",
    })
}

/// Auditor side: given a disclosure package, scan the pool and return the owner's notes
/// the package reveals — each verified against its on-chain commitment. The auditor
/// learns balances/values but gains no spend authority.
pub fn audit(pkg_json: &str) -> Result<Vec<DisclosedNote>, CoreError> {
    let pkg: serde_json::Value = serde_json::from_str(pkg_json)
        .map_err(|e| CoreError::Crypto(format!("parse disclosure: {e}")))?;
    let viewing_hex = pkg["viewing_secret"].as_str().ok_or_else(|| CoreError::Crypto("missing viewing_secret".into()))?;
    let owner_pk = pkg["owner_pk"].as_str().and_then(Fr::from_hex)
        .ok_or_else(|| CoreError::Crypto("missing/invalid owner_pk".into()))?;
    let pool = pkg["pool_contract"].as_str().ok_or_else(|| CoreError::Crypto("missing pool_contract".into()))?;

    let mut secret = [0u8; 32];
    let bytes = hex::decode(viewing_hex.strip_prefix("0x").unwrap_or(viewing_hex))
        .map_err(|_| CoreError::Crypto("viewing_secret not hex".into()))?;
    if bytes.len() != 32 {
        return Err(CoreError::Crypto("viewing_secret must be 32 bytes".into()));
    }
    secret.copy_from_slice(&bytes);
    let transmission_sk = StaticSecret::from(secret);

    // Scan the disclosed pool with the disclosed viewing key (auditor has no wallet).
    let cfg = PoolConfig::load_for_audit(pool)?;
    let state = chain::pool_state(&cfg)?;
    let h = Hasher::new();
    let mut out = Vec::new();
    for entry in &state.commits {
        if let Some(n) = scan::match_owned(&h, entry, &transmission_sk, &owner_pk) {
            out.push(DisclosedNote {
                leaf_index: n.leaf_index,
                value: n.value,
                asset_tag: n.asset_tag.to_decimal(),
                epoch: n.epoch,
                commitment: n.commitment.to_hex(),
            });
        }
    }
    Ok(out)
}

/// Convenience for the auditor view: total disclosed value.
pub fn disclosed_total(notes: &[DisclosedNote]) -> u64 {
    notes.iter().map(|n| n.value).sum()
}

/// Re-derive an [`OwnedNote`]'s commitment under `owner_pk` (the check `audit` performs
/// per note); exposed for tests.
pub fn commitment_of(h: &Hasher, n: &OwnedNote, owner_pk: &Fr) -> Fr {
    h.commitment(
        &Fr::from_u64(n.value),
        &n.asset_tag,
        owner_pk,
        &n.blinding,
        &Fr::from_u64(n.epoch as u64),
        &n.rho,
    )
}

/// Record the on-chain disclosure grant (register the scope's public key halves, then
/// `disclose`) via the viewkeys contract. No-op-with-error if no viewkeys contract is
/// configured. Submitted by the wallet (the grant requires the owner's auth).
fn record_grant(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    auditor: &str,
    epoch: u32,
) -> Result<String, CoreError> {
    let viewkeys = cfg.viewkeys_contract.as_deref().ok_or_else(|| {
        CoreError::Chain("OZKY_VIEWKEYS_CONTRACT not set (needed to record the disclosure grant)".into())
    })?;
    let id = scan::wallet_identity(wallet)?;
    let view = wallet.scoped_view_key(wallet.account(), SCAN_ASSET_TAG, 0);
    // The PUBLIC halves go on-chain (secrets stay off-chain): viewing_pub = the
    // transmission pubkey; detection_pub = the detection key (public scanning hint).
    let viewing_pub = hex::encode(id.transmission_pub);
    let detection_pub = hex::encode(view.detection);
    chain::submit_disclosure(
        cfg,
        viewkeys,
        wallet.stellar_secret(),
        wallet.stellar_address(),
        auditor,
        wallet.account(),
        &cfg.asset_tag.to_decimal(),
        epoch,
        &viewing_pub,
        &detection_pub,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::encrypt::{self, NotePlaintext};
    use crate::core::chain::CommitEntry;

    fn cfg() -> PoolConfig {
        PoolConfig {
            pool_contract: "CTEST".into(),
            policy_contract: "CPOL".into(),
            viewkeys_contract: Some("CVK".into()),
            pool_id: Fr::from_u64(7),
            network_id: Fr::from_u64(42),
            asset_tag: Fr::from_u64(1),
            rpc_url: "http://localhost".into(),
            network: "testnet".into(),
            network_passphrase: "Test SDF Network ; September 2015".into(),
            relayer_secret: None,
        }
    }

    const MNEMONIC: &str =
        "illness spike retreat truth genius clock brain pass fit cave bargain toe";

    #[test]
    fn package_carries_viewing_secret_not_spend_key() {
        let wallet = keys::derive_from_mnemonic(MNEMONIC).unwrap();
        let pkg = build_package(&wallet, &cfg(), 28).unwrap();
        // owner_pk is public; the package must NOT carry owner_sk (no spend authority).
        let sk_hex = wallet.owner_sk_hex();
        let json = serde_json::to_string(&pkg).unwrap();
        assert!(!json.contains(sk_hex.trim_start_matches("0x")), "must not leak owner_sk");
        assert_eq!(pkg.epoch, 28);
        assert_eq!(pkg.account, scan::SCAN_ACCOUNT);
    }

    #[test]
    fn auditor_rederives_owner_notes_only() {
        // An owner note (encrypted to the owner's transmission key) is disclosed; a
        // foreign note (different key) must NOT appear in the audit.
        let h = Hasher::new();
        let wallet = keys::derive_from_mnemonic(MNEMONIC).unwrap();
        let id = scan::wallet_identity(&wallet).unwrap();
        let view = wallet.scoped_view_key(scan::SCAN_ACCOUNT, SCAN_ASSET_TAG, 0);
        let transmission_sk = encrypt::transmission_secret(&view.viewing);

        let mk_entry = |owner_pk: &Fr, tpub: &[u8; 32], leaf: u32, value: u64| {
            let note = NotePlaintext { value, asset_tag: Fr::from_u64(1), blinding: Fr::from_u64(7), epoch: 28, rho: Fr::from_u64(leaf as u64 + 1) };
            let commitment = h.commitment(&Fr::from_u64(value), &note.asset_tag, owner_pk, &note.blinding, &Fr::from_u64(28), &note.rho);
            let enc = encrypt::encrypt_note(&note.serialize(), tpub).unwrap();
            CommitEntry {
                leaf_index: leaf,
                commitment: commitment.to_hex(),
                enc_note: Some(format!("0x{}", hex::encode(&enc.enc_note))),
                ephemeral_pub: Some(format!("0x{}", hex::encode(enc.ephemeral_pub))),
                view_tag: Some(enc.view_tag),
            }
        };

        let mine = mk_entry(&id.owner_pk, &id.transmission_pub, 0, 500);
        let foreign_pk = h.owner_pk(&Fr::from_u64(999));
        let foreign = mk_entry(&foreign_pk, &encrypt::transmission_public(&[9u8; 32]), 1, 800);

        // The auditor (disclosed viewing key + owner_pk) recovers ONLY the owner's note.
        assert!(scan::match_owned(&h, &mine, &transmission_sk, &id.owner_pk).is_some());
        assert!(scan::match_owned(&h, &foreign, &transmission_sk, &id.owner_pk).is_none());
    }
}
