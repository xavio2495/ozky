//! Circuit-matching Poseidon2 over BN254 Fr (Phase A2). Reproduces, in native
//! Rust, the exact hashing the Noir circuits and the pool contract use, so the
//! witnesses this core builds are byte-for-byte acceptable to the on-chain verifier.
//!
//! We reuse `soroban-poseidon` (the CAP-0075 host-function wrapper) — the same crate
//! the contract and the indexer use, whose parity with Noir's `Poseidon2::hash` is
//! proven against the frozen reference vector `Poseidon2([1,2]) = 0x0386…ed7383`.
//! It pulls `soroban-sdk` (heavy), accepted because Poseidon parity is the
//! foundational correctness risk and this is the one path already proven.
//!
//! Field elements cross this module's boundary as [`Fr`] (32-byte big-endian),
//! decoupling the rest of the witness/proving code from soroban types and matching
//! the hex interchange with the indexer.

use soroban_poseidon::poseidon2_hash;
use soroban_sdk::{crypto::BnScalar, Bytes, Env, Vec as SVec, U256};

/// Domain-separation tag for owner-key derivation (circuit `notes::DOMAIN_OWNER`,
/// ASCII "ozky_own"). MUST match the Noir constant or `owner_pk` will not match.
pub const DOMAIN_OWNER: u64 = 0x6f7a6b795f6f776e;

/// Domain tag for the withdraw destination binding (`dest_bind = Poseidon(DOMAIN_DEST,
/// dest)`, ASCII "ozky_dst"). NOTE: the on-chain binding of `dest_bind` to the actual
/// destination is not yet enforced (the pool only checks `dest_bind != 0` — a Z4 debt),
/// so this tag is forward-compat; any non-zero `dest_bind` is currently accepted.
pub const DOMAIN_DEST: u64 = 0x6f7a6b795f647374;

/// Circuit selectors for `domain_sep` (pool `domain.rs`).
pub const SELECTOR_DEPOSIT: u64 = 1;
pub const SELECTOR_TRANSFER: u64 = 2;
pub const SELECTOR_WITHDRAW: u64 = 3;

/// A BN254 Fr field element, 32-byte big-endian. The canonical wire form everywhere
/// witnesses, commitments, nullifiers and roots are passed around in this core.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Fr(pub [u8; 32]);

impl Fr {
    pub const ZERO: Fr = Fr([0u8; 32]);

    pub fn from_u64(n: u64) -> Fr {
        let mut b = [0u8; 32];
        b[24..].copy_from_slice(&n.to_be_bytes());
        Fr(b)
    }

    /// A random field element from the OS RNG, with the top 4 bits cleared so it is
    /// always a valid BN254 Fr (< the ~2^254 modulus) and `nargo` accepts it as a
    /// `Field` literal. Used for output-note blindings / rhos.
    pub fn random() -> Fr {
        use rand_core::RngCore;
        let mut b = [0u8; 32];
        rand_core::OsRng.fill_bytes(&mut b);
        b[0] &= 0x0f;
        Fr(b)
    }

    /// Parse `0x…` (or bare) hex; shorter inputs are left-zero-padded.
    pub fn from_hex(h: &str) -> Option<Fr> {
        let h = h.strip_prefix("0x").unwrap_or(h);
        if h.len() > 64 {
            return None;
        }
        let padded = format!("{h:0>64}");
        let mut b = [0u8; 32];
        for i in 0..32 {
            b[i] = u8::from_str_radix(&padded[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(Fr(b))
    }

    /// `0x`-prefixed big-endian hex (matches the indexer / Noir `println` form).
    pub fn to_hex(&self) -> String {
        format!("0x{}", hex::encode(self.0))
    }

    /// Decimal big-endian (for Prover.toml integer fields like `value`/`epoch`).
    pub fn to_decimal(&self) -> String {
        num_bigint::BigUint::from_bytes_be(&self.0).to_string()
    }

    /// Field comparison `self < other` over the integers (the circuit's `Field.lt`,
    /// valid for the in-range values nullifiers/accumulator leaves take).
    pub fn lt(&self, other: &Fr) -> bool {
        self.0 < other.0
    }

    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 32]
    }
}

/// Owns a metering-disabled soroban `Env` and reuses it across a witness build (one
/// witness does many hashes — a fresh `Env` per hash would be wasteful).
pub struct Hasher {
    env: Env,
}

impl Default for Hasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher {
    pub fn new() -> Self {
        let env = Env::default();
        // Off-chain tool: lift the host budget (a witness build does many Poseidon
        // hashes, which would otherwise exhaust the default metering budget).
        env.cost_estimate().budget().reset_unlimited();
        Hasher { env }
    }

    fn u256(&self, f: &Fr) -> U256 {
        U256::from_be_bytes(&self.env, &Bytes::from_array(&self.env, &f.0))
    }

    fn from_u256(&self, v: &U256) -> Fr {
        let mut b = [0u8; 32];
        v.to_be_bytes().copy_into_slice(&mut b);
        Fr(b)
    }

    /// General Poseidon2 sponge — matches Noir's `Poseidon2::hash(xs, n)`. Each input
    /// is reduced mod the BN254 scalar field first (the circuit's field semantics).
    pub fn hash(&self, inputs: &[Fr]) -> Fr {
        let modulus = <BnScalar as soroban_poseidon::Field>::modulus(&self.env);
        let mut reduced = SVec::new(&self.env);
        for x in inputs {
            reduced.push_back(self.u256(x).rem_euclid(&modulus));
        }
        self.from_u256(&poseidon2_hash::<4, BnScalar>(&self.env, &reduced))
    }

    /// Internal Merkle node hash: `Poseidon2::hash([left, right], 2)`.
    pub fn hash_node(&self, left: &Fr, right: &Fr) -> Fr {
        self.hash(&[*left, *right])
    }

    /// `owner_pk = Poseidon(DOMAIN_OWNER, owner_sk)` (circuit D4).
    pub fn owner_pk(&self, owner_sk: &Fr) -> Fr {
        self.hash(&[Fr::from_u64(DOMAIN_OWNER), *owner_sk])
    }

    /// `commitment = Poseidon(value, asset_tag, owner_pk, blinding, epoch, rho)`.
    pub fn commitment(
        &self,
        value: &Fr,
        asset_tag: &Fr,
        owner_pk: &Fr,
        blinding: &Fr,
        epoch: &Fr,
        rho: &Fr,
    ) -> Fr {
        self.hash(&[*value, *asset_tag, *owner_pk, *blinding, *epoch, *rho])
    }

    /// `nullifier = Poseidon(rho, owner_sk)` (FROZEN).
    pub fn nullifier(&self, rho: &Fr, owner_sk: &Fr) -> Fr {
        self.hash(&[*rho, *owner_sk])
    }

    /// `domain_sep = Poseidon(pool_id, network_id, selector)` (D5, pool `domain.rs`).
    pub fn domain_sep(&self, pool_id: &Fr, network_id: &Fr, selector: u64) -> Fr {
        self.hash(&[*pool_id, *network_id, Fr::from_u64(selector)])
    }

    /// Indexed-accumulator leaf hash: `Poseidon([value, next_index, next_value], 3)`
    /// (circuit `accumulator.nr::IndexedLeaf::hash`).
    pub fn indexed_leaf(&self, value: &Fr, next_index: u64, next_value: &Fr) -> Fr {
        self.hash(&[*value, Fr::from_u64(next_index), *next_value])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Frozen Noir reference vectors (captured via `nargo test print_reference_vectors`).
    const REF_2: &str = "0x038682aa1cb5ae4e0a3f13da432a95c77c5c111f6f030faf9cad641ce1ed7383";
    const REF_6: &str = "0x07f57fcda925c06dc0a311f3f17fa0218e079b514552744a25ba8a74ee8c9e7a";
    const REF_OWNER_PK_12345: &str =
        "0x2c4e230de185e2ffa3ee2f95b2895c8a30241018973f57f0e16102c36de1590e";

    #[test]
    fn poseidon2_arity2_matches_noir() {
        let h = Hasher::new();
        let got = h.hash(&[Fr::from_u64(1), Fr::from_u64(2)]);
        assert_eq!(got.to_hex(), REF_2);
    }

    #[test]
    fn poseidon2_arity6_matches_noir() {
        let h = Hasher::new();
        let got = h.hash(&[
            Fr::from_u64(1),
            Fr::from_u64(2),
            Fr::from_u64(3),
            Fr::from_u64(4),
            Fr::from_u64(5),
            Fr::from_u64(6),
        ]);
        assert_eq!(got.to_hex(), REF_6);
    }

    #[test]
    fn owner_pk_matches_noir() {
        let h = Hasher::new();
        let got = h.owner_pk(&Fr::from_u64(12345));
        assert_eq!(got.to_hex(), REF_OWNER_PK_12345);
    }

    #[test]
    fn fr_hex_roundtrip() {
        let f = Fr::from_hex(REF_2).unwrap();
        assert_eq!(f.to_hex(), REF_2);
        // Left-padding of short hex.
        assert_eq!(Fr::from_hex("0x1").unwrap(), Fr::from_u64(1));
    }

    #[test]
    fn node_hash_order_sensitive() {
        let h = Hasher::new();
        assert_ne!(
            h.hash_node(&Fr::from_u64(1), &Fr::from_u64(2)),
            h.hash_node(&Fr::from_u64(2), &Fr::from_u64(1))
        );
    }
}
