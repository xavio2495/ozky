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
/// split = transfer with 6 outputs: 6 base + 2 nullifiers + 6 out_cm + asp_root.
pub const SPLIT_N: u32 = 15;
/// escrow contribute = withdraw-shaped spend (10 base through asp_root) + running-commitment
/// fold (c_raised_old, c_raised_new, c_contrib) + refund_bind.
pub const ESCROW_CONTRIBUTE_N: u32 = 14;
/// escrow payout (release/refund) = domain_sep, asset_tag, epoch, commitment_hash, floor,
/// out_commitment, recipient_bind.
pub const ESCROW_PAYOUT_N: u32 = 7;
/// channel close (building block B phase 2) = domain_sep, asset_tag, epoch, cap_hash, auth_key,
/// valid_after_ledger, merchant_out, subscriber_out, merchant_bind, subscriber_bind.
pub const CHANNEL_CLOSE_N: u32 = 10;
/// shielded swap (roadmap 2.5 Phase 2) = domain_sep, asset_a_tag, asset_b_tag, epoch,
/// commitment_root, nullifier_old_root, nullifier_new_root, nf0, nf1, change_commitment,
/// out_commitment_b, asp_root, value_a, value_b.
pub const SWAP_N: u32 = 14;
/// 4-input multi-note transfer (next-build scope #1) = transfer with 4 nullifiers: domain_sep,
/// asset_tag, epoch, commitment_root, nullifier_old_root, nullifier_new_root, nf0..nf3,
/// out_cm0, out_cm1, asp_root (13 fields).
pub const TRANSFER4_N: u32 = 13;

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
