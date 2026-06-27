//! Local notes store (Phase A3.3). Persists note openings the wallet CANNOT
//! rediscover by scanning the chain — specifically the **withdraw change note**, for
//! which the pool publishes no on-chain ciphertext. Without this, a withdraw's change
//! is recoverable only from the in-memory `WithdrawReceipt` and is lost on restart.
//!
//! A stored record is just the note opening ([`NotePlaintext`] = value / asset_tag /
//! blinding / epoch / rho, 108 bytes). The note's leaf index and spent-status are NOT
//! stored — they are resolved against live chain state at query time (see
//! [`super::scan::owned_notes`]), so the store never goes stale relative to the chain.
//!
//! The file holds spending secrets, so it is encrypted at rest with a wallet-derived
//! key (key-committing ChaCha20-Poly1305): `nonce(12) ‖ AEAD(concat of 108-byte
//! records)`. One file per wallet, named by a hash of its public Stellar address.

use super::encrypt::NotePlaintext;
use super::keys::WalletKeys;
use super::CoreError;
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use rand_core::RngCore;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const RECORD_LEN: usize = 108; // NotePlaintext serialized length.

/// Shared lock serializing every test that mutates the process-global `OZKY_NOTES_DIR`
/// (notes + payroll stores both key off `data_dir()`), so concurrent tests don't clobber
/// each other's directory.
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// App data directory. `OZKY_NOTES_DIR` overrides (set by the command layer to Tauri's
/// app-data dir, or by tests to a temp dir); else a platform default. Shared by the
/// encrypted notes store and the (plaintext) per-pool scan cache ([`super::chain`]).
pub(crate) fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("OZKY_NOTES_DIR") {
        return PathBuf::from(d);
    }
    let base = std::env::var("APPDATA")
        .ok()
        .or_else(|| std::env::var("XDG_DATA_HOME").ok())
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.local/share")));
    PathBuf::from(base.unwrap_or_else(|| ".".into())).join("ozky")
}

/// Remove every per-wallet encrypted data file (`*.enc`: notes, payroll, history,
/// escrow, channel, keeper stores) in the data dir. Used by logout to wipe the device.
/// Best-effort — a missing dir or unremovable file is ignored.
pub(crate) fn wipe_data_files() {
    let Ok(entries) = std::fs::read_dir(data_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("enc") {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// `notes-<16 hex of sha256(stellar_address)>.enc` — keyed by the public address so
/// multiple wallets on one machine don't collide. The address is public (not secret).
fn store_path(wallet: &WalletKeys) -> PathBuf {
    let digest = Sha256::digest(wallet.stellar_address().as_bytes());
    data_dir().join(format!("notes-{}.enc", hex::encode(&digest[..8])))
}

fn cipher(wallet: &WalletKeys) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(Key::from_slice(&wallet.notes_key()))
}

/// Load all stored note openings for this wallet (empty if no file yet).
pub fn load(wallet: &WalletKeys) -> Result<Vec<NotePlaintext>, CoreError> {
    let path = store_path(wallet);
    let blob = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(CoreError::Crypto(format!("read notes store: {e}"))),
    };
    if blob.len() < 12 {
        return Err(CoreError::Crypto("notes store too short".into()));
    }
    let (nonce, ct) = blob.split_at(12);
    let plain = cipher(wallet)
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| CoreError::Crypto("notes store decrypt failed".into()))?;
    if plain.len() % RECORD_LEN != 0 {
        return Err(CoreError::Crypto("corrupt notes store (bad length)".into()));
    }
    plain
        .chunks_exact(RECORD_LEN)
        .map(NotePlaintext::deserialize)
        .collect()
}

/// Persist the full note set (encrypted, atomic-ish overwrite).
fn save(wallet: &WalletKeys, notes: &[NotePlaintext]) -> Result<(), CoreError> {
    let mut plain = Vec::with_capacity(notes.len() * RECORD_LEN);
    for n in notes {
        plain.extend_from_slice(&n.serialize());
    }
    let mut nonce = [0u8; 12];
    rand_core::OsRng.fill_bytes(&mut nonce);
    let ct = cipher(wallet)
        .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
        .map_err(|_| CoreError::Crypto("notes store encrypt failed".into()))?;

    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| CoreError::Crypto(format!("mkdir notes dir: {e}")))?;
    let mut blob = Vec::with_capacity(12 + ct.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    std::fs::write(store_path(wallet), blob)
        .map_err(|e| CoreError::Crypto(format!("write notes store: {e}")))
}

/// True if `a` and `b` are the same note opening (a note is identified by its secret
/// fields; two distinct notes never share `rho`, so this is a safe de-dup key).
fn same(a: &NotePlaintext, b: &NotePlaintext) -> bool {
    a.value == b.value
        && a.asset_tag == b.asset_tag
        && a.blinding == b.blinding
        && a.epoch == b.epoch
        && a.rho == b.rho
}

/// Add a note opening to the store (idempotent — a duplicate opening is ignored).
pub fn add(wallet: &WalletKeys, note: NotePlaintext) -> Result<(), CoreError> {
    let mut notes = load(wallet)?;
    if notes.iter().any(|n| same(n, &note)) {
        return Ok(());
    }
    notes.push(note);
    save(wallet, &notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::keys;
    use crate::core::poseidon::Fr;

    use std::sync::atomic::{AtomicU64, Ordering};

    const MNEMONIC: &str =
        "illness spike retreat truth genius clock brain pass fit cave bargain toe";

    // `OZKY_NOTES_DIR` is a process-global env var; serialize the notes tests (which
    // run as threads in one process) so they don't clobber each other's dir. Each test
    // also gets a unique dir, so even the serialized runs never share a store file.
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Hold the env lock for the test's lifetime + point `OZKY_NOTES_DIR` at a fresh
    /// unique dir. Returns the guard (kept alive by the caller).
    fn isolated_env() -> std::sync::MutexGuard<'static, ()> {
        let guard = super::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("ozky-notes-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::env::set_var("OZKY_NOTES_DIR", &d);
        guard
    }

    fn sample(rho: u64) -> NotePlaintext {
        NotePlaintext {
            value: 200,
            asset_tag: Fr::from_u64(1),
            blinding: Fr::from_u64(444),
            epoch: 28,
            rho: Fr::from_u64(rho),
        }
    }

    #[test]
    fn add_then_load_roundtrips_and_dedups() {
        let _env = isolated_env();
        let wallet = keys::derive_from_mnemonic(MNEMONIC).unwrap();

        assert!(load(&wallet).unwrap().is_empty(), "empty before any add");
        add(&wallet, sample(111)).unwrap();
        add(&wallet, sample(222)).unwrap();
        add(&wallet, sample(111)).unwrap(); // duplicate -> ignored

        let notes = load(&wallet).unwrap();
        assert_eq!(notes.len(), 2, "two distinct notes, duplicate de-duped");
        assert_eq!(notes[0].value, 200);
        assert!(notes.iter().any(|n| n.rho == Fr::from_u64(222)));
    }

    #[test]
    fn store_is_encrypted_at_rest() {
        let _env = isolated_env();
        let wallet = keys::derive_from_mnemonic(MNEMONIC).unwrap();
        add(&wallet, sample(0xfeed)).unwrap();
        let raw = std::fs::read(store_path(&wallet)).unwrap();
        // The blinding (444 = 0x01bc) must not appear in cleartext on disk.
        assert!(!raw.windows(2).any(|w| w == [0x01, 0xbc]), "opening must be encrypted");
        assert!(raw.len() > 12, "nonce + ciphertext present");
    }
}
