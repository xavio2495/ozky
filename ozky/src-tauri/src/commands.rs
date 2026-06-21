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

/// Withdraw `amount` of `asset` out of the shielded pool to a public Stellar `dest`
/// address (the off-ramp). Returns the tx hash. (A3/G6)
#[tauri::command]
pub fn withdraw(asset: String, dest: String, amount: u64) -> Result<String, CoreError> {
    core::withdraw::withdraw(&asset, &dest, amount)
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
