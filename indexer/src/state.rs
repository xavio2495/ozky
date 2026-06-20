//! In-memory indexed state, updated by the background poller. Everything here is a
//! cache of on-chain data — it can be rebuilt from scratch by replaying events, and
//! nothing downstream depends on it for correctness (Z6 invariant).

use crate::events::{classify, Commit, PoolEvent};
use crate::rpc::Rpc;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct State {
    /// Commitments in append order (index == Merkle leaf index).
    pub commits: Vec<Commit>,
    /// Published nullifiers (for rebuilding the indexed accumulator).
    pub nullifiers: Vec<String>,
    /// Latest roots the contract published (authoritative; used to self-check).
    pub commitment_root: Option<String>,
    pub nullifier_root: Option<String>,
    pub last_ledger: u32,
    pub cursor: Option<String>,
    pub start_ledger: u32,
    pub healthy: bool,
}

impl State {
    fn ingest(&mut self, ev: PoolEvent) {
        match ev {
            PoolEvent::Commit(c) => {
                // Idempotent on leaf_index (cursor paging shouldn't repeat, but be safe).
                if !self.commits.iter().any(|x| x.leaf_index == c.leaf_index) {
                    self.commits.push(c);
                    self.commits.sort_by_key(|c| c.leaf_index);
                }
            }
            PoolEvent::Nullifier { value, .. } => {
                if !self.nullifiers.contains(&value) {
                    self.nullifiers.push(value);
                }
            }
            PoolEvent::Roots {
                commitment_root,
                nullifier_root,
                ..
            } => {
                self.commitment_root = Some(commitment_root);
                self.nullifier_root = Some(nullifier_root);
            }
        }
    }
}

/// Run one polling pass. `getEvents` scans only a bounded ledger window per call,
/// so we drain pages via the cursor until caught up to the tip. Stops when a page
/// returns no events for a few consecutive windows (the empty tail past the latest
/// event), with a hard cap so a single pass can't loop unbounded. Returns the number
/// of new raw events ingested.
pub fn poll_once(rpc: &Rpc, pool: &str, state: &Arc<Mutex<State>>) -> Result<usize, String> {
    const MAX_PAGES: u32 = 500;
    const EMPTY_TOLERANCE: u32 = 4;

    let mut total = 0usize;
    let mut empty_run = 0u32;
    for _ in 0..MAX_PAGES {
        let (cursor, start) = {
            let s = state.lock().unwrap();
            (s.cursor.clone(), s.start_ledger)
        };
        let page = rpc.get_events(
            pool,
            if cursor.is_none() { Some(start) } else { None },
            cursor.as_deref(),
        )?;
        let n = page.events.len();
        total += n;

        let mut s = state.lock().unwrap();
        for raw in &page.events {
            if raw.ledger > s.last_ledger {
                s.last_ledger = raw.ledger;
            }
            if let Some(ev) = classify(raw) {
                s.ingest(ev);
            }
        }
        let advanced = page.cursor.is_some() && page.cursor != s.cursor;
        if page.cursor.is_some() {
            s.cursor = page.cursor;
        }
        if page.latest_ledger > s.last_ledger {
            s.last_ledger = page.latest_ledger;
        }
        s.healthy = true;
        drop(s);

        // Stop when the cursor stops advancing (reached the tip), or — once we've
        // already ingested events this pass — after a few empty windows (scanned
        // past the last event). Early empty windows (before the first event, when
        // starting far back) keep paging because the cursor is still advancing.
        if n == 0 {
            empty_run += 1;
            if !advanced || (total > 0 && empty_run >= EMPTY_TOLERANCE) {
                break;
            }
        } else {
            empty_run = 0;
        }
    }
    Ok(total)
}
