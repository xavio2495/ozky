//! Note scanning (Phase A2). View-tag trial match against the indexer's scan
//! stream; full decrypt only on a tag hit. Reconstructs the wallet's own notes
//! (and only those) from chain/indexer data. A0: interface skeleton.

use super::CoreError;

/// A discovered, decrypted note owned by this wallet.
pub struct OwnedNote {
    pub leaf_index: u32,
    pub value: u64,
    pub asset_tag: String,
}

/// Scan commitments from `from_leaf`, returning the wallet's owned notes. (A2)
pub fn scan(_from_leaf: u32) -> Result<Vec<OwnedNote>, CoreError> {
    Err(CoreError::not_implemented("scan::scan (A2)"))
}
