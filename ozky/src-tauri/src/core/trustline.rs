//! Auto-trustline onboarding (FEATURE_SET scope #6). A fresh Stellar account has no
//! trustlines, so receiving/depositing USDC or EURC fails with "trustline missing". This
//! establishes those trustlines for the wallet's account with the reserves **sponsored by
//! the relayer** (`begin/end_sponsoring_future_reserves` around the `ChangeTrust` ops), so
//! the user needs no XLM — matching the relayer fee-abstraction model and generalizing to
//! mainnet. If the account doesn't exist yet, the same sponsored transaction creates it.
//!
//! Idempotent: it reads the account's current trustlines and only adds the missing ones;
//! a no-op (everything already trusted) returns without a transaction. Safe to call at
//! account create AND again later.

use super::config::{self, PoolConfig};
use super::{chain, keys, CoreError};
use serde::Serialize;

/// Outcome of an [`ensure_trustlines`] call (surfaced to the UI).
#[derive(Serialize)]
pub struct TrustlineReport {
    /// The account didn't exist and was created (sponsored) by this call.
    pub account_created: bool,
    /// Asset codes whose trustlines this call added (empty ⇒ nothing to do).
    pub added: Vec<String>,
    /// Everything was already established (no transaction submitted).
    pub already: bool,
    /// The confirmed transaction hash, when one was submitted.
    pub tx: Option<String>,
}

/// Establish the missing USDC/EURC trustlines for the keychain wallet's account.
pub fn ensure_trustlines() -> Result<TrustlineReport, CoreError> {
    let wallet = keys::current_wallet()?;
    let cfg = PoolConfig::load()?;
    ensure_trustlines_with(&wallet, &cfg)
}

/// Keychain-independent core: check existing trustlines, then sponsor-establish the
/// missing auto-trust assets (creating the account first if it doesn't exist).
pub fn ensure_trustlines_with(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
) -> Result<TrustlineReport, CoreError> {
    let relayer = cfg.relayer_secret.as_deref().ok_or_else(|| {
        CoreError::Chain(
            "OZKY_RELAYER_SECRET not set (needed to sponsor the trustline reserves)".into(),
        )
    })?;
    let addr = wallet.stellar_address();

    // An unfunded/nonexistent account returns no balances; a funded one always has native
    // XLM, so an empty result ⇒ the account must be created in this same sponsored tx.
    let balances = chain::public_balances(addr)?;
    let account_created = balances.is_empty();
    let present: Vec<(String, Option<String>)> = balances
        .into_iter()
        .map(|b| (b.code, b.issuer))
        .collect();

    // Auto-trust assets (USDC, EURC) not already trusted on this account (match code+issuer).
    let missing: Vec<(&'static str, &'static str)> = config::auto_trust_assets()
        .into_iter()
        .filter(|(code, issuer)| {
            !present
                .iter()
                .any(|(c, iss)| c == code && iss.as_deref() == Some(*issuer))
        })
        .collect();

    if missing.is_empty() && !account_created {
        return Ok(TrustlineReport { account_created: false, added: vec![], already: true, tx: None });
    }

    let tx = chain::submit_sponsored_trustlines(
        cfg,
        relayer,
        wallet.stellar_secret(),
        addr,
        account_created,
        &missing,
    )?;

    Ok(TrustlineReport {
        account_created,
        added: missing.iter().map(|(c, _)| c.to_string()).collect(),
        already: false,
        tx: Some(tx),
    })
}

/// Onboarding provisioning for a brand-new wallet: (1) ask the funder service to create +
/// fund the account (10 XLM), (2) wait for it to appear on-chain, (3) establish the missing
/// USDC/EURC trustlines LOCALLY — signed + fee-paid by the now-funded account (no relayer,
/// no pool config needed). Idempotent: an already-funded/trusted account just reports what
/// was (not) done. Errors if the account doesn't exist and no funder is configured.
pub fn provision_new_account() -> Result<TrustlineReport, CoreError> {
    let wallet = keys::current_wallet()?;
    let addr = wallet.stellar_address();

    // 1. Create + fund the account via the server funder, unless it already exists.
    let mut balances = chain::public_balances(addr)?;
    let mut account_created = false;
    if balances.is_empty() {
        if !super::funder::request_funding(addr)? {
            return Err(CoreError::Chain(
                "no funder configured (OZKY_FUNDER_URL) — a new account can't be created".into(),
            ));
        }
        account_created = true;
        balances = wait_for_account(addr)?; // 2. CreateAccount settles in a few seconds.
    }

    // 3. Add the missing auto-trust trustlines, paid by the user's own account.
    let present: Vec<(String, Option<String>)> =
        balances.into_iter().map(|b| (b.code, b.issuer)).collect();
    let missing: Vec<(&'static str, &'static str)> = config::auto_trust_assets()
        .into_iter()
        .filter(|(code, issuer)| {
            !present.iter().any(|(c, iss)| c == code && iss.as_deref() == Some(*issuer))
        })
        .collect();
    if missing.is_empty() {
        return Ok(TrustlineReport { account_created, added: vec![], already: true, tx: None });
    }

    let rpc_url = config::cfg_var("OZKY_RPC_URL").unwrap_or_else(|| chain::DEFAULT_RPC_URL.to_string());
    let passphrase = config::cfg_var("OZKY_NETWORK_PASSPHRASE")
        .unwrap_or_else(|| "Test SDF Network ; September 2015".to_string());
    let tx = chain::submit_local_trustlines(&rpc_url, &passphrase, wallet.stellar_secret(), &missing)?;
    Ok(TrustlineReport {
        account_created,
        added: missing.iter().map(|(c, _)| c.to_string()).collect(),
        already: false,
        tx: Some(tx),
    })
}

/// Poll Horizon until the freshly-funded account exists (returns its balances), up to a
/// short timeout (~30s) to cover ledger close + propagation.
fn wait_for_account(addr: &str) -> Result<Vec<chain::PublicBalance>, CoreError> {
    for _ in 0..20 {
        let b = chain::public_balances(addr)?;
        if !b.is_empty() {
            return Ok(b);
        }
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }
    Err(CoreError::Chain("funded account did not appear on-chain in time".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The auto-trust set is exactly the non-native assets (USDC, EURC) with issuers — no
    /// XLM (native, no trustline) and no USDT (dropped).
    #[test]
    fn auto_trust_set_is_usdc_and_eurc() {
        let codes: Vec<&str> = config::auto_trust_assets().iter().map(|(c, _)| *c).collect();
        assert_eq!(codes, vec!["USDC", "EURC"]);
        assert!(config::auto_trust_assets().iter().all(|(_, iss)| iss.starts_with('G')));
        assert!(!codes.contains(&"XLM"), "native XLM needs no trustline");
        assert!(!codes.contains(&"USDT"), "USDT dropped (no official Stellar issuer)");
    }

    /// LIVE (testnet): a brand-new, never-funded account is created + gets USDC + EURC
    /// trustlines in one relayer-SPONSORED transaction (no XLM on the new account), and a
    /// second call is a no-op. Needs `OZKY_*` config + a funded relayer (`OZKY_RELAYER_SECRET`).
    #[test]
    #[ignore = "live testnet: needs OZKY_* config + a funded relayer; run with --ignored"]
    fn sponsored_trustlines_on_fresh_account_testnet() {
        let cfg = PoolConfig::load().expect("OZKY_* config");
        let phrase = keys::generate_mnemonic().unwrap();
        let wallet = keys::derive_from_mnemonic(&phrase).unwrap();
        let addr = wallet.stellar_address().to_string();
        eprintln!("fresh account {addr}");

        let r = ensure_trustlines_with(&wallet, &cfg).expect("sponsored trustlines");
        assert!(r.account_created, "a fresh account must be created by the sponsored tx");
        assert_eq!(r.added, vec!["USDC", "EURC"]);
        eprintln!("sponsored-trustline tx: {:?}", r.tx);

        // The trustlines now exist on-chain (account funded with 0 XLM, reserves sponsored).
        let codes: Vec<String> = chain::public_balances(&addr).unwrap().into_iter().map(|b| b.code).collect();
        assert!(codes.iter().any(|c| c == "USDC") && codes.iter().any(|c| c == "EURC"), "trustlines present");

        // Idempotent: a second call adds nothing.
        let again = ensure_trustlines_with(&wallet, &cfg).unwrap();
        assert!(again.already && again.tx.is_none(), "second call must be a no-op");
    }
}
