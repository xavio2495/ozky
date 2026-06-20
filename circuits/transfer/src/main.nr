//! `transfer` circuit - the private core (Z1 spec section 5). Thin public-interface wrapper
//! over `notes::transfer::verify_transfer`. Public inputs follow spec section 5 canonical
//! order. The end-to-end witness test lives in `notes::transfer`.
use notes::transfer::{InputWitness, N_INPUTS, N_OUTPUTS, OutputWitness, verify_transfer};

fn main(
    domain_sep: pub Field,
    asset_tag: pub Field,
    epoch: pub Field,
    commitment_root: pub Field,
    nullifier_old_root: pub Field,
    nullifier_new_root: pub Field,
    nullifiers: pub [Field; N_INPUTS],
    out_commitments: pub [Field; N_OUTPUTS],
    asp_root: pub Field,
    owner_sk: Field,
    inputs: [InputWitness; N_INPUTS],
    outputs: [OutputWitness; N_OUTPUTS],
) {
    verify_transfer(
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
