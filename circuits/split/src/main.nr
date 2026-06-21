//! `split` circuit - the multi-output private transfer (up to 7 recipients + change).
//! Thin public-interface wrapper over `notes::split::verify_split`. Public inputs follow
//! the canonical order, mirroring `circuits/transfer` but with 8 output commitments.
use notes::split::{N_OUTPUTS_SPLIT, verify_split};
use notes::transfer::{InputWitness, N_INPUTS, OutputWitness};

fn main(
    domain_sep: pub Field,
    asset_tag: pub Field,
    epoch: pub Field,
    commitment_root: pub Field,
    nullifier_old_root: pub Field,
    nullifier_new_root: pub Field,
    nullifiers: pub [Field; N_INPUTS],
    out_commitments: pub [Field; N_OUTPUTS_SPLIT],
    asp_root: pub Field,
    owner_sk: Field,
    inputs: [InputWitness; N_INPUTS],
    outputs: [OutputWitness; N_OUTPUTS_SPLIT],
) {
    verify_split(
        domain_sep,
        asset_tag,
        epoch,
        commitment_root,
        nullifier_old_root,
        nullifier_new_root,
        nullifiers,
        out_commitments,
        asp_root,
        owner_sk,
        inputs,
        outputs,
    );
}
