//! `withdraw` circuit logic (Z1 spec section 7). Spends shielded note(s) and releases a
//! public `amount` of the real asset to `dest`, with a shielded change note. Shares
//! the input-spending machinery with `transfer` via `spend_inputs`.
use crate::derive_owner_pk;
use crate::transfer::{
    build_two_insertions, InputWitness, make_input, N_INPUTS, OutputWitness, single_leaf_tree,
    spend_inputs, zero_ladder,
};
use crate::{nullifier, Note};

pub fn verify_withdraw(
    domain_sep: Field,
    asset_tag: Field,
    epoch: Field,
    commitment_root: Field,
    nullifier_old_root: Field,
    nullifier_new_root: Field,
    nullifiers: [Field; N_INPUTS],
    change_commitment: Field,
    asp_root: Field,
    amount: Field,
    dest_bind: Field,
    owner_sk: Field,
    inputs: [InputWitness; N_INPUTS],
    change: OutputWitness,
) {
    assert(domain_sep != 0);
    // dest_bind is a public input -> the proof is bound to the destination; the
    // contract recomputes it from the real `dest` (Z4). Non-zero guards it in-circuit.
    assert(dest_bind != 0);
    epoch.assert_max_bit_size::<32>();
    amount.assert_max_bit_size::<64>();
    let owner_pk = derive_owner_pk(owner_sk);

    let in_sum = spend_inputs(
        asset_tag,
        commitment_root,
        asp_root,
        owner_pk,
        owner_sk,
        nullifiers,
        nullifier_old_root,
        nullifier_new_root,
        inputs,
    );

    assert(change.commitment(asset_tag, epoch) == change_commitment);
    change.value.assert_max_bit_size::<64>();

    // conservation: spent inputs == public amount released + shielded change
    assert(in_sum == amount + change.value);
}

pub struct WithdrawWitness {
    pub domain_sep: Field,
    pub asset_tag: Field,
    pub epoch: Field,
    pub commitment_root: Field,
    pub nullifier_old_root: Field,
    pub nullifier_new_root: Field,
    pub nullifiers: [Field; N_INPUTS],
    pub change_commitment: Field,
    pub asp_root: Field,
    pub amount: Field,
    pub dest_bind: Field,
    pub owner_sk: Field,
    pub inputs: [InputWitness; N_INPUTS],
    pub change: OutputWitness,
}

impl WithdrawWitness {
    pub fn run(self) {
        verify_withdraw(
            self.domain_sep,
            self.asset_tag,
            self.epoch,
            self.commitment_root,
            self.nullifier_old_root,
            self.nullifier_new_root,
            self.nullifiers,
            self.change_commitment,
            self.asp_root,
            self.amount,
            self.dest_bind,
            self.owner_sk,
            self.inputs,
            self.change,
        );
    }
}

/// Deterministic 1-real + 1-dummy withdraw: spend 1000, release 700, keep 300 change.
/// Parameterized by `epoch` and `domain_sep` so it binds to a live pool; spends the
/// same note `deposit::demo_witness_at` mints (so deposit->withdraw composes on one pool).
pub fn demo_witness_at(epoch: Field, domain_sep: Field) -> WithdrawWitness {
    let z = zero_ladder();
    let asset_tag = 1;
    let owner_sk = 12345;
    let owner_pk = derive_owner_pk(owner_sk);

    let in_note = Note { value: 1000, asset_tag, owner_pk, blinding: 777, epoch, rho: 111 };
    let (commitment_root, cm_ir, cm_sib) = single_leaf_tree(in_note.commitment(), z);
    let (asp_root, asp_ir, asp_sib) = single_leaf_tree(owner_pk, z);

    let change = OutputWitness { value: 300, owner_pk, blinding: 444, rho: 555 };

    let dummy_rho = 0xdead;
    let nf0 = nullifier(111, owner_sk);
    let nf1 = nullifier(dummy_rho, owner_sk);
    let (nf_old, nf_new, w0, w1) = build_two_insertions(nf0, nf1);

    let real_in =
        make_input(1000, 777, epoch, 111, false, cm_ir, cm_sib, asp_ir, asp_sib, w0);
    let dummy_in = make_input(
        0,
        0,
        epoch,
        dummy_rho,
        true,
        [false; crate::merkle::TREE_DEPTH],
        [0; crate::merkle::TREE_DEPTH],
        [false; crate::merkle::TREE_DEPTH],
        [0; crate::merkle::TREE_DEPTH],
        w1,
    );

    // dest_bind = Poseidon(DOMAIN_DEST, dest) - placeholder dest for the demo.
    let dest_bind = poseidon::poseidon2::Poseidon2::hash([0xde57, 0xbeef], 2);

    WithdrawWitness {
        domain_sep,
        asset_tag,
        epoch,
        commitment_root,
        nullifier_old_root: nf_old,
        nullifier_new_root: nf_new,
        nullifiers: [nf0, nf1],
        change_commitment: change.commitment(asset_tag, epoch),
        asp_root,
        amount: 700,
        dest_bind,
        owner_sk,
        inputs: [real_in, dummy_in],
        change,
    }
}

/// Deterministic withdraw demo (epoch 5, demo domain_sep).
pub fn demo_witness() -> WithdrawWitness {
    demo_witness_at(5, 0xabc)
}

#[test]
fn withdraw_end_to_end() {
    demo_witness().run();
}

#[test(should_fail)]
fn withdraw_rejects_value_mismatch() {
    let mut w = demo_witness();
    w.amount = 701; // 1000 != 701 + 300
    w.run();
}
