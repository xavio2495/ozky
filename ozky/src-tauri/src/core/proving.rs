//! Client-side proving (Phase A2). Builds witnesses for the deposit/transfer/
//! withdraw circuits against live chain state (commitment Merkle paths + nullifier
//! non-membership/insertion witnesses, sourced from the indexer or raw chain) and
//! produces Noir/UltraHonk proofs off the UI thread. This is the **stateful witness
//! generator** the ZK phases (Z4/Z7) deferred here.
//!
//! A0: interface skeleton.

use super::CoreError;

/// A proof + its public-input vector, ready to submit to the pool contract.
pub struct ProofBundle {
    pub proof: Vec<u8>,
    pub public_inputs: Vec<u8>,
}

/// Build a `transfer` proof spending the given owned notes. (A2)
pub fn prove_transfer(_recipient: &str, _amount: u64) -> Result<ProofBundle, CoreError> {
    Err(CoreError::not_implemented("proving::prove_transfer (A2)"))
}

/// Build a `deposit` proof binding `amount` to a fresh note. (A2)
pub fn prove_deposit(_amount: u64) -> Result<ProofBundle, CoreError> {
    Err(CoreError::not_implemented("proving::prove_deposit (A2)"))
}

/// Build a `withdraw` proof releasing `amount` to `dest`. (A2)
pub fn prove_withdraw(_dest: &str, _amount: u64) -> Result<ProofBundle, CoreError> {
    Err(CoreError::not_implemented("proving::prove_withdraw (A2)"))
}
