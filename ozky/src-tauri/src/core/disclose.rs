//! Selective disclosure (FEATURE_SET G5; spec D6 + the FROZEN view-key tree's
//! "per-transaction disclosure path for one-off regulator requests"). Lets a wallet hand
//! an auditor a TIME-BOUNDED, read-only view of its activity: the owner's notes for a
//! chosen epoch range `[from_epoch, to_epoch]`, each provably its own — with no spend
//! authority and no way to see other epochs.
//!
//! Model: **owner-curated package** (NOT a key handover). The owner scans its own notes
//! (every epoch is derivable from the seed), filters to the granted epoch range, and
//! packages each note's OPENING (`value, asset_tag, epoch, blinding, rho, leaf_index`).
//! The auditor re-derives each commitment under the owner's `owner_pk` and checks it
//! against the on-chain leaf — so every disclosed note is provably the owner's and
//! provably on-chain. Because no decryption key is handed over:
//!   - epochs OUTSIDE the range are never included and stay cryptographically shielded
//!     (the auditor holds nothing that could open them) — **expiry/revocation is real**;
//!   - the package is a verifiable snapshot, not a live-scanning capability.
//! Completeness within the range is owner-asserted (proving the set is exhaustive is the
//! separate "completeness proof" upgrade, not this scope).
//!
//! The on-chain trail (`register_view_key` + `disclose` on the viewkeys contract, one
//! node per epoch in the range) records an auditable, timestamped grant.

use super::config::PoolConfig;
use super::poseidon::{Fr, Hasher};
use super::scan::{self, OwnedNote, SCAN_ASSET_TAG};
use super::{chain, encrypt, keys, notes, CoreError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One note's OPENING, revealed to the auditor. The auditor re-derives the commitment
/// from these fields under the owner's `owner_pk` and matches it to the on-chain leaf.
#[derive(Serialize, Deserialize, Clone)]
pub struct DisclosedOpening {
    pub leaf_index: u32,
    pub value: u64,
    pub asset_tag: String,
    pub epoch: u32,
    /// Note blinding factor (hex) — part of the opening.
    pub blinding: String,
    /// Note `rho` (hex) — part of the opening.
    pub rho: String,
    /// The note's on-chain commitment (hex); the auditor recomputes + checks this.
    pub commitment: String,
}

/// The portable disclosure package handed to an auditor (out-of-band). JSON, no spend
/// authority and no decryption key: just the openings of the owner's notes in the granted
/// epoch range, bindable to chain via `owner_pk`.
#[derive(Serialize, Deserialize)]
pub struct DisclosurePackage {
    pub owner_stellar: String,
    /// The owner's `owner_pk` (hex) — to re-derive + match commitments.
    pub owner_pk: String,
    /// The pool the disclosed notes live in (so the auditor verifies the right pool).
    pub pool_contract: String,
    pub from_epoch: u32,
    pub to_epoch: u32,
    pub notes: Vec<DisclosedOpening>,
    pub note: String,
}

/// A note verified by [`audit`]: its opening checked against the on-chain commitment.
#[derive(Serialize)]
pub struct DisclosedNote {
    pub leaf_index: u32,
    pub value: u64,
    pub asset_tag: String,
    pub epoch: u32,
    pub commitment: String,
}

/// Build the disclosure package for the keychain wallet over the epoch range and record
/// the on-chain grant to `auditor` (a Stellar `G…`). Returns the package (JSON).
pub fn share_with_auditor(auditor: &str, from_epoch: u32, to_epoch: u32) -> Result<String, CoreError> {
    let wallet = keys::current_wallet()?;
    let cfg = PoolConfig::load()?;
    share_with_auditor_with(&wallet, &cfg, auditor, from_epoch, to_epoch)
}

/// Keychain-independent disclosure (used by the live-run driver): build the package +
/// record the on-chain grant with an explicit wallet + config.
pub fn share_with_auditor_with(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    auditor: &str,
    from_epoch: u32,
    to_epoch: u32,
) -> Result<String, CoreError> {
    let pkg = build_curated_package(wallet, cfg, from_epoch, to_epoch)?;
    let json = serde_json::to_string_pretty(&pkg)
        .map_err(|e| CoreError::Crypto(format!("serialize disclosure: {e}")))?;
    record_grant(wallet, cfg, auditor, from_epoch, to_epoch)?;
    Ok(json)
}

/// Scan the owner's own notes (spent + unspent), filter to `[from_epoch, to_epoch]`, and
/// package their openings. No chain WRITES (the grant is recorded separately).
pub fn build_curated_package(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    from_epoch: u32,
    to_epoch: u32,
) -> Result<DisclosurePackage, CoreError> {
    if to_epoch < from_epoch {
        return Err(CoreError::Crypto(format!(
            "invalid epoch range: from {from_epoch} > to {to_epoch}"
        )));
    }
    let id = scan::wallet_identity(wallet)?;
    let state = chain::pool_state(cfg)?;
    let local = notes::load(wallet)?;
    let owned = scan::scan_all(&id, &state, &local, 0)?;
    Ok(build_package_from_notes(
        &id.owner_pk,
        wallet.stellar_address(),
        &cfg.pool_contract,
        &owned,
        from_epoch,
        to_epoch,
    ))
}

/// Pure assembly of the package from already-scanned notes (no network) — keeps the
/// epoch-range filtering testable on its own.
pub fn build_package_from_notes(
    owner_pk: &Fr,
    owner_stellar: &str,
    pool_contract: &str,
    owned: &[OwnedNote],
    from_epoch: u32,
    to_epoch: u32,
) -> DisclosurePackage {
    let notes = owned
        .iter()
        .filter(|n| n.epoch >= from_epoch && n.epoch <= to_epoch)
        .map(|n| DisclosedOpening {
            leaf_index: n.leaf_index,
            value: n.value,
            asset_tag: n.asset_tag.to_decimal(),
            epoch: n.epoch,
            blinding: n.blinding.to_hex(),
            rho: n.rho.to_hex(),
            commitment: n.commitment.to_hex(),
        })
        .collect();
    DisclosurePackage {
        owner_stellar: owner_stellar.to_string(),
        owner_pk: owner_pk.to_hex(),
        pool_contract: pool_contract.to_string(),
        from_epoch,
        to_epoch,
        notes,
        note: "ozky time-bounded disclosure: read-only. Each note's opening re-derives its \
               on-chain commitment under owner_pk; cannot spend. Epochs outside the range \
               are not included and remain shielded."
            .to_string(),
    }
}

/// Recompute the commitment an opening claims, under the owner's `owner_pk`. `None` if
/// any field is malformed.
fn opening_commitment(h: &Hasher, owner_pk: &Fr, o: &DisclosedOpening) -> Option<Fr> {
    let asset_tag = Fr::from_u64(o.asset_tag.parse::<u64>().ok()?);
    let blinding = Fr::from_hex(&o.blinding)?;
    let rho = Fr::from_hex(&o.rho)?;
    Some(h.commitment(
        &Fr::from_u64(o.value),
        &asset_tag,
        owner_pk,
        &blinding,
        &Fr::from_u64(o.epoch as u64),
        &rho,
    ))
}

/// Auditor side: verify a disclosure package and return the notes it reveals. For each
/// opening: require its epoch ∈ the granted range, re-derive its commitment under
/// `owner_pk`, and require that commitment to equal both the opening's claimed value AND
/// the actual on-chain leaf. Any mismatch fails the whole package (tamper-evident). The
/// auditor learns balances/values but gains no spend authority.
pub fn audit(pkg_json: &str) -> Result<Vec<DisclosedNote>, CoreError> {
    let pkg: DisclosurePackage = serde_json::from_str(pkg_json)
        .map_err(|e| CoreError::Crypto(format!("parse disclosure: {e}")))?;
    let owner_pk = Fr::from_hex(&pkg.owner_pk)
        .ok_or_else(|| CoreError::Crypto("missing/invalid owner_pk".into()))?;

    let cfg = PoolConfig::load_for_audit(&pkg.pool_contract)?;
    let state = chain::pool_state(&cfg)?;
    let on_chain: HashMap<u32, Fr> = state
        .commits
        .iter()
        .filter_map(|e| Fr::from_hex(&e.commitment).map(|c| (e.leaf_index, c)))
        .collect();

    let h = Hasher::new();
    let mut out = Vec::new();
    for o in &pkg.notes {
        if o.epoch < pkg.from_epoch || o.epoch > pkg.to_epoch {
            return Err(CoreError::Crypto(format!(
                "note at leaf {} has epoch {} outside granted range {}..={}",
                o.leaf_index, o.epoch, pkg.from_epoch, pkg.to_epoch
            )));
        }
        let recomputed = opening_commitment(&h, &owner_pk, o)
            .ok_or_else(|| CoreError::Crypto(format!("malformed opening at leaf {}", o.leaf_index)))?;
        let claimed = Fr::from_hex(&o.commitment)
            .ok_or_else(|| CoreError::Crypto(format!("invalid commitment hex at leaf {}", o.leaf_index)))?;
        if recomputed != claimed {
            return Err(CoreError::Crypto(format!(
                "opening at leaf {} does not match its stated commitment",
                o.leaf_index
            )));
        }
        match on_chain.get(&o.leaf_index) {
            Some(c) if *c == recomputed => {}
            _ => {
                return Err(CoreError::Crypto(format!(
                    "commitment at leaf {} not found on-chain (tampered or wrong pool)",
                    o.leaf_index
                )))
            }
        }
        out.push(DisclosedNote {
            leaf_index: o.leaf_index,
            value: o.value,
            asset_tag: o.asset_tag.clone(),
            epoch: o.epoch,
            commitment: o.commitment.clone(),
        });
    }
    Ok(out)
}

/// Convenience for the auditor view: total disclosed value.
pub fn disclosed_total(notes: &[DisclosedNote]) -> u64 {
    notes.iter().map(|n| n.value).sum()
}

/// Cap on a grant's epoch span — keeps the on-chain trail (one node per epoch) bounded.
const MAX_GRANT_EPOCHS: u32 = 52;

/// Record the on-chain disclosure grant: one `register_view_key` + `disclose` per epoch in
/// the range, publishing each epoch's PUBLIC key halves (no secret leaves the wallet).
/// No-op-with-error if no viewkeys contract is configured. Returns the last tx hash.
fn record_grant(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    auditor: &str,
    from_epoch: u32,
    to_epoch: u32,
) -> Result<String, CoreError> {
    let viewkeys = cfg.viewkeys_contract.as_deref().ok_or_else(|| {
        CoreError::Chain("OZKY_VIEWKEYS_CONTRACT not set (needed to record the disclosure grant)".into())
    })?;
    if to_epoch.saturating_sub(from_epoch) >= MAX_GRANT_EPOCHS {
        return Err(CoreError::Chain(format!(
            "disclosure range too large ({} epochs; max {MAX_GRANT_EPOCHS})",
            to_epoch - from_epoch + 1
        )));
    }
    let mut last = String::new();
    for epoch in from_epoch..=to_epoch {
        // PUBLIC halves only (secrets stay off-chain): viewing_pub = the per-epoch
        // transmission pubkey; detection_pub = the per-epoch detection key.
        let view = wallet.scoped_view_key(wallet.account(), SCAN_ASSET_TAG, epoch);
        let viewing_pub = hex::encode(encrypt::transmission_public(&view.viewing));
        let detection_pub = hex::encode(view.detection);
        last = chain::submit_disclosure(
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
        )?;
    }
    Ok(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MNEMONIC: &str =
        "illness spike retreat truth genius clock brain pass fit cave bargain toe";

    /// Synthesize an owned note (with a real commitment under `owner_pk`) at an epoch.
    fn note_at(h: &Hasher, owner_pk: &Fr, leaf: u32, value: u64, epoch: u32) -> OwnedNote {
        let blinding = Fr::from_u64(leaf as u64 + 700);
        let rho = Fr::from_u64(leaf as u64 + 100);
        let asset_tag = Fr::from_u64(1);
        let commitment =
            h.commitment(&Fr::from_u64(value), &asset_tag, owner_pk, &blinding, &Fr::from_u64(epoch as u64), &rho);
        OwnedNote { leaf_index: leaf, value, asset_tag, blinding, epoch, rho, commitment }
    }

    #[test]
    fn package_filters_to_range_and_omits_spend_key() {
        let h = Hasher::new();
        let wallet = keys::derive_from_mnemonic(MNEMONIC).unwrap();
        let owner_pk = scan::wallet_identity(&wallet).unwrap().owner_pk;
        let owned = vec![
            note_at(&h, &owner_pk, 0, 100, 27),
            note_at(&h, &owner_pk, 1, 200, 28),
            note_at(&h, &owner_pk, 2, 300, 29),
            note_at(&h, &owner_pk, 3, 400, 30),
        ];
        // Grant epochs 28..=29 only.
        let pkg = build_package_from_notes(&owner_pk, wallet.stellar_address(), "CPOOL", &owned, 28, 29);
        assert_eq!(pkg.notes.len(), 2, "only the in-range epochs are disclosed");
        assert!(pkg.notes.iter().all(|n| n.epoch >= 28 && n.epoch <= 29));
        // The package must NOT carry owner_sk (no spend authority) and no viewing secret.
        let json = serde_json::to_string(&pkg).unwrap();
        assert!(!json.contains(wallet.owner_sk_hex().trim_start_matches("0x")), "must not leak owner_sk");
        assert!(!json.contains("viewing_secret"), "owner-curated package hands over no decryption key");
    }

    #[test]
    fn opening_verifies_against_commitment_and_rejects_tamper() {
        let h = Hasher::new();
        let wallet = keys::derive_from_mnemonic(MNEMONIC).unwrap();
        let owner_pk = scan::wallet_identity(&wallet).unwrap().owner_pk;
        let n = note_at(&h, &owner_pk, 5, 1000, 28);
        let pkg = build_package_from_notes(&owner_pk, wallet.stellar_address(), "CPOOL", &[n.clone()], 28, 28);
        let o = &pkg.notes[0];

        // A faithful opening re-derives exactly the on-chain commitment.
        assert_eq!(opening_commitment(&h, &owner_pk, o).unwrap(), n.commitment, "opening must re-derive the commitment");

        // Tampering any opening field breaks the binding (the audit check that follows).
        let mut bad = o.clone();
        bad.value = 999;
        assert_ne!(opening_commitment(&h, &owner_pk, &bad).unwrap(), n.commitment, "tampered value must not verify");
        // A foreign owner_pk also fails to reproduce the commitment.
        let foreign = h.owner_pk(&Fr::from_u64(424242));
        assert_ne!(opening_commitment(&h, &foreign, o).unwrap(), n.commitment, "wrong owner_pk must not verify");
    }
}
