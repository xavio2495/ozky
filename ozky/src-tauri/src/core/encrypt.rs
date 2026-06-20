//! Note payload encryption (Phase A2) — the trustless-receive wire format.
//!
//! A sender encrypts the spendable note fields to the recipient's transmission key so
//! the recipient (and only the recipient) can rediscover and later spend the output.
//! Scheme (the encrypted-payload format the spec deferred to here):
//!
//! 1. **Key agreement:** ephemeral-static X25519 ECDH between a fresh per-note
//!    ephemeral key and the recipient's static transmission key (derived from the A1
//!    viewing-key hierarchy).
//! 2. **KDF:** HKDF-SHA256 over the shared secret (salt = `ephemeral_pub`) expands a
//!    block split into an AEAD key, nonce, a 32-byte **key commitment**, and the
//!    view-tag bytes.
//! 3. **AEAD:** ChaCha20-Poly1305 over the serialized note. To make it
//!    **key-committing** (plain AEADs are not — the spec forbids plain AES-GCM), the
//!    payload carries the HKDF key commitment, which decryption checks first; a wrong
//!    key is rejected deterministically instead of producing garbage.
//! 4. **View tag:** 12 bits derived from the same shared secret, so both sender and
//!    recipient compute it. Scanning trial-matches the tag before attempting decrypt.
//!
//! On-chain payload = `enc_note = key_commitment(32) || ciphertext`, published with
//! `ephemeral_pub(32)` and `view_tag(u32)` (see `Note` event in the pool contract).

use super::poseidon::Fr;
use super::CoreError;
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};

const COMMIT_LEN: usize = 32;
/// HKDF output block: AEAD key(32) ‖ nonce(12) ‖ key-commit(32) ‖ view-tag(2).
const OKM_LEN: usize = 32 + 12 + COMMIT_LEN + 2;
const VIEW_TAG_BITS: u32 = 12;
const VIEW_TAG_MASK: u32 = (1 << VIEW_TAG_BITS) - 1;

/// The on-chain encrypted-note payload plus the public scanning metadata.
pub struct EncryptedNote {
    pub enc_note: Vec<u8>,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u32,
}

/// The spendable note fields a recipient needs (its own `owner_pk` is not carried —
/// the recipient derives it from its own key). Serialized as the AEAD plaintext.
#[derive(Clone)]
pub struct NotePlaintext {
    pub value: u64,
    pub asset_tag: Fr,
    pub blinding: Fr,
    pub epoch: u32,
    pub rho: Fr,
}

impl NotePlaintext {
    /// Fixed 108-byte layout: value(8) ‖ asset_tag(32) ‖ blinding(32) ‖ epoch(4) ‖ rho(32).
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(108);
        out.extend_from_slice(&self.value.to_be_bytes());
        out.extend_from_slice(&self.asset_tag.0);
        out.extend_from_slice(&self.blinding.0);
        out.extend_from_slice(&self.epoch.to_be_bytes());
        out.extend_from_slice(&self.rho.0);
        out
    }

    pub fn deserialize(b: &[u8]) -> Result<NotePlaintext, CoreError> {
        if b.len() != 108 {
            return Err(CoreError::Crypto(format!("note plaintext len {} != 108", b.len())));
        }
        let mut value = [0u8; 8];
        value.copy_from_slice(&b[0..8]);
        let mut asset_tag = [0u8; 32];
        asset_tag.copy_from_slice(&b[8..40]);
        let mut blinding = [0u8; 32];
        blinding.copy_from_slice(&b[40..72]);
        let mut epoch = [0u8; 4];
        epoch.copy_from_slice(&b[72..76]);
        let mut rho = [0u8; 32];
        rho.copy_from_slice(&b[76..108]);
        Ok(NotePlaintext {
            value: u64::from_be_bytes(value),
            asset_tag: Fr(asset_tag),
            blinding: Fr(blinding),
            epoch: u32::from_be_bytes(epoch),
            rho: Fr(rho),
        })
    }
}

/// The recipient's static X25519 transmission secret, derived from a 32-byte incoming
/// viewing key (`ivk`, from the A1 view-key hierarchy). `StaticSecret::from` clamps it.
pub fn transmission_secret(ivk: &[u8; 32]) -> StaticSecret {
    StaticSecret::from(*ivk)
}

/// The recipient's public transmission key (the shielded payment code) for an `ivk`.
pub fn transmission_public(ivk: &[u8; 32]) -> [u8; 32] {
    PublicKey::from(&transmission_secret(ivk)).to_bytes()
}

/// HKDF-expand the shared secret (salt = ephemeral_pub) into the key/nonce/commit/tag.
fn derive(shared: &[u8; 32], ephemeral_pub: &[u8; 32]) -> [u8; OKM_LEN] {
    let hk = Hkdf::<Sha256>::new(Some(ephemeral_pub), shared);
    let mut okm = [0u8; OKM_LEN];
    hk.expand(b"ozky-note-v1", &mut okm)
        .expect("OKM_LEN within HKDF-SHA256 output bound");
    okm
}

fn view_tag_from(okm: &[u8; OKM_LEN]) -> u32 {
    (u16::from_be_bytes([okm[76], okm[77]]) as u32) & VIEW_TAG_MASK
}

fn aead(okm: &[u8; OKM_LEN]) -> (ChaCha20Poly1305, Nonce) {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&okm[0..32]));
    let nonce = *Nonce::from_slice(&okm[32..44]);
    (cipher, nonce)
}

/// Encrypt a serialized note to a recipient's transmission key, returning the on-chain
/// payload + scanning metadata.
pub fn encrypt_note(note: &[u8], recipient_transmission_pub: &[u8; 32]) -> Result<EncryptedNote, CoreError> {
    let ephemeral = EphemeralSecret::random_from_rng(rand_core::OsRng);
    let ephemeral_pub = PublicKey::from(&ephemeral).to_bytes();
    let shared = ephemeral
        .diffie_hellman(&PublicKey::from(*recipient_transmission_pub))
        .to_bytes();

    let okm = derive(&shared, &ephemeral_pub);
    let (cipher, nonce) = aead(&okm);
    let ciphertext = cipher
        .encrypt(&nonce, note)
        .map_err(|_| CoreError::Crypto("AEAD encrypt failed".into()))?;

    let mut enc_note = Vec::with_capacity(COMMIT_LEN + ciphertext.len());
    enc_note.extend_from_slice(&okm[44..76]); // key commitment
    enc_note.extend_from_slice(&ciphertext);

    Ok(EncryptedNote {
        enc_note,
        ephemeral_pub,
        view_tag: view_tag_from(&okm),
    })
}

/// The view tag the recipient expects for `ephemeral_pub` (recomputed during scan from
/// the shared secret); compared against the on-chain tag before any decrypt attempt.
pub fn expected_view_tag(transmission_sk: &StaticSecret, ephemeral_pub: &[u8; 32]) -> u32 {
    let shared = transmission_sk.diffie_hellman(&PublicKey::from(*ephemeral_pub)).to_bytes();
    view_tag_from(&derive(&shared, ephemeral_pub))
}

/// Decrypt an on-chain payload with the wallet's transmission secret. Verifies the
/// key commitment first (key-committing AEAD), then authenticates + decrypts.
pub fn decrypt_note(
    enc_note: &[u8],
    ephemeral_pub: &[u8; 32],
    transmission_sk: &StaticSecret,
) -> Result<Vec<u8>, CoreError> {
    if enc_note.len() < COMMIT_LEN {
        return Err(CoreError::Crypto("enc_note shorter than key commitment".into()));
    }
    let shared = transmission_sk.diffie_hellman(&PublicKey::from(*ephemeral_pub)).to_bytes();
    let okm = derive(&shared, ephemeral_pub);

    // Key commitment: reject a wrong key deterministically (not as AEAD garbage).
    if enc_note[0..COMMIT_LEN] != okm[44..76] {
        return Err(CoreError::Crypto("key commitment mismatch".into()));
    }
    let (cipher, nonce) = aead(&okm);
    cipher
        .decrypt(&nonce, &enc_note[COMMIT_LEN..])
        .map_err(|_| CoreError::Crypto("AEAD decrypt/authentication failed".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_note() -> NotePlaintext {
        NotePlaintext {
            value: 1000,
            asset_tag: Fr::from_u64(1),
            blinding: Fr::from_u64(777),
            epoch: 28,
            rho: Fr::from_u64(111),
        }
    }

    #[test]
    fn plaintext_roundtrip() {
        let n = sample_note();
        let de = NotePlaintext::deserialize(&n.serialize()).unwrap();
        assert_eq!(de.value, 1000);
        assert_eq!(de.asset_tag, Fr::from_u64(1));
        assert_eq!(de.epoch, 28);
        assert_eq!(de.rho, Fr::from_u64(111));
    }

    #[test]
    fn encrypt_then_decrypt_recovers_note() {
        let ivk = [7u8; 32];
        let tpub = transmission_public(&ivk);
        let tsk = transmission_secret(&ivk);

        let pt = sample_note().serialize();
        let enc = encrypt_note(&pt, &tpub).unwrap();

        // Tag the recipient expects matches the published tag.
        assert_eq!(expected_view_tag(&tsk, &enc.ephemeral_pub), enc.view_tag);

        let dec = decrypt_note(&enc.enc_note, &enc.ephemeral_pub, &tsk).unwrap();
        assert_eq!(dec, pt);
    }

    #[test]
    fn wrong_key_is_rejected_by_commitment() {
        let tpub = transmission_public(&[7u8; 32]);
        let wrong = transmission_secret(&[9u8; 32]);
        let enc = encrypt_note(&sample_note().serialize(), &tpub).unwrap();
        assert!(decrypt_note(&enc.enc_note, &enc.ephemeral_pub, &wrong).is_err());
    }

    #[test]
    fn tamper_is_rejected() {
        let ivk = [3u8; 32];
        let enc = encrypt_note(&sample_note().serialize(), &transmission_public(&ivk)).unwrap();
        let mut bad = enc.enc_note.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0x01;
        assert!(decrypt_note(&bad, &enc.ephemeral_pub, &transmission_secret(&ivk)).is_err());
    }
}
