//! Chain client (Phase A2/A3). Reads the TARGET pool's events directly from Stellar
//! RPC (`getEvents`) and reconstructs its commitment + nullifier sets locally; the
//! wallet then rebuilds Merkle/accumulator witnesses itself (see [`super::witness`]).
//! This works against ANY pool with no external service — the ozky indexer (Z6) is
//! only ever an optional accelerator, raw RPC is the correctness path (spec: recovery
//! must work with the indexer offline).

use super::config::PoolConfig;
use super::poseidon::Fr;
use super::{notes, CoreError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use stellar_xdr::curr::{
    AccountId, BytesM, ContractId, DecoratedSignature, Hash, HostFunction, Int128Parts,
    InvokeContractArgs, InvokeHostFunctionOp, LedgerEntryData, LedgerKey, LedgerKeyAccount, Limits,
    Memo, MuxedAccount, Operation, OperationBody, Preconditions, PublicKey, ReadXdr, ScAddress,
    ScBytes, ScMap, ScMapEntry, ScSymbol, ScVal, ScVec, SequenceNumber, SorobanAuthorizationEntry,
    SorobanTransactionData, Transaction, TransactionEnvelope, TransactionExt,
    TransactionV1Envelope, UInt256Parts, Uint256, VecM, WriteXdr,
};

/// Ledgers per epoch (FROZEN, matches the pool contract's `LEDGER_PER_EPOCH`).
pub const LEDGER_PER_EPOCH: u64 = 110_000;

/// The target network. Testnet throughout Part 1/2; mainnet only after audit.
pub const DEFAULT_NETWORK: &str = "testnet";
pub const DEFAULT_RPC_URL: &str = "https://soroban-testnet.stellar.org";
/// Horizon (classic) endpoint — for reading the wallet's PUBLIC account balances
/// (the unshielded side). Overridable via `OZKY_HORIZON_URL`.
pub const DEFAULT_HORIZON_URL: &str = "https://horizon-testnet.stellar.org";

/// A public (unshielded) balance on the wallet's classic Stellar account.
#[derive(serde::Serialize)]
pub struct PublicBalance {
    /// "XLM" for the native asset, else the asset code (e.g. "USDC").
    pub code: String,
    /// Human-readable amount (Horizon returns it already scaled).
    pub balance: String,
    /// Classic issuer (`G…`) for non-native assets.
    pub issuer: Option<String>,
}

/// Read the PUBLIC (unshielded) balances of a classic Stellar account from Horizon.
/// An unfunded account (404) returns an empty list rather than an error.
pub fn public_balances(addr: &str) -> Result<Vec<PublicBalance>, CoreError> {
    let url = format!(
        "{}/accounts/{}",
        super::config::cfg_var("OZKY_HORIZON_URL").unwrap_or_else(|| DEFAULT_HORIZON_URL.into()),
        addr
    );
    let resp = match ureq::get(&url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(404, _)) => return Ok(vec![]), // account not yet funded
        Err(e) => return Err(CoreError::Chain(format!("horizon: {e}"))),
    };
    let v: serde_json::Value = resp
        .into_json()
        .map_err(|e| CoreError::Chain(format!("horizon decode: {e}")))?;
    let mut out = Vec::new();
    if let Some(arr) = v.get("balances").and_then(|b| b.as_array()) {
        for b in arr {
            let asset_type = b.get("asset_type").and_then(|x| x.as_str()).unwrap_or("");
            let balance = b.get("balance").and_then(|x| x.as_str()).unwrap_or("0").to_string();
            if asset_type == "native" {
                out.push(PublicBalance { code: "XLM".into(), balance, issuer: None });
            } else {
                out.push(PublicBalance {
                    code: b.get("asset_code").and_then(|x| x.as_str()).unwrap_or("?").to_string(),
                    balance,
                    issuer: b.get("asset_issuer").and_then(|x| x.as_str()).map(String::from),
                });
            }
        }
    }
    Ok(out)
}

/// How far back (ledgers) to scan a pool's events; testnet RPC retains ~120k.
const SCAN_LOOKBACK: u32 = 120_000;
/// Paging safety bounds for one event drain (mirrors the indexer's poller).
const MAX_PAGES: u32 = 500;
const EMPTY_TOLERANCE: u32 = 4;

/// One commitment leaf + its (optional) encrypted payload, decoded from a `commit`
/// event. (Same shape the indexer's `/scan` served, now sourced from raw RPC.)
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// The latest ledger sequence. Used to turn a user's escrow deadline (a wall-clock instant) into
/// a ledger number for `open_escrow`, and to decide release/refund eligibility (guard: `ledger >
/// deadline`). Ledgers close ~every 5s on testnet.
pub fn latest_ledger(rpc_url: &str) -> Result<u32, CoreError> {
    let r = rpc_call(rpc_url, "getLatestLedger", json!({})).map_err(CoreError::Chain)?;
    r.get("sequence")
        .and_then(|v| v.as_u64())
        .map(|s| s as u32)
        .ok_or_else(|| CoreError::Chain("getLatestLedger: no sequence".into()))
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

/// An on-disk, per-pool **incremental scan cache** (G9). Pool events are public, so this
/// is plaintext (the secret-bearing openings live in the encrypted notes store). A scan
/// resumes from `cursor_ledger` (the highest ledger seen so far) instead of re-draining
/// the whole retention window, then appends only the new events.
#[derive(Serialize, Deserialize, Default)]
struct PoolCache {
    cursor_ledger: u32,
    commits: Vec<CommitEntry>,
    /// Published nullifiers (hex) seen so far.
    nullifiers: Vec<String>,
}

/// `poolcache-<pool id>.json` under the app data dir. Pool ids are StrKey (filesystem-safe).
fn cache_path(pool: &str) -> PathBuf {
    notes::data_dir().join(format!("poolcache-{pool}.json"))
}

/// Load the pool's scan cache (empty/default on any miss — the cache is best-effort and
/// never a correctness dependency: a missing/corrupt cache just means a full re-drain).
fn load_cache(pool: &str) -> PoolCache {
    std::fs::read(cache_path(pool))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Persist the pool's scan cache (best-effort: a write failure never fails the scan).
fn save_cache(pool: &str, c: &PoolCache) {
    if let Ok(b) = serde_json::to_vec(c) {
        let dir = notes::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(cache_path(pool), b);
    }
}

/// Reconstruct a pool's `commit`/`nullif` state from RPC. **Incremental (G9):** seeds
/// from the per-pool cache and resumes the `getEvents` drain from the last ledger seen,
/// so a repeat call costs O(new events) instead of re-draining the whole window. The
/// cumulative set is identical to a full drain (events are append-only + deduped); set
/// `OZKY_NO_POOL_CACHE` to force a fresh full drain. Pages to the tip via the cursor
/// (the Z6 drain: keep paging while the cursor advances; stop after a few empty windows
/// once events have been seen).
pub fn pool_state(cfg: &PoolConfig) -> Result<PoolState, CoreError> {
    let pool = &cfg.pool_contract;
    let use_cache = std::env::var("OZKY_NO_POOL_CACHE").is_err();

    // Seed accumulators from the cache (resume), or start empty (fresh full drain).
    let cache = if use_cache { load_cache(pool) } else { PoolCache::default() };
    let mut commits: Vec<CommitEntry> = cache.commits;
    let mut nullifiers: Vec<Fr> = cache
        .nullifiers
        .iter()
        .filter_map(|h| Fr::from_hex(h))
        .collect();

    // Resume from the cached cursor; else from the retention-window start.
    let mut start = if use_cache && cache.cursor_ledger > 0 {
        cache.cursor_ledger
    } else {
        resolve_start(&cfg.rpc_url, pool).map_err(CoreError::Chain)?
    };

    let mut cursor: Option<String> = None;
    let mut total = 0usize;
    let mut empty_run = 0u32;
    let mut max_ledger = start;
    let mut tried_fallback = false;

    for _ in 0..MAX_PAGES {
        let page = get_events_page(
            &cfg.rpc_url,
            pool,
            if cursor.is_none() { Some(start) } else { None },
            cursor.as_deref(),
        );
        let (events, next) = match page {
            Ok(p) => p,
            // The cached cursor aged out of the RPC retention window: fall back to a
            // fresh in-window start ONCE (older cached commits stay; the unqueryable gap
            // is the same horizon the non-cached path has, so this is no regression).
            Err(_) if use_cache && cursor.is_none() && !tried_fallback => {
                tried_fallback = true;
                start = resolve_start(&cfg.rpc_url, pool).map_err(CoreError::Chain)?;
                max_ledger = max_ledger.max(start);
                continue;
            }
            Err(e) => return Err(CoreError::Chain(e)),
        };
        let n = events.len();
        total += n;

        for raw in &events {
            if raw.ledger > max_ledger {
                max_ledger = raw.ledger;
            }
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

    if use_cache {
        save_cache(
            pool,
            &PoolCache {
                cursor_ledger: max_ledger,
                commits: commits.clone(),
                nullifiers: nullifiers.iter().map(|f| f.to_hex()).collect(),
            },
        );
    }

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

// ----------------------------- ASP approved set -----------------------------

/// Decode a policy `asp_mem` event → `(leaf index, owner_pk)`. Topics are
/// `[Symbol("asp_mem"), U32 index]`; value is the bare `U256 owner_pk` (single-value).
fn classify_member(e: &RawEvent) -> Option<(u32, Fr)> {
    match scval(e.topics.first()?)? {
        ScVal::Symbol(s) if s.0.as_slice() == b"asp_mem" => {}
        _ => return None,
    }
    let index = match scval(e.topics.get(1)?)? {
        ScVal::U32(n) => n,
        _ => return None,
    };
    let owner_pk = Fr::from_hex(&u256_hex(&scval(&e.value)?)?)?;
    Some((index, owner_pk))
}

/// Reconstruct the ASP approved set (the ordered `owner_pk` leaves) by draining the
/// policy contract's `asp_mem` events from raw RPC — so a client builds its membership
/// path with no indexer (the reconstructed root self-checks against the pool's
/// `asp_root`). Returns leaves in enrollment order (= Merkle leaf order).
pub fn approved_set(cfg: &PoolConfig) -> Result<Vec<Fr>, CoreError> {
    let policy = &cfg.policy_contract;
    let start = resolve_start(&cfg.rpc_url, policy).map_err(CoreError::Chain)?;

    let mut members: Vec<(u32, Fr)> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut total = 0usize;
    let mut empty_run = 0u32;

    for _ in 0..MAX_PAGES {
        let (events, next) = get_events_page(
            &cfg.rpc_url,
            policy,
            if cursor.is_none() { Some(start) } else { None },
            cursor.as_deref(),
        )
        .map_err(CoreError::Chain)?;
        let n = events.len();
        total += n;
        for raw in &events {
            if let Some((index, owner_pk)) = classify_member(raw) {
                if !members.iter().any(|(i, _)| *i == index) {
                    members.push((index, owner_pk));
                }
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

    members.sort_by_key(|(i, _)| *i);
    Ok(members.into_iter().map(|(_, pk)| pk).collect())
}

// ----------------------------- on-chain submission (native, G14) -----------------------------
//
// Each entrypoint (deposit / transfer / withdraw / enroll / disclose) is built, signed,
// and submitted NATIVELY here — no stellar CLI, no Docker, and the source secret never
// leaves this process. The flow per op (the standard Soroban path): build the
// `InvokeHostFunction` transaction (proof + public_inputs passed as `Bytes` straight
// from the in-memory [`super::proving::ProofBundle`]) → `simulateTransaction` for the
// resource fee + footprint + auth → assemble + Ed25519-sign ([`super::sign`]) →
// `sendTransaction` → poll `getTransaction`. (Proving still runs in the ZK container;
// a Docker-free prover is a separate packaging task.)

/// The encrypted output payloads to publish with a transfer (one per output note).
pub struct OutputPayload {
    pub enc_note: Vec<u8>,
    pub ephemeral_pub: [u8; 32],
    pub view_tag: u32,
}

// --- ScVal argument builders (contract param types) ---

/// A BN254 field element / `U256` from 32 big-endian bytes.
fn sc_u256_be(bytes: &[u8; 32]) -> ScVal {
    ScVal::U256(UInt256Parts {
        hi_hi: u64::from_be_bytes(bytes[0..8].try_into().unwrap()),
        hi_lo: u64::from_be_bytes(bytes[8..16].try_into().unwrap()),
        lo_hi: u64::from_be_bytes(bytes[16..24].try_into().unwrap()),
        lo_lo: u64::from_be_bytes(bytes[24..32].try_into().unwrap()),
    })
}

fn sc_u256_fr(fr: &Fr) -> ScVal {
    sc_u256_be(&fr.0)
}

/// A `U256` from a decimal string (e.g. an `owner_pk` / `asset_tag` decimal).
fn sc_u256_decimal(dec: &str) -> Result<ScVal, CoreError> {
    let n = num_bigint::BigUint::parse_bytes(dec.as_bytes(), 10)
        .ok_or_else(|| CoreError::Chain(format!("bad U256 decimal: {dec}")))?;
    let be = n.to_bytes_be();
    if be.len() > 32 {
        return Err(CoreError::Chain(format!("U256 overflow: {dec}")));
    }
    let mut b = [0u8; 32];
    b[32 - be.len()..].copy_from_slice(&be);
    Ok(sc_u256_be(&b))
}

/// An `i128` from a non-negative `u64` token amount (hi = 0).
fn sc_i128(amount: u64) -> ScVal {
    ScVal::I128(Int128Parts { hi: 0, lo: amount })
}

/// `Bytes` / `BytesN<N>` from a byte slice.
fn sc_bytes(b: &[u8]) -> Result<ScVal, CoreError> {
    let bm: BytesM = b
        .to_vec()
        .try_into()
        .map_err(|_| CoreError::Chain("bytes argument too long".into()))?;
    Ok(ScVal::Bytes(ScBytes(bm)))
}

/// A classic-account `Address` (`G…`) argument.
fn sc_account(g: &str) -> Result<ScVal, CoreError> {
    let pk = stellar_strkey::ed25519::PublicKey::from_string(g)
        .map_err(|e| CoreError::Chain(format!("bad address {g}: {e}")))?;
    Ok(ScVal::Address(ScAddress::Account(AccountId(
        PublicKey::PublicKeyTypeEd25519(Uint256(pk.0)),
    ))))
}

fn sc_symbol_str(s: &str) -> Result<ScSymbol, CoreError> {
    Ok(ScSymbol(
        s.try_into()
            .map_err(|_| CoreError::Chain(format!("bad symbol: {s}")))?,
    ))
}

fn sc_symbol_val(s: &str) -> Result<ScVal, CoreError> {
    Ok(ScVal::Symbol(sc_symbol_str(s)?))
}

fn sc_vec(items: Vec<ScVal>) -> Result<ScVal, CoreError> {
    let v: VecM<ScVal> = items
        .try_into()
        .map_err(|_| CoreError::Chain("vec argument too long".into()))?;
    Ok(ScVal::Vec(Some(ScVec(v))))
}

/// A `ViewScope { account: u32, asset_tag: U256, epoch: u32 }` struct, encoded as the
/// Soroban map form (entries ordered by symbol key: account < asset_tag < epoch).
fn sc_view_scope(account: u32, asset_tag_dec: &str, epoch: u32) -> Result<ScVal, CoreError> {
    let entries = vec![
        ScMapEntry { key: sc_symbol_val("account")?, val: ScVal::U32(account) },
        ScMapEntry { key: sc_symbol_val("asset_tag")?, val: sc_u256_decimal(asset_tag_dec)? },
        ScMapEntry { key: sc_symbol_val("epoch")?, val: ScVal::U32(epoch) },
    ];
    let m: VecM<ScMapEntry> = entries
        .try_into()
        .map_err(|_| CoreError::Chain("scope map".into()))?;
    Ok(ScVal::Map(Some(ScMap(m))))
}

/// A contract `Address` (`C…`) for the invoke target.
fn contract_address(c: &str) -> Result<ScAddress, CoreError> {
    let id = stellar_strkey::Contract::from_string(c)
        .map_err(|e| CoreError::Chain(format!("bad contract id {c}: {e}")))?;
    Ok(ScAddress::Contract(ContractId(Hash(id.0))))
}

// --- native build / simulate / sign / submit ---

/// Base inclusion fee per operation (stroops); the resource fee from simulation is
/// added on top.
const BASE_FEE: u32 = 100;

/// The source account's current sequence number (via `getLedgerEntries`).
fn account_seq(rpc_url: &str, account: &AccountId) -> Result<i64, CoreError> {
    let key = LedgerKey::Account(LedgerKeyAccount { account_id: account.clone() });
    let kb64 = key
        .to_xdr_base64(Limits::none())
        .map_err(|e| CoreError::Chain(format!("xdr ledger key: {e}")))?;
    let r = rpc_call(rpc_url, "getLedgerEntries", json!({ "keys": [kb64] })).map_err(CoreError::Chain)?;
    let xdr = r
        .get("entries")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|e| e.get("xdr"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| CoreError::Chain("source account not found on-chain (unfunded?)".into()))?;
    match LedgerEntryData::from_xdr_base64(xdr, Limits::none())
        .map_err(|e| CoreError::Chain(format!("decode account entry: {e}")))?
    {
        LedgerEntryData::Account(a) => Ok(a.seq_num.0),
        _ => Err(CoreError::Chain("ledger entry is not an account".into())),
    }
}

/// A single-op `InvokeHostFunction` transaction.
fn build_tx(
    source: &MuxedAccount,
    seq: i64,
    fee: u32,
    host_function: HostFunction,
    auth: VecM<SorobanAuthorizationEntry>,
    ext: TransactionExt,
) -> Result<Transaction, CoreError> {
    let op = Operation {
        source_account: None,
        body: OperationBody::InvokeHostFunction(InvokeHostFunctionOp { host_function, auth }),
    };
    let operations: VecM<Operation, 100> = vec![op]
        .try_into()
        .map_err(|_| CoreError::Chain("operations".into()))?;
    Ok(Transaction {
        source_account: source.clone(),
        fee,
        seq_num: SequenceNumber(seq),
        cond: Preconditions::None,
        memo: Memo::None,
        operations,
        ext,
    })
}

/// Turn a raw simulate diagnostic (often a multi-KB XDR/event dump) into a short,
/// actionable message. Recognizes the common asset-funding failures; otherwise keeps
/// just the head of the diagnostic.
fn friendly_sim_error(fn_name: &str, err: &str) -> String {
    if err.contains("trustline entry is missing") {
        return format!(
            "{fn_name}: your account has no trustline for this asset — add a trustline and \
             fund it with the asset before depositing."
        );
    }
    if err.contains("insufficient") || err.contains("balance is not sufficient") {
        return format!("{fn_name}: insufficient balance of this asset in your account.");
    }
    if err.contains("#13") {
        return format!(
            "{fn_name}: the deposit token transfer failed (no trustline or insufficient \
             balance for this asset)."
        );
    }
    let head: String = err.chars().take(180).collect();
    format!("{fn_name} simulate failed: {head}")
}

/// Decode the auth entries simulation says the op needs (empty for our permissionless
/// transfer/withdraw; source-account credentials — covered by the tx signature — for the
/// deposit/enroll/disclose flows where the required address IS the source account).
fn parse_sim_auth(sim: &Value) -> Result<VecM<SorobanAuthorizationEntry>, CoreError> {
    let mut entries: Vec<SorobanAuthorizationEntry> = Vec::new();
    if let Some(auths) = sim
        .get("results")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|r| r.get("auth"))
        .and_then(|v| v.as_array())
    {
        for a in auths {
            if let Some(s) = a.as_str() {
                entries.push(
                    SorobanAuthorizationEntry::from_xdr_base64(s, Limits::none())
                        .map_err(|e| CoreError::Chain(format!("decode auth entry: {e}")))?,
                );
            }
        }
    }
    entries
        .try_into()
        .map_err(|_| CoreError::Chain("auth entries".into()))
}

/// Build → simulate → sign → submit → poll an `InvokeHostFunction` against `contract_id`.
/// `source_secret` (wallet or relayer `S…`) is signed natively and never leaves this
/// process. Returns the confirmed transaction hash.
fn invoke_contract(
    cfg: &PoolConfig,
    source_secret: &str,
    contract_id: &str,
    fn_name: &str,
    args: Vec<ScVal>,
) -> Result<String, CoreError> {
    let signer = super::sign::Signer::from_secret(source_secret)?;
    let source = signer.muxed();
    let seq = account_seq(&cfg.rpc_url, &signer.account_id())? + 1;

    let call_args: VecM<ScVal> = args
        .try_into()
        .map_err(|_| CoreError::Chain("too many call args".into()))?;
    let host_function = HostFunction::InvokeContract(InvokeContractArgs {
        contract_address: contract_address(contract_id)?,
        function_name: sc_symbol_str(fn_name)?,
        args: call_args,
    });

    // 1. Simulate (placeholder fee, empty auth, V0 ext) → resource fee + footprint + auth.
    let sim_tx = build_tx(&source, seq, BASE_FEE, host_function.clone(), VecM::default(), TransactionExt::V0)?;
    let sim_env = TransactionEnvelope::Tx(TransactionV1Envelope { tx: sim_tx, signatures: VecM::default() });
    let sim_b64 = sim_env
        .to_xdr_base64(Limits::none())
        .map_err(|e| CoreError::Chain(format!("xdr sim envelope: {e}")))?;
    let sim = rpc_call(&cfg.rpc_url, "simulateTransaction", json!({ "transaction": sim_b64 }))
        .map_err(CoreError::Chain)?;
    if let Some(err) = sim.get("error").and_then(|v| v.as_str()) {
        return Err(CoreError::Chain(friendly_sim_error(fn_name, err)));
    }
    if sim.get("restorePreamble").map(|v| !v.is_null()).unwrap_or(false) {
        return Err(CoreError::Chain(format!(
            "{fn_name}: contract state needs restore (archived entries)"
        )));
    }
    let soroban_data = sim
        .get("transactionData")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CoreError::Chain(format!("{fn_name} simulate: no transactionData")))?;
    let soroban_data = SorobanTransactionData::from_xdr_base64(soroban_data, Limits::none())
        .map_err(|e| CoreError::Chain(format!("decode soroban data: {e}")))?;
    let min_resource_fee: u64 = sim
        .get("minResourceFee")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| CoreError::Chain(format!("{fn_name} simulate: no minResourceFee")))?;
    let auth = parse_sim_auth(&sim)?;

    // 2. Assemble the final tx: fee = base + resource fee, V1 (soroban) ext, op auth.
    let fee = BASE_FEE.saturating_add(u32::try_from(min_resource_fee).unwrap_or(u32::MAX));
    let tx = build_tx(&source, seq, fee, host_function, auth, TransactionExt::V1(soroban_data))?;

    // 3. Sign natively + envelope.
    let sig = super::sign::sign_transaction(&signer, &cfg.network_passphrase, &tx)?;
    let signatures: VecM<DecoratedSignature, 20> = vec![sig]
        .try_into()
        .map_err(|_| CoreError::Chain("signatures".into()))?;
    let env = TransactionEnvelope::Tx(TransactionV1Envelope { tx, signatures });
    let env_b64 = env
        .to_xdr_base64(Limits::none())
        .map_err(|e| CoreError::Chain(format!("xdr envelope: {e}")))?;

    // 4. Submit + poll for confirmation.
    submit_and_poll(&cfg.rpc_url, fn_name, &env_b64)
}

/// `sendTransaction` then poll `getTransaction` to SUCCESS/FAILED. Returns the hash.
fn submit_and_poll(rpc_url: &str, what: &str, env_b64: &str) -> Result<String, CoreError> {
    let send = rpc_call(rpc_url, "sendTransaction", json!({ "transaction": env_b64 }))
        .map_err(CoreError::Chain)?;
    let hash = send.get("hash").and_then(|v| v.as_str()).unwrap_or("").to_string();
    match send.get("status").and_then(|v| v.as_str()).unwrap_or("") {
        "PENDING" | "DUPLICATE" => {}
        "ERROR" => {
            let detail = send.get("errorResultXdr").and_then(|v| v.as_str()).unwrap_or("");
            return Err(CoreError::Chain(format!("{what} send ERROR: {detail}")));
        }
        "TRY_AGAIN_LATER" => {
            return Err(CoreError::Chain(format!("{what} send: try again later (seq/rate)")))
        }
        other => return Err(CoreError::Chain(format!("{what} send: unexpected status {other}"))),
    }
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(1000));
        let r = rpc_call(rpc_url, "getTransaction", json!({ "hash": hash })).map_err(CoreError::Chain)?;
        match r.get("status").and_then(|v| v.as_str()).unwrap_or("NOT_FOUND") {
            "SUCCESS" => return Ok(hash),
            "FAILED" => {
                let rx = r.get("resultXdr").and_then(|v| v.as_str()).unwrap_or("");
                return Err(CoreError::Chain(format!("{what} FAILED on-chain: {rx}")));
            }
            _ => continue, // NOT_FOUND → keep polling
        }
    }
    Err(CoreError::Chain(format!("{what}: timed out awaiting confirmation (hash {hash})")))
}

/// Submit a `deposit` to the pool. Returns the transaction hash.
#[allow(clippy::too_many_arguments)]
pub fn submit_deposit(
    cfg: &PoolConfig,
    source_secret: &str,
    from: &str,
    amount: u64,
    public_inputs: &[u8],
    proof: &[u8],
    out: &OutputPayload,
) -> Result<String, CoreError> {
    let args = vec![
        sc_account(from)?,
        sc_u256_fr(&cfg.asset_tag),
        sc_i128(amount),
        sc_bytes(public_inputs)?,
        sc_bytes(proof)?,
        sc_bytes(&out.enc_note)?,
        sc_bytes(&out.ephemeral_pub)?,
        ScVal::U32(out.view_tag),
    ];
    invoke_contract(cfg, source_secret, &cfg.pool_contract, "deposit", args)
}

/// Submit a `transfer` to the pool. Returns the transaction hash.
pub fn submit_transfer(
    cfg: &PoolConfig,
    source_secret: &str,
    public_inputs: &[u8],
    proof: &[u8],
    outputs: &[OutputPayload],
) -> Result<String, CoreError> {
    let enc_notes = sc_vec(
        outputs
            .iter()
            .map(|o| sc_bytes(&o.enc_note))
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    let ephemeral_pubs = sc_vec(
        outputs
            .iter()
            .map(|o| sc_bytes(&o.ephemeral_pub))
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    let view_tags = sc_vec(outputs.iter().map(|o| ScVal::U32(o.view_tag)).collect())?;
    let args = vec![
        sc_u256_fr(&cfg.asset_tag),
        sc_bytes(public_inputs)?,
        sc_bytes(proof)?,
        enc_notes,
        ephemeral_pubs,
        view_tags,
    ];
    invoke_contract(cfg, source_secret, &cfg.pool_contract, "transfer", args)
}

/// Submit a `split` (2-in / 8-out) to the pool. `outputs` must be the 8 output payloads
/// (recipients, change, dummies) in the same order as the proof's out-commitments.
pub fn submit_split(
    cfg: &PoolConfig,
    source_secret: &str,
    public_inputs: &[u8],
    proof: &[u8],
    outputs: &[OutputPayload],
) -> Result<String, CoreError> {
    let enc_notes = sc_vec(
        outputs
            .iter()
            .map(|o| sc_bytes(&o.enc_note))
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    let ephemeral_pubs = sc_vec(
        outputs
            .iter()
            .map(|o| sc_bytes(&o.ephemeral_pub))
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    let view_tags = sc_vec(outputs.iter().map(|o| ScVal::U32(o.view_tag)).collect())?;
    let args = vec![
        sc_u256_fr(&cfg.asset_tag),
        sc_bytes(public_inputs)?,
        sc_bytes(proof)?,
        enc_notes,
        ephemeral_pubs,
        view_tags,
    ];
    invoke_contract(cfg, source_secret, &cfg.pool_contract, "split", args)
}

/// Submit a `withdraw` to the pool. Returns the transaction hash.
pub fn submit_withdraw(
    cfg: &PoolConfig,
    source_secret: &str,
    dest: &str,
    amount: u64,
    public_inputs: &[u8],
    proof: &[u8],
) -> Result<String, CoreError> {
    let args = vec![
        sc_account(dest)?,
        sc_u256_fr(&cfg.asset_tag),
        sc_i128(amount),
        sc_bytes(public_inputs)?,
        sc_bytes(proof)?,
    ];
    invoke_contract(cfg, source_secret, &cfg.pool_contract, "withdraw", args)
}

// ----------------------------- escrow (building block B) -----------------------------

/// Public escrow state read from the pool's `escrow(id)` view (the fields the client needs).
pub struct EscrowState {
    pub asset_tag: Fr,
    pub target: u64,
    pub deadline: u64,
    pub mode: u32,
    pub payee_bind: Fr,
    pub c_raised: Fr,
    /// The running commitment point coordinates (identity = (0,0)), for the next contributor's fold.
    pub raised_x: Fr,
    pub raised_y: Fr,
    pub n_contrib: u32,
    pub status: u32,
}

fn sc_u64(n: u64) -> ScVal {
    ScVal::U64(n)
}

fn u256_to_fr(v: &ScVal) -> Option<Fr> {
    if let ScVal::U256(p) = v {
        let mut b = [0u8; 32];
        b[0..8].copy_from_slice(&p.hi_hi.to_be_bytes());
        b[8..16].copy_from_slice(&p.hi_lo.to_be_bytes());
        b[16..24].copy_from_slice(&p.lo_hi.to_be_bytes());
        b[24..32].copy_from_slice(&p.lo_lo.to_be_bytes());
        Some(Fr(b))
    } else {
        None
    }
}

/// Simulate a read-only contract call and return its result `ScVal` (no signing/submit).
fn simulate_invoke(cfg: &PoolConfig, contract: &str, fn_name: &str, args: Vec<ScVal>) -> Result<ScVal, CoreError> {
    // A throwaway source just to form a well-typed simulate envelope (no signature needed).
    let source = MuxedAccount::Ed25519(Uint256([0u8; 32]));
    let call_args: VecM<ScVal> = args.try_into().map_err(|_| CoreError::Chain("too many args".into()))?;
    let host_function = HostFunction::InvokeContract(InvokeContractArgs {
        contract_address: contract_address(contract)?,
        function_name: sc_symbol_str(fn_name)?,
        args: call_args,
    });
    let tx = build_tx(&source, 1, BASE_FEE, host_function, VecM::default(), TransactionExt::V0)?;
    let env = TransactionEnvelope::Tx(TransactionV1Envelope { tx, signatures: VecM::default() });
    let b64 = env.to_xdr_base64(Limits::none()).map_err(|e| CoreError::Chain(format!("xdr: {e}")))?;
    let sim = rpc_call(&cfg.rpc_url, "simulateTransaction", json!({ "transaction": b64 })).map_err(CoreError::Chain)?;
    if let Some(err) = sim.get("error").and_then(|v| v.as_str()) {
        return Err(CoreError::Chain(friendly_sim_error(fn_name, err)));
    }
    let xdr = sim
        .get("results")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|r| r.get("xdr"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| CoreError::Chain(format!("{fn_name}: no result returnValue")))?;
    ScVal::from_xdr_base64(xdr, Limits::none()).map_err(|e| CoreError::Chain(format!("decode {fn_name} result: {e}")))
}

/// The id the next `open_escrow` will assign (pool `next_escrow_id` view).
pub fn escrow_next_id(cfg: &PoolConfig) -> Result<u64, CoreError> {
    match simulate_invoke(cfg, &cfg.pool_contract, "next_escrow_id", vec![])? {
        ScVal::U64(n) => Ok(n),
        _ => Err(CoreError::Chain("next_escrow_id: unexpected return type".into())),
    }
}

/// Read an escrow's public state (pool `escrow(id)` view). Errors if no such escrow.
pub fn read_escrow(cfg: &PoolConfig, escrow_id: u64) -> Result<EscrowState, CoreError> {
    let v = simulate_invoke(cfg, &cfg.pool_contract, "escrow", vec![sc_u64(escrow_id)])?;
    let entries = match v {
        ScVal::Map(Some(ScMap(m))) => m,
        _ => return Err(CoreError::Chain("escrow: unexpected return type".into())),
    };
    let field = |name: &str| -> Option<&ScVal> {
        entries.iter().find(|e| matches!(&e.key, ScVal::Symbol(s) if s.0.as_slice() == name.as_bytes())).map(|e| &e.val)
    };
    let u64f = |name: &str| -> Result<u64, CoreError> {
        match field(name) {
            Some(ScVal::U64(n)) => Ok(*n),
            _ => Err(CoreError::Chain(format!("escrow: missing/bad u64 {name}"))),
        }
    };
    let u32f = |name: &str| -> Result<u32, CoreError> {
        match field(name) {
            Some(ScVal::U32(n)) => Ok(*n),
            _ => Err(CoreError::Chain(format!("escrow: missing/bad u32 {name}"))),
        }
    };
    let frf = |name: &str| -> Result<Fr, CoreError> {
        field(name).and_then(u256_to_fr).ok_or_else(|| CoreError::Chain(format!("escrow: missing/bad U256 {name}")))
    };
    Ok(EscrowState {
        asset_tag: frf("asset_tag")?,
        target: u64f("target")?,
        deadline: u64f("deadline")?,
        mode: u32f("mode")?,
        payee_bind: frf("payee_bind")?,
        c_raised: frf("c_raised")?,
        raised_x: frf("raised_x")?,
        raised_y: frf("raised_y")?,
        n_contrib: u32f("n_contrib")?,
        status: u32f("status")?,
    })
}

/// Drain this escrow's `escrcon` blobs (the `(amount, r)` payloads encrypted to the payee),
/// returned in contribution-index order. The payee decrypts these to recover the running total
/// `(S, R)` it must open at release ([`super::escrow::scan_total`]). No cache: contributions per
/// escrow are few and this is only read at release time.
pub fn escrow_contributions(cfg: &PoolConfig, escrow_id: u64) -> Result<Vec<Vec<u8>>, CoreError> {
    let pool = &cfg.pool_contract;
    let start = resolve_start(&cfg.rpc_url, pool).map_err(CoreError::Chain)?;
    let mut cursor: Option<String> = None;
    let mut found: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut empty_run = 0u32;
    let mut seen = 0usize;

    for _ in 0..MAX_PAGES {
        let (events, next) = get_events_page(
            &cfg.rpc_url,
            pool,
            if cursor.is_none() { Some(start) } else { None },
            cursor.as_deref(),
        )
        .map_err(CoreError::Chain)?;
        let n = events.len();
        seen += n;
        for raw in &events {
            if let Some((idx, blob)) = classify_escrow_contribution(raw, escrow_id) {
                if !found.iter().any(|(i, _)| *i == idx) {
                    found.push((idx, blob));
                }
            }
        }
        let advanced = next.is_some() && next != cursor;
        if next.is_some() {
            cursor = next;
        }
        if n == 0 {
            empty_run += 1;
            if !advanced || (seen > 0 && empty_run >= EMPTY_TOLERANCE) {
                break;
            }
        } else {
            empty_run = 0;
        }
    }
    found.sort_by_key(|(i, _)| *i);
    Ok(found.into_iter().map(|(_, b)| b).collect())
}

/// Decode an `escrcon` event for `escrow_id` → `(contrib_index, payee_enc blob)`. Topics are
/// `(Symbol "escrcon", U64 escrow_id, U32 idx)`; value is the opaque `payee_enc` Bytes.
fn classify_escrow_contribution(e: &RawEvent, escrow_id: u64) -> Option<(u32, Vec<u8>)> {
    match scval(e.topics.first()?)? {
        ScVal::Symbol(s) if s.0.as_slice() == b"escrcon" => {}
        _ => return None,
    };
    match scval(e.topics.get(1)?)? {
        ScVal::U64(n) if n == escrow_id => {}
        _ => return None,
    };
    let idx = match scval(e.topics.get(2)?)? {
        ScVal::U32(n) => n,
        _ => return None,
    };
    let blob = match scval(&e.value)? {
        ScVal::Bytes(b) => b.0.as_slice().to_vec(),
        _ => return None,
    };
    Some((idx, blob))
}

/// Open an escrow: `open_escrow(asset_tag, target, deadline, mode, payee_bind)`. Returns the tx
/// hash; the assigned id is read separately via [`escrow_next_id`] before the call.
pub fn submit_open_escrow(
    cfg: &PoolConfig,
    source_secret: &str,
    target: u64,
    deadline: u64,
    mode: u32,
    payee_bind: &Fr,
) -> Result<String, CoreError> {
    let args = vec![
        sc_u256_fr(&cfg.asset_tag),
        sc_u64(target),
        sc_u64(deadline),
        ScVal::U32(mode),
        sc_u256_fr(payee_bind),
    ];
    invoke_contract(cfg, source_secret, &cfg.pool_contract, "open_escrow", args)
}

/// Contribute to an escrow. `change` is the shielded change-note payload; `payee_enc` is the
/// `(amount, r)` blob encrypted to the payee; `(raised_x, raised_y)` is the new running point.
#[allow(clippy::too_many_arguments)]
pub fn submit_escrow_contribute(
    cfg: &PoolConfig,
    source_secret: &str,
    escrow_id: u64,
    public_inputs: &[u8],
    proof: &[u8],
    change: &OutputPayload,
    payee_enc: &[u8],
    raised_x: &Fr,
    raised_y: &Fr,
) -> Result<String, CoreError> {
    let args = vec![
        sc_u64(escrow_id),
        sc_u256_fr(&cfg.asset_tag),
        sc_bytes(public_inputs)?,
        sc_bytes(proof)?,
        sc_bytes(&change.enc_note)?,
        sc_bytes(&change.ephemeral_pub)?,
        ScVal::U32(change.view_tag),
        sc_bytes(payee_enc)?,
        sc_u256_fr(raised_x),
        sc_u256_fr(raised_y),
    ];
    invoke_contract(cfg, source_secret, &cfg.pool_contract, "escrow_contribute", args)
}

/// Release an escrow to the payee: `escrow_release(id, pi, proof, enc_note, eph, view_tag)`.
pub fn submit_escrow_release(
    cfg: &PoolConfig,
    source_secret: &str,
    escrow_id: u64,
    public_inputs: &[u8],
    proof: &[u8],
    out: &OutputPayload,
) -> Result<String, CoreError> {
    let args = vec![
        sc_u64(escrow_id),
        sc_bytes(public_inputs)?,
        sc_bytes(proof)?,
        sc_bytes(&out.enc_note)?,
        sc_bytes(&out.ephemeral_pub)?,
        ScVal::U32(out.view_tag),
    ];
    invoke_contract(cfg, source_secret, &cfg.pool_contract, "escrow_release", args)
}

/// Refund one contribution: `escrow_refund(id, contrib_index, pi, proof, enc_note, eph, view_tag)`.
#[allow(clippy::too_many_arguments)]
pub fn submit_escrow_refund(
    cfg: &PoolConfig,
    source_secret: &str,
    escrow_id: u64,
    contrib_index: u32,
    public_inputs: &[u8],
    proof: &[u8],
    out: &OutputPayload,
) -> Result<String, CoreError> {
    let args = vec![
        sc_u64(escrow_id),
        ScVal::U32(contrib_index),
        sc_bytes(public_inputs)?,
        sc_bytes(proof)?,
        sc_bytes(&out.enc_note)?,
        sc_bytes(&out.ephemeral_pub)?,
        ScVal::U32(out.view_tag),
    ];
    invoke_contract(cfg, source_secret, &cfg.pool_contract, "escrow_refund", args)
}

/// Enroll a wallet (admin path): `policy.enroll(owner_pk, who)` then `pool.sync_asp_root`
/// (two transactions — a Soroban tx carries exactly one host-function op). `owner_pk_dec`
/// is the decimal `U256`. Returns the enroll transaction hash.
pub fn submit_enroll(
    cfg: &PoolConfig,
    admin_secret: &str,
    owner_pk_dec: &str,
    who: &str,
) -> Result<String, CoreError> {
    let enroll_hash = invoke_contract(
        cfg,
        admin_secret,
        &cfg.policy_contract,
        "enroll",
        vec![sc_u256_decimal(owner_pk_dec)?, sc_account(who)?],
    )?;
    invoke_contract(cfg, admin_secret, &cfg.pool_contract, "sync_asp_root", vec![])?;
    Ok(enroll_hash)
}

/// Record a disclosure grant on the viewkeys contract: `register_view_key` (publish the
/// scope's PUBLIC key halves) then `disclose` (the auditable, revocable grant) — two
/// transactions, both owner-signed. `viewing_pub`/`detection_pub` are 32-byte hex.
#[allow(clippy::too_many_arguments)]
pub fn submit_disclosure(
    cfg: &PoolConfig,
    viewkeys: &str,
    owner_secret: &str,
    owner_addr: &str,
    auditor_addr: &str,
    account: u32,
    asset_tag_dec: &str,
    epoch: u32,
    viewing_pub: &str,
    detection_pub: &str,
) -> Result<String, CoreError> {
    let scope = sc_view_scope(account, asset_tag_dec, epoch)?;
    let viewing = sc_bytes(
        &hex::decode(viewing_pub).map_err(|e| CoreError::Chain(format!("viewing_pub hex: {e}")))?,
    )?;
    let detection = sc_bytes(
        &hex::decode(detection_pub).map_err(|e| CoreError::Chain(format!("detection_pub hex: {e}")))?,
    )?;
    invoke_contract(
        cfg,
        owner_secret,
        viewkeys,
        "register_view_key",
        vec![sc_account(owner_addr)?, scope.clone(), viewing, detection],
    )?;
    invoke_contract(
        cfg,
        owner_secret,
        viewkeys,
        "disclose",
        vec![sc_account(owner_addr)?, sc_account(auditor_addr)?, scope],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(leaf: u32) -> CommitEntry {
        CommitEntry {
            leaf_index: leaf,
            commitment: format!("0x{leaf:064x}"),
            enc_note: Some("0xabcd".into()),
            ephemeral_pub: Some("0x00".into()),
            view_tag: Some(7),
        }
    }

    #[test]
    fn pool_cache_roundtrips_cursor_and_state() {
        // The scan cache must serialize losslessly so an incremental resume sees the
        // same commits/nullifiers/cursor it persisted (the basis of O(new-events) scans).
        let c = PoolCache {
            cursor_ledger: 123_456,
            commits: vec![entry(0), entry(1)],
            nullifiers: vec![Fr::from_u64(9).to_hex(), Fr::from_u64(10).to_hex()],
        };
        let bytes = serde_json::to_vec(&c).unwrap();
        let back: PoolCache = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back.cursor_ledger, 123_456);
        assert_eq!(back.commits.len(), 2);
        assert_eq!(back.commits[1].leaf_index, 1);
        assert_eq!(back.commits[0].view_tag, Some(7));
        assert_eq!(back.nullifiers.len(), 2);
        // Nullifier hex round-trips back to the same field element.
        assert_eq!(Fr::from_hex(&back.nullifiers[0]).unwrap(), Fr::from_u64(9));
    }

    #[test]
    fn missing_cache_is_default_not_an_error() {
        // A miss (no file / bad json) yields an empty cache → a full drain, never a failure.
        let c = load_cache("CNONEXISTENTPOOLIDFORTEST______________________________");
        assert_eq!(c.cursor_ledger, 0);
        assert!(c.commits.is_empty());
    }

    // --- native invoke argument builders (G14) ---

    #[test]
    fn fr_to_scval_u256_roundtrips_via_xdr() {
        // A field element → ScVal::U256 must serialize and decode back to the same 32 BE
        // bytes the contract reads as the U256 arg (asset_tag / owner_pk, etc.).
        let fr = Fr::from_hex("0x0123456789abcdeffedcba98765432100011223344556677889900aabbccddee")
            .unwrap();
        let sv = sc_u256_fr(&fr);
        let b64 = sv.to_xdr_base64(Limits::none()).unwrap();
        let back = ScVal::from_xdr_base64(&b64, Limits::none()).unwrap();
        match back {
            ScVal::U256(p) => {
                let mut b = [0u8; 32];
                b[0..8].copy_from_slice(&p.hi_hi.to_be_bytes());
                b[8..16].copy_from_slice(&p.hi_lo.to_be_bytes());
                b[16..24].copy_from_slice(&p.lo_hi.to_be_bytes());
                b[24..32].copy_from_slice(&p.lo_lo.to_be_bytes());
                assert_eq!(b, fr.0);
            }
            other => panic!("expected U256, got {other:?}"),
        }
    }

    #[test]
    fn u256_decimal_matches_fr_decimal() {
        // sc_u256_decimal (used for owner_pk/asset_tag decimals) agrees with the Fr path.
        let fr = Fr::from_u64(123_456_789);
        let a = sc_u256_decimal(&fr.to_decimal()).unwrap();
        assert_eq!(a, sc_u256_fr(&fr));
        assert!(sc_u256_decimal("not-a-number").is_err());
    }

    #[test]
    fn sc_account_decodes_strkey_into_account_address() {
        // A G-address arg → ScAddress::Account holding the same ed25519 bytes strkey decodes.
        let g = "GDRXE2BQUC3AZNPVFSCEZ76NJ3WWL25FYFK6RGZGIEKWE4SOOHSUJUJ6";
        let want = stellar_strkey::ed25519::PublicKey::from_string(g).unwrap().0;
        match sc_account(g).unwrap() {
            ScVal::Address(ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(
                Uint256(b),
            )))) => assert_eq!(b, want),
            other => panic!("expected account address, got {other:?}"),
        }
        assert!(sc_account("not-an-address").is_err());
    }

    #[test]
    fn i128_amount_is_nonnegative_lo() {
        match sc_i128(400) {
            ScVal::I128(Int128Parts { hi, lo }) => {
                assert_eq!(hi, 0);
                assert_eq!(lo, 400);
            }
            other => panic!("expected I128, got {other:?}"),
        }
    }

    #[test]
    fn view_scope_map_is_key_ordered() {
        // The ViewScope struct encodes as a map whose entries MUST be sorted by symbol
        // key (account < asset_tag < epoch) or the host rejects the map.
        match sc_view_scope(0, "1", 28).unwrap() {
            ScVal::Map(Some(ScMap(m))) => {
                let keys: Vec<String> = m
                    .iter()
                    .map(|e| match &e.key {
                        ScVal::Symbol(s) => s.0.to_string(),
                        _ => panic!("non-symbol map key"),
                    })
                    .collect();
                assert_eq!(keys, ["account", "asset_tag", "epoch"]);
            }
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn contract_address_roundtrips_strkey() {
        // Encode 32 known bytes as a valid C-strkey, then ensure contract_address decodes
        // them back into the ScAddress::Contract hash.
        let raw = [0x11u8; 32];
        let c = stellar_strkey::Contract(raw).to_string();
        match contract_address(&c) {
            Ok(ScAddress::Contract(ContractId(Hash(b)))) => assert_eq!(b, raw),
            Ok(other) => panic!("expected contract address, got {other:?}"),
            Err(e) => panic!("valid C-strkey should decode: {e}"),
        }
        assert!(contract_address("CNOTVALID").is_err());
    }
}
