//! `withdraw` circuit - public-interface wrapper over `notes::withdraw::verify_withdraw`
//! (Z1 spec section 7). Public inputs in canonical order; `owner_sk`, `inputs`, `change` private.
use notes::transfer::{InputWitness, N_INPUTS, OutputWitness};
use notes::withdraw::verify_withdraw;

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
    amount: pub Field,
    dest_bind: pub Field,
    owner_sk: Field,
    inputs: [InputWitness; N_INPUTS],
    change: OutputWitness,
) {
    verify_withdraw(
        domain_sep,
        asset_tag,
        epoch,
        commitment_root,
        nullifier_old_root,
        nullifier_new_root,
        nullifiers,
        change_commitment,
        asp_root,
        amount,
        dest_bind,
        owner_sk,
        inputs,
        change,
    );
}
