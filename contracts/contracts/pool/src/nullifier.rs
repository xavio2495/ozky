//! Nullifier accumulator root (Z1 spec §4b, decision D3). The accumulator is an
//! indexed Merkle tree maintained in-circuit / off-chain; the contract stores only
//! its root. A spend proves (in-circuit) non-membership of each input nullifier
//! against `nullifier_old_root` and the post-insertion `nullifier_new_root`.
//!
//! The contract's job is narrow: check `nullifier_old_root == stored_root`, then
//! advance `stored_root = nullifier_new_root`. Double-spend protection falls out of
//! this: spending inserts the nullifier (advancing the root), so any later proof
//! that reused it would have to present a stale `old_root` and fail the check.

use crate::Error;
use soroban_sdk::{contracttype, Bytes, Env, U256};

/// Canonical empty-accumulator root: an indexed tree holding a single init leaf
/// `{value: 0, next_index: 0, next_value: 0}` at index 0, depth 20. MUST equal the
/// circuit's fresh `old_root` from `notes::transfer::build_two_insertions`
/// (captured as `nullifier_old_root` in circuits/transfer/Prover.toml).
const EMPTY_NULLIFIER_ROOT: [u8; 32] = [
    0x0b, 0x37, 0xaa, 0xb3, 0xb4, 0x22, 0xb7, 0x6a, 0xf6, 0x77, 0x94, 0x57, 0x11, 0x98, 0xd0, 0xe4,
    0x23, 0x66, 0xb6, 0x4b, 0x3e, 0xe7, 0x79, 0xb7, 0x2d, 0x72, 0xe8, 0x1f, 0x91, 0x8a, 0x49, 0x94,
];

#[contracttype]
#[derive(Clone)]
pub enum NfKey {
    Root,
}

/// The canonical empty-accumulator root as a field element.
pub fn empty_root(env: &Env) -> U256 {
    U256::from_be_bytes(env, &Bytes::from_array(env, &EMPTY_NULLIFIER_ROOT))
}

/// Initialize the stored root to the empty accumulator (idempotent).
pub fn init(env: &Env) {
    if !env.storage().instance().has(&NfKey::Root) {
        let root = empty_root(env);
        env.storage().instance().set(&NfKey::Root, &root);
    }
}

/// The current accumulator root (empty-accumulator root if never advanced).
pub fn current_root(env: &Env) -> U256 {
    env.storage()
        .instance()
        .get(&NfKey::Root)
        .unwrap_or_else(|| empty_root(env))
}

/// Overwrite the stored root (used after a verified spend).
pub fn set_root(env: &Env, new: &U256) {
    env.storage().instance().set(&NfKey::Root, new);
}

/// Check `old == stored`, then advance `stored = new`. Rejects a stale or replayed
/// `old` root (the double-spend / wrong-base guard).
pub fn advance(env: &Env, old: &U256, new: &U256) -> Result<(), Error> {
    let stored = current_root(env);
    if &stored != old {
        return Err(Error::NullifierRootMismatch);
    }
    set_root(env, new);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutils::harness;
    use soroban_sdk::{Bytes, Env, U256};

    // From circuits/transfer/Prover.toml: a fresh-accumulator spend.
    const OLD_ROOT: [u8; 32] = EMPTY_NULLIFIER_ROOT;
    const NEW_ROOT: [u8; 32] = [
        0x07, 0x07, 0xf8, 0x32, 0x70, 0xa0, 0xa5, 0x89, 0x95, 0x55, 0x61, 0xc9, 0x47, 0x17, 0xd2,
        0xbc, 0x05, 0x78, 0x63, 0xc4, 0xc5, 0x3e, 0x48, 0x16, 0xb7, 0xca, 0xe4, 0xe6, 0x19, 0xf2,
        0x5a, 0xef,
    ];

    fn u256_be(env: &Env, bytes: &[u8; 32]) -> U256 {
        U256::from_be_bytes(env, &Bytes::from_array(env, bytes))
    }

    #[test]
    fn fresh_pool_starts_at_empty_accumulator_root() {
        let env = Env::default();
        let id = harness(&env);
        env.as_contract(&id, || {
            init(&env);
            assert_eq!(current_root(&env), empty_root(&env));
        });
    }

    #[test]
    fn advance_then_reject_stale_old_root() {
        let env = Env::default();
        let id = harness(&env);
        env.as_contract(&id, || {
            init(&env);
            let old = u256_be(&env, &OLD_ROOT);
            let new = u256_be(&env, &NEW_ROOT);
            // First spend advances from the empty root to the circuit's new root.
            advance(&env, &old, &new).unwrap();
            assert_eq!(current_root(&env), new);
            // Replaying the same proof (same old_root) now fails: root has moved on.
            assert_eq!(advance(&env, &old, &new), Err(Error::NullifierRootMismatch));
        });
    }
}
