//! `transfer4` circuit logic - the 4-input multi-note transfer (next-build scope #1,
//! spec claude-docs/multi_input_transfer_interface.md). Spends up to 4 owned notes in one
//! operation (the rest are dummies), so a payment larger than any single note works and a
//! fragmented balance can be consolidated.
//!
//! `transfer`/`withdraw`/`split`/`shielded_swap` are FROZEN and reuse `transfer::spend_inputs`
//! (N_INPUTS = 2). To avoid any blast radius on those VKs this module keeps its own 4-input copy
//! of the spend loop (`spend_inputs_4`) and reuses only the arity-independent gadgets + the
//! `InputWitness`/`OutputWitness` structs.
use crate::accumulator::{IndexedLeaf, insert};
use crate::merkle::{assert_membership, compute_root, hash_node, TREE_DEPTH};
use crate::transfer::{
    build_path4, InputWitness, make_input, NfInsert, OutputWitness, root4, single_leaf_tree,
    zero_ladder,
};
use crate::{derive_owner_pk, nullifier, Note};

pub global N_INPUTS_4: u32 = 4;
pub global N_OUTPUTS: u32 = 2;

/// Spend 4 inputs (real or dummy): per input verify commitment + ASP membership (real only),
/// match & insert its nullifier into the accumulator, accumulate its value. Returns the input
/// value sum. The 4-input twin of `transfer::spend_inputs`.
pub fn spend_inputs_4(
    asset_tag: Field,
    commitment_root: Field,
    asp_root: Field,
    owner_pk: Field,
    owner_sk: Field,
    nullifiers: [Field; N_INPUTS_4],
    nullifier_old_root: Field,
    nullifier_new_root: Field,
    inputs: [InputWitness; N_INPUTS_4],
) -> Field {
    let mut in_sum: Field = 0;
    let mut nf_root = nullifier_old_root;
    for i in 0..N_INPUTS_4 {
        let w = inputs[i];
        let note =
            Note { value: w.value, asset_tag, owner_pk, blinding: w.blinding, epoch: w.epoch, rho: w.rho };

        if !w.is_dummy {
            assert_membership(note.commitment(), w.cm_is_right, w.cm_siblings, commitment_root);
            assert_membership(owner_pk, w.asp_is_right, w.asp_siblings, asp_root);
        }

        let nf = nullifier(w.rho, owner_sk);
        assert(nf == nullifiers[i]);
        nf_root = insert(
            nf,
            w.nf_low,
            w.nf_low_is_right,
            w.nf_low_siblings,
            nf_root,
            w.nf_new_index,
            w.nf_new_is_right,
            w.nf_new_siblings,
        );

        w.value.assert_max_bit_size::<64>();
        in_sum += if w.is_dummy { 0 } else { w.value };
    }
    assert(nf_root == nullifier_new_root);
    in_sum
}

/// Core 4-input transfer constraints (the 4-input twin of `transfer::verify_transfer`).
pub fn verify_transfer4(
    domain_sep: Field,
    asset_tag: Field,
    epoch: Field,
    commitment_root: Field,
    nullifier_old_root: Field,
    nullifier_new_root: Field,
    nullifiers: [Field; N_INPUTS_4],
    out_commitments: [Field; N_OUTPUTS],
    asp_root: Field,
    owner_sk: Field,
    inputs: [InputWitness; N_INPUTS_4],
    outputs: [OutputWitness; N_OUTPUTS],
) {
    assert(domain_sep != 0);
    epoch.assert_max_bit_size::<32>();
    let owner_pk = derive_owner_pk(owner_sk);

    let in_sum = spend_inputs_4(
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
    for j in 0..N_OUTPUTS {
        assert(outputs[j].commitment(asset_tag, epoch) == out_commitments[j]);
        outputs[j].value.assert_max_bit_size::<64>();
        out_sum += outputs[j].value;
    }
    assert(in_sum == out_sum);
}

// ----------------------------- 4-insertion accumulator witness (tests) -----------------------------
// Mirrors the Rust IndexedTree (witness.rs): build the 4 chained nullifier insertions into a fresh
// accumulator (canonical init leaf {0,0,0} at slot 0). Used only by the in-circuit demos; the
// production witness is built in the Rust core.

/// Leaf hashes of the first `count` accumulator leaves (rest = empty leaf 0), in a width-8 buffer.
fn hashes_of(leaves: [IndexedLeaf; 5], count: u32) -> [Field; 8] {
    let mut h: [Field; 8] = [0; 8];
    for i in 0..5 {
        if i < count {
            h[i] = leaves[i].hash();
        }
    }
    h
}

/// Depth-20 authentication path for `index` over the first `count` leaf hashes (the rest pad with
/// the zero ladder), plus the recomputed root. Generalizes `build_path4` to a growing tree.
fn path_over(
    hashes: [Field; 8],
    count: u32,
    index: u32,
    z: [Field; TREE_DEPTH],
) -> ([bool; TREE_DEPTH], [Field; TREE_DEPTH], Field) {
    let mut cur = hashes;
    let mut cur_count = count;
    let mut pos = index;
    let mut is_right = [false; TREE_DEPTH];
    let mut sib: [Field; TREE_DEPTH] = [0; TREE_DEPTH];
    for level in 0..TREE_DEPTH {
        let si = pos ^ 1;
        sib[level] = if si < cur_count { cur[si] } else { z[level] };
        is_right[level] = (pos & 1) == 1;
        let mut next: [Field; 8] = [0; 8];
        for i in 0..4 {
            let l = cur[2 * i];
            let r = if (2 * i + 1) < cur_count { cur[2 * i + 1] } else { z[level] };
            next[i] = hash_node(l, r);
        }
        cur = next;
        cur_count = (cur_count + 1) / 2;
        pos = pos >> 1;
    }
    (is_right, sib, cur[0])
}

/// Index of the low leaf bracketing `target` among the first `count` leaves.
fn low_index(leaves: [IndexedLeaf; 5], count: u32, target: Field) -> u32 {
    let mut lo: u32 = 0;
    for i in 0..5 {
        if i < count {
            let l = leaves[i];
            let above = l.value.lt(target);
            let is_tail = l.next_value == 0;
            let below = (!is_tail) & target.lt(l.next_value);
            if above & (is_tail | below) {
                lo = i;
            }
        }
    }
    lo
}

/// Build the chained 4-insertion witness (`nfs[0..4]` in order) into a fresh accumulator.
/// Returns old root, new root, and the four insertion witnesses. Self-checking: each step calls
/// the `insert` gadget, so an inconsistent path trips its internal asserts.
pub fn build_four_insertions(nfs: [Field; 4]) -> (Field, Field, [NfInsert; 4]) {
    let z = zero_ladder();
    let init = IndexedLeaf { value: 0, next_index: 0, next_value: 0 };
    let mut leaves: [IndexedLeaf; 5] = [init, init, init, init, init];
    let mut count: u32 = 1;

    let (_, _, old_root) = path_over(hashes_of(leaves, count), count, 0, z);

    let dflt = NfInsert {
        nf_low: init,
        low_is_right: [false; TREE_DEPTH],
        low_siblings: [0; TREE_DEPTH],
        new_is_right: [false; TREE_DEPTH],
        new_siblings: [0; TREE_DEPTH],
        new_index: 0,
    };
    let mut out: [NfInsert; 4] = [dflt; 4];

    let mut cur_root = old_root;
    for k in 0..4 {
        let target = nfs[k];
        let lo = low_index(leaves, count, target);
        let low_before = leaves[lo];
        let new_index = count;

        // Low-leaf non-membership path over the current tree.
        let (lo_ir, lo_sib, _r) = path_over(hashes_of(leaves, count), count, lo, z);

        // Repoint the low leaf at the new slot; the new slot itself is still empty (0).
        leaves[lo] = IndexedLeaf { value: low_before.value, next_index: new_index as Field, next_value: target };
        let (new_ir, new_sib, _r2) = path_over(hashes_of(leaves, count), count + 1, new_index, z);

        cur_root = insert(target, low_before, lo_ir, lo_sib, cur_root, new_index as Field, new_ir, new_sib);

        // Write the new leaf (inherits the low leaf's previous successor).
        leaves[new_index] = IndexedLeaf {
            value: target,
            next_index: low_before.next_index,
            next_value: low_before.next_value,
        };
        count += 1;

        out[k] = NfInsert {
            nf_low: low_before,
            low_is_right: lo_ir,
            low_siblings: lo_sib,
            new_is_right: new_ir,
            new_siblings: new_sib,
            new_index: new_index as Field,
        };
    }
    (old_root, cur_root, out)
}

// ----------------------------- witness bundle + demos -----------------------------

pub struct Transfer4Witness {
    pub domain_sep: Field,
    pub asset_tag: Field,
    pub epoch: Field,
    pub commitment_root: Field,
    pub nullifier_old_root: Field,
    pub nullifier_new_root: Field,
    pub nullifiers: [Field; N_INPUTS_4],
    pub out_commitments: [Field; N_OUTPUTS],
    pub asp_root: Field,
    pub owner_sk: Field,
    pub inputs: [InputWitness; N_INPUTS_4],
    pub outputs: [OutputWitness; N_OUTPUTS],
}

impl Transfer4Witness {
    pub fn run(self) {
        verify_transfer4(
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

/// Deterministic 3-real + 1-dummy transfer: spend three owned notes (1000 + 500 + 300) at leaves
/// 0,1,2, pay 1500 to a recipient with 300 change to self. Four nullifier insertions. Fully valid
/// by construction.
pub fn demo_witness_at(epoch: Field, domain_sep: Field) -> Transfer4Witness {
    let z = zero_ladder();
    let asset_tag = 1;
    let owner_sk = 12345;
    let owner_pk = derive_owner_pk(owner_sk);

    let n0 = Note { value: 1000, asset_tag, owner_pk, blinding: 777, epoch, rho: 111 };
    let n1 = Note { value: 500, asset_tag, owner_pk, blinding: 778, epoch, rho: 112 };
    let n2 = Note { value: 300, asset_tag, owner_pk, blinding: 779, epoch, rho: 113 };
    let cms = [n0.commitment(), n1.commitment(), n2.commitment(), 0];
    let commitment_root = root4(cms, z);
    let (cm0_ir, cm0_sib) = build_path4(cms, 0, z);
    let (cm1_ir, cm1_sib) = build_path4(cms, 1, z);
    let (cm2_ir, cm2_sib) = build_path4(cms, 2, z);
    let (asp_root, asp_ir, asp_sib) = single_leaf_tree(owner_pk, z);

    let recipient_pk = derive_owner_pk(99);
    let out0 = OutputWitness { value: 1500, owner_pk: recipient_pk, blinding: 222, rho: 333 };
    let out1 = OutputWitness { value: 300, owner_pk, blinding: 444, rho: 555 };

    let dummy_rho = 0xdead;
    let nf0 = nullifier(111, owner_sk);
    let nf1 = nullifier(112, owner_sk);
    let nf2 = nullifier(113, owner_sk);
    let nf3 = nullifier(dummy_rho, owner_sk);
    let (nf_old, nf_new, ws) = build_four_insertions([nf0, nf1, nf2, nf3]);

    let in0 = make_input(1000, 777, epoch, 111, false, cm0_ir, cm0_sib, asp_ir, asp_sib, ws[0]);
    let in1 = make_input(500, 778, epoch, 112, false, cm1_ir, cm1_sib, asp_ir, asp_sib, ws[1]);
    let in2 = make_input(300, 779, epoch, 113, false, cm2_ir, cm2_sib, asp_ir, asp_sib, ws[2]);
    let in3 = make_input(
        0,
        0,
        epoch,
        dummy_rho,
        true,
        [false; TREE_DEPTH],
        [0; TREE_DEPTH],
        [false; TREE_DEPTH],
        [0; TREE_DEPTH],
        ws[3],
    );

    Transfer4Witness {
        domain_sep,
        asset_tag,
        epoch,
        commitment_root,
        nullifier_old_root: nf_old,
        nullifier_new_root: nf_new,
        nullifiers: [nf0, nf1, nf2, nf3],
        out_commitments: [out0.commitment(asset_tag, epoch), out1.commitment(asset_tag, epoch)],
        asp_root,
        owner_sk,
        inputs: [in0, in1, in2, in3],
        outputs: [out0, out1],
    }
}

/// Deterministic 4-input transfer demo (epoch 5, demo domain_sep).
pub fn demo_witness() -> Transfer4Witness {
    demo_witness_at(5, 0xabc)
}

#[test]
fn transfer4_end_to_end() {
    demo_witness().run();
}

#[test(should_fail)]
fn transfer4_rejects_value_nonconservation() {
    let mut w = demo_witness();
    w.outputs[0].value = 1501; // 1800 in != 1801 out
    w.out_commitments[0] =
        w.outputs[0].commitment(w.asset_tag, w.epoch);
    w.run();
}

/// Consolidation shape: 3 real inputs collapse to ONE self note (out0 = total, out1 = 0).
#[test]
fn transfer4_consolidation_shape() {
    let z = zero_ladder();
    let asset_tag = 1;
    let epoch = 5;
    let owner_sk = 12345;
    let owner_pk = derive_owner_pk(owner_sk);

    let n0 = Note { value: 1000, asset_tag, owner_pk, blinding: 777, epoch, rho: 111 };
    let n1 = Note { value: 500, asset_tag, owner_pk, blinding: 778, epoch, rho: 112 };
    let n2 = Note { value: 300, asset_tag, owner_pk, blinding: 779, epoch, rho: 113 };
    let cms = [n0.commitment(), n1.commitment(), n2.commitment(), 0];
    let commitment_root = root4(cms, z);
    let (cm0_ir, cm0_sib) = build_path4(cms, 0, z);
    let (cm1_ir, cm1_sib) = build_path4(cms, 1, z);
    let (cm2_ir, cm2_sib) = build_path4(cms, 2, z);
    let (asp_root, asp_ir, asp_sib) = single_leaf_tree(owner_pk, z);

    let out0 = OutputWitness { value: 1800, owner_pk, blinding: 222, rho: 333 };
    let out1 = OutputWitness { value: 0, owner_pk, blinding: 444, rho: 555 };

    let dummy_rho = 0xdead;
    let nf0 = nullifier(111, owner_sk);
    let nf1 = nullifier(112, owner_sk);
    let nf2 = nullifier(113, owner_sk);
    let nf3 = nullifier(dummy_rho, owner_sk);
    let (nf_old, nf_new, ws) = build_four_insertions([nf0, nf1, nf2, nf3]);

    let in0 = make_input(1000, 777, epoch, 111, false, cm0_ir, cm0_sib, asp_ir, asp_sib, ws[0]);
    let in1 = make_input(500, 778, epoch, 112, false, cm1_ir, cm1_sib, asp_ir, asp_sib, ws[1]);
    let in2 = make_input(300, 779, epoch, 113, false, cm2_ir, cm2_sib, asp_ir, asp_sib, ws[2]);
    let in3 = make_input(
        0, 0, epoch, dummy_rho, true,
        [false; TREE_DEPTH], [0; TREE_DEPTH], [false; TREE_DEPTH], [0; TREE_DEPTH], ws[3],
    );

    Transfer4Witness {
        domain_sep: 0xabc,
        asset_tag,
        epoch,
        commitment_root,
        nullifier_old_root: nf_old,
        nullifier_new_root: nf_new,
        nullifiers: [nf0, nf1, nf2, nf3],
        out_commitments: [out0.commitment(asset_tag, epoch), out1.commitment(asset_tag, epoch)],
        asp_root,
        owner_sk,
        inputs: [in0, in1, in2, in3],
        outputs: [out0, out1],
    }
        .run();
}
