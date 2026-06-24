//! The `invoke` command surface — the only thing the Svelte UI calls. Each command
//! is a thin shim over [`crate::core`]; the UI never sees a crypto primitive. A0
//! wires the skeleton: `wallet_status` is a real working command (it reads the OS
//! keychain), the action commands return `NotImplemented` until their phase lands.

use crate::core::{self, CoreError};
use serde::Serialize;

/// High-level wallet state for the UI shell. Drives the onboarding gate:
/// `!initialized` → sign-up; `initialized && !unlocked` → sign-in; else → the app.
#[derive(Serialize)]
pub struct WalletStatus {
    /// Whether an encrypted vault exists (a wallet has been created/restored).
    pub initialized: bool,
    /// Whether the wallet is unlocked this session (password + TOTP passed).
    pub unlocked: bool,
    /// Target network (testnet through Part 1/2).
    pub network: String,
}

/// What a fresh sign-up / restore hands back to the UI: the recovery phrase to back up
/// (shown once) and the TOTP secret to add to an authenticator app.
#[derive(Serialize)]
pub struct WalletSetup {
    /// The 12-word recovery phrase (only returned on create; empty on restore).
    pub mnemonic: String,
    /// TOTP shared secret, base32 — for manual authenticator entry.
    pub totp_secret: String,
    /// `otpauth://` provisioning URI — rendered as a QR for the authenticator app.
    pub totp_uri: String,
}

/// Report whether a wallet exists and whether it's unlocked this session.
#[tauri::command]
pub fn wallet_status() -> Result<WalletStatus, CoreError> {
    Ok(WalletStatus {
        initialized: core::vault::exists()?,
        unlocked: core::session::is_unlocked(),
        network: core::chain::DEFAULT_NETWORK.to_string(),
    })
}

/// Create a new wallet (its first account): generate a 12-word phrase + a TOTP secret,
/// encrypt them at rest under `password` (Argon2id + ChaCha20-Poly1305), and open the
/// session. Returns the phrase (back it up) + TOTP provisioning. `async` + `spawn_blocking`
/// so the heavy Argon2 KDF runs off the UI thread (otherwise the window freezes). (auth)
#[tauri::command]
pub async fn create_wallet(password: String) -> Result<WalletSetup, CoreError> {
    blocking(move || {
        let phrase = core::keys::generate_mnemonic()?;
        let keys = core::keys::derive_from_mnemonic(&phrase)?; // validate + label
        setup_vault_and_session(&password, &phrase, keys.stellar_address(), phrase.clone())
    })
    .await
}

/// Restore a wallet from a 12-word phrase: validate it, set a new `password`, provision a
/// fresh TOTP secret, encrypt at rest, and open the session. Off-thread (Argon2). (auth)
#[tauri::command]
pub async fn restore_wallet(phrase: String, password: String) -> Result<WalletSetup, CoreError> {
    blocking(move || {
        let phrase = phrase.trim().to_string();
        let keys = core::keys::derive_from_mnemonic(&phrase)?; // validates the phrase
        setup_vault_and_session(&password, &phrase, keys.stellar_address(), String::new())
    })
    .await
}

/// Shared create/restore tail: provision TOTP, write the encrypted vault (one account),
/// open the session, and return the setup payload.
fn setup_vault_and_session(
    password: &str,
    phrase: &str,
    account_label: &str,
    mnemonic_out: String,
) -> Result<WalletSetup, CoreError> {
    let totp_secret = core::totp::generate_secret();
    let content = core::vault::VaultContent {
        totp_secret,
        accounts: vec![zeroize::Zeroizing::new(phrase.to_string())],
    };
    let key = core::vault::create(password, &content)?;
    core::accounts::reset()?; // fresh wallet starts with a single account
    core::session::set(content, key, 0);
    Ok(WalletSetup {
        mnemonic: mnemonic_out,
        totp_secret: core::totp::secret_base32(&totp_secret),
        totp_uri: core::totp::provisioning_uri(&totp_secret, account_label, "ozky"),
    })
}

/// Unlock the wallet for this session: decrypt the vault with `password` (first factor)
/// and verify the TOTP `code` (second factor). Off-thread (Argon2). (auth)
#[tauri::command]
pub async fn unlock(password: String, code: String) -> Result<(), CoreError> {
    blocking(move || {
        let (content, key) = core::vault::unlock(&password)?;
        if !core::totp::verify(&content.totp_secret, &code, core::totp::now()) {
            return Err(CoreError::Crypto("invalid 2FA code".into()));
        }
        let meta = core::accounts::load()?;
        core::session::set(content, key, meta.active);
        Ok(())
    })
    .await
}

/// Lock the wallet (clear the in-memory session). The vault stays encrypted at rest.
#[tauri::command]
pub fn lock() -> Result<(), CoreError> {
    core::session::clear();
    Ok(())
}

/// Verify a TOTP code against the unlocked session (the sign-up 2FA-confirm step). (auth)
#[tauri::command]
pub fn verify_totp(code: String) -> Result<bool, CoreError> {
    core::totp::verify_session(&code)
}

/// One account in the wallet (each is an independent seed; create or import).
#[derive(Serialize)]
pub struct AccountInfo {
    pub index: u32,
    pub label: String,
    /// The account's public Stellar funding address (`G…`).
    pub address: String,
    pub active: bool,
}

/// Result of creating a fresh account: its index + the new recovery phrase to back up.
#[derive(Serialize)]
pub struct NewAccount {
    pub index: u32,
    pub mnemonic: String,
}

/// List the wallet's accounts (derives each one's Stellar address). Requires unlock.
#[tauri::command]
pub fn list_accounts() -> Result<Vec<AccountInfo>, CoreError> {
    let count = core::session::account_count();
    let active = core::session::active_account();
    let mut out = Vec::with_capacity(count as usize);
    for index in 0..count {
        let phrase = core::session::mnemonic_at(index)?;
        let keys = core::keys::derive_from_mnemonic(&phrase)?;
        out.push(AccountInfo {
            index,
            label: core::accounts::label(index)?,
            address: keys.stellar_address().to_string(),
            active: index == active,
        });
    }
    Ok(out)
}

/// Create a brand-new account (its own fresh seed; max 5) and switch to it. Returns the
/// new index + recovery phrase — the UI must show it once for backup.
#[tauri::command]
pub fn create_account(label: Option<String>) -> Result<NewAccount, CoreError> {
    core::session::mnemonic()?; // must be unlocked
    if core::session::account_count() >= core::accounts::MAX_ACCOUNTS {
        return Err(CoreError::Crypto(format!(
            "account limit reached ({})",
            core::accounts::MAX_ACCOUNTS
        )));
    }
    let phrase = core::keys::generate_mnemonic()?;
    core::keys::derive_from_mnemonic(&phrase)?; // validate before storing
    let index = core::session::add_account(phrase.clone())?;
    core::accounts::add(label)?;
    Ok(NewAccount { index, mnemonic: phrase })
}

/// Import an existing wallet by its 12-word recovery phrase (max 5) and switch to it. (auth)
#[tauri::command]
pub fn import_account(phrase: String, label: Option<String>) -> Result<u32, CoreError> {
    core::session::mnemonic()?; // must be unlocked
    if core::session::account_count() >= core::accounts::MAX_ACCOUNTS {
        return Err(CoreError::Crypto(format!(
            "account limit reached ({})",
            core::accounts::MAX_ACCOUNTS
        )));
    }
    let phrase = phrase.trim().to_string();
    core::keys::derive_from_mnemonic(&phrase)?; // validates the phrase
    let index = core::session::add_account(phrase)?;
    core::accounts::add(label)?;
    Ok(index)
}

/// Switch the active account; subsequent balance/send/etc. use it. (auth)
#[tauri::command]
pub fn switch_account(index: u32) -> Result<(), CoreError> {
    core::session::mnemonic()?; // must be unlocked
    core::accounts::set_active(index)?;
    core::session::set_active_account(index);
    Ok(())
}

/// The active account's PUBLIC (unshielded) Stellar balances — XLM + any trustline assets.
/// Read from Horizon; off the UI thread. (deriving keys + network)
#[tauri::command]
pub async fn public_balances() -> Result<Vec<core::chain::PublicBalance>, CoreError> {
    blocking(|| {
        let wallet = core::keys::current_wallet()?;
        core::chain::public_balances(wallet.stellar_address())
    })
    .await
}

/// The active account's PUBLIC payment history from Horizon (funding + classic in/out), newest
/// first. The public half of transaction history (G8); off the UI thread. (network)
#[tauri::command]
pub async fn public_history() -> Result<Vec<core::chain::PublicTx>, CoreError> {
    blocking(|| {
        let wallet = core::keys::current_wallet()?;
        core::chain::public_payments(wallet.stellar_address())
    })
    .await
}

/// The active account's durable SHIELDED history (the wallet's pool actions, persisted encrypted
/// at rest), newest first. The shielded half of transaction history (G8). (keychain)
#[tauri::command]
pub async fn shielded_history() -> Result<Vec<core::history::ShieldedTx>, CoreError> {
    blocking(|| {
        let wallet = core::keys::current_wallet()?;
        core::history::load(&wallet)
    })
    .await
}

/// Persist one shielded action to the durable history store (mirrors the UI's in-session log so it
/// survives restart). Returns the stored entry (id/ts filled in). (keychain)
#[tauri::command]
pub async fn record_activity(
    kind: String,
    label: String,
    detail: Option<String>,
    hash: Option<String>,
) -> Result<core::history::ShieldedTx, CoreError> {
    blocking(move || {
        let wallet = core::keys::current_wallet()?;
        core::history::record(
            &wallet,
            core::history::ShieldedTx { id: 0, kind, label, detail, hash, ts: 0 },
        )
    })
    .await
}

/// Current USD spot prices (+ 24h change) for the wallet's assets. Public market data;
/// needs no wallet. Network I/O runs off the UI thread.
#[tauri::command]
pub async fn asset_prices() -> Result<Vec<core::price::Spot>, CoreError> {
    let codes: Vec<String> = core::config::ASSETS.iter().map(|a| a.code.to_string()).collect();
    blocking(move || core::price::spot(&codes)).await
}

/// USD price history for one asset over `days` (for the price chart).
#[tauri::command]
pub async fn price_history(code: String, days: u32) -> Result<Vec<core::price::Point>, CoreError> {
    blocking(move || core::price::history(&code, days)).await
}

/// Run a blocking (CPU-heavy or network) closure off the UI thread.
async fn blocking<T, F>(f: F) -> Result<T, CoreError>
where
    F: FnOnce() -> Result<T, CoreError> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| CoreError::Crypto(format!("task join: {e}")))?
}

/// Spendable balance of one asset the wallet holds shielded notes in.
#[derive(Serialize)]
pub struct AssetBalance {
    /// v1 asset code (e.g. "USDC"), or the raw `asset_tag` decimal if unknown.
    pub code: String,
    /// The in-circuit `asset_tag` (decimal).
    pub asset_tag: String,
    /// Total spendable value in base units.
    pub raw: u64,
    /// Human-readable amount (base units scaled by `decimals`).
    pub display: String,
    pub decimals: u32,
}

/// Total spendable balance **per asset** (one row per known v1 asset; 0 if none held).
/// Notes carry their `asset_tag` in plaintext, so a single scan covers every asset. (A2/G6)
#[tauri::command]
pub fn balance() -> Result<Vec<AssetBalance>, CoreError> {
    let notes = core::scan::scan(0)?;
    let mut out = Vec::new();
    for a in core::config::ASSETS {
        let tag_dec = a.tag.to_string();
        let raw: u64 = notes
            .iter()
            .filter(|n| n.asset_tag.to_decimal() == tag_dec)
            .map(|n| n.value)
            .sum();
        out.push(AssetBalance {
            code: a.code.to_string(),
            asset_tag: tag_dec,
            raw,
            display: format_units(raw, a.decimals),
            decimals: a.decimals,
        });
    }
    Ok(out)
}

/// Format `raw` base units as a decimal string scaled by `decimals` (e.g. 1000 @ 7 → "0.0001000").
fn format_units(raw: u64, decimals: u32) -> String {
    if decimals == 0 {
        return raw.to_string();
    }
    let scale = 10u64.pow(decimals);
    let whole = raw / scale;
    let frac = raw % scale;
    format!("{whole}.{frac:0>width$}", width = decimals as usize)
}

/// This wallet's spending public key (`owner_pk`, hex) — share it with the ASP to be
/// enrolled into a shared pool's anonymity set. (A3 / ASP enrollment)
#[tauri::command]
pub fn spending_key() -> Result<String, CoreError> {
    core::enroll::spending_key()
}

/// Enroll this wallet into the configured pool's ASP approved set + deposit allow-list
/// (testnet/dev: the wallet must be the policy admin). Returns the tx hash. (A3)
#[tauri::command]
pub fn enroll() -> Result<String, CoreError> {
    core::enroll::enroll_self()
}

/// Deposit `amount` of `asset` (a v1 code, e.g. "USDC") into the shielded pool from the
/// wallet's Stellar account (the public on-ramp: fund [`funding_address`] from any
/// wallet, then deposit to shield it). Returns the tx hash. (A3/G6)
#[tauri::command]
pub fn deposit(asset: String, amount: u64) -> Result<String, CoreError> {
    core::deposit::deposit(&asset, amount)
}

/// Send `amount` of `asset` privately to `recipient` (a shielded payment code). Builds +
/// proves the transfer against live pool state and submits it; returns the tx hash. (A3/G6)
#[tauri::command]
pub fn send(asset: String, recipient: String, amount: u64) -> Result<String, CoreError> {
    core::send::send(&asset, &recipient, amount)
}

/// Consolidate a fragmented `asset` balance: collapse up to 4 owned notes into ONE self note via a
/// 4-input transfer. Proves off the UI thread; returns the tx hash. (multi-input transfer, scope #1)
#[tauri::command]
pub async fn consolidate(asset: String) -> Result<String, CoreError> {
    blocking(move || core::send::consolidate(&asset)).await
}

/// One recipient of a split payment: a shielded payment code + base-unit amount.
#[derive(serde::Deserialize)]
pub struct SplitRecipientArg {
    pub recipient: String,
    pub amount: u64,
}

/// Split `asset` from one shielded note to up to 7 recipients in a single private
/// transfer (recipients + change + dummy-padded to 8 outputs). Proves off the UI thread;
/// returns the tx hash. (payment split)
#[tauri::command]
pub async fn split(asset: String, recipients: Vec<SplitRecipientArg>) -> Result<String, CoreError> {
    blocking(move || {
        let rs: Vec<core::send::SplitRecipient> = recipients
            .into_iter()
            .map(|r| core::send::SplitRecipient { code: r.recipient, amount: r.amount })
            .collect();
        core::send::split(&asset, &rs)
    })
    .await
}

/// A payee row for a payroll (shielded code + base-unit amount).
#[derive(serde::Deserialize)]
pub struct PayeeArg {
    pub code: String,
    pub amount: u64,
}

/// Payroll create/update input from the UI.
#[derive(serde::Deserialize)]
pub struct PayrollInput {
    /// 0 to create; an existing id to update.
    pub id: u64,
    pub label: String,
    pub asset: String,
    pub payees: Vec<PayeeArg>,
    /// "weekly" | "monthly" | "days".
    pub cadence: String,
    /// interval days when cadence == "days".
    pub interval_days: u32,
    /// Unix seconds for the first run (defaults to now if 0).
    pub start_unix: i64,
}

/// A payroll as shown in the UI (+ a computed `due` flag).
#[derive(Serialize)]
pub struct PayrollView {
    pub id: u64,
    pub label: String,
    pub asset: String,
    pub payees: Vec<PayeeView>,
    pub cadence: String,
    pub interval_days: u32,
    pub next_run_unix: i64,
    pub last_run_unix: Option<i64>,
    pub enabled: bool,
    pub due: bool,
    pub total: u64,
}

#[derive(Serialize)]
pub struct PayeeView {
    pub code: String,
    pub amount: u64,
}

fn cadence_to_str(c: core::payroll::Cadence) -> (String, u32) {
    match c {
        core::payroll::Cadence::Weekly => ("weekly".into(), 0),
        core::payroll::Cadence::Monthly => ("monthly".into(), 0),
        core::payroll::Cadence::EveryDays(n) => ("days".into(), n),
    }
}

fn cadence_from(cadence: &str, interval_days: u32) -> core::payroll::Cadence {
    match cadence {
        "monthly" => core::payroll::Cadence::Monthly,
        "days" => core::payroll::Cadence::EveryDays(interval_days.max(1)),
        _ => core::payroll::Cadence::Weekly,
    }
}

fn view(p: core::payroll::Payroll, now: i64) -> PayrollView {
    let (cadence, interval_days) = cadence_to_str(p.cadence);
    PayrollView {
        id: p.id,
        label: p.label.clone(),
        asset: p.asset.clone(),
        payees: p.payees.iter().map(|x| PayeeView { code: x.code.clone(), amount: x.amount }).collect(),
        cadence,
        interval_days,
        next_run_unix: p.next_run_unix,
        last_run_unix: p.last_run_unix,
        enabled: p.enabled,
        due: p.is_due(now),
        total: p.total(),
    }
}

/// List this wallet's payrolls with a computed `due` flag. (recurring payroll)
#[tauri::command]
pub fn list_payrolls() -> Result<Vec<PayrollView>, CoreError> {
    let wallet = core::keys::current_wallet()?;
    let now = core::payroll::now();
    Ok(core::payroll::load(&wallet)?.into_iter().map(|p| view(p, now)).collect())
}

/// Create (id=0) or update a payroll. Returns its id. (recurring payroll)
#[tauri::command]
pub fn save_payroll(input: PayrollInput) -> Result<u64, CoreError> {
    let wallet = core::keys::current_wallet()?;
    let cadence = cadence_from(&input.cadence, input.interval_days);
    let start = if input.start_unix > 0 { input.start_unix } else { core::payroll::now() };
    // Preserve last_run when updating an existing payroll.
    let last_run_unix = core::payroll::load(&wallet)?
        .into_iter()
        .find(|p| p.id == input.id)
        .and_then(|p| p.last_run_unix);
    let p = core::payroll::Payroll {
        id: input.id,
        label: input.label,
        asset: input.asset,
        payees: input.payees.into_iter().map(|x| core::payroll::Payee { code: x.code, amount: x.amount }).collect(),
        cadence,
        next_run_unix: start,
        last_run_unix,
        enabled: true,
    };
    core::payroll::upsert(&wallet, p)
}

/// Delete a payroll. (recurring payroll)
#[tauri::command]
pub fn delete_payroll(id: u64) -> Result<(), CoreError> {
    let wallet = core::keys::current_wallet()?;
    core::payroll::remove(&wallet, id)
}

/// Enable/disable a payroll (disabled payrolls are never "due"). (recurring payroll)
#[tauri::command]
pub fn set_payroll_enabled(id: u64, enabled: bool) -> Result<(), CoreError> {
    let wallet = core::keys::current_wallet()?;
    core::payroll::set_enabled(&wallet, id, enabled)
}

/// Run a payroll now: pays all payees via ceil(N/5) split transactions, advances the
/// schedule, returns the tx hashes. Off the UI thread (proves). (recurring payroll)
#[tauri::command]
pub async fn run_payroll(id: u64) -> Result<Vec<String>, CoreError> {
    blocking(move || {
        let wallet = core::keys::current_wallet()?;
        let cfg = core::config::PoolConfig::load()?;
        core::payroll::run(&wallet, &cfg, id)
    })
    .await
}

/// Subscription create/update input from the UI. (push subscriptions)
#[derive(serde::Deserialize)]
pub struct SubscriptionInput {
    /// 0 to create; an existing id to update.
    pub id: u64,
    pub label: String,
    pub asset: String,
    pub code: String,
    pub amount: u64,
    /// "weekly" | "monthly" | "days".
    pub cadence: String,
    /// interval days when cadence == "days".
    pub interval_days: u32,
    /// Unix seconds for the first charge (defaults to now if 0).
    pub start_unix: i64,
    /// Unix seconds to stop after (0 = no end).
    pub end_unix: i64,
}

/// A subscription as shown in the UI (+ a computed `due` flag). (push subscriptions)
#[derive(Serialize)]
pub struct SubscriptionView {
    pub id: u64,
    pub label: String,
    pub asset: String,
    pub code: String,
    pub amount: u64,
    pub cadence: String,
    pub interval_days: u32,
    pub next_run_unix: i64,
    pub last_run_unix: Option<i64>,
    pub end_unix: Option<i64>,
    pub enabled: bool,
    pub due: bool,
}

fn sub_view(s: core::subscriptions::Subscription, now: i64) -> SubscriptionView {
    let (cadence, interval_days) = cadence_to_str(s.cadence);
    SubscriptionView {
        id: s.id,
        label: s.label.clone(),
        asset: s.asset.clone(),
        code: s.code.clone(),
        amount: s.amount,
        cadence,
        interval_days,
        next_run_unix: s.next_run_unix,
        last_run_unix: s.last_run_unix,
        end_unix: s.end_unix,
        due: s.is_due(now),
        enabled: s.enabled,
    }
}

/// List this wallet's subscriptions with a computed `due` flag. (push subscriptions)
#[tauri::command]
pub fn list_subscriptions() -> Result<Vec<SubscriptionView>, CoreError> {
    let wallet = core::keys::current_wallet()?;
    let now = core::payroll::now();
    Ok(core::subscriptions::load(&wallet)?.into_iter().map(|s| sub_view(s, now)).collect())
}

/// Create (id=0) or update a subscription. Returns its id. (push subscriptions)
#[tauri::command]
pub fn save_subscription(input: SubscriptionInput) -> Result<u64, CoreError> {
    let wallet = core::keys::current_wallet()?;
    let cadence = cadence_from(&input.cadence, input.interval_days);
    let start = if input.start_unix > 0 { input.start_unix } else { core::payroll::now() };
    // Preserve last_run when updating an existing subscription.
    let last_run_unix = core::subscriptions::load(&wallet)?
        .into_iter()
        .find(|s| s.id == input.id)
        .and_then(|s| s.last_run_unix);
    let s = core::subscriptions::Subscription {
        id: input.id,
        label: input.label,
        asset: input.asset,
        code: input.code,
        amount: input.amount,
        cadence,
        next_run_unix: start,
        last_run_unix,
        end_unix: if input.end_unix > 0 { Some(input.end_unix) } else { None },
        enabled: true,
    };
    core::subscriptions::upsert(&wallet, s)
}

/// Delete a subscription. (push subscriptions)
#[tauri::command]
pub fn delete_subscription(id: u64) -> Result<(), CoreError> {
    let wallet = core::keys::current_wallet()?;
    core::subscriptions::remove(&wallet, id)
}

/// Enable/disable a subscription (disabled ones are never "due"). (push subscriptions)
#[tauri::command]
pub fn set_subscription_enabled(id: u64, enabled: bool) -> Result<(), CoreError> {
    let wallet = core::keys::current_wallet()?;
    core::subscriptions::set_enabled(&wallet, id, enabled)
}

/// Charge a subscription now: one shielded transfer, advances the schedule, returns the tx
/// hash. Off the UI thread (proves). (push subscriptions)
#[tauri::command]
pub async fn run_subscription(id: u64) -> Result<String, CoreError> {
    blocking(move || {
        let wallet = core::keys::current_wallet()?;
        let cfg = core::config::PoolConfig::load()?;
        core::subscriptions::run(&wallet, &cfg, id)
    })
    .await
}

// ----------------------------- shielded escrow (building block B) -----------------------------

/// One contribution this wallet made to an escrow (for the refund affordance). (shielded escrow)
#[derive(Serialize)]
pub struct EscrowContribution {
    pub index: u32,
    pub amount: u64,
}

/// An escrow this wallet is involved in (opened as payee and/or contributed to), merged with its
/// public on-chain state. Amounts stay hidden on-chain; `raised` is the payee's own decrypted
/// total (None for contributors). (shielded escrow)
#[derive(Serialize)]
pub struct EscrowView {
    pub id: u64,
    pub asset: String,
    pub target: u64,
    pub mode: String, // "all_or_nothing" | "keep_what_you_raise"
    pub n_contrib: u32,
    pub status: String, // "open" | "released"
    pub deadline_unix: i64,
    pub deadline_passed: bool,
    pub is_payee: bool,
    pub my_contributions: Vec<EscrowContribution>,
    /// Payee-only: this wallet's decrypted running total `S`. None for contributors / on scan error.
    pub raised: Option<u64>,
    pub releasable: bool,
    pub refundable: bool,
}

fn mode_to_str(mode: u32) -> String {
    if mode == core::escrow::MODE_KEEP_WHAT_YOU_RAISE { "keep_what_you_raise" } else { "all_or_nothing" }
        .to_string()
}

fn mode_from(mode: &str) -> u32 {
    if mode == "keep_what_you_raise" {
        core::escrow::MODE_KEEP_WHAT_YOU_RAISE
    } else {
        core::escrow::MODE_ALL_OR_NOTHING
    }
}

/// List the escrows this wallet opened or contributed to, with on-chain state + eligibility flags.
/// Network-heavy (reads each escrow + scans the payee's contribution blobs), so it runs off the UI
/// thread. (shielded escrow)
#[tauri::command]
pub async fn list_escrows() -> Result<Vec<EscrowView>, CoreError> {
    blocking(move || {
        let wallet = core::keys::current_wallet()?;
        let cfg = core::config::PoolConfig::load()?;
        let now = core::payroll::now();
        let latest = core::chain::latest_ledger(&cfg.rpc_url)? as u64;

        let opened = core::escrow::list_opened(&wallet)?;
        let contributions = core::escrow::list_contributions(&wallet)?;

        // Distinct escrow ids this wallet touches, payee-opened first.
        let mut ids: Vec<u64> = Vec::new();
        for o in &opened {
            if !ids.contains(&o.escrow_id) {
                ids.push(o.escrow_id);
            }
        }
        for c in &contributions {
            if !ids.contains(&c.escrow_id) {
                ids.push(c.escrow_id);
            }
        }

        let mut views = Vec::new();
        for id in ids {
            // A stale local record (escrow gone) shouldn't break the whole list.
            let st = match core::chain::read_escrow(&cfg, id) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let is_payee = opened.iter().any(|o| o.escrow_id == id);
            let mine: Vec<EscrowContribution> = contributions
                .iter()
                .filter(|c| c.escrow_id == id)
                .map(|c| EscrowContribution { index: c.contrib_index, amount: c.amount })
                .collect();

            let asset = core::config::asset_by_tag(&st.asset_tag)
                .map(|a| a.code.to_string())
                .unwrap_or_else(|| st.asset_tag.to_decimal());
            let deadline_passed = latest > st.deadline;
            // Ledgers close ~5s apart; estimate the deadline as a wall-clock instant for display.
            let deadline_unix = now + (st.deadline as i64 - latest as i64) * 5;
            let is_open = st.status == 0;

            // Only the payee can decrypt the running total; skip (None) on any scan error.
            let raised = if is_payee {
                core::escrow::scan_total(&wallet, &cfg, id).ok().map(|(s, _)| s)
            } else {
                None
            };

            let releasable = is_payee
                && is_open
                && if st.mode == core::escrow::MODE_KEEP_WHAT_YOU_RAISE {
                    deadline_passed
                } else {
                    raised.map(|s| s >= st.target).unwrap_or(false)
                };
            let refundable = !mine.is_empty()
                && st.mode == core::escrow::MODE_ALL_OR_NOTHING
                && is_open
                && deadline_passed;

            views.push(EscrowView {
                id,
                asset,
                target: st.target,
                mode: mode_to_str(st.mode),
                n_contrib: st.n_contrib,
                status: if is_open { "open".into() } else { "released".into() },
                deadline_unix,
                deadline_passed,
                is_payee,
                my_contributions: mine,
                raised,
                releasable,
                refundable,
            });
        }
        Ok(views)
    })
    .await
}

/// Open a hidden-sum escrow as the payee. `target`/`amount` are base units; `deadline_unix` is the
/// wall-clock deadline (converted to a ledger number ~5s/ledger). Returns the escrow id. (escrow)
#[tauri::command]
pub async fn open_escrow(asset: String, target: u64, deadline_unix: i64, mode: String) -> Result<u64, CoreError> {
    blocking(move || {
        let wallet = core::keys::current_wallet()?;
        let cfg = core::config::PoolConfig::load()?.with_asset(&asset)?;
        let now = core::payroll::now();
        let latest = core::chain::latest_ledger(&cfg.rpc_url)? as i64;
        let deadline_ledger = (latest + (deadline_unix - now).max(0) / 5).max(latest + 1) as u64;
        core::escrow::open(&wallet, &cfg, target, deadline_ledger, mode_from(&mode))
    })
    .await
}

/// Contribute `amount` (base units) to an escrow, hidden. `payee_code` is the payee's shielded
/// code (the `(amount, r)` opener is encrypted to them). Returns the contribution index. (escrow)
#[tauri::command]
pub async fn contribute_escrow(escrow_id: u64, payee_code: String, amount: u64) -> Result<u32, CoreError> {
    blocking(move || {
        let wallet = core::keys::current_wallet()?;
        let cfg = core::config::PoolConfig::load()?;
        core::escrow::contribute(&wallet, &cfg, escrow_id, &payee_code, amount)
    })
    .await
}

/// Release an escrow to the payee (this wallet): scans the contribution blobs to recover the total
/// `(S, R)`, then mints a shielded note of `S`. Returns the tx hash. (escrow)
#[tauri::command]
pub async fn release_escrow(escrow_id: u64) -> Result<String, CoreError> {
    blocking(move || {
        let wallet = core::keys::current_wallet()?;
        let cfg = core::config::PoolConfig::load()?;
        let (total_value, total_r) = core::escrow::scan_total(&wallet, &cfg, escrow_id)?;
        core::escrow::release(&wallet, &cfg, escrow_id, total_value, total_r)
    })
    .await
}

/// Refund this wallet's contribution `contrib_index` to a failed all-or-nothing escrow. Mints the
/// contribution amount back to this wallet. Returns the tx hash. (escrow)
#[tauri::command]
pub async fn refund_escrow(escrow_id: u64, contrib_index: u32) -> Result<String, CoreError> {
    blocking(move || {
        let wallet = core::keys::current_wallet()?;
        let cfg = core::config::PoolConfig::load()?;
        core::escrow::refund(&wallet, &cfg, escrow_id, contrib_index)
    })
    .await
}

// ----------------------------- merchant-pull channel (building block B phase 2) -----------------------------

/// A subscription channel this wallet is involved in (subscriber and/or merchant), merged with its
/// public on-chain state. Cap + draw amounts stay hidden on-chain; `cap`/`drawn_so_far` are the
/// wallet's own local knowledge (it holds the ramp). (merchant-pull channel)
#[derive(Serialize)]
pub struct ChannelView {
    pub id: u64,
    pub asset: String,
    pub status: String, // "open" | "closed"
    pub expiry_unix: i64,
    pub expiry_passed: bool,
    pub is_subscriber: bool,
    pub is_merchant: bool,
    /// The hidden cap (this wallet's own knowledge from the ramp).
    pub cap: u64,
    pub amount_per_period: u64,
    /// The highest cumulative amount currently authorized (elapsed periods) — what a close would draw.
    pub drawn_so_far: u64,
    /// Merchant: a close is possible now (open + an elapsed authorization exists).
    pub closeable: bool,
    /// Subscriber: a reclaim is possible now (open + past expiry).
    pub reclaimable: bool,
}

/// List the subscription channels this wallet opened (subscriber) or imported (merchant), with
/// on-chain status + eligibility flags. Network-heavy (reads each channel + latest ledger), so it
/// runs off the UI thread. (merchant-pull channel)
#[tauri::command]
pub async fn list_channels() -> Result<Vec<ChannelView>, CoreError> {
    blocking(move || {
        let wallet = core::keys::current_wallet()?;
        let cfg = core::config::PoolConfig::load()?;
        let now = core::payroll::now();
        let latest = core::chain::latest_ledger(&cfg.rpc_url)? as u64;
        let id_w = core::scan::wallet_identity(&wallet)?;
        let my_pk = id_w.owner_pk.to_hex();

        let records = core::channel::list_records(&wallet)?;
        let mut views = Vec::new();
        for rec in records {
            // A stale local record (channel gone) shouldn't break the whole list.
            let st = match core::chain::read_channel(&cfg, rec.channel_id) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let is_subscriber = rec.subscriber_owner_pk == my_pk;
            let is_merchant = rec.merchant_owner_pk == my_pk;
            let is_open = st.status == 0;
            let expiry_passed = latest > st.expiry;
            let expiry_unix = now + (st.expiry as i64 - latest as i64) * 5;

            // The highest cumulative the merchant could draw right now (elapsed periods).
            let drawn_so_far = rec
                .ramp
                .iter()
                .filter(|e| e.valid_after_ledger <= latest)
                .map(|e| e.cum_amount)
                .max()
                .unwrap_or(0);

            let closeable = is_merchant && is_open && drawn_so_far > 0;
            let reclaimable = is_subscriber && is_open && expiry_passed;

            views.push(ChannelView {
                id: rec.channel_id,
                asset: rec.asset,
                status: if is_open { "open".into() } else { "closed".into() },
                expiry_unix,
                expiry_passed,
                is_subscriber,
                is_merchant,
                cap: rec.cap,
                amount_per_period: rec.amount_per_period,
                drawn_so_far,
                closeable,
                reclaimable,
            });
        }
        Ok(views)
    })
    .await
}

/// Open a subscription channel as the subscriber: lock `cap` (hidden), pre-sign a ramp of
/// `n_periods` cumulative authorizations (`amount_per_period` each, `period_secs` apart), and seal
/// it to the merchant. Returns the channel id. (merchant-pull channel)
#[tauri::command]
pub async fn open_channel(
    asset: String,
    cap: u64,
    merchant_code: String,
    amount_per_period: u64,
    n_periods: u32,
    period_secs: i64,
) -> Result<u64, CoreError> {
    blocking(move || {
        let wallet = core::keys::current_wallet()?;
        let cfg = core::config::PoolConfig::load()?;
        // Ledgers close ~5s apart; convert the wall-clock period to a ledger span (min 1).
        let ledgers_per_period = (period_secs / 5).max(1) as u64;
        core::channel::open(&wallet, &cfg, &asset, cap, &merchant_code, amount_per_period, n_periods, ledgers_per_period)
    })
    .await
}

/// Close a channel (merchant) at the highest elapsed authorization: mints the drawn amount to the
/// merchant and the remainder back to the subscriber. Returns the tx hash. (merchant-pull channel)
#[tauri::command]
pub async fn close_channel(channel_id: u64) -> Result<String, CoreError> {
    blocking(move || {
        let wallet = core::keys::current_wallet()?;
        let cfg = core::config::PoolConfig::load()?;
        core::channel::close(&wallet, &cfg, channel_id)
    })
    .await
}

/// Reclaim the full cap (subscriber) after a channel expires unclosed. Returns the tx hash.
/// (merchant-pull channel)
#[tauri::command]
pub async fn reclaim_channel(channel_id: u64) -> Result<String, CoreError> {
    blocking(move || {
        let wallet = core::keys::current_wallet()?;
        let cfg = core::config::PoolConfig::load()?;
        core::channel::reclaim(&wallet, &cfg, channel_id)
    })
    .await
}

/// Import a channel this wallet is the merchant for (decrypt the on-chain `chanopen` blob into a
/// local record so it can be closed). Returns nothing. (merchant-pull channel)
#[tauri::command]
pub async fn import_channel(channel_id: u64) -> Result<(), CoreError> {
    blocking(move || {
        let wallet = core::keys::current_wallet()?;
        let cfg = core::config::PoolConfig::load()?;
        core::channel::import_from_chain(&wallet, &cfg, channel_id)
    })
    .await
}

/// Withdraw `amount` of `asset` out of the shielded pool to a public Stellar `dest`
/// address (the off-ramp). Returns the tx hash. (A3/G6)
#[tauri::command]
pub fn withdraw(asset: String, dest: String, amount: u64) -> Result<String, CoreError> {
    core::withdraw::withdraw(&asset, &dest, amount)
}

/// Quote swapping `amount` (base units) of `from` into `to` via the Stellar DEX (strict-send,
/// Phase 1 edge swap). Read-only — moves no funds. (asset swap)
#[tauri::command]
pub async fn swap_quote(from: String, to: String, amount: u64) -> Result<core::swap::SwapQuote, CoreError> {
    blocking(move || core::swap::quote(&from, &to, amount)).await
}

/// Swap `amount` (base units) of `from` into shielded `to`, tolerating up to `slippage_bps`
/// basis points of slippage. PRIVACY-LEAKY edge swap (withdraw A -> public DEX -> deposit B);
/// the UI warns. Proves off the UI thread; returns a per-leg receipt. (asset swap)
#[tauri::command]
pub async fn swap(
    from: String,
    to: String,
    amount: u64,
    slippage_bps: u32,
) -> Result<core::swap::SwapReceipt, CoreError> {
    blocking(move || core::swap::swap(&from, &to, amount, slippage_bps)).await
}

/// This wallet's **public Stellar funding address** (`G…`). Give this to any wallet or
/// exchange to receive funds publicly; then [`deposit`] shields them into the pool.
/// This is a normal Stellar account — usable from non-ozky wallets. (A3)
#[tauri::command]
pub fn funding_address() -> Result<String, CoreError> {
    let keys = core::keys::current_wallet()?;
    Ok(keys.stellar_address().to_string())
}

/// This wallet's **shielded receive address** (an `ozky…` payment code). Give this to
/// another ozky wallet to receive a PRIVATE transfer. Not usable from non-ozky wallets —
/// for external/public funding use [`funding_address`]. (A3)
#[tauri::command]
pub fn receive_address() -> Result<String, CoreError> {
    core::send::receive_code()
}

/// Export a scoped, read-only disclosure for an auditor (a Stellar `G…`) and record the
/// auditable on-chain grant. Returns the disclosure package (JSON) to hand the auditor
/// out-of-band: it lets them re-derive + verify this wallet's notes for the scope, with
/// no spend authority. (A3 / G5)
#[tauri::command]
pub fn share_with_auditor(auditor: String, epoch: u32) -> Result<String, CoreError> {
    core::disclose::share_with_auditor(&auditor, epoch)
}

/// Auditor side: given a disclosure package (JSON from [`share_with_auditor`]), scan the
/// disclosed pool and return the owner's notes it reveals (each verified against its
/// on-chain commitment), as JSON. Read-only; needs no wallet. (A3 / G5)
#[tauri::command]
pub fn audit_disclosure(package: String) -> Result<String, CoreError> {
    let notes = core::disclose::audit(&package)?;
    let total = core::disclose::disclosed_total(&notes);
    serde_json::to_string(&serde_json::json!({ "total": total, "notes": notes }))
        .map_err(|e| CoreError::Crypto(format!("serialize audit: {e}")))
}
