//! ozky cloud keeper (scope #2, K7) — the managed headless payroll submitter.
//!
//! A standalone Cloud Run service (sibling to the indexer). The wallet app PUSHes pre-proved payroll
//! runs here; a Cloud Scheduler cron hits `/tick` and the service submits any DUE bundle's proof via
//! this user's DEDICATED relayer. It holds NO `owner_sk` and NO `notes_key` — a `KeeperBundle` carries
//! no key material and no plaintext amounts, so a leak risks only the relayer's small fee float, never
//! user funds. A submitted proof's nullifier is consumed ⇒ no replay.
//!
//! Cloud Run posture: `--min-instances 0` ⇒ $0 idle (nothing to shut down). Work is request-driven
//! (`/tick` from Cloud Scheduler), so it does NOT rely on a background thread that scale-to-zero would
//! suspend. The run store is in-memory (per warm instance) — push then tick within the warm window;
//! GCS-backed durability across cold starts is the production follow-up (see README).
//!
//! Routes (all except `/health` require `Authorization: Bearer $OZKY_KEEPER_TOKEN`):
//!   GET  /health        liveness
//!   POST /push          body = a KeeperRun JSON  → store it (replaces any run for that payroll)
//!   GET  /status        the stored runs' summaries
//!   DELETE /run/<id>    drop the stored run (revoke)
//!   POST /tick          submit every DUE run now; returns the outcomes (Cloud Scheduler calls this)
//!
//! Env: PORT (Cloud Run sets it), OZKY_KEEPER_TOKEN, OZKY_POOL_CONTRACT, OZKY_RPC_URL,
//! OZKY_NETWORK_PASSPHRASE, OZKY_RELAYER_SECRET.

mod chain;
mod gcs;

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// The GCS object name the run store is persisted under (one per service/pool).
const STORE_OBJECT: &str = "keeper-store.json";

// --- bundle types (mirror the app's serde shape in core/keeper.rs) -------------------

#[derive(Deserialize, Serialize, Clone)]
struct OutputPayload {
    enc_note: Vec<u8>,
    ephemeral_pub: [u8; 32],
    view_tag: u32,
}

#[derive(Deserialize, Serialize, Clone, PartialEq)]
enum BundleMethod {
    Split,
    Transfer4,
}

#[derive(Deserialize, Serialize, Clone)]
struct KeeperBundle {
    bundle_id: String,
    payroll_id: u64,
    asset: String,
    asset_tag: String,
    pool_contract: String,
    method: BundleMethod,
    proof: Vec<u8>,
    public_inputs: Vec<u8>,
    outputs: Vec<OutputPayload>,
    bound_epoch: u32,
    nullifier_old: String,
    nullifier_new: String,
    commitment_root: String,
    earliest_submit_unix: i64,
    chain_index: u32,
    chain_len: u32,
}

#[derive(Deserialize, Serialize, Clone, Default)]
struct RunResult {
    submitted_unix: i64,
    tx_hashes: Vec<String>,
    error: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
struct KeeperRun {
    payroll_id: u64,
    bundles: Vec<KeeperBundle>,
    #[serde(default)]
    last_result: Option<RunResult>,
}

#[derive(Default, Deserialize, Serialize)]
struct Store {
    runs: Vec<KeeperRun>,
}

struct Config {
    rpc_url: String,
    passphrase: String,
    pool_contract: String,
    relayer_secret: String,
    token: String,
    port: u16,
    /// GCS bucket for cold-start durability; `None` ⇒ in-memory only (off-GCP / local).
    bucket: Option<String>,
}

/// Load the run store from GCS on boot (empty if no bucket configured or no object yet). Resilient:
/// a load error logs + starts empty rather than crashing the service.
fn load_store(cfg: &Config) -> Store {
    let Some(bucket) = cfg.bucket.as_deref() else {
        return Store::default();
    };
    match gcs::get_object(bucket, STORE_OBJECT) {
        Ok(Some(bytes)) => match serde_json::from_slice::<Store>(&bytes) {
            Ok(s) => {
                eprintln!("loaded {} run(s) from gs://{bucket}/{STORE_OBJECT}", s.runs.len());
                s
            }
            Err(e) => {
                eprintln!("WARN: decode store from GCS failed ({e}); starting empty");
                Store::default()
            }
        },
        Ok(None) => {
            eprintln!("no store object yet at gs://{bucket}/{STORE_OBJECT}; starting empty");
            Store::default()
        }
        Err(e) => {
            eprintln!("WARN: load store from GCS failed ({e}); starting empty");
            Store::default()
        }
    }
}

/// Persist the store to GCS after a mutation (no-op without a bucket). Best-effort: a failure logs
/// but does not fail the request — the in-memory store stays authoritative for the warm instance.
fn persist_store(cfg: &Config, store: &Store) {
    let Some(bucket) = cfg.bucket.as_deref() else {
        return;
    };
    match serde_json::to_vec(store) {
        Ok(body) => {
            if let Err(e) = gcs::put_object(bucket, STORE_OBJECT, &body) {
                eprintln!("WARN: persist store to GCS failed: {e}");
            }
        }
        Err(e) => eprintln!("WARN: encode store for GCS failed: {e}"),
    }
}

fn env(k: &str) -> Result<String, String> {
    std::env::var(k).ok().filter(|s| !s.trim().is_empty()).ok_or_else(|| format!("missing env {k}"))
}

fn load_config() -> Result<Config, String> {
    Ok(Config {
        rpc_url: std::env::var("OZKY_RPC_URL")
            .unwrap_or_else(|_| "https://soroban-testnet.stellar.org".into()),
        passphrase: std::env::var("OZKY_NETWORK_PASSPHRASE")
            .unwrap_or_else(|_| "Test SDF Network ; September 2015".into()),
        pool_contract: env("OZKY_POOL_CONTRACT")?,
        relayer_secret: env("OZKY_RELAYER_SECRET")?,
        token: env("OZKY_KEEPER_TOKEN")?,
        port: std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080),
        bucket: std::env::var("OZKY_KEEPER_BUCKET").ok().filter(|s| !s.trim().is_empty()),
    })
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// --- submit (request-driven, /tick) -------------------------------------------------

#[derive(Serialize)]
struct TickOutcome {
    payroll_id: u64,
    submitted: usize,
    tx_hashes: Vec<String>,
    error: Option<String>,
}

/// Submit one run's remaining chunks in order; abort on first failure. Pre-flight: pool match +
/// epoch still current (the on-chain verifier enforces the nullifier root, so no tree rebuild here).
fn submit_run(cfg: &Config, run: &mut KeeperRun, now: i64) -> Option<TickOutcome> {
    if run.bundles.is_empty() {
        return None;
    }
    let already = run.last_result.as_ref().map_or(0, |r| r.tx_hashes.len());
    if already >= run.bundles.len() {
        return None; // complete
    }
    if run.bundles[0].earliest_submit_unix > now {
        return None; // not due
    }

    let live_epoch = match chain::current_epoch(&cfg.rpc_url) {
        Ok(e) => e,
        Err(e) => {
            let res = RunResult { submitted_unix: now, tx_hashes: vec![], error: Some(e.clone()) };
            run.last_result = Some(res);
            return Some(TickOutcome { payroll_id: run.payroll_id, submitted: 0, tx_hashes: vec![], error: Some(e) });
        }
    };

    let mut tx_hashes: Vec<String> =
        run.last_result.as_ref().map(|r| r.tx_hashes.clone()).unwrap_or_default();
    let start = tx_hashes.len();
    let mut error: Option<String> = None;

    for bundle in run.bundles.iter().skip(start) {
        if bundle.pool_contract != cfg.pool_contract {
            error = Some(format!(
                "pool mismatch: bundle {} != configured {}",
                bundle.pool_contract, cfg.pool_contract
            ));
            break;
        }
        if bundle.bound_epoch != live_epoch {
            error = Some(format!("epoch rolled: bundle {} != live {live_epoch}", bundle.bound_epoch));
            break;
        }
        let fn_name = match bundle.method {
            BundleMethod::Split => "split",
            BundleMethod::Transfer4 => "transfer4",
        };
        let outs: Vec<(Vec<u8>, [u8; 32], u32)> = bundle
            .outputs
            .iter()
            .map(|o| (o.enc_note.clone(), o.ephemeral_pub, o.view_tag))
            .collect();
        match chain::submit_bundle(
            &cfg.rpc_url,
            &cfg.passphrase,
            &cfg.relayer_secret,
            &bundle.pool_contract,
            fn_name,
            &bundle.asset_tag,
            &bundle.public_inputs,
            &bundle.proof,
            &outs,
        ) {
            Ok(hash) => tx_hashes.push(hash),
            Err(e) => {
                error = Some(format!("chunk submit failed: {e}"));
                break;
            }
        }
    }

    let submitted = tx_hashes.len() - start;
    run.last_result = Some(RunResult { submitted_unix: now, tx_hashes: tx_hashes.clone(), error: error.clone() });
    Some(TickOutcome { payroll_id: run.payroll_id, submitted, tx_hashes, error })
}

// --- HTTP server --------------------------------------------------------------------

fn main() {
    let cfg = match load_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ozky-keeper-service: {e}");
            std::process::exit(1);
        }
    };
    let store: Mutex<Store> = Mutex::new(load_store(&cfg));
    let addr = format!("0.0.0.0:{}", cfg.port);
    let server = match tiny_http::Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ozky-keeper-service: bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("ozky cloud keeper serving on {addr} (pool {})", cfg.pool_contract);

    for mut req in server.incoming_requests() {
        let method = req.method().clone();
        let url = req.url().to_string();

        // /health is open; everything else needs the bearer token.
        if method == tiny_http::Method::Get && url == "/health" {
            respond(req, 200, "ok");
            continue;
        }
        let authed = req.headers().iter().any(|h| {
            h.field.equiv("Authorization") && h.value.as_str() == format!("Bearer {}", cfg.token)
        });
        if !authed {
            respond(req, 401, "unauthorized");
            continue;
        }

        let (code, body) = match (&method, url.as_str()) {
            (tiny_http::Method::Post, "/push") => {
                let mut buf = String::new();
                if std::io::Read::read_to_string(req.as_reader(), &mut buf).is_err() {
                    (400, "bad body".to_string())
                } else {
                    match serde_json::from_str::<KeeperRun>(&buf) {
                        Ok(run) => {
                            let pid = run.payroll_id;
                            let mut s = store.lock().unwrap();
                            match s.runs.iter_mut().find(|r| r.payroll_id == pid) {
                                Some(slot) => *slot = run,
                                None => s.runs.push(run),
                            }
                            persist_store(&cfg, &s);
                            (200, format!("stored run {pid}"))
                        }
                        Err(e) => (400, format!("bad run json: {e}")),
                    }
                }
            }
            (tiny_http::Method::Get, "/status") => {
                let s = store.lock().unwrap();
                let summary: Vec<_> = s
                    .runs
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "payroll_id": r.payroll_id,
                            "chunks": r.bundles.len(),
                            "submitted": r.last_result.as_ref().map(|x| x.tx_hashes.len()).unwrap_or(0),
                            "tx_hashes": r.last_result.as_ref().map(|x| x.tx_hashes.clone()).unwrap_or_default(),
                            "error": r.last_result.as_ref().and_then(|x| x.error.clone()),
                        })
                    })
                    .collect();
                (200, serde_json::Value::Array(summary).to_string())
            }
            (tiny_http::Method::Post, "/tick") => {
                let now = now_unix();
                let mut s = store.lock().unwrap();
                let mut outcomes: Vec<TickOutcome> = Vec::new();
                for run in s.runs.iter_mut() {
                    if let Some(o) = submit_run(&cfg, run, now) {
                        outcomes.push(o);
                    }
                }
                // Persist only if a run actually advanced (every /tick otherwise would rewrite GCS).
                if !outcomes.is_empty() {
                    persist_store(&cfg, &s);
                }
                match serde_json::to_string(&outcomes) {
                    Ok(j) => (200, j),
                    Err(e) => (500, format!("encode outcomes: {e}")),
                }
            }
            (tiny_http::Method::Delete, path) if path.starts_with("/run/") => {
                match path.trim_start_matches("/run/").parse::<u64>() {
                    Ok(id) => {
                        let mut s = store.lock().unwrap();
                        let before = s.runs.len();
                        s.runs.retain(|r| r.payroll_id != id);
                        let removed = before != s.runs.len();
                        if removed {
                            persist_store(&cfg, &s);
                        }
                        (200, format!("removed: {removed}"))
                    }
                    Err(_) => (400, "bad id".to_string()),
                }
            }
            _ => (404, "not found".to_string()),
        };
        respond(req, code, &body);
    }
}

fn respond(req: tiny_http::Request, code: u16, body: &str) {
    let _ = req.respond(tiny_http::Response::from_string(body).with_status_code(code));
}
