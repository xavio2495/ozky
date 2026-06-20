//! Note scanning (Phase A2). Rediscovers the wallet's own notes from the indexer's
//! commitment scan stream: a cheap **view-tag trial match** filters candidates, then
//! full decryption + a commitment re-derivation confirm ownership (so only genuinely
//! owned notes are accepted). Spent notes (whose nullifier is in the published set)
//! are excluded, leaving the spendable set. This closes the A1 carry-forward: a
//! restored wallet rediscovers its notes from chain alone.

use super::config::PoolConfig;
use super::encrypt::{self, NotePlaintext};
use super::keys::{self, WalletKeys};
use super::poseidon::{Fr, Hasher};
use super::witness::SpendNote;
use super::{chain, notes, CoreError};
use std::collections::HashSet;
use x25519_dalek::StaticSecret;

/// The scope under which transmission/detection keys are derived for scanning. The
/// transmission key is epoch-independent (account+asset) so it is a stable receive
/// address; the per-note epoch lives inside the encrypted payload.
pub const SCAN_ACCOUNT: u32 = 0;
pub const SCAN_ASSET_TAG: u32 = 1;
const TRANSMISSION_EPOCH: u32 = 0;

/// A discovered, decrypted note owned by this wallet (spendable input material).
pub struct OwnedNote {
    pub leaf_index: u32,
    pub value: u64,
    pub asset_tag: Fr,
    pub blinding: Fr,
    pub epoch: u32,
    pub rho: Fr,
    pub commitment: Fr,
}

impl OwnedNote {
    /// View into the witness layer's spend representation.
    pub fn as_spend_note(&self) -> SpendNote {
        SpendNote {
            value: self.value,
            blinding: self.blinding,
            epoch: Fr::from_u64(self.epoch as u64),
            rho: self.rho,
            leaf_index: self.leaf_index as usize,
        }
    }
}

fn parse_hex_bytes(h: &str) -> Option<Vec<u8>> {
    hex::decode(h.strip_prefix("0x").unwrap_or(h)).ok()
}

fn parse_pub32(h: &str) -> Option<[u8; 32]> {
    let v = parse_hex_bytes(h)?;
    (v.len() == 32).then(|| {
        let mut a = [0u8; 32];
        a.copy_from_slice(&v);
        a
    })
}

/// Try to match a single scan entry to this wallet: view-tag filter, decrypt, then
/// re-derive the commitment with our `owner_pk` and require it to equal the on-chain
/// leaf. Returns the owned note only on a full, authenticated match.
fn try_match(
    h: &Hasher,
    entry: &chain::CommitEntry,
    transmission_sk: &StaticSecret,
    owner_pk: &Fr,
) -> Option<OwnedNote> {
    let enc_note = parse_hex_bytes(entry.enc_note.as_deref()?)?;
    let ephemeral_pub = parse_pub32(entry.ephemeral_pub.as_deref()?)?;
    let view_tag = entry.view_tag?;

    // Cheap filter: only proceed when the expected tag matches (skips ~all foreign notes).
    if encrypt::expected_view_tag(transmission_sk, &ephemeral_pub) != view_tag {
        return None;
    }
    let plaintext = encrypt::decrypt_note(&enc_note, &ephemeral_pub, transmission_sk).ok()?;
    let note = NotePlaintext::deserialize(&plaintext).ok()?;

    // Bind the decrypted note to the published leaf (rejects tag collisions / spoofs).
    let commitment = h.commitment(
        &Fr::from_u64(note.value),
        &note.asset_tag,
        owner_pk,
        &note.blinding,
        &Fr::from_u64(note.epoch as u64),
        &note.rho,
    );
    if commitment != Fr::from_hex(&entry.commitment)? {
        return None;
    }

    Some(OwnedNote {
        leaf_index: entry.leaf_index,
        value: note.value,
        asset_tag: note.asset_tag,
        blinding: note.blinding,
        epoch: note.epoch,
        rho: note.rho,
        commitment,
    })
}

/// The wallet's spend keys + transmission key pair — the identity used to scan for,
/// receive, and spend notes. Shared by [`scan`] and the send flow ([`super::send`]).
pub struct WalletIdentity {
    pub owner_sk: Fr,
    pub owner_pk: Fr,
    pub transmission_sk: StaticSecret,
    pub transmission_pub: [u8; 32],
}

/// Derive the wallet's scan/spend identity from its keys (the transmission key is the
/// epoch-independent {account 0, asset 1} viewing key — a stable receive address).
pub fn wallet_identity(w: &WalletKeys) -> Result<WalletIdentity, CoreError> {
    let view = w.scoped_view_key(SCAN_ACCOUNT, SCAN_ASSET_TAG, TRANSMISSION_EPOCH);
    let owner_sk = Fr::from_hex(&w.owner_sk_hex())
        .ok_or_else(|| CoreError::Crypto("owner_sk hex".into()))?;
    let h = Hasher::new();
    Ok(WalletIdentity {
        owner_sk,
        owner_pk: h.owner_pk(&owner_sk),
        transmission_sk: encrypt::transmission_secret(&view.viewing),
        transmission_pub: encrypt::transmission_public(&view.viewing),
    })
}

/// Match a commit entry against a locally-stored note opening: recompute the opening's
/// commitment with our `owner_pk` and accept if it equals the on-chain leaf. This
/// recovers notes with NO on-chain ciphertext (the withdraw change note).
fn match_local(
    h: &Hasher,
    entry: &chain::CommitEntry,
    owner_pk: &Fr,
    local: &[NotePlaintext],
) -> Option<OwnedNote> {
    let leaf = Fr::from_hex(&entry.commitment)?;
    for n in local {
        let commitment = h.commitment(
            &Fr::from_u64(n.value),
            &n.asset_tag,
            owner_pk,
            &n.blinding,
            &Fr::from_u64(n.epoch as u64),
            &n.rho,
        );
        if commitment == leaf {
            return Some(OwnedNote {
                leaf_index: entry.leaf_index,
                value: n.value,
                asset_tag: n.asset_tag,
                blinding: n.blinding,
                epoch: n.epoch,
                rho: n.rho,
                commitment,
            });
        }
    }
    None
}

/// Scan the configured pool, returning the keychain wallet's UNSPENT notes from
/// `from_leaf` onward — discovered from BOTH on-chain ciphertexts AND the local notes
/// store (for notes the chain doesn't carry, e.g. withdraw change). Reads pool state
/// from raw RPC (any pool, no indexer).
pub fn scan(from_leaf: u32) -> Result<Vec<OwnedNote>, CoreError> {
    let wallet = keys::current_wallet()?;
    let cfg = PoolConfig::load()?;
    let id = wallet_identity(&wallet)?;
    let state = chain::pool_state(&cfg)?;
    let local = notes::load(&wallet)?;
    owned_notes(&id, &state, &local, from_leaf)
}

/// Match owned notes against already-fetched pool state via on-chain ciphertexts only
/// (no local store). Keychain- and network-independent.
pub fn scan_state(
    id: &WalletIdentity,
    state: &chain::PoolState,
    from_leaf: u32,
) -> Result<Vec<OwnedNote>, CoreError> {
    owned_notes(id, state, &[], from_leaf)
}

/// The wallet's UNSPENT owned notes in `state` from `from_leaf` on: for each on-chain
/// commitment, recover the opening either by decrypting its published ciphertext OR by
/// matching a `local` store opening (notes with no ciphertext); exclude spent notes.
pub fn owned_notes(
    id: &WalletIdentity,
    state: &chain::PoolState,
    local: &[NotePlaintext],
    from_leaf: u32,
) -> Result<Vec<OwnedNote>, CoreError> {
    let h = Hasher::new();
    let spent: HashSet<[u8; 32]> = state.nullifiers.iter().map(|n| n.0).collect();

    let mut owned = Vec::new();
    for entry in &state.commits {
        if entry.leaf_index < from_leaf {
            continue;
        }
        let note = try_match(&h, entry, &id.transmission_sk, &id.owner_pk)
            .or_else(|| match_local(&h, entry, &id.owner_pk, local));
        if let Some(note) = note {
            // Exclude already-spent notes (nullifier published).
            let nf = h.nullifier(&note.rho, &id.owner_sk);
            if !spent.contains(&nf.0) {
                owned.push(note);
            }
        }
    }
    Ok(owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::chain::CommitEntry;
    use crate::core::encrypt::{encrypt_note, transmission_public};

    // A self-addressed note: encrypt to our own transmission key, mint the matching
    // commitment leaf, and confirm scan matching recovers exactly it.
    fn owned_entry(h: &Hasher, owner_pk: &Fr, ivk: &[u8; 32], leaf_index: u32) -> CommitEntry {
        let note = NotePlaintext {
            value: 1000,
            asset_tag: Fr::from_u64(1),
            blinding: Fr::from_u64(777),
            epoch: 28,
            rho: Fr::from_u64(111),
        };
        let commitment = h.commitment(
            &Fr::from_u64(note.value),
            &note.asset_tag,
            owner_pk,
            &note.blinding,
            &Fr::from_u64(note.epoch as u64),
            &note.rho,
        );
        let enc = encrypt_note(&note.serialize(), &transmission_public(ivk)).unwrap();
        CommitEntry {
            leaf_index,
            commitment: commitment.to_hex(),
            enc_note: Some(format!("0x{}", hex::encode(&enc.enc_note))),
            ephemeral_pub: Some(format!("0x{}", hex::encode(enc.ephemeral_pub))),
            view_tag: Some(enc.view_tag),
        }
    }

    #[test]
    fn matches_own_note_only() {
        let h = Hasher::new();
        let owner_sk = Fr::from_u64(12345);
        let owner_pk = h.owner_pk(&owner_sk);
        let ivk = [7u8; 32];
        let tsk = encrypt::transmission_secret(&ivk);

        // Ours.
        let mine = owned_entry(&h, &owner_pk, &ivk, 0);
        assert!(try_match(&h, &mine, &tsk, &owner_pk).is_some(), "must match own note");

        // Someone else's note (encrypted to a different transmission key).
        let other = owned_entry(&h, &h.owner_pk(&Fr::from_u64(99)), &[8u8; 32], 1);
        assert!(
            try_match(&h, &other, &tsk, &owner_pk).is_none(),
            "must NOT match a foreign note"
        );

        // A bare commitment with no payload (e.g. a deposit without enc data) is skipped.
        let bare = CommitEntry {
            leaf_index: 2,
            commitment: "0x01".into(),
            enc_note: None,
            ephemeral_pub: None,
            view_tag: None,
        };
        assert!(try_match(&h, &bare, &tsk, &owner_pk).is_none());
    }

    #[test]
    fn recovered_note_fields_are_correct() {
        let h = Hasher::new();
        let owner_pk = h.owner_pk(&Fr::from_u64(12345));
        let ivk = [7u8; 32];
        let note = try_match(
            &h,
            &owned_entry(&h, &owner_pk, &ivk, 5),
            &encrypt::transmission_secret(&ivk),
            &owner_pk,
        )
        .unwrap();
        assert_eq!(note.leaf_index, 5);
        assert_eq!(note.value, 1000);
        assert_eq!(note.epoch, 28);
        assert_eq!(note.rho, Fr::from_u64(111));
    }
}
