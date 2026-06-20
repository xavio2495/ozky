//! Public-input parsing. The verifier consumes the raw `public_inputs` byte blob
//! (concatenated 32-byte big-endian BN254 field elements, in the circuit's
//! canonical `pub` order). The pool parses the same blob into typed fields so it
//! can validate their semantics against its own state, then forwards the identical
//! bytes to the verifier (no re-serialization — the bytes checked are the bytes
//! verified).

use crate::Error;
use soroban_sdk::{Bytes, Env, Vec, U256};

pub const DEPOSIT_N: u32 = 5;
pub const TRANSFER_N: u32 = 11;
pub const WITHDRAW_N: u32 = 12;

/// Parse exactly `n` field elements (n*32 bytes, big-endian) from `pi`.
pub fn read_fields(env: &Env, pi: &Bytes, n: u32) -> Result<Vec<U256>, Error> {
    if pi.len() != n * 32 {
        return Err(Error::BadPublicInputs);
    }
    let mut out = Vec::new(env);
    for i in 0..n {
        let mut buf = [0u8; 32];
        for j in 0..32u32 {
            buf[j as usize] = pi.get(i * 32 + j).unwrap();
        }
        out.push_back(U256::from_be_bytes(env, &Bytes::from_array(env, &buf)));
    }
    Ok(out)
}
