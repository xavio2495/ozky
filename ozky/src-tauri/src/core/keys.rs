//! Key management (Phase A1). A single 12-word BIP39 phrase derives BOTH the
//! Stellar Ed25519 account key (SEP-0005 / SLIP-0010, path `m/44'/148'/account'`)
//! AND a distinct BN254-native ZK spending key (`owner_sk`) + a BIP32-style
//! viewing-key hierarchy scoped by `account / asset / epoch`.
//!
//! The ZK keys are derived from, **never equal to**, the Stellar key: `owner_sk`
//! comes from a domain-separated HMAC of the seed (not the ed25519 key) and is
//! reduced into the BN254 scalar field, so it is a valid in-circuit `Field` and is
//! unlinked from the on-chain Ed25519 account. `owner_pk = Poseidon(DOMAIN_OWNER,
//! owner_sk)` (the circuit's formula) is computed in [`super::proving`] where the
//! circuit-matching Poseidon lives (A2); A1 owns the secret derivation.

use super::CoreError;
use bip39::Mnemonic;
use hmac::{Hmac, Mac};
use num_bigint::BigUint;
use sha2::{Sha256, Sha512};
use zeroize::Zeroizing;

type HmacSha512 = Hmac<Sha512>;
type HmacSha256 = Hmac<Sha256>;

/// The keychain account name under which the wallet mnemonic is stored.
pub const SEED_ACCOUNT: &str = "seed";

/// BN254 scalar field modulus r (decimal), the order of the Fr field `owner_sk`
/// lives in — the same field the Noir circuits operate over.
const BN254_FR_MODULUS_DEC: &[u8] =
    b"21888242871839275222246405745257275088548364400416034343698204186575808495617";

/// Derived key material for the wallet. Secret-bearing; never serialized to the UI.
pub struct WalletKeys {
    /// BIP39 seed (kept only in memory; the mnemonic is the keychain secret).
    seed: Zeroizing<[u8; 64]>,
    /// Stellar Ed25519 account public address (G...).
    pub stellar_address: String,
    /// Stellar Ed25519 secret seed (S...). Backup/sign use only.
    stellar_secret: Zeroizing<String>,
    /// BN254 `owner_sk` (32-byte big-endian, < r). The in-circuit spending key.
    owner_sk: Zeroizing<[u8; 32]>,
    /// Master viewing secret (root of the view-key hierarchy).
    view_master: Zeroizing<[u8; 32]>,
}

/// A scoped viewing key pair (account / asset / epoch) for selective disclosure.
pub struct ScopedViewKey {
    pub viewing: [u8; 32],
    pub detection: [u8; 32],
}

impl WalletKeys {
    pub fn stellar_address(&self) -> &str {
        &self.stellar_address
    }
    /// `owner_sk` as 0x-prefixed big-endian hex (a BN254 field element).
    pub fn owner_sk_hex(&self) -> String {
        format!("0x{}", hex::encode(&*self.owner_sk))
    }
    pub fn stellar_secret(&self) -> &str {
        &self.stellar_secret
    }
    /// Derive the viewing + detection keys for a disclosure scope.
    pub fn scoped_view_key(&self, account: u32, asset_tag: u32, epoch: u32) -> ScopedViewKey {
        let a = child(&self.view_master, b"account", account);
        let s = child(&a, b"asset", asset_tag);
        let e = child(&s, b"epoch", epoch);
        ScopedViewKey {
            viewing: child(&e, b"ivk", 0),
            detection: child(&e, b"dtk", 0),
        }
    }
}

/// Generate a fresh 12-word BIP39 mnemonic.
pub fn generate_mnemonic() -> Result<String, CoreError> {
    let m = Mnemonic::generate(12)
        .map_err(|e| CoreError::Keychain(format!("mnemonic gen: {e}")))?;
    Ok(m.to_string())
}

/// Derive all keys from a 12-word phrase (empty BIP39 passphrase, per SEP-0005).
pub fn derive_from_mnemonic(phrase: &str) -> Result<WalletKeys, CoreError> {
    let mnemonic = Mnemonic::parse_normalized(phrase.trim())
        .map_err(|e| CoreError::Keychain(format!("invalid mnemonic: {e}")))?;
    let seed = Zeroizing::new(mnemonic.to_seed(""));

    // --- Stellar Ed25519 (SEP-0005: SLIP-0010 ed25519, m/44'/148'/0') ---
    let ed_seed = stellar_ed25519_seed(&seed, 0);
    let signing = ed25519_dalek::SigningKey::from_bytes(&ed_seed);
    let public = signing.verifying_key().to_bytes();
    let stellar_address = stellar_strkey::ed25519::PublicKey(public).to_string();
    let stellar_secret = Zeroizing::new(stellar_strkey::ed25519::PrivateKey(ed_seed).to_string());

    // --- BN254 owner_sk (distinct domain off the SEED, reduced into Fr) ---
    let owner_sk = derive_owner_sk(&seed);

    // --- Viewing-key hierarchy root ---
    let view_master = Zeroizing::new(hmac32(b"ozky-view-master-v1", &*seed));

    Ok(WalletKeys {
        seed,
        stellar_address,
        stellar_secret,
        owner_sk: Zeroizing::new(owner_sk),
        view_master,
    })
}

/// Load the stored mnemonic from the OS keychain and derive the wallet's keys.
pub fn current_wallet() -> Result<WalletKeys, CoreError> {
    let phrase = super::keychain::load(SEED_ACCOUNT)?.ok_or(CoreError::NoWallet)?;
    derive_from_mnemonic(&phrase)
}

// ----------------------------- derivation internals -----------------------------

/// `owner_sk = (HMAC-SHA512("ozky-bn254-spend-v1", seed)) mod r`, as 32-byte BE.
fn derive_owner_sk(seed: &[u8; 64]) -> [u8; 32] {
    let wide = hmac64(b"ozky-bn254-spend-v1", seed);
    let r = BigUint::parse_bytes(BN254_FR_MODULUS_DEC, 10).expect("valid modulus");
    let reduced = BigUint::from_bytes_be(&wide) % &r;
    let mut out = [0u8; 32];
    let be = reduced.to_bytes_be();
    out[32 - be.len()..].copy_from_slice(&be);
    out
}

/// SLIP-0010 ed25519 master key from the seed: (key, chain_code).
fn slip10_master(seed: &[u8]) -> ([u8; 32], [u8; 32]) {
    let i = hmac64(b"ed25519 seed", seed);
    split32(&i)
}

/// SLIP-0010 ed25519 hardened child derivation (ed25519 supports hardened only).
fn slip10_ckd(key: &[u8; 32], chain_code: &[u8; 32], index: u32) -> ([u8; 32], [u8; 32]) {
    let hardened = index | 0x8000_0000;
    let mut mac = HmacSha512::new_from_slice(chain_code).expect("hmac key");
    mac.update(&[0u8]);
    mac.update(key);
    mac.update(&hardened.to_be_bytes());
    split32(&mac.finalize().into_bytes())
}

/// Stellar SEP-0005 derivation: m/44'/148'/account'. Returns the 32-byte ed25519 seed.
fn stellar_ed25519_seed(seed: &[u8; 64], account: u32) -> [u8; 32] {
    let (mut key, mut cc) = slip10_master(seed);
    for index in [44u32, 148u32, account] {
        let (k, c) = slip10_ckd(&key, &cc, index);
        key = k;
        cc = c;
    }
    key
}

/// BIP32-style scoped child: HMAC-SHA256(parent, label || index_be)[..32].
fn child(parent: &[u8; 32], label: &[u8], index: u32) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(parent).expect("hmac key");
    mac.update(label);
    mac.update(&index.to_be_bytes());
    let out = mac.finalize().into_bytes();
    let mut k = [0u8; 32];
    k.copy_from_slice(&out);
    k
}

fn hmac64(key: &[u8], data: &[u8]) -> [u8; 64] {
    let mut mac = HmacSha512::new_from_slice(key).expect("hmac key");
    mac.update(data);
    let mut out = [0u8; 64];
    out.copy_from_slice(&mac.finalize().into_bytes());
    out
}

fn hmac32(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac key");
    mac.update(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&mac.finalize().into_bytes());
    out
}

fn split32(i: &[u8]) -> ([u8; 32], [u8; 32]) {
    let mut a = [0u8; 32];
    let mut b = [0u8; 32];
    a.copy_from_slice(&i[..32]);
    b.copy_from_slice(&i[32..64]);
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    // SEP-0005 official test vector 1 (no passphrase), path m/44'/148'/0'.
    const SEP5_MNEMONIC: &str =
        "illness spike retreat truth genius clock brain pass fit cave bargain toe";
    const SEP5_ADDR_0: &str = "GDRXE2BQUC3AZNPVFSCEZ76NJ3WWL25FYFK6RGZGIEKWE4SOOHSUJUJ6";
    const SEP5_SECRET_0: &str = "SBGWSG6BTNCKCOB3DIFBGCVMUPQFYPA2G4O34RMTB343OYPXU5DJDVMN";

    #[test]
    fn stellar_key_matches_sep0005_vector() {
        let k = derive_from_mnemonic(SEP5_MNEMONIC).unwrap();
        assert_eq!(k.stellar_address(), SEP5_ADDR_0, "SEP-0005 G-address");
        assert_eq!(k.stellar_secret(), SEP5_SECRET_0, "SEP-0005 S-secret");
    }

    #[test]
    fn create_and_restore_reproduce_identical_keys() {
        // "create" = generate a phrase, derive once.
        let phrase = generate_mnemonic().unwrap();
        let a = derive_from_mnemonic(&phrase).unwrap();
        // "restore" = derive again from the same phrase.
        let b = derive_from_mnemonic(&phrase).unwrap();

        assert_eq!(a.stellar_address(), b.stellar_address());
        assert_eq!(a.owner_sk_hex(), b.owner_sk_hex());
        let va = a.scoped_view_key(0, 1, 28);
        let vb = b.scoped_view_key(0, 1, 28);
        assert_eq!(va.viewing, vb.viewing);
        assert_eq!(va.detection, vb.detection);
    }

    #[test]
    fn owner_sk_is_in_field_and_distinct_from_stellar() {
        let k = derive_from_mnemonic(SEP5_MNEMONIC).unwrap();
        // owner_sk < r (reduced into the field).
        let r = BigUint::parse_bytes(BN254_FR_MODULUS_DEC, 10).unwrap();
        let sk = BigUint::from_bytes_be(&*k.owner_sk);
        assert!(sk < r, "owner_sk must be a valid BN254 Fr element");
        // owner_sk must not equal the ed25519 seed (never reuse the Stellar key).
        let ed = stellar_ed25519_seed(&k.seed, 0);
        assert_ne!(&*k.owner_sk, &ed, "owner_sk must be distinct from the Stellar key");
    }

    #[test]
    fn scoped_view_keys_are_scope_separated() {
        let k = derive_from_mnemonic(SEP5_MNEMONIC).unwrap();
        let e28 = k.scoped_view_key(0, 1, 28);
        let e29 = k.scoped_view_key(0, 1, 29);
        let a1 = k.scoped_view_key(1, 1, 28);
        assert_ne!(e28.viewing, e29.viewing, "different epoch -> different key");
        assert_ne!(e28.viewing, a1.viewing, "different account -> different key");
        assert_ne!(e28.viewing, e28.detection, "viewing != detection");
    }
}
