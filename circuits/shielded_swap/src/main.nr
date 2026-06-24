//! `shielded_swap` circuit - public-interface wrapper over `notes::swap::verify_swap`
//! (roadmap 2.5 Phase 2). In-pool constant-product AMM swap: spend an A-note, mint a B-note
//! bound to the public `value_b`, re-shield the A remainder. Public inputs in canonical order;
//! `owner_sk`, `inputs`, `change`, `out_b` are private.
use notes::swap::verify_swap;
use notes::transfer::{InputWitness, N_INPUTS, OutputWitness};

fn main(
    domain_sep: pub Field,
    asset_a_tag: pub Field,
    asset_b_tag: pub Field,
    epoch: pub Field,
    commitment_root: pub Field,
    nullifier_old_root: pub Field,
    nullifier_new_root: pub Field,
    nullifiers: pub [Field; N_INPUTS],
    change_commitment: pub Field,
    out_commitment_b: pub Field,
    asp_root: pub Field,
    value_a: pub Field,
    value_b: pub Field,
    owner_sk: Field,
    inputs: [InputWitness; N_INPUTS],
    change: OutputWitness,
    out_b: OutputWitness,
) {
    verify_swap(
        domain_sep,
        asset_a_tag,
        asset_b_tag,
        epoch,
        commitment_root,
        nullifier_old_root,
        nullifier_new_root,
        nullifiers,
        change_commitment,
        out_commitment_b,
        asp_root,
        value_a,
        value_b,
        owner_sk,
        inputs,
        change,
        out_b,
    );
}
