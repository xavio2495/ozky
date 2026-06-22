//! `escrow_contribute` circuit - public-interface wrapper over
//! `notes::escrow::verify_escrow_contribute`. Withdraw-shaped spend that folds a HIDDEN amount
//! into a running Pedersen commitment. Public inputs in canonical order (14 fields); the
//! contract parses them as ESCROW_CONTRIBUTE_N. `owner_sk`, the spent inputs, change note,
//! amount, blinding, salt, and the prior running point are private.
use notes::escrow::verify_escrow_contribute;
use notes::transfer::{InputWitness, N_INPUTS, OutputWitness};
use std::embedded_curve_ops::EmbeddedCurvePoint;

fn main(
    domain_sep: pub Field,
    asset_tag: pub Field,
    epoch: pub Field,
    commitment_root: pub Field,
    nullifier_old_root: pub Field,
    nullifier_new_root: pub Field,
    nullifiers: pub [Field; N_INPUTS],
    change_commitment: pub Field,
    asp_root: pub Field,
    c_raised_old: pub Field,
    c_raised_new: pub Field,
    c_contrib: pub Field,
    refund_bind: pub Field,
    owner_sk: Field,
    inputs: [InputWitness; N_INPUTS],
    change: OutputWitness,
    amount: Field,
    blinding_r: Field,
    contrib_salt: Field,
    p_old: EmbeddedCurvePoint,
) {
    verify_escrow_contribute(
        domain_sep,
        asset_tag,
        epoch,
        commitment_root,
        nullifier_old_root,
        nullifier_new_root,
        nullifiers,
        change_commitment,
        asp_root,
        c_raised_old,
        c_raised_new,
        c_contrib,
        refund_bind,
        owner_sk,
        inputs,
        change,
        amount,
        blinding_r,
        contrib_salt,
        p_old,
    );
}
