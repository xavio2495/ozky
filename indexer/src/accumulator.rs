//! Off-chain reconstruction of the nullifier indexed-Merkle accumulator (the
//! Aztec-style sorted linked list of spent nullifiers, Z1 §4b). Replaying the
//! published `nullif` events in order rebuilds the exact tree the circuit/contract
//! maintain; the rebuilt root is self-checked against the contract's published
//! `nullifier_root`. The indexer serves a NON-MEMBERSHIP witness: the "low leaf"
//! that brackets a queried nullifier plus its Merkle path — what a spender needs to
//! prove the nullifier isn't yet spent.

use crate::tree::{indexed_leaf_hash_hex, merkle_path, u256_from_hex, MerklePath};
use soroban_sdk::{Env, U256};

#[derive(Clone)]
struct Leaf {
    value: U256,
    next_index: u64,
    next_value: U256,
}

/// Rebuild the indexed tree by replaying `nullifiers` (hex) in insertion order.
/// Slot 0 is the canonical init leaf {0,0,0}; each insert appends a leaf and
/// repoints its low leaf (matches `accumulator.nr::insert` /
/// `transfer::build_two_insertions`).
fn rebuild(env: &Env, nullifiers: &[String]) -> Option<Vec<Leaf>> {
    let zero = U256::from_u32(env, 0);
    let mut leaves: Vec<Leaf> = vec![Leaf {
        value: zero.clone(),
        next_index: 0,
        next_value: zero.clone(),
    }];

    for nf_hex in nullifiers {
        let nf = u256_from_hex(env, nf_hex)?;
        let new_index = leaves.len() as u64;
        let low_i = low_leaf_index(env, &leaves, &nf)?;
        let low = leaves[low_i].clone();
        // Repoint the low leaf at the new leaf; the new leaf inherits the old successor.
        leaves[low_i] = Leaf {
            value: low.value.clone(),
            next_index: new_index,
            next_value: nf.clone(),
        };
        leaves.push(Leaf {
            value: nf,
            next_index: low.next_index,
            next_value: low.next_value,
        });
    }
    Some(leaves)
}

/// The low leaf bracketing `target`: `low.value < target` and (`target < next_value`
/// or `low` is the tail with `next_value == 0`). Returns None if `target` is already
/// present (membership) — the caller reports that distinctly.
fn low_leaf_index(env: &Env, leaves: &[Leaf], target: &U256) -> Option<usize> {
    let zero = U256::from_u32(env, 0);
    for (i, l) in leaves.iter().enumerate() {
        if &l.value == target {
            return None; // already a member
        }
        let above_low = l.value < *target;
        let is_tail = l.next_value == zero;
        let below_next = !is_tail && *target < l.next_value;
        if above_low && (is_tail || below_next) {
            return Some(i);
        }
    }
    None
}

fn leaf_hashes(env: &Env, leaves: &[Leaf]) -> Vec<String> {
    leaves
        .iter()
        .map(|l| indexed_leaf_hash_hex(env, &l.value, l.next_index, &l.next_value))
        .collect()
}

/// The accumulator root from the rebuilt tree (Merkle over leaf hashes, depth 20).
pub fn root(env: &Env, nullifiers: &[String]) -> Option<String> {
    let leaves = rebuild(env, nullifiers)?;
    let hashes = leaf_hashes(env, &leaves);
    // merkle_path(index 0) returns the root over all leaves.
    Some(merkle_path(env, &hashes, 0)?.root)
}

pub struct NonMembership {
    pub target: String,
    pub present: bool,
    pub low_value: String,
    pub low_next_index: u64,
    pub low_next_value: String,
    pub low_index: u32,
    pub low_path: Option<MerklePath>,
    pub root: String,
}

/// Build a non-membership witness for `target` against the rebuilt accumulator.
pub fn non_membership(env: &Env, nullifiers: &[String], target_hex: &str) -> Option<NonMembership> {
    let leaves = rebuild(env, nullifiers)?;
    let target = u256_from_hex(env, target_hex)?;
    let hashes = leaf_hashes(env, &leaves);
    let root = merkle_path(env, &hashes, 0)?.root;

    match low_leaf_index(env, &leaves, &target) {
        None => {
            // Either already a member, or no bracketing leaf (shouldn't happen for a
            // well-formed sorted list with a tail). Report present if value matches.
            let present = leaves.iter().any(|l| l.value == target);
            Some(NonMembership {
                target: target_hex.to_string(),
                present,
                low_value: String::new(),
                low_next_index: 0,
                low_next_value: String::new(),
                low_index: 0,
                low_path: None,
                root,
            })
        }
        Some(i) => {
            let low = leaves[i].clone();
            let low_path = merkle_path(env, &hashes, i as u32);
            Some(NonMembership {
                target: target_hex.to_string(),
                present: false,
                low_value: crate::tree::u256_to_hex(env, &low.value),
                low_next_index: low.next_index,
                low_next_value: crate::tree::u256_to_hex(env, &low.next_value),
                low_index: i as u32,
                low_path,
                root,
            })
        }
    }
}
