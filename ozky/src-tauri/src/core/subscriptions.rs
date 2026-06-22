//! Push subscriptions (roadmap building block C): a wallet-resident scheduler that pays a
//! single recipient on a cadence until an optional end date. NO new circuit/contract — a run
//! is one [`super::send::send_with`] (a 2-in/2-out transfer). This is the honest "subscriptions
//! you pay" direction; merchant-pull ("subscriptions you charge") needs an escrow/channel
//! (building block B) and is out of scope here.
//!
//! Spending needs the unlocked wallet (only the owner can prove a nullifier), so subscriptions
//! run while the app is open: the UI surfaces due subscriptions and the user triggers a run.
//! Schedules are persisted encrypted at rest with the wallet key (same scheme as notes/payroll).

use super::config::PoolConfig;
use super::keys::WalletKeys;
use super::notes::data_dir;
use super::payroll::{next_after, now, Cadence};
use super::{send, CoreError};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use rand_core::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: u64,
    /// What you're paying for (merchant / service name).
    pub label: String,
    /// Asset code (e.g. "USDC").
    pub asset: String,
    /// Recipient shielded payment code (`ozky…`).
    pub code: String,
    /// Amount in base units, paid each cycle.
    pub amount: u64,
    pub cadence: Cadence,
    /// Unix seconds when the next charge is due.
    pub next_run_unix: i64,
    /// Unix seconds of the last successful charge (None if never run).
    pub last_run_unix: Option<i64>,
    /// Unix seconds after which the subscription stops (None = no end).
    pub end_unix: Option<i64>,
    pub enabled: bool,
}

impl Subscription {
    /// True when enabled, the next charge is due, and the schedule hasn't passed the end date.
    pub fn is_due(&self, now_unix: i64) -> bool {
        self.enabled
            && now_unix >= self.next_run_unix
            && self.end_unix.map_or(true, |end| self.next_run_unix <= end)
    }
}

// --- store (encrypted at rest, per wallet) -----------------------------------------

fn store_path(wallet: &WalletKeys) -> PathBuf {
    let digest = Sha256::digest(wallet.stellar_address().as_bytes());
    data_dir().join(format!("subs-{}.enc", hex::encode(&digest[..8])))
}

fn cipher(wallet: &WalletKeys) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(Key::from_slice(&wallet.notes_key()))
}

/// Load all subscriptions for this wallet (empty if no file yet).
pub fn load(wallet: &WalletKeys) -> Result<Vec<Subscription>, CoreError> {
    let path = store_path(wallet);
    let blob = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(CoreError::Crypto(format!("read subscription store: {e}"))),
    };
    if blob.len() < 12 {
        return Err(CoreError::Crypto("subscription store too short".into()));
    }
    let (nonce, ct) = blob.split_at(12);
    let plain = cipher(wallet)
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| CoreError::Crypto("subscription store decrypt failed".into()))?;
    serde_json::from_slice(&plain)
        .map_err(|e| CoreError::Crypto(format!("subscription decode: {e}")))
}

fn save(wallet: &WalletKeys, subs: &[Subscription]) -> Result<(), CoreError> {
    let plain =
        serde_json::to_vec(subs).map_err(|e| CoreError::Crypto(format!("subscription encode: {e}")))?;
    let mut nonce = [0u8; 12];
    rand_core::OsRng.fill_bytes(&mut nonce);
    let ct = cipher(wallet)
        .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
        .map_err(|_| CoreError::Crypto("subscription store encrypt failed".into()))?;
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| CoreError::Crypto(format!("mkdir subs dir: {e}")))?;
    let mut blob = Vec::with_capacity(12 + ct.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    std::fs::write(store_path(wallet), blob)
        .map_err(|e| CoreError::Crypto(format!("write subscription store: {e}")))
}

/// Insert or update a subscription (matched by `id`). A zero `id` gets a fresh one.
pub fn upsert(wallet: &WalletKeys, mut s: Subscription) -> Result<u64, CoreError> {
    let mut list = load(wallet)?;
    if s.id == 0 {
        s.id = now() as u64 * 1000 + (list.len() as u64 + 1); // monotonic-ish unique id
    }
    match list.iter_mut().find(|x| x.id == s.id) {
        Some(slot) => *slot = s.clone(),
        None => list.push(s.clone()),
    }
    save(wallet, &list)?;
    Ok(s.id)
}

pub fn remove(wallet: &WalletKeys, id: u64) -> Result<(), CoreError> {
    let mut list = load(wallet)?;
    list.retain(|s| s.id != id);
    save(wallet, &list)
}

pub fn set_enabled(wallet: &WalletKeys, id: u64, enabled: bool) -> Result<(), CoreError> {
    let mut list = load(wallet)?;
    if let Some(s) = list.iter_mut().find(|s| s.id == id) {
        s.enabled = enabled;
    }
    save(wallet, &list)
}

// --- execution ----------------------------------------------------------------------

/// Charge one subscription now: one shielded transfer to the recipient, then advance the
/// schedule and persist. Returns the tx hash. An error leaves the schedule un-advanced (a
/// retry re-runs the due cycle). If the advanced next run falls past `end_unix`, the
/// subscription is disabled (it has reached its end).
pub fn run(wallet: &WalletKeys, cfg_base: &PoolConfig, id: u64) -> Result<String, CoreError> {
    let mut list = load(wallet)?;
    let idx = list
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(|| CoreError::Crypto("no such subscription".into()))?;
    let sub = list[idx].clone();
    let cfg = cfg_base.with_asset(&sub.asset)?;

    let hash = send::send_with(wallet, &cfg, &sub.code, sub.amount)?;

    // Paid: advance from the later of (due time, now) so a late run doesn't bunch the next
    // cycle. If the new next run is past the end date, the subscription is finished.
    let t = now();
    list[idx].last_run_unix = Some(t);
    let base = sub.next_run_unix.max(t);
    let next = next_after(base, sub.cadence);
    list[idx].next_run_unix = next;
    if sub.end_unix.map_or(false, |end| next > end) {
        list[idx].enabled = false;
    }
    save(wallet, &list)?;
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(cadence: Cadence, next: i64, end: Option<i64>) -> Subscription {
        Subscription {
            id: 1,
            label: "Streaming".into(),
            asset: "USDC".into(),
            code: "ozkyABC".into(),
            amount: 1000,
            cadence,
            next_run_unix: next,
            last_run_unix: None,
            end_unix: end,
            enabled: true,
        }
    }

    #[test]
    fn is_due_respects_enabled_time_and_end() {
        let mut s = sub(Cadence::Monthly, 1000, None);
        assert!(s.is_due(1000));
        assert!(s.is_due(2000));
        assert!(!s.is_due(999));
        s.enabled = false;
        assert!(!s.is_due(5000), "disabled subscription is never due");

        // End date: a charge scheduled after the end is not due.
        let s2 = sub(Cadence::Monthly, 5000, Some(4000));
        assert!(!s2.is_due(6000), "next_run past end date is not due");
        let s3 = sub(Cadence::Monthly, 3000, Some(4000));
        assert!(s3.is_due(3000), "next_run within the end date is due");
    }

    #[test]
    fn store_roundtrips_encrypted() {
        // Shared lock: OZKY_NOTES_DIR is process-global (notes + payroll + subs tests share it).
        let _g = super::super::notes::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("ozky-subs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("OZKY_NOTES_DIR", &dir);
        let wallet = super::super::keys::derive_from_mnemonic(
            "illness spike retreat truth genius clock brain pass fit cave bargain toe",
        )
        .unwrap();

        assert!(load(&wallet).unwrap().is_empty());
        let id = upsert(&wallet, sub(Cadence::Weekly, 1000, None)).unwrap();
        let got = load(&wallet).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, id);
        assert_eq!(got[0].code, "ozkyABC");
        // The file is encrypted (not plaintext JSON).
        let raw = std::fs::read(store_path(&wallet)).unwrap();
        assert!(!raw.windows(7).any(|w| w == b"ozkyABC"), "recipient code must not be cleartext");

        set_enabled(&wallet, id, false).unwrap();
        assert!(!load(&wallet).unwrap()[0].enabled);
        remove(&wallet, id).unwrap();
        assert!(load(&wallet).unwrap().is_empty());
        std::env::remove_var("OZKY_NOTES_DIR");
    }

    /// One-off: a REAL subscription charge on testnet. Creates a weekly subscription paying
    /// 1 XLM to the wallet's own code, runs it, asserts the tx lands, the schedule advanced,
    /// and the self-paid output is discoverable.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture core::subscriptions::tests::subscription_lifecycle_on_testnet
    #[test]
    #[ignore = "live subscription lifecycle; needs network + ozky.config.json + $OZKY_DEPLOY_MNEMONIC"]
    fn subscription_lifecycle_on_testnet() {
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => return,
        };
        let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo.join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", &repo);
        let notes_dir = std::env::temp_dir().join("ozky-subs-live-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);

        let wallet = super::super::keys::derive_from_mnemonic(&mnemonic).unwrap();
        let cfg = PoolConfig::load().unwrap();
        let id_w = super::super::scan::wallet_identity(&wallet).unwrap();
        let code = super::super::send::payment_code(&id_w);
        let one = 10_000_000u64; // 1 XLM

        let id = upsert(
            &wallet,
            Subscription {
                id: 0,
                label: "Live test".into(),
                asset: "XLM".into(),
                code: code.clone(),
                amount: one,
                cadence: Cadence::Weekly,
                next_run_unix: now(),
                last_run_unix: None,
                end_unix: None,
                enabled: true,
            },
        )
        .unwrap();

        let before_next = load(&wallet).unwrap().iter().find(|s| s.id == id).unwrap().next_run_unix;
        let hash = run(&wallet, &cfg, id).expect("subscription charge must succeed");
        eprintln!("subscription charged in tx {hash}");

        let after = load(&wallet).unwrap();
        let s = after.iter().find(|s| s.id == id).unwrap();
        assert!(s.last_run_unix.is_some(), "last_run recorded");
        assert!(s.next_run_unix > before_next, "schedule advanced");

        let st = super::super::chain::pool_state(&cfg.clone().with_asset("XLM").unwrap()).unwrap();
        let notes = super::super::scan::owned_notes(&id_w, &st, &[], 0).unwrap();
        let ones = notes.iter().filter(|n| n.value == one).count();
        assert!(ones >= 1, "expected >=1 one-XLM subscription output, got {ones}");
        println!("SUBSCRIPTION LIFECYCLE OK");
    }
}
