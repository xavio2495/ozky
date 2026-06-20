//! Target-pool configuration (Phase A3). Everything a send must bind to on-chain:
//! the deployed pool contract, its identity fields (`pool_id` / `network_id` — which
//! the proof's `domain_sep` is computed from), the asset, and the network endpoints.
//!
//! Pools on testnet are deployed per-run (no canonical mainnet pool yet), so the pool
//! contract id is supplied via the environment; the field values default to the
//! testnet round-trip's (`pool_id=7`, `network_id=42`, `asset_tag=1`). The live-run
//! script (`contracts/roundtrip/a3_send_testnet.sh`) deploys a pool bound to the
//! wallet's own `owner_pk` and exports `OZKY_POOL_CONTRACT` before invoking a send.

use super::poseidon::Fr;
use super::CoreError;

/// Resolved configuration for the pool a send targets.
#[derive(Clone)]
pub struct PoolConfig {
    /// Deployed pool contract id (`C…`).
    pub pool_contract: String,
    /// `pool_id` field (folded into `domain_sep`).
    pub pool_id: Fr,
    /// `network_id` field (folded into `domain_sep`).
    pub network_id: Fr,
    /// The single asset for the transfer (`asset_tag` field).
    pub asset_tag: Fr,
    pub rpc_url: String,
    pub network: String,
    pub network_passphrase: String,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// `pool_id=7`, `network_id=42`, `asset_tag=1` — the values the frozen-VK round-trips
/// used. Overridable via `OZKY_POOL_ID` / `OZKY_NETWORK_ID` / `OZKY_ASSET_TAG`.
fn env_field(key: &str, default: u64) -> Result<Fr, CoreError> {
    match std::env::var(key) {
        Ok(v) => v
            .parse::<u64>()
            .map(Fr::from_u64)
            .map_err(|_| CoreError::Chain(format!("{key} must be a u64: {v}"))),
        Err(_) => Ok(Fr::from_u64(default)),
    }
}

impl PoolConfig {
    /// Resolve from the environment. `OZKY_POOL_CONTRACT` is required (the deployed
    /// pool's id); the rest fall back to testnet defaults.
    pub fn load() -> Result<PoolConfig, CoreError> {
        let pool_contract = std::env::var("OZKY_POOL_CONTRACT").map_err(|_| {
            CoreError::Chain(
                "OZKY_POOL_CONTRACT not set (the deployed pool contract id to send against)".into(),
            )
        })?;
        Ok(PoolConfig {
            pool_contract,
            pool_id: env_field("OZKY_POOL_ID", 7)?,
            network_id: env_field("OZKY_NETWORK_ID", 42)?,
            asset_tag: env_field("OZKY_ASSET_TAG", 1)?,
            rpc_url: env_or("OZKY_RPC_URL", super::chain::DEFAULT_RPC_URL),
            network: env_or("OZKY_NETWORK", super::chain::DEFAULT_NETWORK),
            network_passphrase: env_or(
                "OZKY_NETWORK_PASSPHRASE",
                "Test SDF Network ; September 2015",
            ),
        })
    }

    /// `asset_tag` as the decimal string the stellar CLI expects for a `U256` arg.
    pub fn asset_tag_decimal(&self) -> String {
        self.asset_tag.to_decimal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_requires_pool_contract() {
        // Without OZKY_POOL_CONTRACT set, load() errors (the test env doesn't set it).
        // (Other tests don't set it either; this asserts the required-field behaviour.)
        std::env::remove_var("OZKY_POOL_CONTRACT");
        assert!(PoolConfig::load().is_err());
    }

    #[test]
    fn field_defaults_match_roundtrip() {
        assert_eq!(env_field("OZKY_POOL_ID", 7).unwrap(), Fr::from_u64(7));
        assert_eq!(env_field("OZKY_NETWORK_ID", 42).unwrap(), Fr::from_u64(42));
        assert_eq!(Fr::from_u64(1).to_decimal(), "1");
    }
}
