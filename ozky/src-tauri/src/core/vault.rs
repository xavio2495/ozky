//! Encrypted wallet vault — the seeds (and TOTP secret) at rest.
//!
//! Holds the app-level TOTP secret + **one or more independent wallet seeds** (each
//! account is its own BIP39 mnemonic: create generates one, import adds an existing one),
//! encrypted under a key derived from the user's password via **Argon2id**. The encrypted
//! blob lives in the OS keychain; the password is never stored. Unlock derives the key,
//! decrypts, and loads the contents (and the derived key, so accounts can be added later
//! without re-prompting) into [`super::session`].
//!
//! Blob layout (then hex for keychain string storage):
//!   magic(4) ‖ version(1) ‖ salt(16) ‖ nonce(12) ‖ ciphertext
//! ciphertext = ChaCha20-Poly1305( plaintext ), where plaintext =
//!   totp_len(1) ‖ totp_secret ‖ n_accounts(1) ‖ [ mnemonic_len(2 BE) ‖ mnemonic_utf8 ]*

use super::CoreError;
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use rand_core::{OsRng, RngCore};
use zeroize::Zeroizing;

/// Keychain account holding the encrypted vault blob.
const VAULT_ACCOUNT: &str = "vault";
const MAGIC: &[u8; 4] = b"OZKV";
const VERSION: u8 = 2; // v2: multi-seed
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;

/// The decrypted vault contents (secret-bearing; never crosses the UI boundary).
pub struct VaultContent {
    pub totp_secret: [u8; super::totp::SECRET_LEN],
    /// One mnemonic per account (account index = position in this vec).
    pub accounts: Vec<Zeroizing<String>>,
}

/// The derived encryption key + its salt, kept in the session so accounts can be added
/// (re-encrypting the vault) without re-deriving from the password.
pub struct VaultKey {
    key: Zeroizing<[u8; 32]>,
    salt: [u8; SALT_LEN],
}

/// Whether a vault has been created (drives sign-up vs sign-in in the UI).
pub fn exists() -> Result<bool, CoreError> {
    super::keychain::exists(VAULT_ACCOUNT)
}

/// Create (or overwrite) the vault from `content`, encrypted under `password`. Returns
/// the derived key so the caller can store it in the session for later re-saves.
pub fn create(password: &str, content: &VaultContent) -> Result<VaultKey, CoreError> {
    if password.is_empty() {
        return Err(CoreError::Crypto("password must not be empty".into()));
    }
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let key = derive_key(password, &salt)?;
    let vkey = VaultKey { key, salt };
    write_blob(&vkey, content)?;
    Ok(vkey)
}

/// Unlock the vault with `password`. Returns the decrypted contents + the derived key.
/// A wrong password fails the AEAD tag → `Crypto("wrong password")`.
pub fn unlock(password: &str) -> Result<(VaultContent, VaultKey), CoreError> {
    let encoded = super::keychain::load(VAULT_ACCOUNT)?.ok_or(CoreError::NoWallet)?;
    let blob = hex::decode(encoded.as_bytes())
        .map_err(|_| CoreError::Crypto("corrupt vault blob".into()))?;
    if blob.len() < 4 + 1 + SALT_LEN + NONCE_LEN || &blob[..4] != MAGIC {
        return Err(CoreError::Crypto("unrecognized vault format".into()));
    }
    let version = blob[4];
    if version != 1 && version != VERSION {
        return Err(CoreError::Crypto(format!("unsupported vault version {version}")));
    }
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&blob[5..5 + SALT_LEN]);
    let nonce = &blob[5 + SALT_LEN..5 + SALT_LEN + NONCE_LEN];
    let ct = &blob[5 + SALT_LEN + NONCE_LEN..];

    let key = derive_key(password, &salt)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&*key));
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(Nonce::from_slice(nonce), ct)
            .map_err(|_| CoreError::Crypto("wrong password".into()))?,
    );
    // v1 = single-seed (pre multi-account); v2 = multi-seed. Decode either, then migrate
    // v1 → v2 in place so subsequent saves are uniform.
    let content = if version == 1 {
        deserialize_v1(&plaintext)?
    } else {
        deserialize_content(&plaintext)?
    };
    let vkey = VaultKey { key, salt };
    if version == 1 {
        write_blob(&vkey, &content)?; // upgrade the stored blob to v2
    }
    Ok((content, vkey))
}

/// Re-encrypt the vault with an already-derived key (no password / KDF) — used when
/// adding or importing an account in an unlocked session. Fresh nonce, same salt.
pub fn save(vkey: &VaultKey, content: &VaultContent) -> Result<(), CoreError> {
    write_blob(vkey, content)
}

/// Re-encrypt the vault under a new password (keeps the same contents).
pub fn change_password(old: &str, new: &str) -> Result<(), CoreError> {
    let (content, _) = unlock(old)?;
    create(new, &content)?;
    Ok(())
}

/// Delete the vault (e.g. replace wallet). Irreversible without the recovery phrases.
pub fn delete() -> Result<(), CoreError> {
    super::keychain::delete(VAULT_ACCOUNT)
}

// --- internals ---------------------------------------------------------------------

fn write_blob(vkey: &VaultKey, content: &VaultContent) -> Result<(), CoreError> {
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&*vkey.key));
    let plaintext = serialize_content(content);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|_| CoreError::Crypto("vault encryption failed".into()))?;
    let mut blob = Vec::with_capacity(4 + 1 + SALT_LEN + NONCE_LEN + ct.len());
    blob.extend_from_slice(MAGIC);
    blob.push(VERSION);
    blob.extend_from_slice(&vkey.salt);
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    super::keychain::store(VAULT_ACCOUNT, &hex::encode(&blob))
}

/// Argon2id(password, salt) -> 32-byte key. Interactive-grade parameters (balance UX
/// vs. brute-force cost on a desktop): 64 MiB, 3 passes, 1 lane.
fn derive_key(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, CoreError> {
    let params = Params::new(64 * 1024, 3, 1, Some(32))
        .map_err(|e| CoreError::Crypto(format!("argon2 params: {e}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password.as_bytes(), salt, &mut *key)
        .map_err(|e| CoreError::Crypto(format!("argon2: {e}")))?;
    Ok(key)
}

fn serialize_content(content: &VaultContent) -> Zeroizing<Vec<u8>> {
    let mut v = Vec::new();
    v.push(content.totp_secret.len() as u8);
    v.extend_from_slice(&content.totp_secret);
    v.push(content.accounts.len() as u8);
    for m in &content.accounts {
        let bytes = m.as_bytes();
        v.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        v.extend_from_slice(bytes);
    }
    Zeroizing::new(v)
}

/// v1 plaintext (pre multi-account): `totp_len(1) ‖ totp_secret ‖ mnemonic_utf8` (a single
/// seed, the rest of the buffer). Decoded into the v2 shape with one account.
fn deserialize_v1(p: &[u8]) -> Result<VaultContent, CoreError> {
    let bad = || CoreError::Crypto("malformed v1 vault contents".into());
    let slen = *p.first().ok_or_else(bad)? as usize;
    if slen != super::totp::SECRET_LEN || p.len() < 1 + slen {
        return Err(bad());
    }
    let mut totp_secret = [0u8; super::totp::SECRET_LEN];
    totp_secret.copy_from_slice(&p[1..1 + slen]);
    let mnemonic = String::from_utf8(p[1 + slen..].to_vec())
        .map_err(|_| CoreError::Crypto("vault mnemonic not utf8".into()))?;
    Ok(VaultContent {
        totp_secret,
        accounts: vec![Zeroizing::new(mnemonic)],
    })
}

fn deserialize_content(p: &[u8]) -> Result<VaultContent, CoreError> {
    let mut i = 0usize;
    let bad = || CoreError::Crypto("malformed vault contents".into());
    let slen = *p.get(i).ok_or_else(bad)? as usize;
    i += 1;
    if slen != super::totp::SECRET_LEN || p.len() < i + slen + 1 {
        return Err(bad());
    }
    let mut totp_secret = [0u8; super::totp::SECRET_LEN];
    totp_secret.copy_from_slice(&p[i..i + slen]);
    i += slen;
    let n = p[i] as usize;
    i += 1;
    let mut accounts = Vec::with_capacity(n);
    for _ in 0..n {
        if p.len() < i + 2 {
            return Err(bad());
        }
        let len = u16::from_be_bytes([p[i], p[i + 1]]) as usize;
        i += 2;
        if p.len() < i + len {
            return Err(bad());
        }
        let m = String::from_utf8(p[i..i + len].to_vec())
            .map_err(|_| CoreError::Crypto("vault mnemonic not utf8".into()))?;
        accounts.push(Zeroizing::new(m));
        i += len;
    }
    Ok(VaultContent { totp_secret, accounts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_serialization_roundtrips_multi_seed() {
        let content = VaultContent {
            totp_secret: [7u8; super::super::totp::SECRET_LEN],
            accounts: vec![
                Zeroizing::new("first mnemonic words".to_string()),
                Zeroizing::new("second imported wallet phrase".to_string()),
            ],
        };
        let pt = serialize_content(&content);
        let c = deserialize_content(&pt).unwrap();
        assert_eq!(c.totp_secret, content.totp_secret);
        assert_eq!(c.accounts.len(), 2);
        assert_eq!(&*c.accounts[0], "first mnemonic words");
        assert_eq!(&*c.accounts[1], "second imported wallet phrase");
    }

    #[test]
    fn v1_single_seed_blob_decodes_to_one_account() {
        // Reproduce the v1 plaintext layout: totp_len(1) ‖ totp ‖ mnemonic_utf8.
        let totp = [9u8; super::super::totp::SECRET_LEN];
        let mnemonic = "illness spike retreat truth genius clock brain pass fit cave bargain toe";
        let mut p = Vec::new();
        p.push(totp.len() as u8);
        p.extend_from_slice(&totp);
        p.extend_from_slice(mnemonic.as_bytes());
        let c = deserialize_v1(&p).unwrap();
        assert_eq!(c.totp_secret, totp);
        assert_eq!(c.accounts.len(), 1);
        assert_eq!(&*c.accounts[0], mnemonic);
    }

    #[test]
    fn crypto_roundtrips_and_rejects_wrong_password() {
        let content = VaultContent {
            totp_secret: super::super::totp::generate_secret(),
            accounts: vec![Zeroizing::new(
                "illness spike retreat truth genius clock brain pass fit cave bargain toe".into(),
            )],
        };
        // Encrypt/decrypt directly (no keychain) to test the crypto path.
        let mut salt = [0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        let key = derive_key("correct horse battery staple", &salt).unwrap();
        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&*key));
        let ct = cipher
            .encrypt(Nonce::from_slice(&nonce), serialize_content(&content).as_ref())
            .unwrap();

        let key2 = derive_key("correct horse battery staple", &salt).unwrap();
        let dec = ChaCha20Poly1305::new(Key::from_slice(&*key2))
            .decrypt(Nonce::from_slice(&nonce), ct.as_ref())
            .unwrap();
        assert_eq!(&*deserialize_content(&dec).unwrap().accounts[0], &*content.accounts[0]);

        let bad = derive_key("wrong", &salt).unwrap();
        assert!(ChaCha20Poly1305::new(Key::from_slice(&*bad))
            .decrypt(Nonce::from_slice(&nonce), ct.as_ref())
            .is_err());
    }
}
