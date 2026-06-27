//! Recurring payroll (roadmap building block C): a wallet-resident scheduler that pays a
//! saved group of payees on a cadence. NO new circuit/contract — a run is just `ceil(N/5)`
//! [`super::send::split_with`] transactions (split caps at 5 recipients). The schedule is
//! persisted encrypted at rest with the wallet key (same scheme as the notes store).
//!
//! Spending needs the unlocked wallet (only the owner can prove a nullifier), so payroll
//! runs while the app is open: the UI surfaces due payrolls and the user triggers a run.

use super::config::PoolConfig;
use super::keys::WalletKeys;
use super::notes::data_dir;
use super::send;
use super::CoreError;
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use rand_core::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct Payee {
    /// Recipient shielded payment code (`ozky…`).
    pub code: String,
    /// Amount in base units.
    pub amount: u64,
    /// Asset the recipient receives. `None` (or equal to the payroll asset) = a same-asset payment
    /// bundled into a split; a different code = a cross-asset `pay` (then `amount` is the
    /// DESTINATION amount). `#[serde(default)]` so existing encrypted stores deserialize unchanged.
    #[serde(default)]
    pub recv_asset: Option<String>,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "days")]
pub enum Cadence {
    Weekly,
    Monthly,
    EveryDays(u32),
}

/// One funding group of a payroll: a set of payees all paid FROM `asset`. A multi-token
/// payroll holds one group per funding asset (each a tab in the UI). A run pays each group
/// independently (its own [`super::send::multi_send_with`]).
#[derive(Clone, Serialize, Deserialize)]
pub struct PayGroup {
    /// Funding asset code (e.g. "USDC").
    pub asset: String,
    pub payees: Vec<Payee>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Payroll {
    pub id: u64,
    pub label: String,
    /// One group per funding asset. (Canonical since multi-token; legacy single-asset stores
    /// migrate into a single group on load — see [`Payroll::migrate`].)
    #[serde(default)]
    pub groups: Vec<PayGroup>,
    /// LEGACY single-asset fields — present only to deserialize pre-multi-token encrypted
    /// stores; migrated into `groups` on load and never written back.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asset: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub payees: Vec<Payee>,
    pub cadence: Cadence,
    /// Unix seconds when the next run is due.
    pub next_run_unix: i64,
    /// Unix seconds of the last successful run (None if never run).
    pub last_run_unix: Option<i64>,
    /// Unix seconds after which the payroll stops (None = no end).
    #[serde(default)]
    pub end_unix: Option<i64>,
    /// Stellar `G…` auditor address. When set, a selective-disclosure grant for the current
    /// epoch is recorded to it after each successful run.
    #[serde(default)]
    pub auditor: Option<String>,
    /// How a due run is approved: `"auto"` = the headless keeper submits when due; `"manual"`
    /// = notify only, the user runs it. `None`/empty defaults to `"manual"`.
    #[serde(default)]
    pub approval: Option<String>,
    /// Where a headless run executes: `"local"` = an OS-scheduled task on this machine;
    /// `"cloud"` = the ozky cloud keeper. `None`/empty defaults to `"local"`.
    #[serde(default)]
    pub run_location: Option<String>,
    pub enabled: bool,
}

impl Payroll {
    /// Fold a legacy single-asset payroll into the `groups` model in place. Idempotent.
    fn migrate(&mut self) {
        if self.groups.is_empty() && !self.payees.is_empty() {
            self.groups = vec![PayGroup {
                asset: std::mem::take(&mut self.asset),
                payees: std::mem::take(&mut self.payees),
            }];
        }
        self.asset.clear();
        self.payees.clear();
    }
    pub fn total(&self) -> u64 {
        self.groups.iter().flat_map(|g| &g.payees).map(|p| p.amount).sum()
    }
    pub fn payee_count(&self) -> usize {
        self.groups.iter().map(|g| g.payees.len()).sum()
    }
    pub fn is_due(&self, now_unix: i64) -> bool {
        self.enabled
            && now_unix >= self.next_run_unix
            && self.end_unix.map_or(true, |end| self.next_run_unix <= end)
    }
    /// Advance `next_run_unix` by one cadence period from `from` (its current next_run).
    pub fn advance_from(&mut self, from: i64) {
        self.next_run_unix = next_after(from, self.cadence);
    }
}

/// Current wall-clock unix seconds.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The next run time after `from` for `cadence`. Weekly/EveryDays are fixed offsets;
/// Monthly adds one calendar month (clamping the day to the new month's length).
pub fn next_after(from: i64, cadence: Cadence) -> i64 {
    match cadence {
        Cadence::Weekly => from + 7 * 86_400,
        Cadence::EveryDays(n) => from + (n.max(1) as i64) * 86_400,
        Cadence::Monthly => add_one_month(from),
    }
}

// --- store (encrypted at rest, per wallet) -----------------------------------------

fn store_path(wallet: &WalletKeys) -> PathBuf {
    let digest = Sha256::digest(wallet.stellar_address().as_bytes());
    data_dir().join(format!("payroll-{}.enc", hex::encode(&digest[..8])))
}

fn cipher(wallet: &WalletKeys) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(Key::from_slice(&wallet.notes_key()))
}

/// Load all payrolls for this wallet (empty if no file yet).
pub fn load(wallet: &WalletKeys) -> Result<Vec<Payroll>, CoreError> {
    let path = store_path(wallet);
    let blob = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(CoreError::Crypto(format!("read payroll store: {e}"))),
    };
    if blob.len() < 12 {
        return Err(CoreError::Crypto("payroll store too short".into()));
    }
    let (nonce, ct) = blob.split_at(12);
    let plain = cipher(wallet)
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| CoreError::Crypto("payroll store decrypt failed".into()))?;
    let mut list: Vec<Payroll> = serde_json::from_slice(&plain)
        .map_err(|e| CoreError::Crypto(format!("payroll decode: {e}")))?;
    for p in &mut list {
        p.migrate();
    }
    Ok(list)
}

fn save(wallet: &WalletKeys, payrolls: &[Payroll]) -> Result<(), CoreError> {
    let plain =
        serde_json::to_vec(payrolls).map_err(|e| CoreError::Crypto(format!("payroll encode: {e}")))?;
    let mut nonce = [0u8; 12];
    rand_core::OsRng.fill_bytes(&mut nonce);
    let ct = cipher(wallet)
        .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
        .map_err(|_| CoreError::Crypto("payroll store encrypt failed".into()))?;
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| CoreError::Crypto(format!("mkdir payroll dir: {e}")))?;
    let mut blob = Vec::with_capacity(12 + ct.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    std::fs::write(store_path(wallet), blob)
        .map_err(|e| CoreError::Crypto(format!("write payroll store: {e}")))
}

/// Insert or update a payroll (matched by `id`). A zero `id` gets a fresh one.
pub fn upsert(wallet: &WalletKeys, mut p: Payroll) -> Result<u64, CoreError> {
    let mut list = load(wallet)?;
    if p.id == 0 {
        p.id = now() as u64 * 1000 + (list.len() as u64 + 1); // monotonic-ish unique id
    }
    match list.iter_mut().find(|x| x.id == p.id) {
        Some(slot) => *slot = p.clone(),
        None => list.push(p.clone()),
    }
    save(wallet, &list)?;
    Ok(p.id)
}

pub fn remove(wallet: &WalletKeys, id: u64) -> Result<(), CoreError> {
    let mut list = load(wallet)?;
    list.retain(|p| p.id != id);
    save(wallet, &list)
}

pub fn set_enabled(wallet: &WalletKeys, id: u64, enabled: bool) -> Result<(), CoreError> {
    let mut list = load(wallet)?;
    if let Some(p) = list.iter_mut().find(|p| p.id == id) {
        p.enabled = enabled;
    }
    save(wallet, &list)
}

// --- execution ----------------------------------------------------------------------

/// Run one payroll now: pay all payees via `ceil(N/5)` sequential split transactions,
/// then advance the schedule and persist. Returns the tx hashes. Errors leave the
/// schedule un-advanced (a retry re-runs the whole due cycle).
pub fn run(wallet: &WalletKeys, cfg_base: &PoolConfig, id: u64) -> Result<Vec<String>, CoreError> {
    let mut list = load(wallet)?;
    let idx = list
        .iter()
        .position(|p| p.id == id)
        .ok_or_else(|| CoreError::Crypto("no such payroll".into()))?;
    let payroll = list[idx].clone();
    if payroll.groups.iter().all(|g| g.payees.is_empty()) {
        return Err(CoreError::Crypto("payroll has no payees".into()));
    }
    // Pay each funding group independently: same-asset payees bundle into split txs,
    // cross-asset payees are individual `pay` txs (all funded from the group's asset).
    let mut hashes = Vec::new();
    for group in &payroll.groups {
        if group.payees.is_empty() {
            continue;
        }
        let recipients: Vec<send::MultiRecipient> = group
            .payees
            .iter()
            .map(|p| send::MultiRecipient {
                code: p.code.clone(),
                amount: p.amount,
                recv_asset: p.recv_asset.clone(),
            })
            .collect();
        hashes.extend(send::multi_send_with(
            wallet,
            cfg_base,
            &group.asset,
            &recipients,
            send::MULTI_SEND_SLIPPAGE_BPS,
        )?);
    }

    // Auditor disclosure: if an auditor is configured, record a selective-disclosure grant
    // for the current epoch (the one this run's outputs land in). Best-effort — the payment
    // already settled, so a disclosure failure must not fail the run.
    if let Some(auditor) = payroll.auditor.as_deref().filter(|a| !a.is_empty()) {
        if let Ok(epoch) = super::chain::current_epoch(&cfg_base.rpc_url) {
            if let Err(e) =
                super::disclose::share_with_auditor_with(wallet, cfg_base, auditor, epoch, epoch)
            {
                eprintln!("[ozky-payroll] auditor disclosure after run failed: {e}");
            }
        }
    }

    // All groups paid: advance the schedule from the later of (due time, now) so a
    // late run doesn't bunch the next period. If the new next run is past the end date,
    // the payroll is finished (disabled). Then persist.
    let t = now();
    list[idx].last_run_unix = Some(t);
    let base = payroll.next_run_unix.max(t);
    let next = next_after(base, payroll.cadence);
    list[idx].next_run_unix = next;
    if payroll.end_unix.map_or(false, |end| next > end) {
        list[idx].enabled = false;
    }
    save(wallet, &list)?;
    Ok(hashes)
}

// --- monthly date math (self-contained; no chrono) ----------------------------------

/// Add one calendar month to a unix timestamp, clamping the day to the target month's
/// length (e.g. Jan 31 -> Feb 28/29). Keeps the time-of-day.
fn add_one_month(unix: i64) -> i64 {
    let days = unix.div_euclid(86_400);
    let secs_of_day = unix.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let nd = d.min(days_in_month(ny, nm));
    days_from_civil(ny, nm, nd) * 86_400 + secs_of_day
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if is_leap(y) { 29 } else { 28 },
        _ => 30,
    }
}

/// Howard Hinnant's days_from_civil (proleptic Gregorian, days since 1970-01-01).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) as i64 + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of [`days_from_civil`]: (year, month, day) from days since epoch.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payroll(cadence: Cadence, next: i64) -> Payroll {
        Payroll {
            id: 1,
            label: "Team".into(),
            groups: vec![PayGroup {
                asset: "USDC".into(),
                payees: vec![Payee { code: "ozkyA".into(), amount: 100, recv_asset: None }],
            }],
            asset: String::new(),
            payees: Vec::new(),
            cadence,
            next_run_unix: next,
            last_run_unix: None,
            end_unix: None,
            auditor: None,
            approval: None,
            run_location: None,
            enabled: true,
        }
    }

    #[test]
    fn weekly_and_every_days_offsets() {
        assert_eq!(next_after(0, Cadence::Weekly), 7 * 86_400);
        assert_eq!(next_after(1000, Cadence::EveryDays(3)), 1000 + 3 * 86_400);
        assert_eq!(next_after(0, Cadence::EveryDays(0)), 86_400, "0 days clamps to 1");
    }

    #[test]
    fn monthly_adds_a_calendar_month() {
        // 2024-01-15 00:00 UTC = 1705276800.
        let jan15 = days_from_civil(2024, 1, 15) * 86_400;
        let feb15 = days_from_civil(2024, 2, 15) * 86_400;
        assert_eq!(next_after(jan15, Cadence::Monthly), feb15);
    }

    #[test]
    fn monthly_clamps_month_end() {
        // 2024-01-31 -> 2024-02-29 (2024 is a leap year).
        let jan31 = days_from_civil(2024, 1, 31) * 86_400;
        let feb29 = days_from_civil(2024, 2, 29) * 86_400;
        assert_eq!(next_after(jan31, Cadence::Monthly), feb29);
        // Non-leap: 2023-01-31 -> 2023-02-28.
        let j31 = days_from_civil(2023, 1, 31) * 86_400;
        let f28 = days_from_civil(2023, 2, 28) * 86_400;
        assert_eq!(next_after(j31, Cadence::Monthly), f28);
    }

    #[test]
    fn civil_roundtrip() {
        for &(y, m, d) in &[(1970, 1, 1), (2024, 2, 29), (1999, 12, 31), (2026, 6, 22)] {
            let days = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(days), (y, m, d));
        }
    }

    #[test]
    fn is_due_respects_enabled_and_time() {
        let mut p = payroll(Cadence::Weekly, 1000);
        assert!(p.is_due(1000));
        assert!(p.is_due(2000));
        assert!(!p.is_due(999));
        p.enabled = false;
        assert!(!p.is_due(5000), "disabled payroll is never due");
    }

    /// One-off: a REAL payroll run on testnet. Creates a 6-payee payroll (1 XLM each to
    /// the wallet's own code -> 2 split txs), runs it, asserts both txs land, the schedule
    /// advanced, and the 6 self-paid outputs are discoverable.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture core::payroll::tests::payroll_lifecycle_on_testnet
    #[test]
    #[ignore = "live payroll lifecycle; needs network + ozky.config.json + $OZKY_DEPLOY_MNEMONIC"]
    fn payroll_lifecycle_on_testnet() {
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => return,
        };
        let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo.join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", &repo);
        let notes_dir = std::env::temp_dir().join("ozky-payroll-live-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);

        let wallet = super::super::keys::derive_from_mnemonic(&mnemonic).unwrap();
        let cfg = PoolConfig::load().unwrap();
        let id_w = super::super::scan::wallet_identity(&wallet).unwrap();
        let code = super::super::send::payment_code(&id_w);
        let one = 10_000_000u64; // 1 XLM

        // Create a payroll: 6 payees x 1 XLM to self -> 2 split txs (5 + 1).
        let payees: Vec<Payee> =
            (0..6).map(|_| Payee { code: code.clone(), amount: one, recv_asset: None }).collect();
        let id = upsert(
            &wallet,
            Payroll {
                id: 0,
                label: "Live test".into(),
                groups: vec![PayGroup { asset: "XLM".into(), payees }],
                asset: String::new(),
                payees: Vec::new(),
                cadence: Cadence::Weekly,
                next_run_unix: now(),
                last_run_unix: None,
                end_unix: None,
                auditor: None,
                approval: None,
                run_location: None,
                enabled: true,
            },
        )
        .unwrap();

        let before_next = load(&wallet).unwrap().iter().find(|p| p.id == id).unwrap().next_run_unix;
        let hashes = run(&wallet, &cfg, id).expect("payroll run must succeed");
        eprintln!("payroll paid in {} txs: {:?}", hashes.len(), hashes);
        assert_eq!(hashes.len(), 2, "6 payees -> ceil(6/5) = 2 split txs");

        let after = load(&wallet).unwrap();
        let p = after.iter().find(|p| p.id == id).unwrap();
        assert!(p.last_run_unix.is_some(), "last_run recorded");
        assert!(p.next_run_unix > before_next, "schedule advanced");

        // The 6 self-paid 1-XLM outputs are discoverable.
        let st = super::super::chain::pool_state(&cfg).unwrap();
        let notes = super::super::scan::owned_notes(&id_w, &st, &[], 0).unwrap();
        let ones = notes.iter().filter(|n| n.value == one).count();
        assert!(ones >= 6, "expected >=6 one-XLM payroll outputs, got {ones}");
        println!("PAYROLL LIFECYCLE OK");
    }

    #[test]
    fn store_roundtrips_encrypted() {
        // Shared lock: OZKY_NOTES_DIR is process-global (notes + payroll tests share it).
        let _g = super::super::notes::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("ozky-payroll-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("OZKY_NOTES_DIR", &dir);
        let wallet = super::super::keys::derive_from_mnemonic(
            "illness spike retreat truth genius clock brain pass fit cave bargain toe",
        )
        .unwrap();

        assert!(load(&wallet).unwrap().is_empty());
        let id = upsert(&wallet, payroll(Cadence::Monthly, 1000)).unwrap();
        let got = load(&wallet).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, id);
        assert_eq!(got[0].payees[0].code, "ozkyA");
        // The file is encrypted (not plaintext JSON).
        let raw = std::fs::read(store_path(&wallet)).unwrap();
        assert!(!raw.windows(5).any(|w| w == b"ozkyA"), "payee code must not be cleartext");

        set_enabled(&wallet, id, false).unwrap();
        assert!(!load(&wallet).unwrap()[0].enabled);
        remove(&wallet, id).unwrap();
        assert!(load(&wallet).unwrap().is_empty());
        std::env::remove_var("OZKY_NOTES_DIR");
    }
}
