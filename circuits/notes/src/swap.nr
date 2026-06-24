//! `shielded_swap` circuit logic (roadmap 2.5 Phase 2; spec claude-docs/swap_pool_interface.md).
//! In-pool constant-product AMM swap: spend one A-note, mint a B-note bound to the public
//! `value_b`, and re-shield the A remainder as a change A-note. Conservation is enforced ONLY
//! over A -- B's value is set by the contract's AMM math and bound here via `value_b`.
//!
//! Shares the input-spending machinery (`spend_inputs`) with `transfer`/`withdraw`. The contract
//! prices `value_b` against its public reserves and updates them; this circuit only proves a valid
//! A-note was spent, the A remainder is correctly re-shielded, and the minted B-note has value
//! exactly `value_b`.
use crate::derive_owner_pk;
use crate::transfer::{
    build_two_insertions, InputWitness, make_input, N_INPUTS, OutputWitness, single_leaf_tree,
    spend_inputs, zero_ladder,
};
use crate::{nullifier, Note};

pub fn verify_swap(
    domain_sep: Field,
    asset_a_tag: Field,
    asset_b_tag: Field,
    epoch: Field,
    commitment_root: Field,
    nullifier_old_root: Field,
    nullifier_new_root: Field,
    nullifiers: [Field; N_INPUTS],
    change_commitment: Field,
    out_commitment_b: Field,
    asp_root: Field,
    value_a: Field,
    value_b: Field,
    owner_sk: Field,
    inputs: [InputWitness; N_INPUTS],
    change: OutputWitness,
    out_b: OutputWitness,
) {
    assert(domain_sep != 0);
    // A swap must cross assets; equal tags would let it masquerade as a same-asset transfer.
    assert(asset_a_tag != asset_b_tag);
    epoch.assert_max_bit_size::<32>();
    value_a.assert_max_bit_size::<64>();
    value_b.assert_max_bit_size::<64>();
    let owner_pk = derive_owner_pk(owner_sk);

    // Spend the A-note(s): commitment + ASP membership, nullifier insert (old -> new root).
    let in_sum = spend_inputs(
        asset_a_tag,
        commitment_root,
        asp_root,
        owner_pk,
        owner_sk,
        nullifiers,
        nullifier_old_root,
        nullifier_new_root,
        inputs,
    );

    // Change A-note (the A remainder, re-shielded).
    assert(change.commitment(asset_a_tag, epoch) == change_commitment);
    change.value.assert_max_bit_size::<64>();

    // Minted B-note: its value is bound to the public `value_b` the contract debits from reserve_B.
    assert(out_b.value == value_b);
    assert(out_b.commitment(asset_b_tag, epoch) == out_commitment_b);

    // Conservation of A: spent A == swapped value_a (-> reserve) + re-shielded A change.
    assert(in_sum == value_a + change.value);
}

pub struct SwapWitness {
    pub domain_sep: Field,
    pub asset_a_tag: Field,
    pub asset_b_tag: Field,
    pub epoch: Field,
    pub commitment_root: Field,
    pub nullifier_old_root: Field,
    pub nullifier_new_root: Field,
    pub nullifiers: [Field; N_INPUTS],
    pub change_commitment: Field,
    pub out_commitment_b: Field,
    pub asp_root: Field,
    pub value_a: Field,
    pub value_b: Field,
    pub owner_sk: Field,
    pub inputs: [InputWitness; N_INPUTS],
    pub change: OutputWitness,
    pub out_b: OutputWitness,
}

impl SwapWitness {
    pub fn run(self) {
        verify_swap(
            self.domain_sep,
            self.asset_a_tag,
            self.asset_b_tag,
            self.epoch,
            self.commitment_root,
            self.nullifier_old_root,
            self.nullifier_new_root,
            self.nullifiers,
            self.change_commitment,
            self.out_commitment_b,
            self.asp_root,
            self.value_a,
            self.value_b,
            self.owner_sk,
            self.inputs,
            self.change,
            self.out_b,
        );
    }
}

/// Deterministic 1-real + 1-dummy swap: spend a 1000 A-note (asset_tag 1), swap 800 of it into
/// asset_tag 2, keep 200 A change. value_b = 750 (a representative AMM output; the demo doesn't
/// model reserves). Parameterized by `epoch`/`domain_sep` so it binds to a live pool; spends the
/// same note `deposit::demo_witness_at` mints.
pub fn demo_witness_at(epoch: Field, domain_sep: Field) -> SwapWitness {
    let z = zero_ladder();
    let asset_a_tag = 1;
    let asset_b_tag = 2;
    let owner_sk = 12345;
    let owner_pk = derive_owner_pk(owner_sk);

    let in_note = Note { value: 1000, asset_tag: asset_a_tag, owner_pk, blinding: 777, epoch, rho: 111 };
    let (commitment_root, cm_ir, cm_sib) = single_leaf_tree(in_note.commitment(), z);
    let (asp_root, asp_ir, asp_sib) = single_leaf_tree(owner_pk, z);

    let change = OutputWitness { value: 200, owner_pk, blinding: 444, rho: 555 };
    let out_b = OutputWitness { value: 750, owner_pk, blinding: 888, rho: 999 };

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

    SwapWitness {
        domain_sep,
        asset_a_tag,
        asset_b_tag,
        epoch,
        commitment_root,
        nullifier_old_root: nf_old,
        nullifier_new_root: nf_new,
        nullifiers: [nf0, nf1],
        change_commitment: change.commitment(asset_a_tag, epoch),
        out_commitment_b: out_b.commitment(asset_b_tag, epoch),
        asp_root,
        value_a: 800,
        value_b: 750,
        owner_sk,
        inputs: [real_in, dummy_in],
        change,
        out_b,
    }
}

/// Deterministic swap demo (epoch 5, demo domain_sep).
pub fn demo_witness() -> SwapWitness {
    demo_witness_at(5, 0xabc)
}

#[test]
fn swap_end_to_end() {
    demo_witness().run();
}

#[test(should_fail)]
fn swap_rejects_a_conservation_mismatch() {
    let mut w = demo_witness();
    w.value_a = 801; // 1000 != 801 + 200
    w.run();
}

#[test(should_fail)]
fn swap_rejects_value_b_not_matching_note() {
    let mut w = demo_witness();
    w.value_b = 751; // out_b.value (750) != value_b
    w.run();
}

#[test(should_fail)]
fn swap_rejects_same_asset() {
    let mut w = demo_witness();
    w.asset_b_tag = 1; // asset_a_tag == asset_b_tag
    w.run();
}
