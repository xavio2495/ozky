//! `ozky-keeper` — the headless payroll submitter (next-build scope #2, phase K4).
//!
//! Fires the pre-proved payroll bundles the app queued, via a relayer, on schedule. It holds NO
//! `owner_sk`: it decrypts its queue with the wallet's `notes_key` (a one-way HMAC of the seed —
//! you cannot derive a spend key from it) and submits with the relayer secret. So a headless host
//! can RELAY a pre-authorized spend but cannot FORGE one. A submitted proof's nullifier is consumed
//! ⇒ no replay. See `claude-docs/headless_keeper_interface.md`.
//!
//! Modes:
//!   `ozky-keeper --once`   local mode (free tier): load the queue, submit due bundles, write
//!                          results back. Invoked by an OS scheduled task (K6).
//!   `ozky-keeper --serve`  cloud keeper (K7 premium follow-up) — not implemented yet.
//!
//! Env (no `owner_sk`): `OZKY_KEEPER_NOTES_KEY` (64-hex at-rest queue key), `OZKY_KEEPER_ADDRESS`
//! (the `G…` wallet address = queue filename), plus the usual pool config (`OZKY_POOL_CONTRACT`,
//! `OZKY_RPC_URL`, `OZKY_RELAYER_SECRET`, …) — typically `ozky.config.json` next to the repo.

use ozky_lib::core::config::PoolConfig;
use ozky_lib::core::keeper::{self, KeeperKeys};
use ozky_lib::core::payroll;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or_default();
    match mode {
        "--once" => {
            // Optional `--cred <path>` (used by the OS-scheduled task, which inherits no env).
            let cred = args.iter().position(|a| a == "--cred").and_then(|i| args.get(i + 1).cloned());
            if let Err(e) = run_once(cred) {
                eprintln!("ozky-keeper: {e}");
                std::process::exit(1);
            }
        }
        "--serve" => {
            if let Err(e) = serve() {
                eprintln!("ozky-keeper: {e}");
                std::process::exit(1);
            }
        }
        other => {
            eprintln!("usage: ozky-keeper --once [--cred <path>] | --serve   (got: {other:?})");
            std::process::exit(64);
        }
    }
}

/// Submit every due run once, then exit. Idempotent: already-submitted chunks are skipped.
///
/// Key material comes from a `--cred` file (the scheduled-task path: it sets `OZKY_NOTES_DIR` +
/// `OZKY_CONFIG` so the binary reads the same queue + pool config the app uses) or, if no `--cred`,
/// from `OZKY_KEEPER_NOTES_KEY` + `OZKY_KEEPER_ADDRESS` env (manual / test use). Never `owner_sk`.
fn run_once(cred: Option<String>) -> Result<(), String> {
    let keys = match cred {
        Some(path) => {
            let (keys, cred) = ozky_lib::core::keeper::load_cred(std::path::Path::new(&path))
                .map_err(|e| format!("load cred {path}: {e}"))?;
            // Point config + queue at exactly what the app uses.
            std::env::set_var("OZKY_NOTES_DIR", &cred.notes_dir);
            std::env::set_var("OZKY_CONFIG", &cred.config);
            keys
        }
        None => KeeperKeys::new(parse_key(&env("OZKY_KEEPER_NOTES_KEY")?)?, env("OZKY_KEEPER_ADDRESS")?),
    };

    let cfg = PoolConfig::load().map_err(|e| format!("load config: {e}"))?;
    let relayer = cfg
        .relayer_secret
        .clone()
        .ok_or("no OZKY_RELAYER_SECRET configured (the keeper submits via a relayer)")?;
    let now = payroll::now();

    let outcomes =
        keeper::submit_due(&keys, &cfg, &relayer, now).map_err(|e| format!("submit_due: {e}"))?;

    if outcomes.is_empty() {
        println!("ozky-keeper: no due runs");
        return Ok(());
    }
    let mut failed = false;
    for o in &outcomes {
        match &o.error {
            None => println!(
                "ozky-keeper: payroll {} submitted {} chunk(s): {:?}",
                o.payroll_id, o.submitted, o.tx_hashes
            ),
            Some(e) => {
                failed = true;
                println!(
                    "ozky-keeper: payroll {} submitted {} chunk(s) then ABORTED: {e}",
                    o.payroll_id, o.submitted
                );
            }
        }
    }
    if failed {
        return Err("one or more runs aborted (see above)".into());
    }
    Ok(())
}

/// Cloud keeper (K7): an authed HTTP endpoint that accepts pushed runs from the app and submits
/// due ones on a timer using this user's DEDICATED capped relayer. Holds no `notes_key` / `owner_sk`
/// — pushed bundles carry no key material, so a leak risks only the relayer's small fee float.
///
/// Env: `OZKY_KEEPER_BIND` (default `127.0.0.1:8787`), `OZKY_KEEPER_TOKEN` (per-user bearer),
/// plus pool config (`OZKY_RELAYER_SECRET`, …). Routes (all require `Authorization: Bearer <token>`):
///   POST `/push`        body = a KeeperRun JSON  → store it (replaces any run for that payroll)
///   DELETE `/run/<id>`  → drop the stored run (revoke)
///   GET `/status`       → the stored runs' summaries
fn serve() -> Result<(), String> {
    use ozky_lib::core::keeper;

    let bind = std::env::var("OZKY_KEEPER_BIND").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let token = env("OZKY_KEEPER_TOKEN")?;
    let cfg = PoolConfig::load().map_err(|e| format!("load config: {e}"))?;
    let relayer = cfg
        .relayer_secret
        .clone()
        .ok_or("no OZKY_RELAYER_SECRET configured (the cloud keeper submits via this user's relayer)")?;

    // Background submitter: every 60 s, submit any due stored run.
    {
        let cfg = cfg.clone();
        std::thread::spawn(move || loop {
            let now = payroll::now();
            if let Ok(mut store) = keeper::load_cloud_store() {
                let mut changed = false;
                for run in store.runs.iter_mut() {
                    match keeper::submit_run(run, &cfg, &relayer, now) {
                        Ok(Some(o)) => {
                            changed = true;
                            eprintln!(
                                "ozky-keeper(serve): payroll {} submitted {} chunk(s) (err: {:?})",
                                o.payroll_id, o.submitted, o.error
                            );
                        }
                        Ok(None) => {}
                        Err(e) => eprintln!("ozky-keeper(serve): submit error: {e}"),
                    }
                }
                if changed {
                    let _ = keeper::save_cloud_store(&store);
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(60));
        });
    }

    let server = tiny_http::Server::http(&bind).map_err(|e| format!("bind {bind}: {e}"))?;
    eprintln!("ozky-keeper serving on {bind}");
    for mut req in server.incoming_requests() {
        let authed = req.headers().iter().any(|h| {
            h.field.equiv("Authorization") && h.value.as_str() == format!("Bearer {token}")
        });
        let method = req.method().clone();
        let url = req.url().to_string();
        if !authed {
            let _ = req.respond(tiny_http::Response::from_string("unauthorized").with_status_code(401));
            continue;
        }
        let (code, body) = match (method, url.as_str()) {
            (tiny_http::Method::Post, "/push") => {
                let mut buf = String::new();
                if std::io::Read::read_to_string(req.as_reader(), &mut buf).is_err() {
                    (400, "bad body".to_string())
                } else {
                    match serde_json::from_str::<keeper::KeeperRun>(&buf) {
                        Ok(run) => {
                            let mut store = keeper::load_cloud_store().unwrap_or_default();
                            let pid = run.payroll_id;
                            store.upsert_run(run);
                            match keeper::save_cloud_store(&store) {
                                Ok(()) => (200, format!("stored run {pid}")),
                                Err(e) => (500, format!("store: {e}")),
                            }
                        }
                        Err(e) => (400, format!("bad run json: {e}")),
                    }
                }
            }
            (tiny_http::Method::Get, "/status") => {
                let store = keeper::load_cloud_store().unwrap_or_default();
                let summary: Vec<_> = store
                    .runs
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "payroll_id": r.payroll_id,
                            "chunks": r.bundles.len(),
                            "submitted": r.last_result.as_ref().map(|x| x.tx_hashes.len()).unwrap_or(0),
                            "error": r.last_result.as_ref().and_then(|x| x.error.clone()),
                        })
                    })
                    .collect();
                (200, serde_json::Value::Array(summary).to_string())
            }
            (tiny_http::Method::Delete, path) if path.starts_with("/run/") => {
                match path.trim_start_matches("/run/").parse::<u64>() {
                    Ok(id) => {
                        let mut store = keeper::load_cloud_store().unwrap_or_default();
                        let removed = store.remove_run(id);
                        let _ = keeper::save_cloud_store(&store);
                        (200, format!("removed: {removed}"))
                    }
                    Err(_) => (400, "bad id".to_string()),
                }
            }
            _ => (404, "not found".to_string()),
        };
        let _ = req.respond(tiny_http::Response::from_string(body).with_status_code(code));
    }
    Ok(())
}

fn env(k: &str) -> Result<String, String> {
    std::env::var(k)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("missing env {k}"))
}

fn parse_key(h: &str) -> Result<[u8; 32], String> {
    let b = hex::decode(h.strip_prefix("0x").unwrap_or(h))
        .map_err(|_| "OZKY_KEEPER_NOTES_KEY is not valid hex")?;
    if b.len() != 32 {
        return Err(format!("OZKY_KEEPER_NOTES_KEY must be 32 bytes, got {}", b.len()));
    }
    let mut k = [0u8; 32];
    k.copy_from_slice(&b);
    Ok(k)
}
