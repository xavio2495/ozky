//! `transfer4` circuit - the 4-input multi-note transfer (next-build scope #1; spec
//! claude-docs/multi_input_transfer_interface.md). Thin public-interface wrapper over
//! `notes::transfer4::verify_transfer4`. Public inputs in canonical order (13 fields). The
//! end-to-end witness tests live in `notes::transfer4`.
use notes::transfer::{InputWitness, OutputWitness};
use notes::transfer4::{N_INPUTS_4, N_OUTPUTS, verify_transfer4};

fn main(
    domain_sep: pub Field,
    asset_tag: pub Field,
    epoch: pub Field,
    commitment_root: pub Field,
    nullifier_old_root: pub Field,
    nullifier_new_root: pub Field,
    nullifiers: pub [Field; N_INPUTS_4],
    out_commitments: pub [Field; N_OUTPUTS],
    asp_root: pub Field,
    owner_sk: Field,
    inputs: [InputWitness; N_INPUTS_4],
    outputs: [OutputWitness; N_OUTPUTS],
) {
    verify_transfer4(
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
