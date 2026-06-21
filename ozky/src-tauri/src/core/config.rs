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
use std::collections::HashMap;
use std::path::PathBuf;

/// Optional dev config file: a flat JSON map of `OZKY_*` → value. Lets the app pick up
/// deployed contract IDs (and the prover path) WITHOUT environment variables. Path:
/// `$OZKY_CONFIG`, else `<repo>/ozky.config.json`. Missing/invalid → empty. Read fresh
/// per call (config load isn't hot; staying re-readable keeps it testable).
fn file_config() -> HashMap<String, String> {
    let path = std::env::var("OZKY_CONFIG").map(PathBuf::from).unwrap_or_else(|_| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("ozky.config.json")
    });
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<HashMap<String, String>>(&s).ok())
        .unwrap_or_default()
}

/// Resolve an `OZKY_*` setting: environment first, then the config file, else `None`.
pub fn cfg_var(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| file_config().get(key).cloned().filter(|s| !s.is_empty()))
}

/// A v1 asset the wallet can transact. `tag` is the in-circuit `asset_tag` (bound into
/// every note commitment and matched against the pool's `register_asset` registry);
/// `decimals` is for display only. Each asset must be `register_asset`'d on the pool
/// (tag → SAC) before its flows work on-chain.
pub struct AssetInfo {
    pub code: &'static str,
    pub tag: u64,
    pub decimals: u32,
}

/// The v1 asset set (spec: USDC/USDT/EURC stablecoins; native XLM is the testnet
/// default at `asset_tag 1`, the frozen-VK round-trip value). Tags are the canonical
/// `asset_tag` field values; they must agree with the pool's `register_asset` calls.
pub const ASSETS: &[AssetInfo] = &[
    AssetInfo { code: "XLM", tag: 1, decimals: 7 },
    AssetInfo { code: "USDC", tag: 2, decimals: 7 },
    AssetInfo { code: "USDT", tag: 3, decimals: 7 },
    AssetInfo { code: "EURC", tag: 4, decimals: 7 },
];

/// Look up a known asset by its code (case-insensitive, e.g. "usdc").
pub fn asset_by_code(code: &str) -> Option<&'static AssetInfo> {
    ASSETS.iter().find(|a| a.code.eq_ignore_ascii_case(code))
}

/// Look up a known asset by its `asset_tag` (for per-asset balance display).
pub fn asset_by_tag(tag: &Fr) -> Option<&'static AssetInfo> {
    ASSETS.iter().find(|a| Fr::from_u64(a.tag) == *tag)
}

/// Resolved configuration for the pool a send targets.
#[derive(Clone)]
pub struct PoolConfig {
    /// Deployed pool contract id (`C…`).
    pub pool_contract: String,
    /// Deployed policy contract id (`C…`) — the ASP approved-set authority. The wallet
    /// reconstructs the approved set from its `asp_mem` events to build membership paths.
    pub policy_contract: String,
    /// Deployed viewkeys contract id (`C…`) — the on-chain disclosure grant trail
    /// (register_view_key / disclose / revoke). Optional: only needed for selective
    /// disclosure (`share_with_auditor`).
    pub viewkeys_contract: Option<String>,
    /// `pool_id` field (folded into `domain_sep`).
    pub pool_id: Fr,
    /// `network_id` field (folded into `domain_sep`).
    pub network_id: Fr,
    /// The single asset for the transfer (`asset_tag` field).
    pub asset_tag: Fr,
    pub rpc_url: String,
    pub network: String,
    pub network_passphrase: String,
    /// Optional pre-funded relayer secret (`S…`, `OZKY_RELAYER_SECRET`). When set, the
    /// permissionless interior ops (`transfer`/`withdraw`) are submitted + fee-paid by
    /// the relayer, so the user holds no XLM and their account isn't linked as the
    /// fee-payer (build_plan A3 / FEATURE_SET G4). `deposit` stays user-sourced (it
    /// needs `from.require_auth` and is the already-public funding edge).
    pub relayer_secret: Option<String>,
}

fn env_or(key: &str, default: &str) -> String {
    cfg_var(key).unwrap_or_else(|| default.to_string())
}

/// `pool_id=7`, `network_id=42`, `asset_tag=1` — the values the frozen-VK round-trips
/// used. Overridable via `OZKY_POOL_ID` / `OZKY_NETWORK_ID` / `OZKY_ASSET_TAG`.
fn env_field(key: &str, default: u64) -> Result<Fr, CoreError> {
    match cfg_var(key) {
        Some(v) => v
            .parse::<u64>()
            .map(Fr::from_u64)
            .map_err(|_| CoreError::Chain(format!("{key} must be a u64: {v}"))),
        None => Ok(Fr::from_u64(default)),
    }
}

impl PoolConfig {
    /// Resolve from the environment. `OZKY_POOL_CONTRACT` is required (the deployed
    /// pool's id); the rest fall back to testnet defaults.
    pub fn load() -> Result<PoolConfig, CoreError> {
        let pool_contract = cfg_var("OZKY_POOL_CONTRACT").ok_or_else(|| {
            CoreError::Chain(
                "OZKY_POOL_CONTRACT not set (the deployed pool contract id to send against)".into(),
            )
        })?;
        let policy_contract = cfg_var("OZKY_POLICY_CONTRACT").ok_or_else(|| {
            CoreError::Chain(
                "OZKY_POLICY_CONTRACT not set (the deployed policy/ASP contract id)".into(),
            )
        })?;
        Ok(PoolConfig {
            pool_contract,
            policy_contract,
            viewkeys_contract: cfg_var("OZKY_VIEWKEYS_CONTRACT"),
            pool_id: env_field("OZKY_POOL_ID", 7)?,
            network_id: env_field("OZKY_NETWORK_ID", 42)?,
            asset_tag: env_field("OZKY_ASSET_TAG", 1)?,
            rpc_url: env_or("OZKY_RPC_URL", super::chain::DEFAULT_RPC_URL),
            network: env_or("OZKY_NETWORK", super::chain::DEFAULT_NETWORK),
            network_passphrase: env_or(
                "OZKY_NETWORK_PASSPHRASE",
                "Test SDF Network ; September 2015",
            ),
            relayer_secret: cfg_var("OZKY_RELAYER_SECRET"),
        })
    }

    /// Config for an AUDITOR (no wallet): only the disclosed `pool_contract` is needed
    /// to scan it. Network/asset default to testnet; policy/relayer unused for a
    /// read-only audit. `OZKY_POOL_CONTRACT` (if set) is overridden by `pool`.
    pub fn load_for_audit(pool: &str) -> Result<PoolConfig, CoreError> {
        Ok(PoolConfig {
            pool_contract: pool.to_string(),
            policy_contract: String::new(),
            viewkeys_contract: None,
            pool_id: env_field("OZKY_POOL_ID", 7)?,
            network_id: env_field("OZKY_NETWORK_ID", 42)?,
            asset_tag: env_field("OZKY_ASSET_TAG", 1)?,
            rpc_url: env_or("OZKY_RPC_URL", super::chain::DEFAULT_RPC_URL),
            network: env_or("OZKY_NETWORK", super::chain::DEFAULT_NETWORK),
            network_passphrase: env_or(
                "OZKY_NETWORK_PASSPHRASE",
                "Test SDF Network ; September 2015",
            ),
            relayer_secret: None,
        })
    }

    /// Clone this config targeting a different asset (by its v1 code, e.g. "USDC").
    /// Only `asset_tag` changes — the same pool holds every registered asset, keyed by
    /// the note's `asset_tag` (the pool's `register_asset` registry maps tag → SAC).
    pub fn with_asset(&self, code: &str) -> Result<PoolConfig, CoreError> {
        let info = asset_by_code(code).ok_or_else(|| {
            CoreError::Chain(format!("unknown asset '{code}' (known: XLM, USDC, USDT, EURC)"))
        })?;
        let mut cfg = self.clone();
        cfg.asset_tag = Fr::from_u64(info.tag);
        Ok(cfg)
    }

    /// The source secret to submit + fee-pay an interior op (`transfer`/`withdraw`):
    /// the relayer if configured (fee abstraction), else the wallet's own key.
    pub fn submit_source<'a>(&'a self, wallet_secret: &'a str) -> &'a str {
        self.relayer_secret.as_deref().unwrap_or(wallet_secret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_requires_pool_contract() {
        // With neither the env var NOR a config file providing it, load() errors. Point
        // OZKY_CONFIG at a nonexistent path so the dev machine's ozky.config.json (which
        // legitimately sets the contract) doesn't satisfy the requirement here.
        std::env::remove_var("OZKY_POOL_CONTRACT");
        std::env::set_var("OZKY_CONFIG", "/nonexistent/ozky.config.json");
        let r = PoolConfig::load();
        std::env::remove_var("OZKY_CONFIG");
        assert!(r.is_err());
    }

    #[test]
    fn field_defaults_match_roundtrip() {
        assert_eq!(env_field("OZKY_POOL_ID", 7).unwrap(), Fr::from_u64(7));
        assert_eq!(env_field("OZKY_NETWORK_ID", 42).unwrap(), Fr::from_u64(42));
        assert_eq!(Fr::from_u64(1).to_decimal(), "1");
    }

    fn base_cfg() -> PoolConfig {
        PoolConfig {
            pool_contract: "CPOOL".into(),
            policy_contract: "CPOL".into(),
            viewkeys_contract: None,
            pool_id: Fr::from_u64(7),
            network_id: Fr::from_u64(42),
            asset_tag: Fr::from_u64(1),
            rpc_url: "http://localhost".into(),
            network: "testnet".into(),
            network_passphrase: "Test SDF Network ; September 2015".into(),
            relayer_secret: None,
        }
    }

    #[test]
    fn with_asset_overrides_only_the_tag() {
        let cfg = base_cfg();
        // Case-insensitive lookup; only asset_tag changes (USDC == tag 2).
        let usdc = cfg.with_asset("usdc").unwrap();
        assert_eq!(usdc.asset_tag, Fr::from_u64(2));
        assert_eq!(usdc.pool_contract, cfg.pool_contract); // everything else preserved
        // EURC == tag 4; round-trips back to its code via asset_by_tag.
        let eurc = cfg.with_asset("EURC").unwrap();
        assert_eq!(eurc.asset_tag, Fr::from_u64(4));
        assert_eq!(asset_by_tag(&eurc.asset_tag).unwrap().code, "EURC");
        // Unknown asset is a clear error (not a silent default).
        assert!(cfg.with_asset("DOGE").is_err());
    }
}
