//! Onboarding account funder client (scope: server/GKE funder).
//!
//! A brand-new Stellar account doesn't exist on-chain until something runs a classic
//! `CreateAccount` op funding it above the base reserve. The app can't self-fund (it has
//! no XLM and no account yet) and a Soroban contract can't create accounts, so a
//! server-held funded key does it. This posts the account address to the funder service
//! (`OZKY_FUNDER_URL`); the service runs `CreateAccount(10 XLM)` and returns. Best-effort
//! and idempotent: a funder that already funded the address returns success.

use super::config::cfg_var;
use super::CoreError;

/// The configured funder endpoint (`OZKY_FUNDER_URL`), or `None` (funding disabled — e.g.
/// dev without the service deployed).
pub fn funder_url() -> Option<String> {
    cfg_var("OZKY_FUNDER_URL")
}

/// Ask the funder service to create + fund `address` with the onboarding grant (10 XLM).
/// Returns `Ok(false)` when no funder is configured (caller treats funding as skipped);
/// `Ok(true)` on a 2xx; `Err` on a transport/HTTP error. `address` is a `G…` strkey.
pub fn request_funding(address: &str) -> Result<bool, CoreError> {
    let Some(url) = funder_url() else {
        return Ok(false);
    };
    // `address` is a base32 strkey (no quoting hazards) — build the JSON body by hand to
    // avoid depending on ureq's optional json feature.
    let body = format!("{{\"address\":\"{address}\"}}");
    let mut req = ureq::post(&url).set("Content-Type", "application/json");
    if let Some(token) = cfg_var("OZKY_FUNDER_TOKEN") {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    req.send_string(&body)
        .map(|_| true)
        .map_err(|e| CoreError::Chain(format!("funder request failed: {e}")))
}
