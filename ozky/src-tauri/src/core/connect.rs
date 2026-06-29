//! Service discovery via the marketing site's `/connect` broker.
//!
//! The shipped app does NOT hardcode the GCP (Cloud Run) backend URLs or the deployed
//! contract IDs. Instead it asks the website's `/connect` endpoint (`OZKY_CONNECT_URL`,
//! default `https://ozky.vercel.app/connect`), which reads the URLs + non-secret config
//! from its own env vars and live-probes each service's `/health`. The app uses the
//! returned links to reach the servers and the returned `config` (pool/policy/viewkeys
//! contract IDs + network endpoints) as a `cfg_var` fallback — so a build with no
//! `ozky.config.json` still resolves `OZKY_POOL_CONTRACT` etc. If a needed server is
//! missing or down, the UI shows a "service unavailable — contact the developer" popup.
//!
//! Dev override: a present `ozky.config.json` (or env) value short-circuits the matching
//! lookup, so discovery isn't required while developing.

use super::config::cfg_var;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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
    #[serde(default)]
    config: HashMap<String, String>,
}

/// In-memory cache of the non-secret config the broker returned (`OZKY_*` → value).
/// Read by `config::cfg_var` as the last fallback after env + `ozky.config.json`.
fn discovered_cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A config value the broker previously returned, if any.
pub fn discovered_var(key: &str) -> Option<String> {
    discovered_cache().lock().ok()?.get(key).cloned()
}

fn store_config(cfg: HashMap<String, String>) {
    if let Ok(mut g) = discovered_cache().lock() {
        for (k, v) in cfg {
            if !v.trim().is_empty() {
                g.insert(k, v);
            }
        }
    }
}

/// POST the broker and parse the discovery result, caching any returned config. Never
/// errors: a broker/transport failure yields `Discovery::default()` (everything down),
/// which the UI treats as "services unavailable".
pub fn discover() -> Discovery {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(8)).build();
    match agent.post(&connect_url()).call() {
        Ok(resp) => match resp.into_json::<ConnectResponse>() {
            Ok(parsed) => {
                store_config(parsed.config);
                Discovery {
                    broker_reachable: true,
                    reachable: parsed.reachable,
                    services: parsed.services,
                }
            }
            Err(_) => Discovery { broker_reachable: true, ..Default::default() },
        },
        Err(_) => Discovery::default(),
    }
}

/// Lazily populate the config cache from the broker when a needed contract ID isn't yet
/// known — used by `PoolConfig::load()` in a build without `ozky.config.json`. No-op once
/// the cache has the pool contract; throttled to one attempt / 20s so a down broker can't
/// stall every call with repeated timeouts.
pub fn ensure_discovered() {
    if discovered_var("OZKY_POOL_CONTRACT").is_some() {
        return;
    }
    static LAST: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    let gate = LAST.get_or_init(|| Mutex::new(None));
    let attempt = {
        let mut g = match gate.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let now = Instant::now();
        let ok = g.map_or(true, |t| now.duration_since(t) >= Duration::from_secs(20));
        if ok {
            *g = Some(now);
        }
        ok
    };
    if attempt {
        discover();
    }
}
