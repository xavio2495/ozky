//! Commitment Merkle tree (Z1 spec §4a) — contract-maintained, append-only,
//! depth-20 Poseidon2 over BN254 Fr (leaves and nodes are `U256` field elements).
//!
//! Incremental "frontier" insertion (Tornado/Aztec style): the contract keeps one
//! cached left-sibling per level plus the leaf count, so each append is O(depth)
//! hashes and writes — no need to hold the whole tree. Empty subtrees use the
//! precomputed zero-hash ladder (`z[0]=0`, `z[i+1]=H(z[i],z[i])`), matching the
//! circuit's witness builder (`notes::transfer::zero_ladder`).
//!
//! A spend proves membership against some recent `commitment_root`; because
//! concurrent appends advance the root, the contract accepts any of the last
//! `ROOT_WINDOW` roots (a rolling window). `ROOT_WINDOW` is a testnet tunable
//! (spec §12 "rolling-root window R").
//!
//! State lives in instance storage (mirrors the upstream tornado mixer). Fine for
//! testnet sizes (~20 frontier entries + a 64-root window); revisit if it grows.

use crate::poseidon::hash_node;
use crate::Error;
use soroban_sdk::{contracttype, Env, Vec, U256};

pub const TREE_DEPTH: u32 = 20;
pub const MAX_LEAVES: u64 = 1u64 << TREE_DEPTH;
pub const ROOT_WINDOW: u32 = 64;

#[contracttype]
#[derive(Clone)]
pub enum TreeKey {
    /// Number of leaves appended so far (the next free leaf index).
    NextIndex,
    /// Cached left sibling at `level` for incremental insertion.
    Frontier(u32),
    /// Rolling window of the last `ROOT_WINDOW` roots, oldest first.
    Roots,
    /// Cached zero-subtree hash ladder (depth+1 entries) — constant, computed once.
    Zeroes,
}

/// Zero-subtree hash ladder: `z[0] = 0`, `z[i+1] = H(z[i], z[i])`; length depth+1.
/// These are constants, so compute once and cache in storage (each append would
/// otherwise re-spend 20 Poseidon hashes recomputing them).
fn zeroes(env: &Env) -> Vec<U256> {
    if let Some(z) = env.storage().instance().get(&TreeKey::Zeroes) {
        return z;
    }
    let mut z = Vec::new(env);
    let mut cur = U256::from_u32(env, 0);
    z.push_back(cur.clone());
    for _ in 0..TREE_DEPTH {
        cur = hash_node(env, &cur, &cur);
        z.push_back(cur.clone());
    }
    env.storage().instance().set(&TreeKey::Zeroes, &z);
    z
}

fn roots(env: &Env) -> Vec<U256> {
    env.storage()
        .instance()
        .get(&TreeKey::Roots)
        .unwrap_or_else(|| Vec::new(env))
}

/// The current tree root: the most recent appended root, or the empty-tree root
/// (`z[depth]`) when no leaf has been appended yet.
pub fn current_root(env: &Env) -> U256 {
    match roots(env).last() {
        Some(r) => r,
        None => zeroes(env).get(TREE_DEPTH).unwrap(),
    }
}

/// Whether `root` is within the rolling window of recently-published roots.
pub fn root_is_recent(env: &Env, root: &U256) -> bool {
    roots(env).iter().any(|r| &r == root)
}

/// Append a commitment as the next leaf; returns its leaf index. Updates the
/// frontier, the current root, and the rolling-root window.
pub fn append(env: &Env, commitment: &U256) -> Result<u32, Error> {
    let next: u32 = env
        .storage()
        .instance()
        .get(&TreeKey::NextIndex)
        .unwrap_or(0);
    if (next as u64) >= MAX_LEAVES {
        return Err(Error::TreeFull);
    }
    let z = zeroes(env);

    // Fold the new leaf up to the root, caching/consuming left siblings.
    let mut cur = commitment.clone();
    let mut level = 0u32;
    while level < TREE_DEPTH {
        let is_right = (next >> level) & 1 == 1;
        if is_right {
            let left: U256 = env
                .storage()
                .instance()
                .get(&TreeKey::Frontier(level))
                .unwrap_or_else(|| z.get(level).unwrap());
            cur = hash_node(env, &left, &cur);
        } else {
            // This leaf is the left child at this level — cache it for the future
            // right sibling, and pair with the empty subtree for now.
            env.storage().instance().set(&TreeKey::Frontier(level), &cur);
            cur = hash_node(env, &cur, &z.get(level).unwrap());
        }
        level += 1;
    }

    // Push the new root into the rolling window (oldest first, drop overflow).
    let mut rs = roots(env);
    rs.push_back(cur);
    while rs.len() > ROOT_WINDOW {
        rs.pop_front();
    }
    env.storage().instance().set(&TreeKey::Roots, &rs);
    env.storage().instance().set(&TreeKey::NextIndex, &(next + 1));

    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutils::harness;
    use soroban_sdk::{Bytes, Env, U256};

    // Cross-check fixtures from the circuit (Noir witness builder, depth 20):
    // leaf 0 = the demo input note's commitment; its single-leaf tree root is the
    // `commitment_root` public input in circuits/transfer/Prover.toml.
    const LEAF0: [u8; 32] = [
        0x0a, 0x44, 0x67, 0x67, 0xea, 0x07, 0xef, 0x73, 0xcb, 0x79, 0xec, 0x4b, 0xcb, 0x46, 0x69,
        0x4b, 0xd9, 0xaf, 0x50, 0xb0, 0x94, 0x67, 0xe0, 0xf3, 0xb0, 0x32, 0xbe, 0x9a, 0x20, 0xf4,
        0x0f, 0xdd,
    ];
    const ROOT_LEAF0: [u8; 32] = [
        0x03, 0x03, 0x96, 0xe1, 0x0c, 0xf6, 0xde, 0x23, 0xaf, 0xac, 0x50, 0x48, 0x3a, 0xa9, 0xe8,
        0x86, 0x33, 0x12, 0xa5, 0x4d, 0x8f, 0x2c, 0x7e, 0xf8, 0x87, 0xff, 0xdc, 0x49, 0x91, 0x99,
        0x12, 0x4b,
    ];

    fn u256_be(env: &Env, bytes: &[u8; 32]) -> U256 {
        U256::from_be_bytes(env, &Bytes::from_array(env, bytes))
    }

    #[test]
    fn empty_root_is_empty_subtree() {
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        let id = harness(&env);
        env.as_contract(&id, || {
            let z = zeroes(&env);
            assert_eq!(current_root(&env), z.get(TREE_DEPTH).unwrap());
        });
    }

    #[test]
    fn append_leaf0_matches_circuit_commitment_root() {
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        let id = harness(&env);
        env.as_contract(&id, || {
            let leaf = u256_be(&env, &LEAF0);
            let idx = append(&env, &leaf).unwrap();
            assert_eq!(idx, 0);
            let want = u256_be(&env, &ROOT_LEAF0);
            assert_eq!(current_root(&env), want, "contract tree root must equal the circuit's commitment_root");
            assert!(root_is_recent(&env, &want));
        });
    }

    #[test]
    fn appends_advance_root_and_window_tracks_recent() {
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        let id = harness(&env);
        env.as_contract(&id, || {
            let a = append(&env, &U256::from_u32(&env, 11)).unwrap();
            let root_a = current_root(&env);
            let b = append(&env, &U256::from_u32(&env, 22)).unwrap();
            let root_b = current_root(&env);
            assert_eq!((a, b), (0, 1));
            assert_ne!(root_a, root_b);
            // Both roots are still inside the window.
            assert!(root_is_recent(&env, &root_a));
            assert!(root_is_recent(&env, &root_b));
            // An unrelated value is not a known root.
            assert!(!root_is_recent(&env, &U256::from_u32(&env, 99)));
        });
    }

    #[test]
    fn rolling_window_drops_oldest_root() {
        // Each append is its own transaction on-chain, so drive each as a separate
        // invocation (state persists in storage); bundling all into one `as_contract`
        // would exceed the per-invocation instruction limit.
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        let id = harness(&env);

        let first_root = env.as_contract(&id, || {
            append(&env, &U256::from_u32(&env, 1000)).unwrap();
            current_root(&env)
        });
        assert!(env.as_contract(&id, || root_is_recent(&env, &first_root)));

        // Fill the window so the first root falls off (ROOT_WINDOW more appends).
        for i in 0..ROOT_WINDOW {
            env.as_contract(&id, || {
                append(&env, &U256::from_u32(&env, 2000 + i)).unwrap();
            });
        }

        assert!(
            !env.as_contract(&id, || root_is_recent(&env, &first_root)),
            "oldest root should have aged out of the window"
        );
        let cur = env.as_contract(&id, || current_root(&env));
        assert!(env.as_contract(&id, || root_is_recent(&env, &cur)));
    }
}
