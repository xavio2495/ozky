//! Noir↔Soroban Poseidon parity — the commitment tree's node hash.
//!
//! The commitment Merkle tree (Z1 spec §4a) is contract-maintained, so the
//! contract must compute the SAME node hash the circuit does:
//! `notes::merkle::hash_node(l, r) = Poseidon2::hash([l, r], 2)` (poseidon lib
//! v0.2.0, BN254). We use the `soroban-poseidon` host-function wrapper with
//! state width 4 (rate 3 / capacity 1) — the same construction Barretenberg/Noir
//! use — so on-chain hashing matches the proof's hashing exactly.
//!
//! Parity is asserted against the frozen reference vector in `tests` below
//! (`Poseidon2([1, 2]) = 0x0386…ed7383`, captured in Z2). The contract only ever
//! needs the 2-input node hash: leaf commitments arrive as proof public inputs
//! (their 6-input Poseidon is enforced inside the circuit), so the contract
//! never recomputes a commitment — only Merkle nodes.

use soroban_poseidon::{poseidon2_hash, Field};
use soroban_sdk::{crypto::BnScalar, Env, Vec, U256};

/// General Poseidon2 sponge over BN254 Fr — matches Noir's `Poseidon2::hash(xs, n)`.
/// Each input is reduced mod the BN254 scalar field first (the circuit's field
/// semantics). State width 4 (rate 3 / capacity 1) — the bb/Noir construction.
pub fn hash(env: &Env, inputs: &Vec<U256>) -> U256 {
    let modulus = <BnScalar as Field>::modulus(env);
    let mut reduced = Vec::new(env);
    for x in inputs.iter() {
        reduced.push_back(x.rem_euclid(&modulus));
    }
    poseidon2_hash::<4, BnScalar>(env, &reduced)
}

/// Internal Merkle node hash: `Poseidon2::hash([left, right], 2)`.
pub fn hash_node(env: &Env, left: &U256, right: &U256) -> U256 {
    let mut inputs = Vec::new(env);
    inputs.push_back(left.clone());
    inputs.push_back(right.clone());
    hash(env, &inputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{Bytes, Env, U256};

    /// Frozen Z2 reference vector: the canonical Noir node hash for [1, 2].
    /// `Poseidon2::hash([1, 2], 2)` (poseidon lib v0.2.0).
    const REF_1_2: [u8; 32] = [
        0x03, 0x86, 0x82, 0xaa, 0x1c, 0xb5, 0xae, 0x4e, 0x0a, 0x3f, 0x13, 0xda, 0x43, 0x2a, 0x95,
        0xc7, 0x7c, 0x5c, 0x11, 0x1f, 0x6f, 0x03, 0x0f, 0xaf, 0x9c, 0xad, 0x64, 0x1c, 0xe1, 0xed,
        0x73, 0x83,
    ];

    fn u256(env: &Env, n: u64) -> U256 {
        U256::from_u32(env, n as u32)
    }

    #[test]
    fn node_hash_matches_circuit_reference_vector() {
        let env = Env::default();
        let got = hash_node(&env, &u256(&env, 1), &u256(&env, 2));
        let want = U256::from_be_bytes(&env, &Bytes::from_array(&env, &REF_1_2));
        assert_eq!(got, want, "Soroban Poseidon2 node hash must equal the Noir circuit's");
    }

    #[test]
    fn node_hash_is_order_sensitive() {
        let env = Env::default();
        let lr = hash_node(&env, &u256(&env, 1), &u256(&env, 2));
        let rl = hash_node(&env, &u256(&env, 2), &u256(&env, 1));
        assert_ne!(lr, rl, "H(l,r) must differ from H(r,l) — left/right position matters");
    }
}
