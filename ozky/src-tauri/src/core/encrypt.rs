//! Note payload encryption (Phase A2). Ephemeral-static ECDH -> HKDF ->
//! key-committing AEAD (never plain AES-GCM) over the serialized note, published
//! on-chain alongside the commitment for trustless receive. A0: interface skeleton.

use super::CoreError;

/// Encrypt a serialized note to a recipient's transmission key, returning the
/// on-chain payload (ephemeral_pub, view_tag, ciphertext). (A2)
pub fn encrypt_note(_note: &[u8], _recipient_transmission_pub: &[u8]) -> Result<Vec<u8>, CoreError> {
    Err(CoreError::not_implemented("encrypt::encrypt_note (A2)"))
}

/// Decrypt an on-chain payload with the wallet's viewing key. (A2)
pub fn decrypt_note(_payload: &[u8]) -> Result<Vec<u8>, CoreError> {
    Err(CoreError::not_implemented("encrypt::decrypt_note (A2)"))
}
