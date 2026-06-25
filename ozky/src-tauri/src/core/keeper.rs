//! Headless payroll keeper (next-build scope #2) — phase K1 foundation.
//!
//! Goal: run scheduled payroll WITHOUT the app open. While the app is open the wallet
//! pre-proves the next due run and stores the proof + ciphertext payloads as a
//! [`KeeperBundle`]; a headless submitter (a local OS task, or the premium GCP keeper)
//! fires them on schedule via a relayer. The keeper NEVER holds `owner_sk` — it can
//! submit a pre-authorized proof but cannot forge one. See
//! `claude-docs/headless_keeper_interface.md`.
//!
//! K1 lands the data layer only: the bundle types ([`KeeperBundle`]/[`KeeperRun`]) built
//! from a [`super::send::PreparedTx`], and an encrypted-at-rest queue
//! (`keeper-<wallet>.enc`, same ChaCha scheme as the notes/payroll stores). Arming
//! (K2), the submit core (K3), and the `ozky-keeper` binary (K4) build on this.

use super::chain::{self, OutputPayload};
use super::config::PoolConfig;
use super::keys::WalletKeys;
use super::notes::data_dir;
use super::poseidon::{Fr, Hasher};
use super::scan::{self, OwnedNote};
use super::send::{self, PreparedMethod, PreparedTx};
use super::{notes, payroll, witness, CoreError};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Which pool entrypoint a bundle submits through. Mirror of [`PreparedMethod`] that is
/// serializable for the at-rest queue.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum BundleMethod {
    Split,
    Transfer4,
}

impl BundleMethod {
    fn from_prepared(m: PreparedMethod) -> BundleMethod {
        match m {
            PreparedMethod::Split => BundleMethod::Split,
            PreparedMethod::Transfer4 => BundleMethod::Transfer4,
        }
    }
    pub fn to_prepared(self) -> PreparedMethod {
        match self {
            BundleMethod::Split => PreparedMethod::Split,
            BundleMethod::Transfer4 => PreparedMethod::Transfer4,
        }
    }
}

/// The host-agnostic unit the keeper submits. Plain data — NO key material, NO plaintext
/// amounts (`outputs` are ciphertexts only). The `Fr` bound-state fields are stored as `0x…`
/// hex so the bundle serializes cleanly and stays debuggable; the cheap pre-submit checks
/// (epoch still current? nullifier root unmoved? same pool?) read them before paying a fee.
#[derive(Clone, Serialize, Deserialize)]
pub struct KeeperBundle {
    /// Random 16-byte hex; idempotency key (a submitter must not double-submit a bundle).
    pub bundle_id: String,
    /// Source schedule this bundle pays.
    pub payroll_id: u64,
    /// Asset CODE (e.g. "USDC") so a submitter rebuilds an asset-scoped pool config.
    pub asset: String,
    /// Asset tag (`0x…` hex `Fr`) the proof is bound to.
    pub asset_tag: String,
    /// The pool the proof targets (a submitter rejects if the app has since migrated pools).
    pub pool_contract: String,
    pub method: BundleMethod,
    pub proof: Vec<u8>,
    pub public_inputs: Vec<u8>,
    pub outputs: Vec<OutputPayload>,
    // --- pre-submit validation (cheap RPC checks before spending a fee) ---
    pub bound_epoch: u32,
    pub nullifier_old: String,
    pub nullifier_new: String,
    pub commitment_root: String,
    // --- scheduling ---
    pub earliest_submit_unix: i64,
    pub chain_index: u32,
    pub chain_len: u32,
}

impl KeeperBundle {
    /// Wrap a freshly pre-proved [`PreparedTx`] as a queueable bundle.
    pub fn from_prepared(
        payroll_id: u64,
        asset: &str,
        pool_contract: &str,
        prepared: &PreparedTx,
        earliest_submit_unix: i64,
        chain_index: u32,
        chain_len: u32,
    ) -> KeeperBundle {
        KeeperBundle {
            bundle_id: new_bundle_id(),
            payroll_id,
            asset: asset.to_string(),
            asset_tag: prepared.asset_tag.to_hex(),
            pool_contract: pool_contract.to_string(),
            method: BundleMethod::from_prepared(prepared.method),
            proof: prepared.proof.clone(),
            public_inputs: prepared.public_inputs.clone(),
            outputs: prepared.outputs.clone(),
            bound_epoch: prepared.bound_epoch,
            nullifier_old: prepared.nullifier_old.to_hex(),
            nullifier_new: prepared.nullifier_new.to_hex(),
            commitment_root: prepared.commitment_root.to_hex(),
            earliest_submit_unix,
            chain_index,
            chain_len,
        }
    }

    /// Reconstruct the submittable [`PreparedTx`] (proof + bound roots) from the bundle.
    pub fn to_prepared(&self) -> Result<PreparedTx, CoreError> {
        let fr = |h: &str, what: &str| {
            Fr::from_hex(h).ok_or_else(|| CoreError::Crypto(format!("bad {what} hex in bundle")))
        };
        Ok(PreparedTx {
            method: self.method.to_prepared(),
            asset_tag: fr(&self.asset_tag, "asset_tag")?,
            public_inputs: self.public_inputs.clone(),
            proof: self.proof.clone(),
            outputs: self.outputs.clone(),
            bound_epoch: self.bound_epoch,
            nullifier_old: fr(&self.nullifier_old, "nullifier_old")?,
            nullifier_new: fr(&self.nullifier_new, "nullifier_new")?,
            commitment_root: fr(&self.commitment_root, "commitment_root")?,
        })
    }
}

/// The outcome of submitting a run (recorded back into the queue for the UI).
#[derive(Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub submitted_unix: i64,
    pub tx_hashes: Vec<String>,
    /// `None` on success; the failure reason otherwise (e.g. stale epoch, moved root).
    pub error: Option<String>,
}

/// A scheduled run = an ordered chain of bundles (one per chunk) submitted in
/// `chain_index` order with no interleave. `chunk k+1` is bound to `chunk k`'s post-state,
/// so the first failure aborts the rest (K3).
#[derive(Clone, Serialize, Deserialize)]
pub struct KeeperRun {
    pub payroll_id: u64,
    pub bundles: Vec<KeeperBundle>,
    /// The last submission attempt's outcome (for `status()`).
    pub last_result: Option<RunResult>,
}

/// The wallet's keeper queue: the armed runs awaiting their fire time.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct KeeperQueue {
    pub runs: Vec<KeeperRun>,
}

impl KeeperQueue {
    /// Insert or replace the armed run for a payroll (one armed run per schedule in v1).
    pub fn upsert_run(&mut self, run: KeeperRun) {
        match self.runs.iter_mut().find(|r| r.payroll_id == run.payroll_id) {
            Some(slot) => *slot = run,
            None => self.runs.push(run),
        }
    }
    /// Drop the armed run for a payroll (disarm). Returns whether one existed.
    pub fn remove_run(&mut self, payroll_id: u64) -> bool {
        let before = self.runs.len();
        self.runs.retain(|r| r.payroll_id != payroll_id);
        before != self.runs.len()
    }
}

// --- store (encrypted at rest, per wallet — same scheme as payroll/notes) ------------

fn new_bundle_id() -> String {
    let mut b = [0u8; 16];
    OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

/// The MINIMUM key material a headless submitter needs to read its queue + write back results:
/// the at-rest decryption key (`notes_key`) and the wallet address (the queue filename). Crucially
/// this does NOT include `owner_sk` — `notes_key` is a one-way HMAC of the seed, so a host holding
/// only `KeeperKeys` can decrypt queued proofs and submit them but CANNOT derive a spend key to
/// forge one. The local `ozky-keeper` binary runs with exactly this (no `owner_sk` in its env).
#[derive(Clone)]
pub struct KeeperKeys {
    notes_key: [u8; 32],
    address: String,
}

impl KeeperKeys {
    /// The keeper key material for the app side (it has the full wallet).
    pub fn from_wallet(wallet: &WalletKeys) -> KeeperKeys {
        KeeperKeys {
            notes_key: wallet.notes_key(),
            address: wallet.stellar_address().to_string(),
        }
    }
    /// Reconstruct from a raw notes key + address (the headless binary; no `owner_sk`).
    pub fn new(notes_key: [u8; 32], address: String) -> KeeperKeys {
        KeeperKeys { notes_key, address }
    }
}

fn store_path(keys: &KeeperKeys) -> PathBuf {
    let digest = Sha256::digest(keys.address.as_bytes());
    data_dir().join(format!("keeper-{}.enc", hex::encode(&digest[..8])))
}

fn cipher(keys: &KeeperKeys) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(Key::from_slice(&keys.notes_key))
}

/// Load the keeper queue (empty if no file yet). `keys` decrypts at rest; no `owner_sk` needed.
pub fn load_queue(keys: &KeeperKeys) -> Result<KeeperQueue, CoreError> {
    let path = store_path(keys);
    let blob = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(KeeperQueue::default()),
        Err(e) => return Err(CoreError::Crypto(format!("read keeper store: {e}"))),
    };
    if blob.len() < 12 {
        return Err(CoreError::Crypto("keeper store too short".into()));
    }
    let (nonce, ct) = blob.split_at(12);
    let plain = cipher(keys)
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| CoreError::Crypto("keeper store decrypt failed".into()))?;
    serde_json::from_slice(&plain).map_err(|e| CoreError::Crypto(format!("keeper decode: {e}")))
}

/// Persist the keeper queue (encrypted at rest).
pub fn save_queue(keys: &KeeperKeys, queue: &KeeperQueue) -> Result<(), CoreError> {
    let plain =
        serde_json::to_vec(queue).map_err(|e| CoreError::Crypto(format!("keeper encode: {e}")))?;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ct = cipher(keys)
        .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
        .map_err(|_| CoreError::Crypto("keeper store encrypt failed".into()))?;
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| CoreError::Crypto(format!("mkdir keeper dir: {e}")))?;
    let mut blob = Vec::with_capacity(12 + ct.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    std::fs::write(store_path(keys), blob)
        .map_err(|e| CoreError::Crypto(format!("write keeper store: {e}")))
}

// --- local credential file (for the OS-scheduled `ozky-keeper --once`) — K6 ----------

/// The credential the local headless binary reads (via `--cred <path>`) so a Task-Scheduler run
/// with no inherited env can find + decrypt the queue and locate the pool config. It holds the
/// `notes_key` (decrypts the at-rest queue) and the wallet address (queue filename) — NOT
/// `owner_sk`, so the local host still cannot forge a spend. Plaintext, written into the user's
/// app-data dir (a user-scoped local secret, like an SSH key); never synced or pushed.
#[derive(Serialize, Deserialize)]
pub struct KeeperCred {
    pub notes_key: String, // hex
    pub address: String,
    pub notes_dir: String, // OZKY_NOTES_DIR the app uses (so the binary reads the same queue)
    pub config: String,    // path to ozky.config.json (pool + relayer)
}

fn cred_path(keys: &KeeperKeys) -> PathBuf {
    let digest = Sha256::digest(keys.address.as_bytes());
    data_dir().join(format!("keeper-cred-{}.json", hex::encode(&digest[..8])))
}

/// Write the local credential file for the scheduled binary; returns its absolute path.
pub fn write_local_cred(
    keys: &KeeperKeys,
    notes_dir: &str,
    config: &str,
) -> Result<PathBuf, CoreError> {
    let cred = KeeperCred {
        notes_key: hex::encode(keys.notes_key),
        address: keys.address.clone(),
        notes_dir: notes_dir.to_string(),
        config: config.to_string(),
    };
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| CoreError::Crypto(format!("mkdir keeper dir: {e}")))?;
    let path = cred_path(keys);
    let body =
        serde_json::to_vec_pretty(&cred).map_err(|e| CoreError::Crypto(format!("cred encode: {e}")))?;
    std::fs::write(&path, body).map_err(|e| CoreError::Crypto(format!("write keeper cred: {e}")))?;
    Ok(path)
}

/// Remove the local credential file (disable local keeper). Idempotent.
pub fn remove_local_cred(keys: &KeeperKeys) -> Result<(), CoreError> {
    match std::fs::remove_file(cred_path(keys)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CoreError::Crypto(format!("remove keeper cred: {e}"))),
    }
}

/// Load a credential file (the binary's `--cred` path). Returns the keeper keys + the env it
/// should apply (`notes_dir`, `config`) before reading the queue.
pub fn load_cred(path: &std::path::Path) -> Result<(KeeperKeys, KeeperCred), CoreError> {
    let body = std::fs::read(path).map_err(|e| CoreError::Crypto(format!("read keeper cred: {e}")))?;
    let cred: KeeperCred =
        serde_json::from_slice(&body).map_err(|e| CoreError::Crypto(format!("cred decode: {e}")))?;
    let raw = hex::decode(&cred.notes_key)
        .map_err(|_| CoreError::Crypto("cred notes_key not hex".into()))?;
    if raw.len() != 32 {
        return Err(CoreError::Crypto("cred notes_key must be 32 bytes".into()));
    }
    let mut nk = [0u8; 32];
    nk.copy_from_slice(&raw);
    Ok((KeeperKeys::new(nk, cred.address.clone()), cred))
}

// --- keeper endpoint (cloud push target) — set in K5, used by K7 ---------------------

/// Where the app pushes armed bundles for the premium cloud keeper. `url` empty = local-task-only.
/// `token` is a per-user bearer secret, so it's stored encrypted alongside the queue.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct KeeperEndpoint {
    pub url: String,
    pub token: String,
}

fn endpoint_path(keys: &KeeperKeys) -> PathBuf {
    let digest = Sha256::digest(keys.address.as_bytes());
    data_dir().join(format!("keeper-endpoint-{}.enc", hex::encode(&digest[..8])))
}

/// The configured cloud-keeper endpoint, or `None` (local-task-only).
pub fn load_endpoint(keys: &KeeperKeys) -> Result<Option<KeeperEndpoint>, CoreError> {
    let blob = match std::fs::read(endpoint_path(keys)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(CoreError::Crypto(format!("read keeper endpoint: {e}"))),
    };
    if blob.len() < 12 {
        return Err(CoreError::Crypto("keeper endpoint store too short".into()));
    }
    let (nonce, ct) = blob.split_at(12);
    let plain = cipher(keys)
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| CoreError::Crypto("keeper endpoint decrypt failed".into()))?;
    let ep: KeeperEndpoint =
        serde_json::from_slice(&plain).map_err(|e| CoreError::Crypto(format!("endpoint decode: {e}")))?;
    Ok(Some(ep))
}

/// Set (or, with an empty `url`, clear) the cloud-keeper endpoint.
pub fn set_endpoint(keys: &KeeperKeys, ep: &KeeperEndpoint) -> Result<(), CoreError> {
    if ep.url.trim().is_empty() {
        match std::fs::remove_file(endpoint_path(keys)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(CoreError::Crypto(format!("clear keeper endpoint: {e}"))),
        }
    } else {
        let plain = serde_json::to_vec(ep)
            .map_err(|e| CoreError::Crypto(format!("endpoint encode: {e}")))?;
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let ct = cipher(keys)
            .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
            .map_err(|_| CoreError::Crypto("keeper endpoint encrypt failed".into()))?;
        let dir = data_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| CoreError::Crypto(format!("mkdir keeper dir: {e}")))?;
        let mut blob = Vec::with_capacity(12 + ct.len());
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ct);
        std::fs::write(endpoint_path(keys), blob)
            .map_err(|e| CoreError::Crypto(format!("write keeper endpoint: {e}")))
    }
}

// --- arm (pre-prove the next due run) — K2 -------------------------------------------

/// Conservative testnet ledger close time (s). Ledgers close ~5–6 s; using the lower bound
/// UNDERestimates the epoch's remaining wall-clock time, so [`epoch_end_unix`] never lets us
/// arm a run that would outlive the epoch its proof is bound to.
const SECS_PER_LEDGER: i64 = 5;

/// Conservative wall-clock instant the current epoch is expected to end, from the latest ledger
/// `seq`. A proof is valid only while the pool is in the epoch it was proved at, so arm refuses a
/// run scheduled past this instant (the queued proof would fail `check_common` on submit).
fn epoch_end_unix(seq: u32, now: i64) -> i64 {
    let ledgers_left = chain::LEDGER_PER_EPOCH - (seq as u64 % chain::LEDGER_PER_EPOCH);
    now + (ledgers_left as i64) * SECS_PER_LEDGER
}

/// Pre-prove the next due run of `payroll_id` into a chained [`KeeperRun`] and queue it (so a
/// headless submitter can fire it later via a relayer, with no `owner_sk`).
///
/// v1 scope: **same-asset** payrolls only (a cross-asset payee needs a swap bundle — deferred).
/// The run is `ceil(N / SPLIT_CHUNK)` `split` chunks proved as a CHAIN: chunk 0 spends one live
/// note covering the WHOLE payroll; each later chunk is built against PROJECTED state (the prior
/// chunks' nullifiers folded in, their output commitments appended) and spends the prior chunk's
/// change note. So `chunk[k].nullifier_old == chunk[k-1].nullifier_new` and the chain is
/// submit-in-order / abort-on-first-failure (K3). Refuses if the run falls outside the current
/// epoch window (the proofs would expire) or no single note covers the total (consolidate first).
pub fn arm(
    wallet: &WalletKeys,
    cfg_base: &PoolConfig,
    payroll_id: u64,
) -> Result<KeeperRun, CoreError> {
    let pr = payroll::load(wallet)?
        .into_iter()
        .find(|p| p.id == payroll_id)
        .ok_or_else(|| CoreError::Crypto("no such payroll".into()))?;
    if pr.payees.is_empty() {
        return Err(CoreError::Crypto("payroll has no payees".into()));
    }
    if pr
        .payees
        .iter()
        .any(|pe| pe.recv_asset.as_deref().is_some_and(|a| a != pr.asset))
    {
        return Err(CoreError::Crypto(
            "headless keeper v1 supports same-asset payrolls only; a cross-asset payee needs a swap bundle (deferred)".into(),
        ));
    }
    let cfg = cfg_base.with_asset(&pr.asset)?;
    let id = scan::wallet_identity(wallet)?;

    // One live read: ledger seq (epoch + window) and pool state.
    let seq = chain::latest_ledger(&cfg.rpc_url)?;
    let epoch = (seq as u64 / chain::LEDGER_PER_EPOCH) as u32;
    let now = payroll::now();
    let fire = pr.next_run_unix.max(now);
    if fire > epoch_end_unix(seq, now) {
        return Err(CoreError::Crypto(
            "next run falls past the current epoch window (a pre-proved spend would expire); re-arm closer to the run".into(),
        ));
    }

    let state = chain::pool_state(&cfg)?;
    let mut commitment_leaves = chain::commitment_leaves_from(&state.commits)?;
    let asp_leaves = chain::approved_set(&cfg)?;
    let mut prior_nullifiers = state.nullifiers.clone();
    let local = notes::load(wallet)?;

    // Parse payees → (owner_pk, transmission_pub, amount); chunk into <= SPLIT_CHUNK groups.
    let parsed_all: Vec<(Fr, [u8; 32], u64)> = pr
        .payees
        .iter()
        .map(|pe| {
            let (pk, tpub) = send::parse_payment_code(&pe.code)?;
            Ok((pk, tpub, pe.amount))
        })
        .collect::<Result<_, CoreError>>()?;
    let total: u64 = parsed_all.iter().map(|(_, _, v)| *v).sum();
    let chunks: Vec<&[(Fr, [u8; 32], u64)]> = parsed_all.chunks(send::SPLIT_CHUNK).collect();
    let chain_len = chunks.len() as u32;

    // Chunk 0 spends ONE live note covering the whole payroll; its change funds chunk 1, etc.
    let mut input: OwnedNote = scan::owned_notes(&id, &state, &local, 0)?
        .into_iter()
        .filter(|n| n.asset_tag == cfg.asset_tag && n.value >= total)
        .max_by_key(|n| n.value)
        .ok_or_else(|| {
            CoreError::Proving(format!(
                "no single owned note covers the payroll total {total}; consolidate first"
            ))
        })?;

    let mut bundles = Vec::with_capacity(chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        let prepared = send::prepare_split_against(
            &id,
            &cfg,
            epoch,
            &input,
            &commitment_leaves,
            &prior_nullifiers,
            &asp_leaves,
            chunk,
        )?;
        // Project state forward for the next chunk.
        prior_nullifiers.extend_from_slice(&prepared.nullifiers);
        commitment_leaves.extend_from_slice(&prepared.out_commitments);
        input = prepared.change.clone();
        bundles.push(KeeperBundle::from_prepared(
            payroll_id,
            &pr.asset,
            &cfg.pool_contract,
            &prepared.tx,
            fire,
            i as u32,
            chain_len,
        ));
    }

    let run = KeeperRun {
        payroll_id,
        bundles,
        last_result: None,
    };
    let keys = KeeperKeys::from_wallet(wallet);
    let mut q = load_queue(&keys)?;
    q.upsert_run(run.clone());
    save_queue(&keys, &q)?;

    // If a cloud keeper is configured, also push the armed run to it (best-effort — the local
    // queue is the source of truth, so a push failure must not lose the locally-armed run).
    if let Ok(Some(ep)) = load_endpoint(&keys) {
        if !ep.url.trim().is_empty() {
            if let Err(e) = push_run(&ep, &run) {
                eprintln!("keeper: armed locally but cloud push failed: {e}");
            }
        }
    }
    Ok(run)
}

/// Drop the armed run for `payroll_id` from the local queue (revoke). Returns whether one existed.
/// (K7 also DELETEs it at the keeper endpoint.)
pub fn disarm(keys: &KeeperKeys, payroll_id: u64) -> Result<bool, CoreError> {
    let mut q = load_queue(keys)?;
    let removed = q.remove_run(payroll_id);
    if removed {
        save_queue(keys, &q)?;
    }
    Ok(removed)
}

// --- submit core (the headless submitter, shared local + cloud) — K3 -----------------

/// What [`submit_due`] reports for one run it touched this call.
pub struct SubmitOutcome {
    pub payroll_id: u64,
    /// Chunks submitted on THIS call (a resumed run submits only its remaining chunks).
    pub submitted: usize,
    /// All tx hashes for the run so far (resumed runs accumulate).
    pub tx_hashes: Vec<String>,
    /// `None` if the run is now complete; the abort reason otherwise.
    pub error: Option<String>,
}

/// Cheap pre-submit gate (no fee): a queued proof is bound to an exact epoch + nullifier root +
/// pool. Reject before spending a relayer fee on a tx that would fail on-chain. Pure (unit-tested).
fn preflight(
    bundle: &KeeperBundle,
    live_epoch: u32,
    live_nf_root_hex: &str,
    live_pool: &str,
) -> Result<(), String> {
    if bundle.pool_contract != live_pool {
        return Err(format!(
            "pool mismatch: bundle bound to {} but live pool is {} (the app migrated pools)",
            bundle.pool_contract, live_pool
        ));
    }
    if bundle.bound_epoch != live_epoch {
        return Err(format!(
            "epoch rolled: bundle bound to epoch {} but pool is at {} (re-arm)",
            bundle.bound_epoch, live_epoch
        ));
    }
    if bundle.nullifier_old != live_nf_root_hex {
        return Err(
            "nullifier root moved since arming (a pool spend happened; the queued proof is stale — re-arm)"
                .into(),
        );
    }
    Ok(())
}

/// The submitter core, shared by the local OS task and the cloud keeper (K4/K7). For each due run
/// (its `earliest_submit_unix <= now`) that is not already complete: take one live snapshot,
/// pre-flight the resume chunk (epoch/nullifier-root/pool still valid?), then submit the run's
/// remaining chunks IN ORDER via `relayer_secret`, aborting on the first failure. Idempotent:
/// already-submitted chunks are skipped on a re-run, and a confirmed chunk advances the live
/// nullifier root to exactly the next chunk's bound `nullifier_old` (the chain links by
/// construction). Submission uses ONLY the relayer secret — never `owner_sk` — so a headless host
/// cannot forge a spend, only relay a pre-authorized one. Persists results for [`status`].
pub fn submit_due(
    keys: &KeeperKeys,
    cfg_base: &PoolConfig,
    relayer_secret: &str,
    now: i64,
) -> Result<Vec<SubmitOutcome>, CoreError> {
    let mut q = load_queue(keys)?;
    let mut outcomes = Vec::new();
    let mut changed = false;
    for run in q.runs.iter_mut() {
        if let Some(o) = submit_run(run, cfg_base, relayer_secret, now)? {
            outcomes.push(o);
            changed = true;
        }
    }
    if changed {
        save_queue(keys, &q)?;
    }
    Ok(outcomes)
}

/// Pre-flight + submit ONE run's remaining chunks in order via `relayer_secret`, recording the
/// outcome into `run.last_result`. Returns `Ok(None)` if the run is empty / already complete / not
/// yet due (no network touched). Shared by the local encrypted-queue submitter ([`submit_due`]) and
/// the cloud serve loop (K7), which hold their runs in different stores but submit identically.
pub fn submit_run(
    run: &mut KeeperRun,
    cfg_base: &PoolConfig,
    relayer_secret: &str,
    now: i64,
) -> Result<Option<SubmitOutcome>, CoreError> {
    if run.bundles.is_empty() {
        return Ok(None);
    }
    let already = run.last_result.as_ref().map_or(0, |r| r.tx_hashes.len());
    if already >= run.bundles.len() {
        return Ok(None); // complete
    }
    if run.bundles[0].earliest_submit_unix > now {
        return Ok(None); // not due yet
    }

    let cfg = cfg_base.with_asset(&run.bundles[0].asset)?;
    let live_epoch = chain::current_epoch(&cfg.rpc_url)?;
    let state = chain::pool_state(&cfg)?;
    let live_nf_root = witness::nullifier_set_root(&Hasher::new(), &state.nullifiers).to_hex();
    let live_pool = cfg.pool_contract.clone();

    let mut tx_hashes: Vec<String> =
        run.last_result.as_ref().map(|r| r.tx_hashes.clone()).unwrap_or_default();
    let start = tx_hashes.len();
    let mut error: Option<String> = None;

    // Pre-flight the resume chunk against the live snapshot.
    if let Err(e) = preflight(&run.bundles[start], live_epoch, &live_nf_root, &live_pool) {
        error = Some(e);
    } else {
        for bundle in run.bundles.iter().skip(start) {
            if bundle.bound_epoch != live_epoch {
                error = Some(format!(
                    "epoch rolled mid-run (bundle {} != live {live_epoch})",
                    bundle.bound_epoch
                ));
                break;
            }
            let prepared = match bundle.to_prepared() {
                Ok(p) => p,
                Err(e) => {
                    error = Some(e.to_string());
                    break;
                }
            };
            match send::submit_prepared(&cfg, relayer_secret, &prepared) {
                Ok(hash) => tx_hashes.push(hash),
                Err(e) => {
                    error = Some(format!("chunk submit failed: {e}"));
                    break;
                }
            }
        }
    }

    let submitted = tx_hashes.len() - start;
    run.last_result = Some(RunResult {
        submitted_unix: now,
        tx_hashes: tx_hashes.clone(),
        error: error.clone(),
    });
    Ok(Some(SubmitOutcome {
        payroll_id: run.payroll_id,
        submitted,
        tx_hashes,
        error,
    }))
}

// --- cloud keeper store + push client (K7) -------------------------------------------

/// The cloud keeper's run store. Unlike the local queue this is PLAINTEXT: a bundle carries no key
/// material and no plaintext amounts (outputs are ciphertexts), so the managed host can store +
/// submit pushed runs without ever holding the user's `notes_key` or `owner_sk`.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct CloudStore {
    pub runs: Vec<KeeperRun>,
}

impl CloudStore {
    pub fn upsert_run(&mut self, run: KeeperRun) {
        match self.runs.iter_mut().find(|r| r.payroll_id == run.payroll_id) {
            Some(slot) => *slot = run,
            None => self.runs.push(run),
        }
    }
    pub fn remove_run(&mut self, payroll_id: u64) -> bool {
        let before = self.runs.len();
        self.runs.retain(|r| r.payroll_id != payroll_id);
        before != self.runs.len()
    }
}

fn cloud_store_path() -> PathBuf {
    data_dir().join("keeper-cloud.json")
}

/// Load the cloud store (plaintext; empty if none yet).
pub fn load_cloud_store() -> Result<CloudStore, CoreError> {
    match std::fs::read(cloud_store_path()) {
        Ok(b) => serde_json::from_slice(&b)
            .map_err(|e| CoreError::Crypto(format!("cloud store decode: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(CloudStore::default()),
        Err(e) => Err(CoreError::Crypto(format!("read cloud store: {e}"))),
    }
}

/// Persist the cloud store (plaintext).
pub fn save_cloud_store(store: &CloudStore) -> Result<(), CoreError> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| CoreError::Crypto(format!("mkdir keeper dir: {e}")))?;
    let body = serde_json::to_vec_pretty(store)
        .map_err(|e| CoreError::Crypto(format!("cloud store encode: {e}")))?;
    std::fs::write(cloud_store_path(), body)
        .map_err(|e| CoreError::Crypto(format!("write cloud store: {e}")))
}

/// Push an armed run to the cloud keeper endpoint (POST `<url>`, `Authorization: Bearer <token>`).
pub fn push_run(endpoint: &KeeperEndpoint, run: &KeeperRun) -> Result<(), CoreError> {
    if endpoint.url.trim().is_empty() {
        return Ok(());
    }
    let body = serde_json::to_value(run)
        .map_err(|e| CoreError::Crypto(format!("encode run for push: {e}")))?;
    ureq::post(&endpoint.url)
        .set("Authorization", &format!("Bearer {}", endpoint.token))
        .send_json(body)
        .map(|_| ())
        .map_err(|e| CoreError::Chain(format!("push to keeper endpoint failed: {e}")))
}

/// A UI-facing summary of one armed run (no proof bytes / ciphertexts).
#[derive(Clone, Serialize, Deserialize)]
pub struct RunStatus {
    pub payroll_id: u64,
    pub chunks: u32,
    pub bound_epoch: u32,
    pub earliest_submit_unix: i64,
    /// Chunks already submitted (== chunks ⇒ complete).
    pub submitted: usize,
    pub tx_hashes: Vec<String>,
    pub error: Option<String>,
}

/// Armed runs + last results, for the UI.
pub fn status(keys: &KeeperKeys) -> Result<Vec<RunStatus>, CoreError> {
    let q = load_queue(keys)?;
    Ok(q
        .runs
        .iter()
        .map(|r| {
            let (submitted, tx_hashes, error) = match &r.last_result {
                Some(res) => (res.tx_hashes.len(), res.tx_hashes.clone(), res.error.clone()),
                None => (0, Vec::new(), None),
            };
            RunStatus {
                payroll_id: r.payroll_id,
                chunks: r.bundles.len() as u32,
                bound_epoch: r.bundles.first().map_or(0, |b| b.bound_epoch),
                earliest_submit_unix: r.bundles.first().map_or(0, |b| b.earliest_submit_unix),
                submitted,
                tx_hashes,
                error,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::chain::OutputPayload;
    use crate::core::send::{PreparedMethod, PreparedTx};

    fn demo_prepared() -> PreparedTx {
        PreparedTx {
            method: PreparedMethod::Split,
            asset_tag: Fr::from_u64(2),
            public_inputs: vec![1, 2, 3, 4],
            proof: vec![9, 8, 7, 6, 5],
            outputs: vec![OutputPayload {
                enc_note: vec![0xab, 0xcd],
                ephemeral_pub: [7u8; 32],
                view_tag: 42,
            }],
            bound_epoch: 28,
            nullifier_old: Fr::from_u64(0x1111),
            nullifier_new: Fr::from_u64(0x2222),
            commitment_root: Fr::from_u64(0x3333),
        }
    }

    /// A bundle built from a `PreparedTx` carries the exact bound roots (epoch + nullifier
    /// old/new + commitment root), and round-trips back to an identical `PreparedTx`.
    #[test]
    fn bundle_preserves_prepared_roots() {
        let p = demo_prepared();
        let b = KeeperBundle::from_prepared(7, "USDC", "CPOOL", &p, 1_700_000_000, 0, 1);

        assert_eq!(b.method, BundleMethod::Split);
        assert_eq!(b.bound_epoch, 28);
        assert_eq!(b.asset_tag, Fr::from_u64(2).to_hex());
        assert_eq!(b.nullifier_old, Fr::from_u64(0x1111).to_hex());
        assert_eq!(b.nullifier_new, Fr::from_u64(0x2222).to_hex());
        assert_eq!(b.commitment_root, Fr::from_u64(0x3333).to_hex());
        assert_eq!(b.bundle_id.len(), 32, "16-byte hex id");

        let back = b.to_prepared().unwrap();
        assert_eq!(back.method, PreparedMethod::Split);
        assert_eq!(back.asset_tag, p.asset_tag);
        assert_eq!(back.nullifier_old, p.nullifier_old);
        assert_eq!(back.nullifier_new, p.nullifier_new);
        assert_eq!(back.commitment_root, p.commitment_root);
        assert_eq!(back.proof, p.proof);
        assert_eq!(back.public_inputs, p.public_inputs);
        assert_eq!(back.outputs.len(), 1);
        assert_eq!(back.outputs[0].view_tag, 42);
    }

    /// Pre-flight accepts a bundle whose bound state matches the live snapshot and rejects each
    /// kind of drift (wrong pool, rolled epoch, moved nullifier root) — before any fee is spent.
    #[test]
    fn preflight_accepts_match_and_rejects_drift() {
        let b = KeeperBundle::from_prepared(7, "USDC", "CPOOL", &demo_prepared(), 0, 0, 1);
        let nf = Fr::from_u64(0x1111).to_hex(); // == demo's nullifier_old

        assert!(preflight(&b, 28, &nf, "CPOOL").is_ok());
        assert!(preflight(&b, 28, &nf, "COTHER").unwrap_err().contains("pool mismatch"));
        assert!(preflight(&b, 29, &nf, "CPOOL").unwrap_err().contains("epoch rolled"));
        let moved = Fr::from_u64(0x9999).to_hex();
        assert!(preflight(&b, 28, &moved, "CPOOL")
            .unwrap_err()
            .contains("nullifier root moved"));
    }

    /// `submit_due` skips a run whose fire time is in the future — no network touched, nothing
    /// submitted, the armed run left intact. (The local `--once` binary's dry-run path.)
    #[test]
    fn submit_due_skips_not_due_runs() {
        let _g = crate::core::notes::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("ozky-keeper-due-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("OZKY_NOTES_DIR", &dir);
        let wallet = crate::core::keys::derive_from_mnemonic(
            "illness spike retreat truth genius clock brain pass fit cave bargain toe",
        )
        .unwrap();
        let keys = KeeperKeys::from_wallet(&wallet);

        // A run scheduled far in the future (i64::MAX) is never due.
        let bundle = KeeperBundle::from_prepared(7, "USDC", "CPOOL", &demo_prepared(), i64::MAX, 0, 1);
        let mut q = KeeperQueue::default();
        q.upsert_run(KeeperRun { payroll_id: 7, bundles: vec![bundle], last_result: None });
        save_queue(&keys, &q).unwrap();

        let cfg = PoolConfig {
            pool_contract: "CPOOL".into(),
            policy_contract: "CPOL".into(),
            viewkeys_contract: None,
            pool_id: Fr::from_u64(7),
            network_id: Fr::from_u64(42),
            asset_tag: Fr::from_u64(1),
            rpc_url: "http://127.0.0.1:0".into(), // never contacted: the run isn't due
            network: "testnet".into(),
            network_passphrase: "Test SDF Network ; September 2015".into(),
            relayer_secret: None,
        };
        let outcomes = submit_due(&keys, &cfg, "Srelayer", 0).unwrap();
        assert!(outcomes.is_empty(), "a not-due run is skipped");

        let after = load_queue(&keys).unwrap();
        assert_eq!(after.runs.len(), 1, "still armed");
        assert!(after.runs[0].last_result.is_none(), "no submission recorded");
        std::env::remove_var("OZKY_NOTES_DIR");
    }

    /// The epoch-window estimate underestimates remaining time (so we never arm past the epoch).
    #[test]
    fn epoch_window_underestimates_remaining_time() {
        let now = 1_700_000_000;
        // Start of an epoch (seq % 110k == 0): ~all 110k ledgers remain.
        assert_eq!(epoch_end_unix(0, now), now + 110_000 * SECS_PER_LEDGER);
        // One ledger before the boundary: almost no time left.
        assert_eq!(epoch_end_unix(110_000 - 1, now), now + SECS_PER_LEDGER);
        // Mid-epoch sits strictly between.
        let mid = epoch_end_unix(55_000, now);
        assert!(mid > now + SECS_PER_LEDGER && mid < now + 110_000 * SECS_PER_LEDGER);
    }

    /// One-off: arm a 2-chunk payroll on testnet (pre-prove only, NO submit). Asserts the run is
    /// a CHAIN — two bundles, `chunk1.nullifier_old == chunk0.nullifier_new` — and both proofs were
    /// produced (each `prepare_split_against` verifies vs the frozen split VK). Needs the prover
    /// sidecar + network + a single note covering the 8-XLM total (consolidate first if fragmented).
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture core::keeper::tests::arm_two_chunk_chain_on_testnet
    #[test]
    #[ignore = "live keeper arm; needs prover sidecar + network + ozky.config.json + $OZKY_DEPLOY_MNEMONIC"]
    fn arm_two_chunk_chain_on_testnet() {
        use crate::core::payroll::{self, Cadence, Payee, Payroll};
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => return,
        };
        let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo.join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", &repo);
        let notes_dir = std::env::temp_dir().join("ozky-keeper-arm-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);

        let wallet = crate::core::keys::derive_from_mnemonic(&mnemonic).unwrap();
        let cfg = PoolConfig::load().unwrap();
        let id = scan::wallet_identity(&wallet).unwrap();
        let code = send::payment_code(&id);
        let one = 10_000_000u64; // 1 XLM

        // 8 payees x 1 XLM to self -> ceil(8/7) = 2 split chunks; 8-XLM total fits one ~11-XLM fragment.
        let payees: Vec<Payee> =
            (0..8).map(|_| Payee { code: code.clone(), amount: one, recv_asset: None }).collect();
        let pid = payroll::upsert(
            &wallet,
            Payroll {
                id: 0,
                label: "Keeper arm test".into(),
                asset: "XLM".into(),
                payees,
                cadence: Cadence::Weekly,
                next_run_unix: payroll::now(),
                last_run_unix: None,
                enabled: true,
            },
        )
        .unwrap();

        let run = arm(&wallet, &cfg, pid).expect("arm must pre-prove the run");
        assert_eq!(run.bundles.len(), 2, "8 payees / 7-per-split = 2 chunks");
        assert_eq!(run.bundles[0].chain_index, 0);
        assert_eq!(run.bundles[1].chain_index, 1);
        assert_eq!(run.bundles[0].chain_len, 2);
        assert_eq!(
            run.bundles[1].nullifier_old, run.bundles[0].nullifier_new,
            "chunk1 is bound to chunk0's post-state (the chain links)"
        );
        assert!(!run.bundles[0].proof.is_empty() && !run.bundles[1].proof.is_empty());

        // It persisted to the encrypted queue.
        let q = load_queue(&KeeperKeys::from_wallet(&wallet)).unwrap();
        assert_eq!(q.runs.len(), 1);
        assert_eq!(q.runs[0].bundles.len(), 2);
        println!("KEEPER ARM CHAIN OK");
    }

    /// One-off END-TO-END: arm a self-paying payroll (app side, with the wallet), then run the
    /// built `ozky-keeper` binary with NO `owner_sk` in its env — only the notes key + address —
    /// and assert it submitted the run on-chain via the relayer and advanced the queue to complete.
    /// This is the headline proof that a headless submit needs no spend key. Spends are self-paid
    /// (recoverable). Build the binary first: `cargo build --bin ozky-keeper`.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture core::keeper::tests::keeper_arm_then_submit_on_testnet
    #[test]
    #[ignore = "live keeper arm+submit; needs prover sidecar + network + ozky.config.json + built ozky-keeper + $OZKY_DEPLOY_MNEMONIC"]
    fn keeper_arm_then_submit_on_testnet() {
        use crate::core::payroll::{self, Cadence, Payee, Payroll};
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => return,
        };
        let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo.join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", &repo);
        let notes_dir = std::env::temp_dir().join("ozky-keeper-e2e-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);

        let wallet = crate::core::keys::derive_from_mnemonic(&mnemonic).unwrap();
        let cfg = PoolConfig::load().unwrap();
        let id = scan::wallet_identity(&wallet).unwrap();
        let code = send::payment_code(&id);
        let one = 10_000_000u64; // 1 XLM

        // 8 payees x 1 XLM to self -> 2 split chunks; 8-XLM total fits one ~11-XLM fragment.
        let payees: Vec<Payee> =
            (0..8).map(|_| Payee { code: code.clone(), amount: one, recv_asset: None }).collect();
        let pid = payroll::upsert(
            &wallet,
            Payroll {
                id: 0,
                label: "Keeper e2e".into(),
                asset: "XLM".into(),
                payees,
                cadence: Cadence::Weekly,
                next_run_unix: payroll::now(),
                last_run_unix: None,
                enabled: true,
            },
        )
        .unwrap();

        // APP SIDE (has the wallet): pre-prove + queue.
        let run = arm(&wallet, &cfg, pid).expect("arm");
        assert_eq!(run.bundles.len(), 2);

        // Count self-owned 1-XLM notes before submit (idempotent baseline).
        let xlm = cfg.clone().with_asset("XLM").unwrap();
        let ones_before = {
            let st = chain::pool_state(&xlm).unwrap();
            scan::owned_notes(&id, &st, &notes::load(&wallet).unwrap(), 0)
                .unwrap()
                .iter()
                .filter(|n| n.value == one)
                .count()
        };

        // HEADLESS SIDE: invoke the built binary with ONLY the notes key + address (no owner_sk).
        let keys = KeeperKeys::from_wallet(&wallet);
        let bin = repo
            .join("ozky")
            .join("src-tauri")
            .join("target")
            .join("debug")
            .join("ozky-keeper.exe");
        assert!(bin.exists(), "build the binary first: cargo build --bin ozky-keeper ({})", bin.display());
        let out = std::process::Command::new(&bin)
            .arg("--once")
            .env("OZKY_NOTES_DIR", &notes_dir)
            .env("OZKY_REPO_ROOT", &repo)
            .env("OZKY_KEEPER_NOTES_KEY", hex::encode(keys.notes_key))
            .env("OZKY_KEEPER_ADDRESS", &keys.address)
            // Deliberately NO OZKY_DEPLOY_MNEMONIC / owner_sk in the child env.
            .output()
            .expect("spawn ozky-keeper");
        eprintln!(
            "ozky-keeper stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "ozky-keeper --once must succeed");

        // The queue advanced to complete (both chunks submitted).
        let q = load_queue(&keys).unwrap();
        let r = q.runs.iter().find(|r| r.payroll_id == pid).unwrap();
        let res = r.last_result.as_ref().expect("a result was recorded");
        assert!(res.error.is_none(), "run completed without error: {:?}", res.error);
        assert_eq!(res.tx_hashes.len(), 2, "both chunks submitted on-chain");
        eprintln!("keeper submitted txs: {:?}", res.tx_hashes);

        // On-chain effect: 8 new self-paid 1-XLM outputs appeared.
        let ones_after = {
            let st = chain::pool_state(&xlm).unwrap();
            scan::owned_notes(&id, &st, &notes::load(&wallet).unwrap(), 0)
                .unwrap()
                .iter()
                .filter(|n| n.value == one)
                .count()
        };
        assert!(
            ones_after >= ones_before + 8,
            "expected >=8 new 1-XLM outputs (before {ones_before}, after {ones_after})"
        );
        println!("KEEPER E2E OK (headless submit, no owner_sk)");
    }

    /// One-off: arm a self-paying payroll and print the resulting `KeeperRun` as JSON (between
    /// markers). Used to feed the standalone cloud keeper (`keeper-service`) a real run to push +
    /// submit. Pre-prove only; NO submit here.
    ///   OZKY_DEPLOY_MNEMONIC="..." cargo test --lib -- --ignored --test-threads=1 \
    ///     --nocapture core::keeper::tests::print_armed_run_json
    #[test]
    #[ignore = "prints a live armed run as JSON for the cloud keeper; needs prover sidecar + network"]
    fn print_armed_run_json() {
        use crate::core::payroll::{self, Cadence, Payee, Payroll};
        let mnemonic = match std::env::var("OZKY_DEPLOY_MNEMONIC") {
            Ok(m) if !m.trim().is_empty() => m,
            _ => return,
        };
        let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        if std::env::var("OZKY_PROVER_BIN").is_err() {
            std::env::set_var("OZKY_PROVER_BIN", repo.join("prover-sidecar/dist/ozky-prover.exe"));
        }
        std::env::set_var("OZKY_REPO_ROOT", &repo);
        let notes_dir = std::env::temp_dir().join("ozky-keeper-printjson-notes");
        let _ = std::fs::remove_dir_all(&notes_dir);
        std::env::set_var("OZKY_NOTES_DIR", &notes_dir);

        let wallet = crate::core::keys::derive_from_mnemonic(&mnemonic).unwrap();
        let cfg = PoolConfig::load().unwrap();
        let id = scan::wallet_identity(&wallet).unwrap();
        let code = send::payment_code(&id);
        let one = 10_000_000u64;
        let payees: Vec<Payee> =
            (0..8).map(|_| Payee { code: code.clone(), amount: one, recv_asset: None }).collect();
        let pid = payroll::upsert(
            &wallet,
            Payroll {
                id: 0,
                label: "Cloud keeper test".into(),
                asset: "XLM".into(),
                payees,
                cadence: Cadence::Weekly,
                next_run_unix: payroll::now(),
                last_run_unix: None,
                enabled: true,
            },
        )
        .unwrap();
        let run = arm(&wallet, &cfg, pid).expect("arm");
        println!("ARMED_RUN_JSON_BEGIN");
        println!("{}", serde_json::to_string(&run).unwrap());
        println!("ARMED_RUN_JSON_END");
    }

    /// The queue persists encrypted (not plaintext) and reloads identically.
    #[test]
    fn queue_roundtrips_encrypted() {
        // Shared lock: OZKY_NOTES_DIR is process-global (notes + payroll + keeper share it).
        let _g = crate::core::notes::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("ozky-keeper-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("OZKY_NOTES_DIR", &dir);
        let wallet = crate::core::keys::derive_from_mnemonic(
            "illness spike retreat truth genius clock brain pass fit cave bargain toe",
        )
        .unwrap();
        let keys = KeeperKeys::from_wallet(&wallet);

        assert!(load_queue(&keys).unwrap().runs.is_empty());

        let bundle = KeeperBundle::from_prepared(
            7,
            "USDC",
            "CPOOLCONTRACTID",
            &demo_prepared(),
            1_700_000_000,
            0,
            1,
        );
        let bundle_id = bundle.bundle_id.clone();
        let mut q = KeeperQueue::default();
        q.upsert_run(KeeperRun {
            payroll_id: 7,
            bundles: vec![bundle],
            last_result: None,
        });
        save_queue(&keys, &q).unwrap();

        let got = load_queue(&keys).unwrap();
        assert_eq!(got.runs.len(), 1);
        assert_eq!(got.runs[0].payroll_id, 7);
        assert_eq!(got.runs[0].bundles.len(), 1);
        assert_eq!(got.runs[0].bundles[0].bundle_id, bundle_id);
        assert_eq!(got.runs[0].bundles[0].pool_contract, "CPOOLCONTRACTID");

        // The file is ciphertext: neither the pool id nor the proof bytes appear cleartext.
        let raw = std::fs::read(store_path(&keys)).unwrap();
        assert!(
            !raw.windows(15).any(|w| w == b"CPOOLCONTRACTID"),
            "pool id must not be cleartext"
        );
        assert!(
            !raw.windows(5).any(|w| w == [9u8, 8, 7, 6, 5]),
            "proof bytes must not be cleartext"
        );

        // Disarm removes it.
        let mut q2 = load_queue(&keys).unwrap();
        assert!(q2.remove_run(7));
        save_queue(&keys, &q2).unwrap();
        assert!(load_queue(&keys).unwrap().runs.is_empty());

        std::env::remove_var("OZKY_NOTES_DIR");
    }
}
