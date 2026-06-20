//! Asset registry: binds a field-encoded `asset_tag` (the value carried in every
//! note and proof) to its Stellar Asset Contract (SAC) address and decimals, so a
//! new stablecoin can be added without redeploying. Per-asset isolation — a note
//! redeems only against its own vault (the pool's balance in that SAC).

use soroban_sdk::{contracttype, Address, Env, U256};

#[contracttype]
#[derive(Clone)]
pub struct AssetInfo {
    pub sac: Address,
    pub decimals: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum AssetKey {
    Info(U256),
}

pub fn register(env: &Env, asset_tag: &U256, sac: &Address, decimals: u32) {
    let info = AssetInfo {
        sac: sac.clone(),
        decimals,
    };
    env.storage()
        .persistent()
        .set(&AssetKey::Info(asset_tag.clone()), &info);
}

pub fn get(env: &Env, asset_tag: &U256) -> Option<AssetInfo> {
    env.storage()
        .persistent()
        .get(&AssetKey::Info(asset_tag.clone()))
}
