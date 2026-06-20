//! `transfer` circuit logic + shared witness helpers (Z1 spec section 5), as library code so
//! the real circuit (`circuits/transfer`, public interface), the no-input proving
//! harness (`circuits/transfer_demo`), and the withdraw circuit all share one
//! implementation of the input-spending machinery.
use crate::accumulator::{IndexedLeaf, insert};
use crate::merkle::{assert_membership, compute_root, hash_node, TREE_DEPTH};
use crate::{derive_owner_pk, nullifier, Note};

pub global N_INPUTS: u32 = 2;
pub global N_OUTPUTS: u32 = 2;

pub struct InputWitness {
    pub value: Field,
    pub blinding: Field,
    pub epoch: Field,
    pub rho: Field,
    pub is_dummy: bool,
    pub cm_is_right: [bool; TREE_DEPTH],
    pub cm_siblings: [Field; TREE_DEPTH],
    pub asp_is_right: [bool; TREE_DEPTH],
    pub asp_siblings: [Field; TREE_DEPTH],
    pub nf_low: IndexedLeaf,
    pub nf_low_is_right: [bool; TREE_DEPTH],
    pub nf_low_siblings: [Field; TREE_DEPTH],
    pub nf_new_is_right: [bool; TREE_DEPTH],
    pub nf_new_siblings: [Field; TREE_DEPTH],
    pub nf_new_index: Field,
}

pub struct OutputWitness {
    pub value: Field,
    pub owner_pk: Field,
    pub blinding: Field,
    pub rho: Field,
}

impl OutputWitness {
    pub fn commitment(self, asset_tag: Field, epoch: Field) -> Field {
        Note { value: self.value, asset_tag, owner_pk: self.owner_pk, blinding: self.blinding, epoch, rho: self.rho }
            .commitment()
    }
}

/// Spend the inputs: for each, verify commitment + ASP membership (real inputs only),
/// match & insert its nullifier into the accumulator, and accumulate its value.
/// Returns the input value sum. Shared by `transfer` and `withdraw`.
pub fn spend_inputs(
    asset_tag: Field,
    commitment_root: Field,
    asp_root: Field,
    owner_pk: Field,
    owner_sk: Field,
    nullifiers: [Field; N_INPUTS],
    nullifier_old_root: Field,
    nullifier_new_root: Field,
    inputs: [InputWitness; N_INPUTS],
) -> Field {
    let mut in_sum: Field = 0;
    let mut nf_root = nullifier_old_root;
    for i in 0..N_INPUTS {
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

/// Core transfer constraints (Z1 section 5).
pub fn verify_transfer(
    domain_sep: Field,
    asset_tag: Field,
    epoch: Field,
    commitment_root: Field,
    nullifier_old_root: Field,
    nullifier_new_root: Field,
    nullifiers: [Field; N_INPUTS],
    out_commitments: [Field; N_OUTPUTS],
    asp_root: Field,
    owner_sk: Field,
    inputs: [InputWitness; N_INPUTS],
    outputs: [OutputWitness; N_OUTPUTS],
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
    for j in 0..N_OUTPUTS {
        assert(outputs[j].commitment(asset_tag, epoch) == out_commitments[j]);
        outputs[j].value.assert_max_bit_size::<64>();
        out_sum += outputs[j].value;
    }
    assert(in_sum == out_sum);
}

// ----------------------------- witness construction helpers -----------------------------
// Build valid witnesses for a depth-20 tree whose only non-empty leaves are the first
// four. Used by tests and the no-input proving harnesses. Every hash is computed with
// the same gadgets the circuit uses, so results are self-consistent by construction.

/// Zero-subtree-hash ladder: z[0] = empty leaf, z[i] = hash(z[i-1], z[i-1]).
pub fn zero_ladder() -> [Field; TREE_DEPTH] {
    let mut z = [0; TREE_DEPTH];
    for i in 1..TREE_DEPTH {
        z[i] = hash_node(z[i - 1], z[i - 1]);
    }
    z
}

/// Membership path for `index` (0..3) when only the first four leaves are non-empty.
pub fn build_path4(
    hashes: [Field; 4],
    index: u32,
    z: [Field; TREE_DEPTH],
) -> ([bool; TREE_DEPTH], [Field; TREE_DEPTH]) {
    let n01 = hash_node(hashes[0], hashes[1]);
    let n23 = hash_node(hashes[2], hashes[3]);
    let mut is_right = [false; TREE_DEPTH];
    let mut sib = [0; TREE_DEPTH];
    if index == 0 {
        sib[0] = hashes[1];
        sib[1] = n23;
    }
    if index == 1 {
        sib[0] = hashes[0];
        sib[1] = n23;
        is_right[0] = true;
    }
    if index == 2 {
        sib[0] = hashes[3];
        sib[1] = n01;
        is_right[1] = true;
    }
    if index == 3 {
        sib[0] = hashes[2];
        sib[1] = n01;
        is_right[0] = true;
        is_right[1] = true;
    }
    for l in 2..TREE_DEPTH {
        sib[l] = z[l];
    }
    (is_right, sib)
}

pub fn root4(hashes: [Field; 4], z: [Field; TREE_DEPTH]) -> Field {
    let (ir, sib) = build_path4(hashes, 0, z);
    compute_root(hashes[0], ir, sib)
}

/// Single-leaf membership at index 0 (tree with one real leaf): returns (root, path).
pub fn single_leaf_tree(
    leaf: Field,
    z: [Field; TREE_DEPTH],
) -> (Field, [bool; TREE_DEPTH], [Field; TREE_DEPTH]) {
    let hashes = [leaf, 0, 0, 0];
    let (ir, sib) = build_path4(hashes, 0, z);
    (root4(hashes, z), ir, sib)
}

/// The accumulator-insertion witness for one nullifier.
pub struct NfInsert {
    pub nf_low: IndexedLeaf,
    pub low_is_right: [bool; TREE_DEPTH],
    pub low_siblings: [Field; TREE_DEPTH],
    pub new_is_right: [bool; TREE_DEPTH],
    pub new_siblings: [Field; TREE_DEPTH],
    pub new_index: Field,
}

/// Build the two-insertion sequence for `nf0` then `nf1` into a fresh accumulator
/// (canonical init leaf {0,0,0} at index 0). Returns old/new roots + both witnesses.
/// Self-checking: calls the gadget `insert`, so bad paths trip its internal asserts.
pub fn build_two_insertions(nf0: Field, nf1: Field) -> (Field, Field, NfInsert, NfInsert) {
    let z = zero_ladder();
    let init_leaf = IndexedLeaf { value: 0, next_index: 0, next_value: 0 };
    let acc0 = [init_leaf.hash(), 0, 0, 0];
    let old_root = root4(acc0, z);

    // insertion 1 (nf0) at index 1; low = init leaf (tail)
    let (l1lo_ir, l1lo_sib) = build_path4(acc0, 0, z);
    let low0_updated = IndexedLeaf { value: 0, next_index: 1, next_value: nf0 };
    let acc0b = [low0_updated.hash(), 0, 0, 0];
    let (l1new_ir, l1new_sib) = build_path4(acc0b, 1, z);
    let root1 = insert(nf0, init_leaf, l1lo_ir, l1lo_sib, old_root, 1, l1new_ir, l1new_sib);

    // post-insertion-1 leaves: idx0 = {0,1,nf0}, idx1 = {nf0,0,0}
    let leaf0_a1 = IndexedLeaf { value: 0, next_index: 1, next_value: nf0 };
    let leaf1_a1 = IndexedLeaf { value: nf0, next_index: 0, next_value: 0 };
    let acc1 = [leaf0_a1.hash(), leaf1_a1.hash(), 0, 0];

    // insertion 2 (nf1) at index 2; low depends on ordering vs nf0
    let nf1_gt_nf0 = nf0.lt(nf1);
    let low2 = if nf1_gt_nf0 { leaf1_a1 } else { leaf0_a1 };
    let low2_idx = if nf1_gt_nf0 { 1 } else { 0 };
    let low2_updated = if nf1_gt_nf0 {
        IndexedLeaf { value: nf0, next_index: 2, next_value: nf1 }
    } else {
        IndexedLeaf { value: 0, next_index: 2, next_value: nf1 }
    };
    let (l2lo_ir, l2lo_sib) = build_path4(acc1, low2_idx, z);
    let mut acc1b = acc1;
    acc1b[low2_idx] = low2_updated.hash();
    let (l2new_ir, l2new_sib) = build_path4(acc1b, 2, z);
    let new_root = insert(nf1, low2, l2lo_ir, l2lo_sib, root1, 2, l2new_ir, l2new_sib);

    let w0 = NfInsert {
        nf_low: init_leaf,
        low_is_right: l1lo_ir,
        low_siblings: l1lo_sib,
        new_is_right: l1new_ir,
        new_siblings: l1new_sib,
        new_index: 1,
    };
    let w1 = NfInsert {
        nf_low: low2,
        low_is_right: l2lo_ir,
        low_siblings: l2lo_sib,
        new_is_right: l2new_ir,
        new_siblings: l2new_sib,
        new_index: 2,
    };
    (old_root, new_root, w0, w1)
}

/// Assemble an `InputWitness` from a note's private data, its membership paths, and an
/// accumulator-insertion witness.
pub fn make_input(
    value: Field,
    blinding: Field,
    epoch: Field,
    rho: Field,
    is_dummy: bool,
    cm_is_right: [bool; TREE_DEPTH],
    cm_siblings: [Field; TREE_DEPTH],
    asp_is_right: [bool; TREE_DEPTH],
    asp_siblings: [Field; TREE_DEPTH],
    nf: NfInsert,
) -> InputWitness {
    InputWitness {
        value,
        blinding,
        epoch,
        rho,
        is_dummy,
        cm_is_right,
        cm_siblings,
        asp_is_right,
        asp_siblings,
        nf_low: nf.nf_low,
        nf_low_is_right: nf.low_is_right,
        nf_low_siblings: nf.low_siblings,
        nf_new_is_right: nf.new_is_right,
        nf_new_siblings: nf.new_siblings,
        nf_new_index: nf.new_index,
    }
}

/// Bundle of public inputs + private witness for a complete transfer.
pub struct TransferWitness {
    pub domain_sep: Field,
    pub asset_tag: Field,
    pub epoch: Field,
    pub commitment_root: Field,
    pub nullifier_old_root: Field,
    pub nullifier_new_root: Field,
    pub nullifiers: [Field; N_INPUTS],
    pub out_commitments: [Field; N_OUTPUTS],
    pub asp_root: Field,
    pub owner_sk: Field,
    pub inputs: [InputWitness; N_INPUTS],
    pub outputs: [OutputWitness; N_OUTPUTS],
}

impl TransferWitness {
    pub fn run(self) {
        verify_transfer(
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

/// Deterministic 1-real + 1-dummy transfer (1000 -> 600 recipient + 400 change), two
/// nullifier insertions. Fully valid by construction. Parameterized by `epoch` and
/// `domain_sep` so it binds to a live pool; spends exactly the note that
/// `deposit::demo_witness_at` mints.
pub fn demo_witness_at(epoch: Field, domain_sep: Field) -> TransferWitness {
    let z = zero_ladder();
    let asset_tag = 1;
    let owner_sk = 12345;
    let owner_pk = derive_owner_pk(owner_sk);

    let in_note = Note { value: 1000, asset_tag, owner_pk, blinding: 777, epoch, rho: 111 };
    let (commitment_root, cm_ir, cm_sib) = single_leaf_tree(in_note.commitment(), z);
    let (asp_root, asp_ir, asp_sib) = single_leaf_tree(owner_pk, z);

    let recipient_pk = derive_owner_pk(99);
    let out0 = OutputWitness { value: 600, owner_pk: recipient_pk, blinding: 222, rho: 333 };
    let out1 = OutputWitness { value: 400, owner_pk, blinding: 444, rho: 555 };

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
        [false; TREE_DEPTH],
        [0; TREE_DEPTH],
        [false; TREE_DEPTH],
        [0; TREE_DEPTH],
        w1,
    );

    TransferWitness {
        domain_sep,
        asset_tag,
        epoch,
        commitment_root,
        nullifier_old_root: nf_old,
        nullifier_new_root: nf_new,
        nullifiers: [nf0, nf1],
        out_commitments: [out0.commitment(asset_tag, epoch), out1.commitment(asset_tag, epoch)],
        asp_root,
        owner_sk,
        inputs: [real_in, dummy_in],
        outputs: [out0, out1],
    }
}

/// Deterministic transfer demo (epoch 5, demo domain_sep).
pub fn demo_witness() -> TransferWitness {
    demo_witness_at(5, 0xabc)
}

#[test]
fn transfer_end_to_end() {
    demo_witness().run();
}

/// Build a transfer witness whose ASP root is a 4-key approved set (the spender's
/// owner_pk among them), instead of the 1-key set in `demo_witness`. The proof
/// reveals only `asp_root` and proves `owner_pk in asp_root` for a hidden index ->
/// "compliant private transfer": only approved parties can move funds, without
/// revealing WHICH approved party (anonymity set = 4). `spender_index` selects
/// where the real owner_pk sits; if out of 0..4 the set excludes it (negative case).
fn multi_asp_witness(spender_index: u32) -> TransferWitness {
    let z = zero_ladder();
    let asset_tag = 1;
    let epoch = 5;
    let owner_sk = 12345;
    let owner_pk = derive_owner_pk(owner_sk);

    let in_note = Note { value: 1000, asset_tag, owner_pk, blinding: 777, epoch, rho: 111 };
    let (commitment_root, cm_ir, cm_sib) = single_leaf_tree(in_note.commitment(), z);

    // Four approved keys; place the spender at `spender_index` (else absent).
    let mut approved = [derive_owner_pk(111), derive_owner_pk(222), derive_owner_pk(333), derive_owner_pk(444)];
    if spender_index < 4 {
        approved[spender_index] = owner_pk;
    }
    let asp_root = root4(approved, z);
    let path_index = if spender_index < 4 { spender_index } else { 0 };
    let (asp_ir, asp_sib) = build_path4(approved, path_index, z);

    let recipient_pk = derive_owner_pk(99);
    let out0 = OutputWitness { value: 600, owner_pk: recipient_pk, blinding: 222, rho: 333 };
    let out1 = OutputWitness { value: 400, owner_pk, blinding: 444, rho: 555 };

    let dummy_rho = 0xdead;
    let nf0 = nullifier(111, owner_sk);
    let nf1 = nullifier(dummy_rho, owner_sk);
    let (nf_old, nf_new, w0, w1) = build_two_insertions(nf0, nf1);

    let real_in = make_input(1000, 777, epoch, 111, false, cm_ir, cm_sib, asp_ir, asp_sib, w0);
    let dummy_in = make_input(
        0, 0, epoch, dummy_rho, true,
        [false; TREE_DEPTH], [0; TREE_DEPTH], [false; TREE_DEPTH], [0; TREE_DEPTH], w1,
    );

    TransferWitness {
        domain_sep: 0xabc,
        asset_tag,
        epoch,
        commitment_root,
        nullifier_old_root: nf_old,
        nullifier_new_root: nf_new,
        nullifiers: [nf0, nf1],
        out_commitments: [out0.commitment(asset_tag, epoch), out1.commitment(asset_tag, epoch)],
        asp_root,
        owner_sk,
        inputs: [real_in, dummy_in],
        outputs: [out0, out1],
    }
}

#[test]
fn transfer_asp_membership_anonymity_set_of_four() {
    // Spender is the 3rd of four approved keys; the proof binds only asp_root.
    multi_asp_witness(2).run();
}

#[test(should_fail)]
fn transfer_rejects_unapproved_spender() {
    // index 9 -> owner_pk is NOT in the approved set; ASP membership must fail.
    multi_asp_witness(9).run();
}
