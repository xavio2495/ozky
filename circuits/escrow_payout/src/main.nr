//! `escrow_payout` circuit - public-interface wrapper over `notes::escrow::verify_escrow_payout`.
//! Opens a Pedersen commitment to a hidden value V, asserts V >= floor, and mints a note of V to
//! a recipient-bound key. Shared by escrow RELEASE (open the running commitment, floor=target)
//! and REFUND (open a contribution, floor=0). Public inputs in canonical order (7 fields), parsed
//! by the contract as ESCROW_PAYOUT_N. All openings are private.
use notes::escrow::verify_escrow_payout;

fn main(
    domain_sep: pub Field,
    asset_tag: pub Field,
    epoch: pub Field,
    commitment_hash: pub Field,
    floor: pub Field,
    out_commitment: pub Field,
    recipient_bind: pub Field,
    domain_bind: Field,
    recipient_sk: Field,
    value: Field,
    blinding_r: Field,
    out_blinding: Field,
    out_rho: Field,
    salt: Field,
) {
    verify_escrow_payout(
        domain_sep,
        asset_tag,
        epoch,
        commitment_hash,
        floor,
        out_commitment,
        recipient_bind,
        domain_bind,
        recipient_sk,
        value,
        blinding_r,
        out_blinding,
        out_rho,
        salt,
    );
}
