//! ozky indexer (Z6) — a pure speed/availability layer over Stellar RPC. It polls
//! the pool contract's events and serves clients a commitment+view-tag scan stream
//! (and, iteration 2, Merkle paths + nullifier non-membership witnesses). It is
//! NEVER on the correctness/liveness path: every endpoint's data is re-derivable
//! from raw chain events, so a client can recover with the indexer offline.

mod accumulator;
mod events;
mod rpc;
mod state;
mod tree;

use rpc::Rpc;
use soroban_sdk::Env;
use state::{poll_once, State};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Method, Response, Server};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn json_response(body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
    Response::from_string(body).with_header(header)
}

fn main() {
    let rpc_url = env_or("RPC_URL", "https://soroban-testnet.stellar.org");
    let pool = std::env::var("POOL_ID").expect("POOL_ID env var (pool contract id) required");
    let port: u16 = env_or("PORT", "8080").parse().expect("PORT");
    let poll_secs: u64 = env_or("POLL_SECS", "6").parse().unwrap_or(6);
    let lookback: u32 = env_or("LOOKBACK", "120000").parse().unwrap_or(120_000);

    // Fail fast if the off-chain Poseidon2 ever drifts from the circuit/contract.
    assert!(
        tree::parity_self_test(&Env::default()),
        "Poseidon2 parity self-test failed — served Merkle paths would be invalid"
    );

    let rpc = Rpc::new(rpc_url.clone());
    let start = rpc
        .resolve_start(&pool, lookback)
        .expect("resolve start ledger");
    eprintln!("ozky-indexer: pool={pool} rpc={rpc_url} start_ledger={start} port={port}");

    let state = Arc::new(Mutex::new(State {
        start_ledger: start,
        ..Default::default()
    }));

    // Blocking initial ingest so the instance serves correct data immediately on
    // (cold) start — important under Cloud Run scale-to-zero, where each cold start
    // rebuilds state from chain. Non-fatal if it errors; the poller will retry.
    match poll_once(&rpc, &pool, &state) {
        Ok(n) => eprintln!("initial ingest: {n} events"),
        Err(e) => eprintln!("initial ingest error (will retry): {e}"),
    }

    // Background poller.
    {
        let state = Arc::clone(&state);
        let pool = pool.clone();
        thread::spawn(move || {
            let rpc = Rpc::new(rpc_url);
            loop {
                match poll_once(&rpc, &pool, &state) {
                    Ok(n) if n > 0 => eprintln!("poll: +{n} events"),
                    Ok(_) => {}
                    Err(e) => eprintln!("poll error: {e}"),
                }
                thread::sleep(Duration::from_secs(poll_secs));
            }
        });
    }

    let server = Server::http(("0.0.0.0", port)).expect("bind");
    eprintln!("listening on :{port}");
    for req in server.incoming_requests() {
        if *req.method() != Method::Get {
            let _ = req.respond(Response::from_string("method not allowed").with_status_code(405));
            continue;
        }
        let url = req.url().to_string();
        let path = url.split('?').next().unwrap_or("/");
        let query = url.split('?').nth(1).unwrap_or("");

        let resp_body = route(&state, path, query);
        match resp_body {
            Some(body) => {
                let _ = req.respond(json_response(body));
            }
            None => {
                let _ = req.respond(
                    Response::from_string("{\"error\":\"not found\"}").with_status_code(404),
                );
            }
        }
    }
}

/// A serving Env with metering disabled. This is an off-chain tool, not a ledger —
/// the host budget is irrelevant, and reconstructing trees does many Poseidon hashes
/// that would otherwise exhaust the default per-invocation budget.
fn fresh_env() -> Env {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let mut it = kv.splitn(2, '=');
        if it.next()? == key {
            Some(it.next().unwrap_or("").to_string())
        } else {
            None
        }
    })
}

fn route(state: &Arc<Mutex<State>>, path: &str, query: &str) -> Option<String> {
    let s = state.lock().unwrap();
    match path {
        "/health" => Some(format!("{{\"ok\":{}}}", s.healthy)),
        "/status" => Some(format!(
            "{{\"commitments\":{},\"nullifiers\":{},\"commitment_root\":{},\"nullifier_root\":{},\"last_ledger\":{},\"start_ledger\":{}}}",
            s.commits.len(),
            s.nullifiers.len(),
            opt_str(&s.commitment_root),
            opt_str(&s.nullifier_root),
            s.last_ledger,
            s.start_ledger,
        )),
        "/scan" => {
            let from: u32 = query_param(query, "from")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let items: Vec<String> = s
                .commits
                .iter()
                .filter(|c| c.leaf_index >= from)
                .map(commit_json)
                .collect();
            Some(format!("{{\"commitments\":[{}]}}", items.join(",")))
        }
        "/nullifiers" => {
            let items: Vec<String> = s.nullifiers.iter().map(|n| format!("\"{n}\"")).collect();
            Some(format!("{{\"nullifiers\":[{}]}}", items.join(",")))
        }
        p if p.starts_with("/nonmembership/") => {
            let target = p.trim_start_matches("/nonmembership/").to_string();
            let nullifiers = s.nullifiers.clone();
            let published = s.nullifier_root.clone();
            drop(s);
            let env = fresh_env();
            let nm = accumulator::non_membership(&env, &nullifiers, &target)?;
            let matches = published.as_deref() == Some(nm.root.as_str());
            let low_path = match &nm.low_path {
                Some(mp) => {
                    let sibs: Vec<String> = mp.siblings.iter().map(|x| format!("\"{x}\"")).collect();
                    let bits: Vec<String> = mp.path_is_right.iter().map(|b| b.to_string()).collect();
                    format!(
                        "{{\"path_is_right\":[{}],\"siblings\":[{}]}}",
                        bits.join(","),
                        sibs.join(",")
                    )
                }
                None => "null".to_string(),
            };
            Some(format!(
                "{{\"target\":\"{}\",\"present\":{},\"low_value\":\"{}\",\"low_next_index\":{},\"low_next_value\":\"{}\",\"low_index\":{},\"low_path\":{},\"root\":\"{}\",\"root_matches_published\":{},\"published_root\":{}}}",
                nm.target, nm.present, nm.low_value, nm.low_next_index, nm.low_next_value,
                nm.low_index, low_path, nm.root, matches, opt_str(&published),
            ))
        }
        "/nullifier_root" => {
            let nullifiers = s.nullifiers.clone();
            let published = s.nullifier_root.clone();
            drop(s);
            let env = fresh_env();
            let root = accumulator::root(&env, &nullifiers)?;
            let matches = published.as_deref() == Some(root.as_str());
            Some(format!(
                "{{\"reconstructed_root\":\"{}\",\"published_root\":{},\"root_matches_published\":{}}}",
                root, opt_str(&published), matches
            ))
        }
        p if p.starts_with("/path/") => {
            let index: u32 = p.trim_start_matches("/path/").parse().ok()?;
            // Ordered commitment leaves (sorted by leaf_index, contiguous from 0).
            let leaves: Vec<String> = s.commits.iter().map(|c| c.commitment.clone()).collect();
            let published = s.commitment_root.clone();
            drop(s);
            let env = fresh_env();
            let mp = tree::merkle_path(&env, &leaves, index)?;
            let matches = published.as_deref() == Some(mp.root.as_str());
            let sibs: Vec<String> = mp.siblings.iter().map(|x| format!("\"{x}\"")).collect();
            let bits: Vec<String> = mp.path_is_right.iter().map(|b| b.to_string()).collect();
            Some(format!(
                "{{\"leaf_index\":{},\"leaf\":\"{}\",\"root\":\"{}\",\"root_matches_published\":{},\"published_root\":{},\"path_is_right\":[{}],\"siblings\":[{}]}}",
                mp.leaf_index,
                mp.leaf,
                mp.root,
                matches,
                opt_str(&published),
                bits.join(","),
                sibs.join(","),
            ))
        }
        _ => None,
    }
}

fn opt_str(o: &Option<String>) -> String {
    match o {
        Some(v) => format!("\"{v}\""),
        None => "null".to_string(),
    }
}

fn commit_json(c: &events::Commit) -> String {
    format!(
        "{{\"leaf_index\":{},\"commitment\":\"{}\",\"enc_note\":{},\"ephemeral_pub\":{},\"view_tag\":{},\"ledger\":{},\"tx_hash\":\"{}\"}}",
        c.leaf_index,
        c.commitment,
        opt_str(&c.enc_note),
        opt_str(&c.ephemeral_pub),
        c.view_tag.map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
        c.ledger,
        c.tx_hash,
    )
}
