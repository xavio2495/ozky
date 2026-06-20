//! Chain client (Phase A2/A3). Talks to Stellar RPC and the ozky indexer: submit
//! transactions, read pool roots, fetch scan stream / Merkle paths / non-membership
//! witnesses. Recovery path must also work from raw RPC with the indexer offline
//! (the indexer is never a correctness dependency). A0: interface skeleton.

use super::CoreError;

/// The target network. Testnet throughout Part 1/2; mainnet only after audit.
pub const DEFAULT_NETWORK: &str = "testnet";
pub const DEFAULT_RPC_URL: &str = "https://soroban-testnet.stellar.org";

/// Submit a signed transaction envelope, returning its hash. (A3)
pub fn submit(_envelope_xdr: &str) -> Result<String, CoreError> {
    Err(CoreError::not_implemented("chain::submit (A3)"))
}

/// Fetch the pool's current commitment + nullifier roots. (A2)
pub fn pool_roots(_pool_id: &str) -> Result<(String, String), CoreError> {
    Err(CoreError::not_implemented("chain::pool_roots (A2)"))
}
