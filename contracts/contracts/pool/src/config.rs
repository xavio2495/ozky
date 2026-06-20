//! Pool configuration set at deployment: the field-encoded identity used for
//! `domain_sep` (`pool_id`, `network_id`), one verifier address per circuit (each
//! a deployment of the vendored verifier holding that circuit's frozen VK), the
//! `policy` contract address (deposit allow-list authority, spec §8), the ASP
//! approved-set root, and the admin (testnet: dev-controlled; → governance before
//! mainnet, per handoff §8).
//!
//! `asp_root` is cached here for the hot interior path (transfer/withdraw check it
//! locally — no cross-contract read per spend); `policy` is the canonical ASP
//! authority, consulted live only on the cold deposit edge for the allow-list.

use soroban_sdk::{contracttype, Address, Env, U256};

#[contracttype]
#[derive(Clone)]
pub struct Config {
    pub pool_id: U256,
    pub network_id: U256,
    pub deposit_verifier: Address,
    pub transfer_verifier: Address,
    pub withdraw_verifier: Address,
    pub policy: Address,
    pub asp_root: U256,
    pub admin: Address,
}

#[contracttype]
#[derive(Clone)]
pub enum ConfigKey {
    Config,
}

pub fn set(env: &Env, cfg: &Config) {
    env.storage().instance().set(&ConfigKey::Config, cfg);
}

pub fn get(env: &Env) -> Config {
    env.storage().instance().get(&ConfigKey::Config).unwrap()
}

pub fn is_set(env: &Env) -> bool {
    env.storage().instance().has(&ConfigKey::Config)
}
