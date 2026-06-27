//! Off-chain reconstruction of the depth-20 commitment Merkle tree, using the SAME
//! Poseidon2 the contract/circuit use (`soroban-poseidon`), so a served path is
//! byte-for-byte acceptable to the spend circuit. The rebuilt root is self-checked
//! against the contract's published `roots` event — if they differ, the indexer is
//! out of sync (and says so) rather than serving a bad witness.

use soroban_poseidon::{poseidon2_hash, Field};
use soroban_sdk::{crypto::BnScalar, Bytes, Env, Vec as SVec, U256};

pub const DEPTH: usize = 20;

fn hash_node(env: &Env, left: &U256, right: &U256) -> U256 {
    let modulus = <BnScalar as Field>::modulus(env);
    let mut inputs = SVec::new(env);
    inputs.push_back(left.rem_euclid(&modulus));
    inputs.push_back(right.rem_euclid(&modulus));
    poseidon2_hash::<4, BnScalar>(env, &inputs)
}

/// IndexedLeaf hash: `Poseidon2([value, next_index, next_value], 3)` (matches the
/// circuit's `accumulator.nr::IndexedLeaf::hash`).
pub fn indexed_leaf_hash_hex(env: &Env, value: &U256, next_index: u64, next_value: &U256) -> String {
    let modulus = <BnScalar as Field>::modulus(env);
    let mut inputs = SVec::new(env);
    inputs.push_back(value.rem_euclid(&modulus));
    inputs.push_back(U256::from_u128(env, next_index as u128));
    inputs.push_back(next_value.rem_euclid(&modulus));
    u256_to_hex(env, &poseidon2_hash::<4, BnScalar>(env, &inputs))
}

pub fn u256_from_hex(env: &Env, h: &str) -> Option<U256> {
    let h = h.strip_prefix("0x").unwrap_or(h);
    if h.len() > 64 {
        return None;
    }
    let padded = format!("{:0>64}", h);
    let mut bytes = [0u8; 32];
    for i in 0..32 {
        bytes[i] = u8::from_str_radix(&padded[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(U256::from_be_bytes(env, &Bytes::from_array(env, &bytes)))
}

pub fn u256_to_hex(_env: &Env, v: &U256) -> String {
    let bytes = v.to_be_bytes();
    let mut arr = [0u8; 32];
    bytes.copy_into_slice(&mut arr);
    crate::events::to_hex(&arr)
}

/// Zero-subtree ladder: z[0]=0, z[i+1]=H(z[i],z[i]); length DEPTH+1.
fn zeroes(env: &Env) -> Vec<U256> {
    let mut z = Vec::with_capacity(DEPTH + 1);
    let mut cur = U256::from_u32(env, 0);
    z.push(cur.clone());
    for _ in 0..DEPTH {
        cur = hash_node(env, &cur, &cur);
        z.push(cur.clone());
    }
    z
}

pub struct MerklePath {
    pub leaf_index: u32,
    pub leaf: String,
    pub root: String,
    pub path_is_right: Vec<bool>,
    pub siblings: Vec<String>,
}

/// Build the authentication path for `index` from the ordered commitment leaves
/// (hex). Returns the path + the computed root (caller compares to published).
pub fn merkle_path(env: &Env, leaves_hex: &[String], index: u32) -> Option<MerklePath> {
    if (index as usize) >= leaves_hex.len() {
        return None;
    }
    let z = zeroes(env);
    let mut cur: Vec<U256> = leaves_hex
        .iter()
        .map(|h| u256_from_hex(env, h))
        .collect::<Option<Vec<_>>>()?;

    let mut pos = index as usize;
    let mut is_right = vec![false; DEPTH];
    let mut siblings: Vec<String> = Vec::with_capacity(DEPTH);

    for level in 0..DEPTH {
        let sib_index = pos ^ 1;
        let sib = if sib_index < cur.len() {
            cur[sib_index].clone()
        } else {
            z[level].clone()
        };
        is_right[level] = pos & 1 == 1;
        siblings.push(u256_to_hex(env, &sib));

        // Fold to the next level (pad odd tail with the empty subtree).
        let mut next: Vec<U256> = Vec::with_capacity((cur.len() + 1) / 2);
        let mut i = 0;
        while i < cur.len() {
            let l = cur[i].clone();
            let r = if i + 1 < cur.len() {
                cur[i + 1].clone()
            } else {
                z[level].clone()
            };
            next.push(hash_node(env, &l, &r));
            i += 2;
        }
        cur = next;
        pos >>= 1;
    }

    let root = if cur.len() == 1 {
        u256_to_hex(env, &cur[0])
    } else {
        u256_to_hex(env, &z[DEPTH])
    };

    Some(MerklePath {
        leaf_index: index,
        leaf: leaves_hex[index as usize].clone(),
        root,
        path_is_right: is_right,
        siblings,
    })
}

/// Reference-vector self-test (the frozen parity vector): Poseidon2([1,2]) must be
/// `0x0386…ed7383`. Run at startup to fail fast if the hash ever drifts.
pub fn parity_self_test(env: &Env) -> bool {
    let got = hash_node(env, &U256::from_u32(env, 1), &U256::from_u32(env, 2));
    u256_to_hex(env, &got) == "0x038682aa1cb5ae4e0a3f13da432a95c77c5c111f6f030faf9cad641ce1ed7383"
}
