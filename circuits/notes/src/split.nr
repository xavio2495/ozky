//! `split` circuit logic - a transfer with a FIXED 8 outputs (up to 7 recipients + 1
//! change). Reuses the transfer input-spending machinery (`spend_inputs`, `InputWitness`,
//! `OutputWitness`) verbatim; only the output count differs. Unused outputs are dummy-
//! padded (value 0) so every split looks identical on-chain regardless of recipient count.
use crate::merkle::TREE_DEPTH;
use crate::transfer::{
    build_two_insertions, InputWitness, make_input, OutputWitness, single_leaf_tree, spend_inputs,
    zero_ladder, N_INPUTS,
};
use crate::{derive_owner_pk, nullifier, Note};

pub global N_OUTPUTS_SPLIT: u32 = 6;

/// Core split constraints - identical to `verify_transfer` but with `N_OUTPUTS_SPLIT`
/// outputs. Conservation `sum(inputs) == sum(outputs)` holds across all 8 outputs (the
/// dummy/padding outputs carry value 0).
pub fn verify_split(
    domain_sep: Field,
    asset_tag: Field,
    epoch: Field,
    commitment_root: Field,
    nullifier_old_root: Field,
    nullifier_new_root: Field,
    nullifiers: [Field; N_INPUTS],
    out_commitments: [Field; N_OUTPUTS_SPLIT],
    asp_root: Field,
    owner_sk: Field,
    inputs: [InputWitness; N_INPUTS],
    outputs: [OutputWitness; N_OUTPUTS_SPLIT],
) {
    assert(domain_sep != 0);
    epoch.assert_max_bit_size::<32>();
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

    let mut out_sum: Field = 0;
    for j in 0..N_OUTPUTS_SPLIT {
        assert(outputs[j].commitment(asset_tag, epoch) == out_commitments[j]);
        outputs[j].value.assert_max_bit_size::<64>();
        out_sum += outputs[j].value;
    }
    assert(in_sum == out_sum);
}

/// Public inputs + private witness for a complete split.
pub struct SplitWitness {
    pub domain_sep: Field,
    pub asset_tag: Field,
    pub epoch: Field,
    pub commitment_root: Field,
    pub nullifier_old_root: Field,
    pub nullifier_new_root: Field,
    pub nullifiers: [Field; N_INPUTS],
    pub out_commitments: [Field; N_OUTPUTS_SPLIT],
    pub asp_root: Field,
    pub owner_sk: Field,
    pub inputs: [InputWitness; N_INPUTS],
    pub outputs: [OutputWitness; N_OUTPUTS_SPLIT],
}

impl SplitWitness {
    pub fn run(self) {
        verify_split(
            self.domain_sep,
            self.asset_tag,
            self.epoch,
            self.commitment_root,
            self.nullifier_old_root,
            self.nullifier_new_root,
            self.nullifiers,
            self.out_commitments,
            self.asp_root,
            self.owner_sk,
            self.inputs,
            self.outputs,
        );
    }
}

/// Deterministic split demo: spend 1000 (1 real + 1 dummy input) into 3 recipients
/// (250 each) + change (250) + 2 dummy outputs (0). Fully valid by construction.
pub fn demo_witness_at(epoch: Field, domain_sep: Field) -> SplitWitness {
    let z = zero_ladder();
    let asset_tag = 1;
    let owner_sk = 12345;
    let owner_pk = derive_owner_pk(owner_sk);

    let in_note = Note { value: 1000, asset_tag, owner_pk, blinding: 777, epoch, rho: 111 };
    let (commitment_root, cm_ir, cm_sib) = single_leaf_tree(in_note.commitment(), z);
    let (asp_root, asp_ir, asp_sib) = single_leaf_tree(owner_pk, z);

    // 3 recipients + change + 4 dummies (value 0). owner_pk on dummies = sender's.
    let r0 = OutputWitness { value: 250, owner_pk: derive_owner_pk(91), blinding: 11, rho: 21 };
    let r1 = OutputWitness { value: 250, owner_pk: derive_owner_pk(92), blinding: 12, rho: 22 };
    let r2 = OutputWitness { value: 250, owner_pk: derive_owner_pk(93), blinding: 13, rho: 23 };
    let change = OutputWitness { value: 250, owner_pk, blinding: 14, rho: 24 };
    let d = |rho: Field| OutputWitness { value: 0, owner_pk, blinding: 0, rho };
    let outputs = [r0, r1, r2, change, d(31), d(32)];

    let dummy_rho = 0xdead;
    let nf0 = nullifier(111, owner_sk);
    let nf1 = nullifier(dummy_rho, owner_sk);
    let (nf_old, nf_new, w0, w1) = build_two_insertions(nf0, nf1);

    let real_in = make_input(1000, 777, epoch, 111, false, cm_ir, cm_sib, asp_ir, asp_sib, w0);
    let dummy_in = make_input(
        0, 0, epoch, dummy_rho, true,
        [false; TREE_DEPTH], [0; TREE_DEPTH], [false; TREE_DEPTH], [0; TREE_DEPTH], w1,
    );

    let mut out_commitments = [0; N_OUTPUTS_SPLIT];
    for j in 0..N_OUTPUTS_SPLIT {
        out_commitments[j] = outputs[j].commitment(asset_tag, epoch);
    }

    SplitWitness {
        domain_sep,
        asset_tag,
        epoch,
        commitment_root,
        nullifier_old_root: nf_old,
        nullifier_new_root: nf_new,
        nullifiers: [nf0, nf1],
        out_commitments,
        asp_root,
        owner_sk,
        inputs: [real_in, dummy_in],
        outputs,
    }
}

pub fn demo_witness() -> SplitWitness {
    demo_witness_at(5, 0xabc)
}

#[test]
fn split_end_to_end() {
    demo_witness().run();
}

#[test(should_fail)]
fn split_rejects_broken_conservation() {
    let mut w = demo_witness();
    // Inflate one output's value without matching inputs -> conservation must fail.
    w.outputs[0].value = 999;
    w.out_commitments[0] = w.outputs[0].commitment(w.asset_tag, w.epoch);
    w.run();
}
