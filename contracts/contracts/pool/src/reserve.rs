//! AMM liquidity reserves (roadmap 2.5 Phase 2). Per-asset reserve balance backing the in-pool
//! constant-product swap. The pool's SAC balance for an asset backs `outstanding notes + reserve`;
//! a swap re-labels value between the two (no token movement), so reserves are just an i128 ledger
//! counter per asset_tag. Seeded by the admin (`seed_reserve` pulls real tokens in).

use soroban_sdk::{contracttype, Env, U256};

#[contracttype]
#[derive(Clone)]
pub enum ReserveKey {
    Bal(U256),
}

pub fn get(env: &Env, asset_tag: &U256) -> i128 {
    env.storage()
        .persistent()
        .get(&ReserveKey::Bal(asset_tag.clone()))
        .unwrap_or(0i128)
}

pub fn set(env: &Env, asset_tag: &U256, amount: i128) {
    env.storage()
        .persistent()
        .set(&ReserveKey::Bal(asset_tag.clone()), &amount);
}
