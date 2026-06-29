//! Service discovery via the marketing site's `/connect` broker.
//!
//! The shipped app does NOT hardcode the GCP (Cloud Run) backend URLs. Instead it asks
//! the website's `/connect` endpoint (`OZKY_CONNECT_URL`, default `https://ozky.vercel.app
//! /connect`), which reads the URLs from its own env vars and live-probes each service's
//! `/health`. The app uses the returned links to reach the servers; if a needed server is
//! missing or down, the UI shows a "service unavailable — contact the developer" popup.
//!
//! Dev override: a present `ozky.config.json` (or env) `OZKY_FUNDER_URL` short-circuits the
//! funder lookup to a local service, so discovery isn't required while developing.

use super::config::cfg_var;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const DEFAULT_CONNECT_URL: &str = "https://ozky.vercel.app/connect";

/// The broker endpoint to query (`OZKY_CONNECT_URL`, else the public site).
pub fn connect_url() -> String {
    cfg_var("OZKY_CONNECT_URL").unwrap_or_else(|| DEFAULT_CONNECT_URL.to_string())
}

/// One service's discovered link + liveness.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ServiceInfo {
    pub url: Option<String>,
    pub up: bool,
}

/// The three backend services the app talks to.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Services {
    pub funder: ServiceInfo,
    pub indexer: ServiceInfo,
    pub keeper: ServiceInfo,
}

/// Result of a discovery call. `broker_reachable` is whether the website `/connect`
/// answered at all; `reachable` is whether at least one backend service is up.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Discovery {
    pub broker_reachable: bool,
    pub reachable: bool,
    pub services: Services,
}

/// The JSON shape the `/connect` endpoint returns.
#[derive(Deserialize)]
struct ConnectResponse {
    #[serde(default)]
    reachable: bool,
    #[serde(default)]
    services: Services,
}

/// POST the broker and parse the discovery result. Never errors: a broker/transport
/// failure yields `Discovery::default()` (everything down), which the UI treats as
/// "services unavailable".
pub fn discover() -> Discovery {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(8)).build();
    match agent.post(&connect_url()).call() {
        Ok(resp) => match resp.into_json::<ConnectResponse>() {
            Ok(parsed) => Discovery {
                broker_reachable: true,
                reachable: parsed.reachable,
                services: parsed.services,
            },
            Err(_) => Discovery { broker_reachable: true, ..Default::default() },
        },
        Err(_) => Discovery::default(),
    }
}
