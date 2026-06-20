//! Chain client (Phase A2/A3). Reads the TARGET pool's events directly from Stellar
//! RPC (`getEvents`) and reconstructs its commitment + nullifier sets locally; the
//! wallet then rebuilds Merkle/accumulator witnesses itself (see [`super::witness`]).
//! This works against ANY pool with no external service — the ozky indexer (Z6) is
//! only ever an optional accelerator, raw RPC is the correctness path (spec: recovery
//! must work with the indexer offline).

use super::config::PoolConfig;
use super::poseidon::Fr;
use super::CoreError;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;
use stellar_xdr::curr::{Limits, ReadXdr, ScVal};

/// Ledgers per epoch (FROZEN, matches the pool contract's `LEDGER_PER_EPOCH`).
pub const LEDGER_PER_EPOCH: u64 = 110_000;

/// The target network. Testnet throughout Part 1/2; mainnet only after audit.
pub const DEFAULT_NETWORK: &str = "testnet";
pub const DEFAULT_RPC_URL: &str = "https://soroban-testnet.stellar.org";

/// How far back (ledgers) to scan a pool's events; testnet RPC retains ~120k.
const SCAN_LOOKBACK: u32 = 120_000;
/// Paging safety bounds for one event drain (mirrors the indexer's poller).
const MAX_PAGES: u32 = 500;
const EMPTY_TOLERANCE: u32 = 4;

/// One commitment leaf + its (optional) encrypted payload, decoded from a `commit`
/// event. (Same shape the indexer's `/scan` served, now sourced from raw RPC.)
#[derive(Debug, Clone)]
pub struct CommitEntry {
    pub leaf_index: u32,
    pub commitment: String,
    pub enc_note: Option<String>,
    pub ephemeral_pub: Option<String>,
    pub view_tag: Option<u32>,
}

/// A pool's reconstructed-from-chain state: commitment leaves (append order) + the
/// published nullifier set. One RPC drain produces both.
pub struct PoolState {
    pub commits: Vec<CommitEntry>,
    pub nullifiers: Vec<Fr>,
}

// ----------------------------- raw RPC -----------------------------

/// JSON-RPC call; returns the `result` object or the error message string (the latter
/// so callers like [`resolve_start`] can parse the RPC's retention floor out of it).
fn rpc_call(rpc_url: &str, method: &str, params: Value) -> Result<Value, String> {
    let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
    let resp: Value = ureq::post(rpc_url)
        .send_json(body)
        .map_err(|e| format!("rpc {method} transport: {e}"))?
        .into_json()
        .map_err(|e| format!("rpc {method} decode: {e}"))?;
    if let Some(err) = resp.get("error") {
        return Err(format!("rpc {method} error: {err}"));
    }
    resp.get("result").cloned().ok_or_else(|| format!("rpc {method}: no result"))
}

/// The current epoch = `latest_ledger_sequence / 110_000` (the pool's `current_epoch`).
/// Read live so a built proof's `epoch` public input matches on submit.
pub fn current_epoch(rpc_url: &str) -> Result<u32, CoreError> {
    let r = rpc_call(rpc_url, "getLatestLedger", json!({})).map_err(CoreError::Chain)?;
    let seq = r.get("sequence").and_then(|v| v.as_u64()).ok_or_else(|| {
        CoreError::Chain("getLatestLedger: no sequence".into())
    })?;
    Ok((seq / LEDGER_PER_EPOCH) as u32)
}

/// A raw contract event: ledger + base64-XDR topics and value.
struct RawEvent {
    ledger: u32,
    topics: Vec<String>,
    value: String,
}

/// One `getEvents` page: (events, next cursor).
fn get_events_page(
    rpc_url: &str,
    pool: &str,
    start_ledger: Option<u32>,
    cursor: Option<&str>,
) -> Result<(Vec<RawEvent>, Option<String>), String> {
    let mut pagination = json!({ "limit": 200 });
    let mut params = json!({ "filters": [{ "type": "contract", "contractIds": [pool] }] });
    if let Some(c) = cursor {
        pagination["cursor"] = json!(c);
    } else if let Some(s) = start_ledger {
        params["startLedger"] = json!(s);
    }
    params["pagination"] = pagination;

    let r = rpc_call(rpc_url, "getEvents", params)?;
    let cursor = r.get("cursor").and_then(|v| v.as_str()).map(String::from);
    let mut events = Vec::new();
    if let Some(arr) = r.get("events").and_then(|v| v.as_array()) {
        for e in arr {
            let topics = e
                .get("topic")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
                .unwrap_or_default();
            events.push(RawEvent {
                ledger: e.get("ledger").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                topics,
                value: e.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            });
        }
    }
    Ok((events, cursor))
}

/// A start ledger inside the RPC's retention window: `latest - lookback`, or the
/// retention floor parsed from the out-of-range error (the Z6 lesson).
fn resolve_start(rpc_url: &str, pool: &str) -> Result<u32, String> {
    let latest = rpc_call(rpc_url, "getLatestLedger", json!({}))?
        .get("sequence")
        .and_then(|v| v.as_u64())
        .ok_or("getLatestLedger: no sequence")? as u32;
    let want = latest.saturating_sub(SCAN_LOOKBACK).max(2);
    match get_events_page(rpc_url, pool, Some(want), None) {
        Ok(_) => Ok(want),
        Err(e) => e
            .split("range:")
            .nth(1)
            .and_then(|s| s.trim().split('-').next())
            .and_then(|s| s.trim().parse::<u32>().ok())
            .ok_or(e),
    }
}

// ----------------------------- event decode (ScVal XDR) -----------------------------

fn to_hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(2 + b.len() * 2);
    s.push_str("0x");
    for byte in b {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

fn scval(b64: &str) -> Option<ScVal> {
    ScVal::from_xdr_base64(b64, Limits::none()).ok()
}

fn u256_hex(v: &ScVal) -> Option<String> {
    if let ScVal::U256(p) = v {
        let mut b = [0u8; 32];
        b[0..8].copy_from_slice(&p.hi_hi.to_be_bytes());
        b[8..16].copy_from_slice(&p.hi_lo.to_be_bytes());
        b[16..24].copy_from_slice(&p.lo_hi.to_be_bytes());
        b[24..32].copy_from_slice(&p.lo_lo.to_be_bytes());
        Some(to_hex(&b))
    } else {
        None
    }
}

fn bytes_hex(v: &ScVal) -> Option<String> {
    if let ScVal::Bytes(b) = v {
        Some(to_hex(b.0.as_slice()))
    } else {
        None
    }
}

/// Decoded pool event we care about for spending (roots are recomputed locally).
enum Decoded {
    Commit(CommitEntry),
    Nullifier(String),
}

/// Classify a `commit`/`nullif` event (mirrors `indexer/src/events.rs`). `commit`
/// value is `Vec[U256, Bytes enc, Bytes eph, U32 tag]` (deposit/transfer) or a bare
/// `U256` (withdraw change); topic[1] is the leaf index.
fn classify(e: &RawEvent) -> Option<Decoded> {
    let name = match scval(e.topics.first()?)? {
        ScVal::Symbol(s) => String::from_utf8_lossy(s.0.as_slice()).to_string(),
        _ => return None,
    };
    let value = scval(&e.value)?;
    match name.as_str() {
        "commit" => {
            let leaf_index = match scval(e.topics.get(1)?)? {
                ScVal::U32(n) => n,
                _ => return None,
            };
            let (commitment, enc_note, ephemeral_pub, view_tag) = match &value {
                ScVal::Vec(Some(items)) => (
                    u256_hex(items.first()?)?,
                    items.get(1).and_then(bytes_hex),
                    items.get(2).and_then(bytes_hex),
                    items.get(3).and_then(|v| match v {
                        ScVal::U32(n) => Some(*n),
                        _ => None,
                    }),
                ),
                ScVal::U256(_) => (u256_hex(&value)?, None, None, None),
                _ => return None,
            };
            Some(Decoded::Commit(CommitEntry {
                leaf_index,
                commitment,
                enc_note,
                ephemeral_pub,
                view_tag,
            }))
        }
        "nullif" => Some(Decoded::Nullifier(u256_hex(&value)?)),
        _ => None,
    }
}

// ----------------------------- pool state -----------------------------

/// Drain all of a pool's `commit`/`nullif` events from RPC and reconstruct its state.
/// Pages via the cursor to the tip (the Z6 drain: keep paging while the cursor
/// advances; stop after a few empty windows once events have been seen).
pub fn pool_state(cfg: &PoolConfig) -> Result<PoolState, CoreError> {
    let pool = &cfg.pool_contract;
    let start = resolve_start(&cfg.rpc_url, pool).map_err(CoreError::Chain)?;

    let mut commits: Vec<CommitEntry> = Vec::new();
    let mut nullifiers: Vec<Fr> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut total = 0usize;
    let mut empty_run = 0u32;

    for _ in 0..MAX_PAGES {
        let (events, next) = get_events_page(
            &cfg.rpc_url,
            pool,
            if cursor.is_none() { Some(start) } else { None },
            cursor.as_deref(),
        )
        .map_err(CoreError::Chain)?;
        let n = events.len();
        total += n;

        for raw in &events {
            match classify(raw) {
                Some(Decoded::Commit(c)) => {
                    if !commits.iter().any(|x| x.leaf_index == c.leaf_index) {
                        commits.push(c);
                    }
                }
                Some(Decoded::Nullifier(v)) => {
                    let f = Fr::from_hex(&v)
                        .ok_or_else(|| CoreError::Chain(format!("bad nullifier hex: {v}")))?;
                    if !nullifiers.contains(&f) {
                        nullifiers.push(f);
                    }
                }
                None => {}
            }
        }

        let advanced = next.is_some() && next != cursor;
        if next.is_some() {
            cursor = next;
        }
        if n == 0 {
            empty_run += 1;
            if !advanced || (total > 0 && empty_run >= EMPTY_TOLERANCE) {
                break;
            }
        } else {
            empty_run = 0;
        }
    }

    commits.sort_by_key(|c| c.leaf_index);
    Ok(PoolState { commits, nullifiers })
}

/// Commitment leaves in tree order (for local Merkle-path reconstruction).
pub fn commitment_leaves_from(commits: &[CommitEntry]) -> Result<Vec<Fr>, CoreError> {
    commits
        .iter()
        .map(|c| {
            Fr::from_hex(&c.commitment)
                .ok_or_else(|| CoreError::Chain(format!("bad commitment hex: {}", c.commitment)))
        })
        .collect()
}

// ----------------------------- on-chain submission -----------------------------
//
// Each entrypoint (deposit / transfer / withdraw) is invoked via the stellar CLI in
// the ZK container (a Docker-free native submitter is a later packaging task). The
// proof + public_inputs are referenced by their in-container paths (`/workspace/...`,
// written there by `proving`). The source secret is forwarded as `$OZKY_SOURCE_SECRET`
// (an env var, never argv). The `*_invoke_script` builders are pure + unit-tested.

/// The encrypted output payloads to publish with a transfer (one per output note).
pub struct OutputPayload {
    pub enc_note: Vec<u8>,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u32,
}

fn hex_array(items: impl Iterator<Item = String>) -> String {
    let quoted: Vec<String> = items.map(|h| format!("\"{h}\"")).collect();
    format!("[{}]", quoted.join(","))
}

/// The `set -e` + `stellar network add` prelude shared by every invoke script.
fn prelude(cfg: &PoolConfig) -> String {
    format!(
        "set -e; \
         stellar network add {net} --rpc-url {rpc} --network-passphrase '{pass}' 2>/dev/null || true; ",
        net = cfg.network,
        rpc = cfg.rpc_url,
        pass = cfg.network_passphrase,
    )
}

/// `stellar contract invoke --id <pool> --source $OZKY_SOURCE_SECRET --network <net> --send yes --`
fn invoke_head(cfg: &PoolConfig) -> String {
    format!(
        "stellar contract invoke --id {pool} --source \"$OZKY_SOURCE_SECRET\" --network {net} --send yes -- ",
        pool = cfg.pool_contract,
        net = cfg.network,
    )
}

/// `deposit` invoke: lock `amount` of the asset from `from` into the vault, mint the
/// proven note, and publish its encrypted payload (so the wallet can rescan it).
pub fn deposit_invoke_script(
    cfg: &PoolConfig,
    from: &str,
    amount: u64,
    public_inputs_path: &str,
    proof_path: &str,
    out: &OutputPayload,
) -> String {
    format!(
        "{prelude}{head}deposit --from {from} --asset_tag {asset} --amount {amount} \
           --public_inputs-file-path {pi} --proof-file-path {pf} \
           --enc_note {enc} --ephemeral_pub {eph} --view_tag {vt}",
        prelude = prelude(cfg),
        head = invoke_head(cfg),
        from = from,
        asset = cfg.asset_tag_decimal(),
        amount = amount,
        pi = public_inputs_path,
        pf = proof_path,
        enc = hex::encode(&out.enc_note),
        eph = hex::encode(out.ephemeral_pub),
        vt = out.view_tag,
    )
}

/// `transfer` invoke: 2-in/2-out private transfer; publishes both output payloads.
pub fn transfer_invoke_script(
    cfg: &PoolConfig,
    public_inputs_path: &str,
    proof_path: &str,
    outputs: &[OutputPayload],
) -> String {
    let enc_notes = hex_array(outputs.iter().map(|o| hex::encode(&o.enc_note)));
    let ephemeral_pubs = hex_array(outputs.iter().map(|o| hex::encode(o.ephemeral_pub)));
    let view_tags = format!(
        "[{}]",
        outputs.iter().map(|o| o.view_tag.to_string()).collect::<Vec<_>>().join(",")
    );
    format!(
        "{prelude}{head}transfer --asset_tag {asset} \
           --public_inputs-file-path {pi} --proof-file-path {pf} \
           --enc_notes '{enc}' --ephemeral_pubs '{eph}' --view_tags '{vt}'",
        prelude = prelude(cfg),
        head = invoke_head(cfg),
        asset = cfg.asset_tag_decimal(),
        pi = public_inputs_path,
        pf = proof_path,
        enc = enc_notes,
        eph = ephemeral_pubs,
        vt = view_tags,
    )
}

/// `withdraw` invoke: release `amount` of the asset to the public `dest`, re-commit
/// the shielded change. (The contract publishes no ciphertext for the change note.)
pub fn withdraw_invoke_script(
    cfg: &PoolConfig,
    dest: &str,
    amount: u64,
    public_inputs_path: &str,
    proof_path: &str,
) -> String {
    format!(
        "{prelude}{head}withdraw --dest {dest} --asset_tag {asset} --amount {amount} \
           --public_inputs-file-path {pi} --proof-file-path {pf}",
        prelude = prelude(cfg),
        head = invoke_head(cfg),
        dest = dest,
        asset = cfg.asset_tag_decimal(),
        amount = amount,
        pi = public_inputs_path,
        pf = proof_path,
    )
}

fn repo_root() -> PathBuf {
    if let Ok(p) = std::env::var("OZKY_REPO_ROOT") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Run an invoke script in the ZK container; return the tx hash (parsed from the CLI
/// logs) or a `Chain` error with the stderr tail. `what` names the op for errors.
fn run_invoke(source_secret: &str, what: &str, script: &str) -> Result<String, CoreError> {
    let compose = repo_root().join("compose.zk.yaml");
    let out = Command::new("docker")
        .env("OZKY_SOURCE_SECRET", source_secret)
        .args(["compose", "-f"])
        .arg(&compose)
        .args(["run", "--rm", "-e", "OZKY_SOURCE_SECRET", "zk", "bash", "-c", script])
        .output()
        .map_err(|e| CoreError::Chain(format!("spawn docker: {e}")))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(15).collect();
        let tail: String = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(CoreError::Chain(format!("{what} submit failed:\n{tail}")));
    }
    // The CLI logs the tx hash to stderr ("Transaction hash is <hash>").
    let stderr = String::from_utf8_lossy(&out.stderr);
    let hash = stderr
        .lines()
        .find_map(|l| l.split("hash is ").nth(1))
        .map(|h| h.trim().to_string())
        .unwrap_or_else(|| "submitted".to_string());
    Ok(hash)
}

/// Submit a `deposit` to the pool. Returns the transaction hash.
pub fn submit_deposit(
    cfg: &PoolConfig,
    source_secret: &str,
    from: &str,
    amount: u64,
    public_inputs_path: &str,
    proof_path: &str,
    out: &OutputPayload,
) -> Result<String, CoreError> {
    let script = deposit_invoke_script(cfg, from, amount, public_inputs_path, proof_path, out);
    run_invoke(source_secret, "deposit", &script)
}

/// Submit a `transfer` to the pool. Returns the transaction hash.
pub fn submit_transfer(
    cfg: &PoolConfig,
    source_secret: &str,
    public_inputs_path: &str,
    proof_path: &str,
    outputs: &[OutputPayload],
) -> Result<String, CoreError> {
    let script = transfer_invoke_script(cfg, public_inputs_path, proof_path, outputs);
    run_invoke(source_secret, "transfer", &script)
}

/// Submit a `withdraw` to the pool. Returns the transaction hash.
pub fn submit_withdraw(
    cfg: &PoolConfig,
    source_secret: &str,
    dest: &str,
    amount: u64,
    public_inputs_path: &str,
    proof_path: &str,
) -> Result<String, CoreError> {
    let script = withdraw_invoke_script(cfg, dest, amount, public_inputs_path, proof_path);
    run_invoke(source_secret, "withdraw", &script)
}
