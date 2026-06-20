#![no_std]
use soroban_sdk::{
    contract, contractimpl,
    crypto::bn254::{Bn254Fr, Bn254G1Affine},
    vec, Env, String, Vec, U256,
};

#[contract]
pub struct Contract;

#[contractimpl]
impl Contract {
    pub fn hello(env: Env, to: String) -> Vec<String> {
        vec![&env, String::from_str(&env, "Hello"), to]
    }

    /// Z0 readiness smoke test: exercise the BN254 host functions (CAP-0074) on the
    /// live network. Computes 2·G two independent ways — point doubling via `g1_add`
    /// and scalar multiplication via `g1_mul` — and confirms they agree. A `true`
    /// result proves the BN254 curve host functions are present and correct.
    pub fn bn254_smoke(env: Env) -> bool {
        // BN254 G1 generator (1, 2) in Ethereum-compatible big-endian X‖Y (64 bytes).
        let mut g = [0u8; 64];
        g[31] = 1; // X = 1
        g[63] = 2; // Y = 2
        let gen = Bn254G1Affine::from_array(&env, &g);

        let bn = env.crypto().bn254();
        let doubled = bn.g1_add(&gen, &gen); // G + G
        let two = Bn254Fr::from_u256(U256::from_u32(&env, 2));
        let scaled = bn.g1_mul(&gen, &two); // 2 · G

        doubled == scaled
    }
}

mod test;
