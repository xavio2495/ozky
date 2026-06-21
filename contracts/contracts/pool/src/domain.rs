//! Domain separation / anti-replay (Z1 spec §10, decision D5). Every spend's
//! `domain_sep` public input must equal `Poseidon(pool_id, network_id, SELECTOR)`,
//! binding the proof to THIS pool, on THIS network, for THIS operation. The circuit
//! treats `domain_sep` as opaque (only asserts it is non-zero); the contract is what
//! recomputes it from its own identity and rejects a mismatch — so a USDC-transfer
//! proof can't be replayed as a withdraw, on another pool, or another network.
//!
//! `pool_id` and `network_id` are field-encoded identity values fixed at init (the
//! prover uses the same values). SELECTOR constants are fixed protocol-wide.

use crate::poseidon::hash;
use soroban_sdk::{Env, Vec, U256};

pub const SELECTOR_DEPOSIT: u32 = 1;
pub const SELECTOR_TRANSFER: u32 = 2;
pub const SELECTOR_WITHDRAW: u32 = 3;
pub const SELECTOR_SPLIT: u32 = 4;

/// `domain_sep = Poseidon(pool_id, network_id, selector)` — same construction the
/// prover uses (Poseidon2 parity with the circuit's hash is already established).
pub fn compute_domain_sep(env: &Env, pool_id: &U256, network_id: &U256, selector: u32) -> U256 {
    let mut inputs = Vec::new(env);
    inputs.push_back(pool_id.clone());
    inputs.push_back(network_id.clone());
    inputs.push_back(U256::from_u32(env, selector));
    hash(env, &inputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutils::harness;
    use soroban_sdk::{Bytes, Env, U256};

    /// Noir reference: `Poseidon2::hash([7, 42, 2], 3)` (poseidon lib v0.2.0) —
    /// i.e. domain_sep for pool_id=7, network_id=42, selector=TRANSFER(2).
    const REF_7_42_2: [u8; 32] = [
        0x2e, 0xae, 0x4c, 0x36, 0x1f, 0x60, 0x5c, 0x06, 0xc7, 0x66, 0xcb, 0x12, 0x6a, 0x39, 0x1a,
        0x0f, 0x91, 0x63, 0x08, 0x61, 0x0a, 0xe8, 0x12, 0x8f, 0x7e, 0x61, 0x5e, 0x5e, 0x6b, 0x6c,
        0x67, 0xff,
    ];

    #[test]
    fn domain_sep_matches_noir_arity3_reference() {
        let env = Env::default();
        let id = harness(&env);
        env.as_contract(&id, || {
            let got = compute_domain_sep(
                &env,
                &U256::from_u32(&env, 7),
                &U256::from_u32(&env, 42),
                SELECTOR_TRANSFER,
            );
            let want = U256::from_be_bytes(&env, &Bytes::from_array(&env, &REF_7_42_2));
            assert_eq!(got, want, "contract domain_sep must equal Noir's Poseidon2([pool,net,selector])");
        });
    }

    #[test]
    fn domain_sep_is_deterministic_and_selector_bound() {
        let env = Env::default();
        let id = harness(&env);
        env.as_contract(&id, || {
            let pool_id = U256::from_u32(&env, 7);
            let net = U256::from_u32(&env, 42);
            let d_a = compute_domain_sep(&env, &pool_id, &net, SELECTOR_TRANSFER);
            let d_b = compute_domain_sep(&env, &pool_id, &net, SELECTOR_TRANSFER);
            // Deterministic for the same identity + selector.
            assert_eq!(d_a, d_b);
            // Distinct per selector (a transfer proof can't pass as deposit/withdraw).
            assert_ne!(d_a, compute_domain_sep(&env, &pool_id, &net, SELECTOR_DEPOSIT));
            assert_ne!(d_a, compute_domain_sep(&env, &pool_id, &net, SELECTOR_WITHDRAW));
            // Distinct per pool and per network.
            assert_ne!(
                d_a,
                compute_domain_sep(&env, &U256::from_u32(&env, 8), &net, SELECTOR_TRANSFER)
            );
            assert_ne!(
                d_a,
                compute_domain_sep(&env, &pool_id, &U256::from_u32(&env, 43), SELECTOR_TRANSFER)
            );
        });
    }
}
