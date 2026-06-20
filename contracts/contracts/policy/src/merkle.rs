//! On-chain ASP approved-set Merkle root — depth-20 Poseidon2 over BN254 Fr, the
//! SAME construction the circuit's `notes::merkle` and the client's witness builder
//! use, so the root this contract publishes equals the `asp_root` a spender proves
//! membership against. Reuses `soroban-poseidon` (proven parity, the pool/indexer
//! crate).
//!
//! The approved set is a small ordered list of approved `owner_pk`s; we recompute the
//! root from the full list on each enrollment (testnet sets are small). The fold
//! mirrors the indexer/client exactly (`merkle_path` over ordered leaves with the
//! zero-subtree ladder), so all three agree byte-for-byte.

use soroban_poseidon::{poseidon2_hash, Field};
use soroban_sdk::{crypto::BnScalar, Env, Vec, U256};

pub const TREE_DEPTH: u32 = 20;

/// Internal node hash `Poseidon2::hash([left, right], 2)` (inputs reduced mod Fr).
fn hash_node(env: &Env, left: &U256, right: &U256) -> U256 {
    let modulus = <BnScalar as Field>::modulus(env);
    let mut inputs = Vec::new(env);
    inputs.push_back(left.rem_euclid(&modulus));
    inputs.push_back(right.rem_euclid(&modulus));
    poseidon2_hash::<4, BnScalar>(env, &inputs)
}

/// Zero-subtree ladder: `z[0]=0`, `z[i+1]=H(z[i],z[i])`; length `TREE_DEPTH+1`.
fn zeroes(env: &Env) -> Vec<U256> {
    let mut z = Vec::new(env);
    let mut cur = U256::from_u32(env, 0);
    z.push_back(cur.clone());
    for _ in 0..TREE_DEPTH {
        cur = hash_node(env, &cur, &cur);
        z.push_back(cur.clone());
    }
    z
}

/// The depth-20 root of an ordered leaf list (empty list → empty-tree root `z[20]`).
/// Folds level by level, padding the odd tail / missing siblings with the zero ladder
/// — identical to `indexer/src/tree.rs::merkle_path` and the client's `commitment_path`.
pub fn root_of(env: &Env, leaves: &Vec<U256>) -> U256 {
    let z = zeroes(env);
    let modulus = <BnScalar as Field>::modulus(env);
    let mut cur: Vec<U256> = Vec::new(env);
    for l in leaves.iter() {
        cur.push_back(l.rem_euclid(&modulus));
    }
    let mut level: u32 = 0;
    while level < TREE_DEPTH {
        let mut next = Vec::new(env);
        let mut i = 0u32;
        while i < cur.len() {
            let left = cur.get(i).unwrap();
            let right = if i + 1 < cur.len() {
                cur.get(i + 1).unwrap()
            } else {
                z.get(level).unwrap()
            };
            next.push_back(hash_node(env, &left, &right));
            i += 2;
        }
        cur = next;
        level += 1;
    }
    if cur.len() == 1 {
        cur.get(0).unwrap()
    } else {
        z.get(TREE_DEPTH).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{Bytes, Env, U256};

    fn u256_be(env: &Env, bytes: &[u8; 32]) -> U256 {
        U256::from_be_bytes(env, &Bytes::from_array(env, bytes))
    }

    // Cross-check: the single-leaf approved-set root for owner_pk(12345) must equal the
    // `asp_root` the circuit/client produce — the value baked into the frozen-VK
    // round-trip (circuits/transfer/Prover.toml asp_root).
    const OWNER_PK_12345: [u8; 32] = [
        0x2c, 0x4e, 0x23, 0x0d, 0xe1, 0x85, 0xe2, 0xff, 0xa3, 0xee, 0x2f, 0x95, 0xb2, 0x89, 0x5c,
        0x8a, 0x30, 0x24, 0x10, 0x18, 0x97, 0x3f, 0x57, 0xf0, 0xe1, 0x61, 0x02, 0xc3, 0x6d, 0xe1,
        0x59, 0x0e,
    ];
    const ASP_ROOT_SINGLE: [u8; 32] = [
        0x16, 0x10, 0x44, 0x6d, 0x12, 0x3b, 0x3b, 0xe5, 0xa3, 0x38, 0x71, 0x2b, 0xcf, 0x50, 0x80,
        0x07, 0xd9, 0x41, 0x84, 0xa7, 0x1c, 0xb8, 0x04, 0x5d, 0xd3, 0x51, 0xcb, 0xd6, 0x8a, 0x52,
        0xb8, 0xdd,
    ];

    #[test]
    fn single_member_root_matches_circuit() {
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        let mut leaves = Vec::new(&env);
        leaves.push_back(u256_be(&env, &OWNER_PK_12345));
        assert_eq!(root_of(&env, &leaves), u256_be(&env, &ASP_ROOT_SINGLE));
    }

    #[test]
    fn empty_set_is_empty_subtree_root() {
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        let empty: Vec<U256> = Vec::new(&env);
        assert_eq!(root_of(&env, &empty), zeroes(&env).get(TREE_DEPTH).unwrap());
    }

    #[test]
    fn adding_a_member_changes_the_root() {
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        let mut one = Vec::new(&env);
        one.push_back(u256_be(&env, &OWNER_PK_12345));
        let mut two = one.clone();
        two.push_back(U256::from_u32(&env, 99));
        assert_ne!(root_of(&env, &one), root_of(&env, &two));
    }
}
