//! Durable transaction history (FEATURE_SET G8). The wallet's **shielded** activity — every flow it
//! initiates (deposit/send/withdraw/split/escrow/channel/payroll/subscription/enroll/disclose) — is
//! recorded here, encrypted at rest with the wallet key (same scheme as the notes/subs/channel
//! stores), so it survives lock/restart. The in-session log in the UI is mirrored into this store.
//!
//! Scope: this records self-initiated actions. Incoming receives (someone sends to your `ozky…`
//! code) show up in the balance immediately; surfacing them as discrete history lines needs
//! output-leaf tracking and is a documented follow-up. The **public** side of history (the funding
//! `G…` account's classic Stellar payments) is served separately from Horizon
//! ([`super::chain::public_payments`]); the two are toggled in the Transactions UI.

use super::keys::WalletKeys;
use super::notes::data_dir;
use super::CoreError;
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use rand_core::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// One recorded shielded action. Mirrors the UI's `Activity` shape so the frontend can render
/// persisted and freshly-logged entries identically. `kind` is a free string (the UI enforces the
/// known set); `ts` is unix milliseconds.
#[derive(Clone, Serialize, Deserialize)]
pub struct ShieldedTx {
    pub id: u64,
    pub kind: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    pub ts: i64,
}

/// Keep the persisted log bounded (newest kept). History is a convenience, not a ledger of record —
/// the chain is canonical — so an old-entry cap avoids unbounded growth.
const MAX_ENTRIES: usize = 500;

fn store_path(wallet: &WalletKeys) -> PathBuf {
    let digest = Sha256::digest(wallet.stellar_address().as_bytes());
    data_dir().join(format!("history-{}.enc", hex::encode(&digest[..8])))
}

fn cipher(wallet: &WalletKeys) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(Key::from_slice(&wallet.notes_key()))
}

/// Load all recorded shielded transactions for this wallet, newest first.
pub fn load(wallet: &WalletKeys) -> Result<Vec<ShieldedTx>, CoreError> {
    let path = store_path(wallet);
    let blob = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(CoreError::Crypto(format!("read history store: {e}"))),
    };
    if blob.len() < 12 {
        return Err(CoreError::Crypto("history store too short".into()));
    }
    let (nonce, ct) = blob.split_at(12);
    let plain = cipher(wallet)
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| CoreError::Crypto("history store decrypt failed".into()))?;
    let mut list: Vec<ShieldedTx> =
        serde_json::from_slice(&plain).map_err(|e| CoreError::Crypto(format!("history decode: {e}")))?;
    list.sort_by(|a, b| b.ts.cmp(&a.ts).then(b.id.cmp(&a.id)));
    Ok(list)
}

fn save(wallet: &WalletKeys, list: &[ShieldedTx]) -> Result<(), CoreError> {
    let plain = serde_json::to_vec(list).map_err(|e| CoreError::Crypto(format!("history encode: {e}")))?;
    let mut nonce = [0u8; 12];
    rand_core::OsRng.fill_bytes(&mut nonce);
    let ct = cipher(wallet)
        .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
        .map_err(|_| CoreError::Crypto("history store encrypt failed".into()))?;
    std::fs::create_dir_all(data_dir()).map_err(|e| CoreError::Crypto(format!("mkdir history dir: {e}")))?;
    let mut blob = Vec::with_capacity(12 + ct.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    std::fs::write(store_path(wallet), blob).map_err(|e| CoreError::Crypto(format!("write history store: {e}")))
}

/// Append one shielded action. A zero `id`/`ts` is filled in (monotonic id, now). Returns the entry.
pub fn record(wallet: &WalletKeys, mut tx: ShieldedTx) -> Result<ShieldedTx, CoreError> {
    let mut list = load(wallet)?;
    if tx.ts == 0 {
        tx.ts = now_ms();
    }
    if tx.id == 0 {
        tx.id = list.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    }
    list.insert(0, tx.clone());
    if list.len() > MAX_ENTRIES {
        list.truncate(MAX_ENTRIES);
    }
    save(wallet, &list)?;
    Ok(tx)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_roundtrips_encrypted_and_orders_newest_first() {
        let _g = super::super::notes::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("ozky-history-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("OZKY_NOTES_DIR", &dir);
        let wallet = super::super::keys::derive_from_mnemonic(
            "illness spike retreat truth genius clock brain pass fit cave bargain toe",
        )
        .unwrap();

        assert!(load(&wallet).unwrap().is_empty());
        record(&wallet, ShieldedTx { id: 0, kind: "send".into(), label: "Sent 10 XLM".into(), detail: None, hash: Some("abc123".into()), ts: 1000 }).unwrap();
        record(&wallet, ShieldedTx { id: 0, kind: "deposit".into(), label: "Deposited 50".into(), detail: None, hash: None, ts: 2000 }).unwrap();

        let got = load(&wallet).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].ts, 2000, "newest first");
        assert_eq!(got[1].kind, "send");
        // Encrypted at rest (the label is not cleartext).
        let raw = std::fs::read(store_path(&wallet)).unwrap();
        assert!(!raw.windows(11).any(|w| w == b"Sent 10 XLM"), "label must not be cleartext");
        std::env::remove_var("OZKY_NOTES_DIR");
    }

    /// Live: the Horizon public-payment parse against the real funding account (`$OZKY_DEPLOY_MNEMONIC`).
    /// The account was funded (friendbot create_account or a payment), so >=1 entry is expected.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --nocapture public_history_live
    #[test]
    #[ignore = "live Horizon read; needs network + $OZKY_DEPLOY_MNEMONIC"]
    fn public_history_live() {
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => return,
        };
        let wallet = super::super::keys::derive_from_mnemonic(&mnemonic).unwrap();
        let txs = super::super::chain::public_payments(wallet.stellar_address()).expect("horizon read");
        eprintln!("public payments for {}: {} entries", wallet.stellar_address(), txs.len());
        for t in txs.iter().take(8) {
            eprintln!("  {} {} {} (cp {:?}) ts={} {}", t.direction, t.amount, t.asset, t.counterparty, t.ts, &t.hash[..8.min(t.hash.len())]);
        }
        assert!(!txs.is_empty(), "a funded account has >=1 public payment");
        assert!(txs.iter().all(|t| t.ts > 0), "timestamps parse");
        println!("PUBLIC HISTORY LIVE OK ({} entries)", txs.len());
    }
}
