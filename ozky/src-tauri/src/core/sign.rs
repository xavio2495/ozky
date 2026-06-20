//! Transaction signing (Phase A3). The auto-applied transaction-binding /
//! spend-authorization signature over public tx data, under a USER-INITIATED send
//! (the wallet must never sign transfers autonomously). A0: interface skeleton.

use super::CoreError;

/// Sign a Stellar transaction envelope with the wallet's Ed25519 key. (A3)
pub fn sign_envelope(_envelope_xdr: &str) -> Result<String, CoreError> {
    Err(CoreError::not_implemented("sign::sign_envelope (A3)"))
}
