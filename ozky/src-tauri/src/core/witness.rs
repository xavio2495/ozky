//! Stateful witness generation (Phase A2) — the dependency the ZK phases (Z4/Z7)
//! deferred to the Rust core. Builds the full private+public witness for the
//! deposit/transfer/withdraw circuits against LIVE pool state: commitment Merkle
//! paths and nullifier indexed-accumulator non-membership/insertion witnesses
//! (sourced from the indexer, or rebuilt from raw chain), then serializes a
//! `Prover.toml` that `nargo` solves and `bb` proves.
//!
//! Every hash here uses the circuit-matching [`Hasher`] (proven Poseidon2 parity),
//! so a witness this module builds is byte-for-byte acceptable to the on-chain
//! verifier. The merkle/accumulator construction mirrors the Noir helpers in
//! `circuits/notes/src/{merkle,accumulator,transfer}.nr` and the indexer's
//! reconstruction (`indexer/src/{tree,accumulator}.rs`) exactly.

use super::poseidon::{Fr, Hasher};

pub const TREE_DEPTH: usize = 20;

// ----------------------------- Merkle paths -----------------------------

/// Zero-subtree-hash ladder: `z[0] = 0`, `z[i] = hash_node(z[i-1], z[i-1])`.
/// Length `DEPTH + 1` (`z[level]` is the root of an empty subtree of height `level`).
fn zero_ladder(h: &Hasher) -> Vec<Fr> {
    let mut z = Vec::with_capacity(TREE_DEPTH + 1);
    let mut cur = Fr::ZERO;
    z.push(cur);
    for _ in 0..TREE_DEPTH {
        cur = h.hash_node(&cur, &cur);
        z.push(cur);
    }
    z
}

/// An authentication path: per level, the sibling and whether THIS node is the right
/// child (`is_right[i] == true` ⇒ sibling is on the left), plus the recomputed root.
#[derive(Clone)]
pub struct MerklePath {
    pub is_right: [bool; TREE_DEPTH],
    pub siblings: [Fr; TREE_DEPTH],
    pub root: Fr,
}

impl MerklePath {
    /// All-zero path (used for dummy inputs, whose membership checks are skipped).
    fn zero() -> MerklePath {
        MerklePath {
            is_right: [false; TREE_DEPTH],
            siblings: [Fr::ZERO; TREE_DEPTH],
            root: Fr::ZERO,
        }
    }
}

/// Build the depth-20 authentication path for `index` over the ordered `leaves`
/// (padding empty subtrees with the zero ladder). `index < leaves.len()`.
/// Mirrors `indexer/src/tree.rs::merkle_path`.
fn merkle_path(h: &Hasher, z: &[Fr], leaves: &[Fr], index: usize) -> MerklePath {
    let mut cur: Vec<Fr> = leaves.to_vec();
    let mut pos = index;
    let mut is_right = [false; TREE_DEPTH];
    let mut siblings = [Fr::ZERO; TREE_DEPTH];

    for level in 0..TREE_DEPTH {
        let sib_index = pos ^ 1;
        siblings[level] = if sib_index < cur.len() {
            cur[sib_index]
        } else {
            z[level]
        };
        is_right[level] = pos & 1 == 1;

        let mut next = Vec::with_capacity(cur.len().div_ceil(2));
        let mut i = 0;
        while i < cur.len() {
            let l = cur[i];
            let r = if i + 1 < cur.len() { cur[i + 1] } else { z[level] };
            next.push(h.hash_node(&l, &r));
            i += 2;
        }
        cur = next;
        pos >>= 1;
    }

    let root = if cur.len() == 1 { cur[0] } else { z[TREE_DEPTH] };
    MerklePath { is_right, siblings, root }
}

/// Root + single-leaf membership path for a one-leaf tree (the leaf at index 0).
/// Matches the circuit's `single_leaf_tree`.
pub fn single_leaf_tree(h: &Hasher, leaf: Fr) -> MerklePath {
    let z = zero_ladder(h);
    merkle_path(h, &z, &[leaf], 0)
}

/// Membership path for `index` over an arbitrary ordered commitment-leaf set.
pub fn commitment_path(h: &Hasher, leaves: &[Fr], index: usize) -> MerklePath {
    let z = zero_ladder(h);
    merkle_path(h, &z, leaves, index)
}

// ----------------------------- Nullifier accumulator -----------------------------

/// A leaf of the indexed (sorted-linked-list) nullifier accumulator.
#[derive(Clone)]
struct IdxLeaf {
    value: Fr,
    next_index: u64,
    next_value: Fr,
}

impl IdxLeaf {
    fn hash(&self, h: &Hasher) -> Fr {
        h.indexed_leaf(&self.value, self.next_index, &self.next_value)
    }
}

/// The accumulator-insertion witness for one nullifier (low-leaf non-membership +
/// new-slot insertion path). Matches `circuits/notes/src/transfer.nr::NfInsert`.
pub struct NfInsert {
    nf_low: IdxLeaf,
    low: MerklePath,
    new: MerklePath,
    new_index: u64,
}

/// The indexed nullifier accumulator. Slot 0 is the canonical init leaf `{0,0,0}`;
/// each insert appends a leaf and repoints its low leaf (mirrors the indexer and
/// `accumulator.nr::insert`).
struct IndexedTree {
    leaves: Vec<IdxLeaf>,
}

impl IndexedTree {
    fn fresh() -> IndexedTree {
        IndexedTree {
            leaves: vec![IdxLeaf {
                value: Fr::ZERO,
                next_index: 0,
                next_value: Fr::ZERO,
            }],
        }
    }

    /// Replay prior nullifiers (insertion order) to reconstruct the live tree.
    fn from_nullifiers(h: &Hasher, prior: &[Fr]) -> IndexedTree {
        let mut t = IndexedTree::fresh();
        for nf in prior {
            t.insert(h, *nf);
        }
        t
    }

    fn leaf_hashes(&self, h: &Hasher) -> Vec<Fr> {
        self.leaves.iter().map(|l| l.hash(h)).collect()
    }

    fn root(&self, h: &Hasher) -> Fr {
        let z = zero_ladder(h);
        merkle_path(h, &z, &self.leaf_hashes(h), 0).root
    }

    /// Index of the low leaf bracketing `target` (`low.value < target < next_value`,
    /// or `low` is the tail). Panics if `target` is already present.
    fn low_leaf_index(&self, target: &Fr) -> usize {
        for (i, l) in self.leaves.iter().enumerate() {
            assert!(&l.value != target, "nullifier already spent (present in accumulator)");
            let above_low = l.value.lt(target);
            let is_tail = l.next_value.is_zero();
            let below_next = !is_tail && target.lt(&l.next_value);
            if above_low && (is_tail || below_next) {
                return i;
            }
        }
        panic!("no bracketing low leaf — malformed accumulator");
    }

    /// Insert `target`, returning its insertion witness. Mutates the tree to the
    /// post-insertion state (so the next insert sees it).
    fn insert(&mut self, h: &Hasher, target: Fr) -> NfInsert {
        let z = zero_ladder(h);
        let lo = self.low_leaf_index(&target);
        let low_before = self.leaves[lo].clone();
        let new_index = self.leaves.len() as u64;

        // Low-leaf non-membership path against the current root.
        let low = merkle_path(h, &z, &self.leaf_hashes(h), lo);

        // Repoint the low leaf at the new slot, then build the new-slot path against
        // the mid root (the new slot is still empty == 0).
        self.leaves[lo] = IdxLeaf {
            value: low_before.value,
            next_index: new_index,
            next_value: target,
        };
        let mut mid_hashes = self.leaf_hashes(h);
        mid_hashes.push(Fr::ZERO); // empty new slot at `new_index`
        let new = merkle_path(h, &z, &mid_hashes, new_index as usize);

        // The new leaf inherits the low leaf's previous successor.
        self.leaves.push(IdxLeaf {
            value: target,
            next_index: low_before.next_index,
            next_value: low_before.next_value,
        });

        NfInsert { nf_low: low_before, low, new, new_index }
    }
}

/// Build the chained two-insertion witness (`nf0` then `nf1`) into the accumulator
/// holding `prior` nullifiers. Returns old root, new root, and both witnesses.
fn build_two_insertions(
    h: &Hasher,
    prior: &[Fr],
    nf0: Fr,
    nf1: Fr,
) -> (Fr, Fr, NfInsert, NfInsert) {
    let mut tree = IndexedTree::from_nullifiers(h, prior);
    let old_root = tree.root(h);
    let w0 = tree.insert(h, nf0);
    let w1 = tree.insert(h, nf1);
    let new_root = tree.root(h);
    (old_root, new_root, w0, w1)
}

// ----------------------------- Input / output witnesses -----------------------------

pub struct InputWitness {
    pub value: Fr,
    pub blinding: Fr,
    pub epoch: Fr,
    pub rho: Fr,
    pub is_dummy: bool,
    pub cm: MerklePath,
    pub asp: MerklePath,
    pub nf: NfInsert,
}

pub struct OutputWitness {
    pub value: Fr,
    pub owner_pk: Fr,
    pub blinding: Fr,
    pub rho: Fr,
}

impl OutputWitness {
    fn commitment(&self, h: &Hasher, asset_tag: &Fr, epoch: &Fr) -> Fr {
        h.commitment(&self.value, asset_tag, &self.owner_pk, &self.blinding, epoch, &self.rho)
    }
}

// ----------------------------- Prover.toml serialization -----------------------------

/// Minimal big-endian hex (e.g. `0`→`0x00`, `1000`→`0x03e8`), quoted for TOML — the
/// exact form `nargo` accepts (matches the interim witgen's `println` + sed output).
fn q(f: &Fr) -> String {
    let bytes = f.0;
    let mut start = 0;
    while start < 31 && bytes[start] == 0 {
        start += 1;
    }
    format!("\"0x{}\"", hex::encode(&bytes[start..]))
}

fn q_u64(n: u64) -> String {
    q(&Fr::from_u64(n))
}

fn bools(arr: &[bool; TREE_DEPTH]) -> String {
    let items: Vec<String> = arr.iter().map(|b| b.to_string()).collect();
    format!("[{}]", items.join(", "))
}

fn frs(arr: &[Fr; TREE_DEPTH]) -> String {
    let items: Vec<String> = arr.iter().map(q).collect();
    format!("[{}]", items.join(", "))
}

fn fr_array(items: &[Fr]) -> String {
    let items: Vec<String> = items.iter().map(q).collect();
    format!("[{}]", items.join(", "))
}

/// Serialize one `[[inputs]]` block (scalars, the four paths, then the `nf_low` table).
fn input_block(w: &InputWitness) -> String {
    let mut s = String::new();
    s.push_str("\n[[inputs]]\n");
    s.push_str(&format!("value = {}\n", q(&w.value)));
    s.push_str(&format!("blinding = {}\n", q(&w.blinding)));
    s.push_str(&format!("epoch = {}\n", q(&w.epoch)));
    s.push_str(&format!("rho = {}\n", q(&w.rho)));
    s.push_str(&format!("is_dummy = {}\n", w.is_dummy));
    s.push_str(&format!("cm_is_right = {}\n", bools(&w.cm.is_right)));
    s.push_str(&format!("cm_siblings = {}\n", frs(&w.cm.siblings)));
    s.push_str(&format!("asp_is_right = {}\n", bools(&w.asp.is_right)));
    s.push_str(&format!("asp_siblings = {}\n", frs(&w.asp.siblings)));
    s.push_str(&format!("nf_low_is_right = {}\n", bools(&w.nf.low.is_right)));
    s.push_str(&format!("nf_low_siblings = {}\n", frs(&w.nf.low.siblings)));
    s.push_str(&format!("nf_new_is_right = {}\n", bools(&w.nf.new.is_right)));
    s.push_str(&format!("nf_new_siblings = {}\n", frs(&w.nf.new.siblings)));
    s.push_str(&format!("nf_new_index = {}\n", q_u64(w.nf.new_index)));
    s.push_str("[inputs.nf_low]\n");
    s.push_str(&format!("value = {}\n", q(&w.nf.nf_low.value)));
    s.push_str(&format!("next_index = {}\n", q_u64(w.nf.nf_low.next_index)));
    s.push_str(&format!("next_value = {}\n", q(&w.nf.nf_low.next_value)));
    s
}

fn output_block(w: &OutputWitness) -> String {
    let mut s = String::new();
    s.push_str("\n[[outputs]]\n");
    s.push_str(&format!("value = {}\n", q(&w.value)));
    s.push_str(&format!("owner_pk = {}\n", q(&w.owner_pk)));
    s.push_str(&format!("blinding = {}\n", q(&w.blinding)));
    s.push_str(&format!("rho = {}\n", q(&w.rho)));
    s
}

// ----------------------------- Circuit witness bundles -----------------------------

pub struct TransferWitness {
    pub domain_sep: Fr,
    pub asset_tag: Fr,
    pub epoch: Fr,
    pub commitment_root: Fr,
    pub nullifier_old_root: Fr,
    pub nullifier_new_root: Fr,
    pub nullifiers: [Fr; 2],
    pub out_commitments: [Fr; 2],
    pub asp_root: Fr,
    pub owner_sk: Fr,
    pub inputs: [InputWitness; 2],
    pub outputs: [OutputWitness; 2],
}

impl TransferWitness {
    pub fn to_prover_toml(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("domain_sep = {}\n", q(&self.domain_sep)));
        s.push_str(&format!("asset_tag = {}\n", q(&self.asset_tag)));
        s.push_str(&format!("epoch = {}\n", q(&self.epoch)));
        s.push_str(&format!("commitment_root = {}\n", q(&self.commitment_root)));
        s.push_str(&format!("nullifier_old_root = {}\n", q(&self.nullifier_old_root)));
        s.push_str(&format!("nullifier_new_root = {}\n", q(&self.nullifier_new_root)));
        s.push_str(&format!("nullifiers = {}\n", fr_array(&self.nullifiers)));
        s.push_str(&format!("out_commitments = {}\n", fr_array(&self.out_commitments)));
        s.push_str(&format!("asp_root = {}\n", q(&self.asp_root)));
        s.push_str(&format!("owner_sk = {}\n", q(&self.owner_sk)));
        s.push_str(&input_block(&self.inputs[0]));
        s.push_str(&input_block(&self.inputs[1]));
        s.push_str(&output_block(&self.outputs[0]));
        s.push_str(&output_block(&self.outputs[1]));
        s
    }
}

pub struct DepositWitness {
    pub domain_sep: Fr,
    pub asset_tag: Fr,
    pub epoch: Fr,
    pub amount: Fr,
    pub out_commitment: Fr,
    pub owner_pk: Fr,
    pub blinding: Fr,
    pub rho: Fr,
}

impl DepositWitness {
    /// Build a deposit witness: bind public `amount` to a fresh hiding note.
    pub fn build(
        h: &Hasher,
        domain_sep: Fr,
        asset_tag: Fr,
        epoch: Fr,
        amount: u64,
        owner_pk: Fr,
        blinding: Fr,
        rho: Fr,
    ) -> DepositWitness {
        let amount = Fr::from_u64(amount);
        let out_commitment = h.commitment(&amount, &asset_tag, &owner_pk, &blinding, &epoch, &rho);
        DepositWitness {
            domain_sep,
            asset_tag,
            epoch,
            amount,
            out_commitment,
            owner_pk,
            blinding,
            rho,
        }
    }

    pub fn to_prover_toml(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("domain_sep = {}\n", q(&self.domain_sep)));
        s.push_str(&format!("asset_tag = {}\n", q(&self.asset_tag)));
        s.push_str(&format!("epoch = {}\n", q(&self.epoch)));
        s.push_str(&format!("amount = {}\n", q(&self.amount)));
        s.push_str(&format!("out_commitment = {}\n", q(&self.out_commitment)));
        s.push_str(&format!("owner_pk = {}\n", q(&self.owner_pk)));
        s.push_str(&format!("blinding = {}\n", q(&self.blinding)));
        s.push_str(&format!("rho = {}\n", q(&self.rho)));
        s
    }
}

pub struct WithdrawWitness {
    pub domain_sep: Fr,
    pub asset_tag: Fr,
    pub epoch: Fr,
    pub commitment_root: Fr,
    pub nullifier_old_root: Fr,
    pub nullifier_new_root: Fr,
    pub nullifiers: [Fr; 2],
    pub change_commitment: Fr,
    pub asp_root: Fr,
    pub amount: Fr,
    pub dest_bind: Fr,
    pub owner_sk: Fr,
    pub inputs: [InputWitness; 2],
    pub change: OutputWitness,
}

impl WithdrawWitness {
    pub fn to_prover_toml(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("domain_sep = {}\n", q(&self.domain_sep)));
        s.push_str(&format!("asset_tag = {}\n", q(&self.asset_tag)));
        s.push_str(&format!("epoch = {}\n", q(&self.epoch)));
        s.push_str(&format!("commitment_root = {}\n", q(&self.commitment_root)));
        s.push_str(&format!("nullifier_old_root = {}\n", q(&self.nullifier_old_root)));
        s.push_str(&format!("nullifier_new_root = {}\n", q(&self.nullifier_new_root)));
        s.push_str(&format!("nullifiers = {}\n", fr_array(&self.nullifiers)));
        s.push_str(&format!("change_commitment = {}\n", q(&self.change_commitment)));
        s.push_str(&format!("asp_root = {}\n", q(&self.asp_root)));
        s.push_str(&format!("amount = {}\n", q(&self.amount)));
        s.push_str(&format!("dest_bind = {}\n", q(&self.dest_bind)));
        s.push_str(&format!("owner_sk = {}\n", q(&self.owner_sk)));
        s.push_str(&input_block(&self.inputs[0]));
        s.push_str(&input_block(&self.inputs[1]));
        s.push_str("\n[change]\n");
        s.push_str(&format!("value = {}\n", q(&self.change.value)));
        s.push_str(&format!("owner_pk = {}\n", q(&self.change.owner_pk)));
        s.push_str(&format!("blinding = {}\n", q(&self.change.blinding)));
        s.push_str(&format!("rho = {}\n", q(&self.change.rho)));
        s
    }
}

// ----------------------------- escrow (building block B) -----------------------------

use super::pedersen::{self, Point};
use super::poseidon::{DOMAIN_ESCROW_REFUND};

/// Escrow contribute witness: a withdraw-shaped spend that folds a HIDDEN `amount` into the
/// running Pedersen commitment. Public inputs in the canonical 14-field order.
pub struct ContributeWitness {
    pub domain_sep: Fr,
    pub asset_tag: Fr,
    pub epoch: Fr,
    pub commitment_root: Fr,
    pub nullifier_old_root: Fr,
    pub nullifier_new_root: Fr,
    pub nullifiers: [Fr; 2],
    pub change_commitment: Fr,
    pub asp_root: Fr,
    pub c_raised_old: Fr,
    pub c_raised_new: Fr,
    pub c_contrib: Fr,
    pub refund_bind: Fr,
    // private
    pub owner_sk: Fr,
    pub inputs: [InputWitness; 2],
    pub change: OutputWitness,
    pub amount: Fr,
    pub blinding_r: Fr,
    pub contrib_salt: Fr,
    /// The prior running commitment point (identity for the first contribution).
    pub p_old_x: Fr,
    pub p_old_y: Fr,
    pub p_old_inf: bool,
}

pub struct ContributeInputs<'a> {
    pub owner_sk: Fr,
    pub asset_tag: Fr,
    pub epoch: Fr,
    pub note_epoch: Fr,
    pub domain_sep: Fr,
    pub note_value: u64,
    pub note_blinding: Fr,
    pub note_rho: Fr,
    pub note_leaf_index: usize,
    pub commitment_leaves: &'a [Fr],
    pub asp_leaves: &'a [Fr],
    pub prior_nullifiers: &'a [Fr],
    pub dummy_rho: Fr,
    /// The hidden contribution amount (folded into the running commitment).
    pub amount: u64,
    pub blinding_r: Fr,
    pub contrib_salt: Fr,
    pub change_blinding: Fr,
    pub change_rho: Fr,
    /// The escrow's current running commitment POINT (identity for the first contribution).
    pub p_old: Point,
}

impl ContributeWitness {
    pub fn build(h: &Hasher, p: ContributeInputs) -> ContributeWitness {
        let owner_pk = h.owner_pk(&p.owner_sk);
        let nf0 = h.nullifier(&p.note_rho, &p.owner_sk);
        let nf1 = h.nullifier(&p.dummy_rho, &p.owner_sk);
        let (nf_old, nf_new, w0, w1) = build_two_insertions(h, p.prior_nullifiers, nf0, nf1);

        let real_note = SpendNote {
            value: p.note_value,
            blinding: p.note_blinding,
            epoch: p.note_epoch,
            rho: p.note_rho,
            leaf_index: p.note_leaf_index,
        };
        let real_in = real_input(h, &real_note, p.commitment_leaves, p.asp_leaves, owner_pk, w0);
        let dummy_in = dummy_input(p.epoch, p.dummy_rho, w1);

        let change = OutputWitness {
            value: Fr::from_u64(p.note_value - p.amount),
            owner_pk,
            blinding: p.change_blinding,
            rho: p.change_rho,
        };

        // Pedersen fold: c = commit(amount, r); p_new = p_old + c. The contract checks
        // c_raised_old == stored and stores c_raised_new (running-sum induction).
        let amount = Fr::from_u64(p.amount);
        let c = pedersen::commit(&amount, &p.blinding_r);
        let p_new = pedersen::add(&p.p_old, &c);
        let (p_old_x, p_old_y) = pedersen::coords(&p.p_old);

        ContributeWitness {
            domain_sep: p.domain_sep,
            asset_tag: p.asset_tag,
            epoch: p.epoch,
            commitment_root: real_in.cm.root,
            nullifier_old_root: nf_old,
            nullifier_new_root: nf_new,
            nullifiers: [nf0, nf1],
            change_commitment: change.commitment(h, &p.asset_tag, &p.epoch),
            asp_root: real_in.asp.root,
            c_raised_old: pedersen::point_hash(h, &p.p_old),
            c_raised_new: pedersen::point_hash(h, &p_new),
            c_contrib: pedersen::point_hash(h, &c),
            refund_bind: h.escrow_bind(DOMAIN_ESCROW_REFUND, &owner_pk, &p.contrib_salt),
            owner_sk: p.owner_sk,
            inputs: [real_in, dummy_in],
            change,
            amount,
            blinding_r: p.blinding_r,
            contrib_salt: p.contrib_salt,
            p_old_x,
            p_old_y,
            p_old_inf: p.p_old.inf,
        }
    }

    pub fn to_prover_toml(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("domain_sep = {}\n", q(&self.domain_sep)));
        s.push_str(&format!("asset_tag = {}\n", q(&self.asset_tag)));
        s.push_str(&format!("epoch = {}\n", q(&self.epoch)));
        s.push_str(&format!("commitment_root = {}\n", q(&self.commitment_root)));
        s.push_str(&format!("nullifier_old_root = {}\n", q(&self.nullifier_old_root)));
        s.push_str(&format!("nullifier_new_root = {}\n", q(&self.nullifier_new_root)));
        s.push_str(&format!("nullifiers = {}\n", fr_array(&self.nullifiers)));
        s.push_str(&format!("change_commitment = {}\n", q(&self.change_commitment)));
        s.push_str(&format!("asp_root = {}\n", q(&self.asp_root)));
        s.push_str(&format!("c_raised_old = {}\n", q(&self.c_raised_old)));
        s.push_str(&format!("c_raised_new = {}\n", q(&self.c_raised_new)));
        s.push_str(&format!("c_contrib = {}\n", q(&self.c_contrib)));
        s.push_str(&format!("refund_bind = {}\n", q(&self.refund_bind)));
        s.push_str(&format!("owner_sk = {}\n", q(&self.owner_sk)));
        s.push_str(&format!("amount = {}\n", q(&self.amount)));
        s.push_str(&format!("blinding_r = {}\n", q(&self.blinding_r)));
        s.push_str(&format!("contrib_salt = {}\n", q(&self.contrib_salt)));
        s.push_str(&input_block(&self.inputs[0]));
        s.push_str(&input_block(&self.inputs[1]));
        s.push_str("\n[change]\n");
        s.push_str(&format!("value = {}\n", q(&self.change.value)));
        s.push_str(&format!("owner_pk = {}\n", q(&self.change.owner_pk)));
        s.push_str(&format!("blinding = {}\n", q(&self.change.blinding)));
        s.push_str(&format!("rho = {}\n", q(&self.change.rho)));
        s.push_str("\n[p_old]\n");
        s.push_str(&format!("x = {}\n", q(&self.p_old_x)));
        s.push_str(&format!("y = {}\n", q(&self.p_old_y)));
        s.push_str(&format!("is_infinite = {}\n", self.p_old_inf));
        s
    }
}

/// Escrow payout witness (release & refund): open a Pedersen commitment to `value`, prove
/// `value >= floor`, mint a note of `value` to the recipient-bound key. Public inputs (7 fields).
pub struct PayoutWitness {
    pub domain_sep: Fr,
    pub asset_tag: Fr,
    pub epoch: Fr,
    pub commitment_hash: Fr,
    pub floor: Fr,
    pub out_commitment: Fr,
    pub recipient_bind: Fr,
    // private
    pub domain_bind: Fr,
    pub recipient_sk: Fr,
    pub value: Fr,
    pub blinding_r: Fr,
    pub out_blinding: Fr,
    pub out_rho: Fr,
    pub salt: Fr,
}

pub struct PayoutInputs {
    pub domain_sep: Fr,
    pub asset_tag: Fr,
    pub epoch: Fr,
    pub floor: u64,
    /// Binding domain: `DOMAIN_ESCROW_PAYEE` (release) or `_REFUND` (refund).
    pub domain_bind: u64,
    pub recipient_sk: Fr,
    /// The opened committed value (the running total for release, the contribution for refund).
    pub value: u64,
    /// The opening blinding (sum of contributors' `r` for release, the contribution `r` for refund).
    pub blinding_r: Fr,
    pub out_blinding: Fr,
    pub out_rho: Fr,
    pub salt: Fr,
}

impl PayoutWitness {
    pub fn build(h: &Hasher, p: PayoutInputs) -> PayoutWitness {
        let value = Fr::from_u64(p.value);
        let recipient_pk = h.owner_pk(&p.recipient_sk);
        let c = pedersen::commit(&value, &p.blinding_r);
        let out_commitment =
            h.commitment(&value, &p.asset_tag, &recipient_pk, &p.out_blinding, &p.epoch, &p.out_rho);
        PayoutWitness {
            domain_sep: p.domain_sep,
            asset_tag: p.asset_tag,
            epoch: p.epoch,
            commitment_hash: pedersen::point_hash(h, &c),
            floor: Fr::from_u64(p.floor),
            out_commitment,
            recipient_bind: h.escrow_bind(p.domain_bind, &recipient_pk, &p.salt),
            domain_bind: Fr::from_u64(p.domain_bind),
            recipient_sk: p.recipient_sk,
            value,
            blinding_r: p.blinding_r,
            out_blinding: p.out_blinding,
            out_rho: p.out_rho,
            salt: p.salt,
        }
    }

    pub fn to_prover_toml(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("domain_sep = {}\n", q(&self.domain_sep)));
        s.push_str(&format!("asset_tag = {}\n", q(&self.asset_tag)));
        s.push_str(&format!("epoch = {}\n", q(&self.epoch)));
        s.push_str(&format!("commitment_hash = {}\n", q(&self.commitment_hash)));
        s.push_str(&format!("floor = {}\n", q(&self.floor)));
        s.push_str(&format!("out_commitment = {}\n", q(&self.out_commitment)));
        s.push_str(&format!("recipient_bind = {}\n", q(&self.recipient_bind)));
        s.push_str(&format!("domain_bind = {}\n", q(&self.domain_bind)));
        s.push_str(&format!("recipient_sk = {}\n", q(&self.recipient_sk)));
        s.push_str(&format!("value = {}\n", q(&self.value)));
        s.push_str(&format!("blinding_r = {}\n", q(&self.blinding_r)));
        s.push_str(&format!("out_blinding = {}\n", q(&self.out_blinding)));
        s.push_str(&format!("out_rho = {}\n", q(&self.out_rho)));
        s.push_str(&format!("salt = {}\n", q(&self.salt)));
        s
    }
}

// ----------------------------- channel close (building block B phase 2) -----------------------------

use super::pedersen::Signature;
use super::poseidon::DOMAIN_CHANNEL_MERCHANT;

/// Channel close witness: open the cap + the subscriber-signed cumulative commitment, carry the
/// in-circuit Schnorr signature, mint drawn -> merchant and (cap - drawn) -> subscriber. Public
/// inputs in the canonical 10-field order.
pub struct ChannelCloseWitness {
    pub domain_sep: Fr,
    pub asset_tag: Fr,
    pub epoch: Fr,
    pub cap_hash: Fr,
    pub auth_key: Fr,
    pub valid_after_ledger: Fr,
    pub merchant_out: Fr,
    pub subscriber_out: Fr,
    pub merchant_bind: Fr,
    pub subscriber_bind: Fr,
    // private
    pub cap: Fr,
    pub r_cap: Fr,
    pub drawn: Fr,
    pub r_k: Fr,
    pub channel_id: Fr,
    pub pk_x: Fr,
    pub pk_y: Fr,
    pub sig_r_x: Fr,
    pub sig_r_y: Fr,
    pub s_lo: Fr,
    pub s_hi: Fr,
    pub merchant_pk: Fr,
    pub m_salt: Fr,
    pub merchant_blinding: Fr,
    pub merchant_rho: Fr,
    pub subscriber_pk: Fr,
    pub s_salt: Fr,
    pub subscriber_blinding: Fr,
    pub subscriber_rho: Fr,
}

pub struct ChannelCloseInputs {
    pub domain_sep: Fr,
    pub asset_tag: Fr,
    pub epoch: Fr,
    pub valid_after_ledger: u64,
    pub channel_id: u64,
    /// The hidden cap and its Pedersen blinding (opened at close).
    pub cap: u64,
    pub r_cap: Fr,
    /// The drawn cumulative amount and its blinding (the subscriber-signed commitment).
    pub drawn: u64,
    pub r_k: Fr,
    /// The per-channel signing public key and the subscriber's signature over the period.
    pub pk: Point,
    pub sig: Signature,
    /// Merchant output (the draw): owner pk + salt (binds merchant_bind) + note randomness.
    pub merchant_pk: Fr,
    pub m_salt: Fr,
    pub merchant_blinding: Fr,
    pub merchant_rho: Fr,
    /// Subscriber output (the remainder): owner pk + salt (binds subscriber_bind) + note randomness.
    pub subscriber_pk: Fr,
    pub s_salt: Fr,
    pub subscriber_blinding: Fr,
    pub subscriber_rho: Fr,
}

impl ChannelCloseWitness {
    pub fn build(h: &Hasher, p: ChannelCloseInputs) -> ChannelCloseWitness {
        let cap = Fr::from_u64(p.cap);
        let drawn = Fr::from_u64(p.drawn);
        let remainder = Fr::from_u64(p.cap - p.drawn); // caller validates drawn <= cap
        let c_cap = pedersen::commit(&cap, &p.r_cap);
        let (pk_x, pk_y) = pedersen::coords(&p.pk);
        let (sig_r_x, sig_r_y) = pedersen::coords(&p.sig.r);
        ChannelCloseWitness {
            domain_sep: p.domain_sep,
            asset_tag: p.asset_tag,
            epoch: p.epoch,
            cap_hash: pedersen::point_hash(h, &c_cap),
            auth_key: pedersen::point_hash(h, &p.pk),
            valid_after_ledger: Fr::from_u64(p.valid_after_ledger),
            merchant_out: h.commitment(&drawn, &p.asset_tag, &p.merchant_pk, &p.merchant_blinding, &p.epoch, &p.merchant_rho),
            subscriber_out: h.commitment(&remainder, &p.asset_tag, &p.subscriber_pk, &p.subscriber_blinding, &p.epoch, &p.subscriber_rho),
            merchant_bind: h.escrow_bind(DOMAIN_CHANNEL_MERCHANT, &p.merchant_pk, &p.m_salt),
            subscriber_bind: h.escrow_bind(DOMAIN_ESCROW_REFUND, &p.subscriber_pk, &p.s_salt),
            cap,
            r_cap: p.r_cap,
            drawn,
            r_k: p.r_k,
            channel_id: Fr::from_u64(p.channel_id),
            pk_x,
            pk_y,
            sig_r_x,
            sig_r_y,
            s_lo: p.sig.s_lo,
            s_hi: p.sig.s_hi,
            merchant_pk: p.merchant_pk,
            m_salt: p.m_salt,
            merchant_blinding: p.merchant_blinding,
            merchant_rho: p.merchant_rho,
            subscriber_pk: p.subscriber_pk,
            s_salt: p.s_salt,
            subscriber_blinding: p.subscriber_blinding,
            subscriber_rho: p.subscriber_rho,
        }
    }

    pub fn to_prover_toml(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("domain_sep = {}\n", q(&self.domain_sep)));
        s.push_str(&format!("asset_tag = {}\n", q(&self.asset_tag)));
        s.push_str(&format!("epoch = {}\n", q(&self.epoch)));
        s.push_str(&format!("cap_hash = {}\n", q(&self.cap_hash)));
        s.push_str(&format!("auth_key = {}\n", q(&self.auth_key)));
        s.push_str(&format!("valid_after_ledger = {}\n", q(&self.valid_after_ledger)));
        s.push_str(&format!("merchant_out = {}\n", q(&self.merchant_out)));
        s.push_str(&format!("subscriber_out = {}\n", q(&self.subscriber_out)));
        s.push_str(&format!("merchant_bind = {}\n", q(&self.merchant_bind)));
        s.push_str(&format!("subscriber_bind = {}\n", q(&self.subscriber_bind)));
        s.push_str(&format!("cap = {}\n", q(&self.cap)));
        s.push_str(&format!("r_cap = {}\n", q(&self.r_cap)));
        s.push_str(&format!("drawn = {}\n", q(&self.drawn)));
        s.push_str(&format!("r_k = {}\n", q(&self.r_k)));
        s.push_str(&format!("channel_id = {}\n", q(&self.channel_id)));
        s.push_str(&format!("s_lo = {}\n", q(&self.s_lo)));
        s.push_str(&format!("s_hi = {}\n", q(&self.s_hi)));
        s.push_str(&format!("merchant_pk = {}\n", q(&self.merchant_pk)));
        s.push_str(&format!("m_salt = {}\n", q(&self.m_salt)));
        s.push_str(&format!("merchant_blinding = {}\n", q(&self.merchant_blinding)));
        s.push_str(&format!("merchant_rho = {}\n", q(&self.merchant_rho)));
        s.push_str(&format!("subscriber_pk = {}\n", q(&self.subscriber_pk)));
        s.push_str(&format!("s_salt = {}\n", q(&self.s_salt)));
        s.push_str(&format!("subscriber_blinding = {}\n", q(&self.subscriber_blinding)));
        s.push_str(&format!("subscriber_rho = {}\n", q(&self.subscriber_rho)));
        s.push_str("\n[pk]\n");
        s.push_str(&format!("x = {}\n", q(&self.pk_x)));
        s.push_str(&format!("y = {}\n", q(&self.pk_y)));
        s.push_str("is_infinite = false\n");
        s.push_str("\n[sig_r]\n");
        s.push_str(&format!("x = {}\n", q(&self.sig_r_x)));
        s.push_str(&format!("y = {}\n", q(&self.sig_r_y)));
        s.push_str("is_infinite = false\n");
        s
    }
}

// ----------------------------- Spend assembly + demos -----------------------------

/// A note this wallet owns and can spend (the spendable inputs to a transfer/withdraw).
pub struct SpendNote {
    pub value: u64,
    pub blinding: Fr,
    pub epoch: Fr,
    pub rho: Fr,
    /// The note's leaf index in the live commitment tree.
    pub leaf_index: usize,
}

/// Assemble a real input: compute its commitment path + ASP path from live leaf sets,
/// pairing it with the accumulator-insertion witness.
fn real_input(
    h: &Hasher,
    note: &SpendNote,
    commitment_leaves: &[Fr],
    asp_leaves: &[Fr],
    owner_pk: Fr,
    nf: NfInsert,
) -> InputWitness {
    let asp_index = asp_leaves
        .iter()
        .position(|k| *k == owner_pk)
        .expect("owner_pk must be in the approved set");
    InputWitness {
        value: Fr::from_u64(note.value),
        blinding: note.blinding,
        epoch: note.epoch,
        rho: note.rho,
        is_dummy: false,
        cm: commitment_path(h, commitment_leaves, note.leaf_index),
        asp: commitment_path(h, asp_leaves, asp_index),
        nf,
    }
}

/// Assemble a dummy input (value 0): skips membership, still inserts its nullifier.
fn dummy_input(epoch: Fr, rho: Fr, nf: NfInsert) -> InputWitness {
    InputWitness {
        value: Fr::ZERO,
        blinding: Fr::ZERO,
        epoch,
        rho,
        is_dummy: true,
        cm: MerklePath::zero(),
        asp: MerklePath::zero(),
        nf,
    }
}

/// Everything needed to build a real 1-real + 1-dummy `transfer` against LIVE pool
/// state. `commitment_leaves` is the full ordered commitment set (containing the
/// spent note at `note_leaf_index`); `asp_leaves` is the approved set (containing the
/// spender's `owner_pk`); `prior_nullifiers` is the spent set in insertion order. The
/// change goes back to the spender; conservation requires `out0_value <= note_value`.
pub struct TransferInputs<'a> {
    pub owner_sk: Fr,
    pub asset_tag: Fr,
    /// The CURRENT epoch — the transfer's public `epoch` input + output-note stamp.
    pub epoch: Fr,
    /// The epoch the spent note was minted under (may predate `epoch`); the input
    /// note's commitment is recomputed with THIS, so it matches its on-chain leaf.
    pub note_epoch: Fr,
    pub domain_sep: Fr,
    pub note_value: u64,
    pub note_blinding: Fr,
    pub note_rho: Fr,
    pub note_leaf_index: usize,
    pub commitment_leaves: &'a [Fr],
    pub asp_leaves: &'a [Fr],
    pub prior_nullifiers: &'a [Fr],
    pub dummy_rho: Fr,
    pub recipient_owner_pk: Fr,
    pub out0_value: u64,
    pub out0_blinding: Fr,
    pub out0_rho: Fr,
    pub change_blinding: Fr,
    pub change_rho: Fr,
}

impl TransferWitness {
    /// Build a transfer witness against arbitrary live state — the stateful witness
    /// generator. Computes the input's commitment + ASP Merkle paths over the live
    /// leaf sets and the chained two-insertion accumulator witness over the live
    /// nullifier set.
    pub fn build(h: &Hasher, p: TransferInputs) -> TransferWitness {
        let owner_pk = h.owner_pk(&p.owner_sk);
        let nf0 = h.nullifier(&p.note_rho, &p.owner_sk);
        let nf1 = h.nullifier(&p.dummy_rho, &p.owner_sk);
        let (nf_old, nf_new, w0, w1) =
            build_two_insertions(h, p.prior_nullifiers, nf0, nf1);

        let real_note = SpendNote {
            value: p.note_value,
            blinding: p.note_blinding,
            epoch: p.note_epoch,
            rho: p.note_rho,
            leaf_index: p.note_leaf_index,
        };
        let real_in = real_input(h, &real_note, p.commitment_leaves, p.asp_leaves, owner_pk, w0);
        let dummy_in = dummy_input(p.epoch, p.dummy_rho, w1);

        let out0 = OutputWitness {
            value: Fr::from_u64(p.out0_value),
            owner_pk: p.recipient_owner_pk,
            blinding: p.out0_blinding,
            rho: p.out0_rho,
        };
        let change_value = p.note_value - p.out0_value;
        let out1 = OutputWitness {
            value: Fr::from_u64(change_value),
            owner_pk,
            blinding: p.change_blinding,
            rho: p.change_rho,
        };

        TransferWitness {
            domain_sep: p.domain_sep,
            asset_tag: p.asset_tag,
            epoch: p.epoch,
            commitment_root: real_in.cm.root,
            nullifier_old_root: nf_old,
            nullifier_new_root: nf_new,
            nullifiers: [nf0, nf1],
            out_commitments: [
                out0.commitment(h, &p.asset_tag, &p.epoch),
                out1.commitment(h, &p.asset_tag, &p.epoch),
            ],
            asp_root: real_in.asp.root,
            owner_sk: p.owner_sk,
            inputs: [real_in, dummy_in],
            outputs: [out0, out1],
        }
    }

    /// Reproduces the Noir `transfer::demo_witness_at` exactly (owner_sk 12345,
    /// 1000 → 600 recipient + 400 change, fresh pool) via [`TransferWitness::build`].
    /// Cross-checked against the circuit's own witness; also the fresh-pool proving path.
    pub fn demo(h: &Hasher, epoch: u32, domain_sep: Fr) -> TransferWitness {
        let asset_tag = Fr::from_u64(1);
        let epoch_f = Fr::from_u64(epoch as u64);
        let owner_sk = Fr::from_u64(12345);
        let owner_pk = h.owner_pk(&owner_sk);
        let in_commitment = h.commitment(
            &Fr::from_u64(1000),
            &asset_tag,
            &owner_pk,
            &Fr::from_u64(777),
            &epoch_f,
            &Fr::from_u64(111),
        );
        TransferWitness::build(
            h,
            TransferInputs {
                owner_sk,
                asset_tag,
                epoch: epoch_f,
                note_epoch: epoch_f,
                domain_sep,
                note_value: 1000,
                note_blinding: Fr::from_u64(777),
                note_rho: Fr::from_u64(111),
                note_leaf_index: 0,
                commitment_leaves: &[in_commitment],
                asp_leaves: &[owner_pk],
                prior_nullifiers: &[],
                dummy_rho: Fr::from_u64(0xdead),
                recipient_owner_pk: h.owner_pk(&Fr::from_u64(99)),
                out0_value: 600,
                out0_blinding: Fr::from_u64(222),
                out0_rho: Fr::from_u64(333),
                change_blinding: Fr::from_u64(444),
                change_rho: Fr::from_u64(555),
            },
        )
    }
}

// ----------------------------- Split (2-in / 8-out) -----------------------------

/// Number of outputs in the split circuit (up to `N_SPLIT_OUTPUTS - 1` recipients + change).
pub const N_SPLIT_OUTPUTS: usize = 6;

pub struct SplitWitness {
    pub domain_sep: Fr,
    pub asset_tag: Fr,
    pub epoch: Fr,
    pub commitment_root: Fr,
    pub nullifier_old_root: Fr,
    pub nullifier_new_root: Fr,
    pub nullifiers: [Fr; 2],
    pub out_commitments: [Fr; N_SPLIT_OUTPUTS],
    pub asp_root: Fr,
    pub owner_sk: Fr,
    pub inputs: [InputWitness; 2],
    pub outputs: [OutputWitness; N_SPLIT_OUTPUTS],
}

impl SplitWitness {
    pub fn to_prover_toml(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("domain_sep = {}\n", q(&self.domain_sep)));
        s.push_str(&format!("asset_tag = {}\n", q(&self.asset_tag)));
        s.push_str(&format!("epoch = {}\n", q(&self.epoch)));
        s.push_str(&format!("commitment_root = {}\n", q(&self.commitment_root)));
        s.push_str(&format!("nullifier_old_root = {}\n", q(&self.nullifier_old_root)));
        s.push_str(&format!("nullifier_new_root = {}\n", q(&self.nullifier_new_root)));
        s.push_str(&format!("nullifiers = {}\n", fr_array(&self.nullifiers)));
        s.push_str(&format!("out_commitments = {}\n", fr_array(&self.out_commitments)));
        s.push_str(&format!("asp_root = {}\n", q(&self.asp_root)));
        s.push_str(&format!("owner_sk = {}\n", q(&self.owner_sk)));
        s.push_str(&input_block(&self.inputs[0]));
        s.push_str(&input_block(&self.inputs[1]));
        for o in &self.outputs {
            s.push_str(&output_block(o));
        }
        s
    }
}

/// Per-output-slot data the send flow needs to encrypt a split output payload (value +
/// the same blinding/rho the witness used for that slot).
pub struct SplitOutMeta {
    pub value: u64,
    pub blinding: Fr,
    pub rho: Fr,
}

/// A split's recipient outputs (each = recipient `owner_pk` + value), plus the blinding/
/// rho randomness for every output slot (recipients, then change, then padding dummies).
pub struct SplitInputs<'a> {
    pub owner_sk: Fr,
    pub asset_tag: Fr,
    pub epoch: Fr,
    pub note_epoch: Fr,
    pub domain_sep: Fr,
    pub note_value: u64,
    pub note_blinding: Fr,
    pub note_rho: Fr,
    pub note_leaf_index: usize,
    pub commitment_leaves: &'a [Fr],
    pub asp_leaves: &'a [Fr],
    pub prior_nullifiers: &'a [Fr],
    pub dummy_rho: Fr,
    /// Recipient `(owner_pk, value)` pairs (1..=N_SPLIT_OUTPUTS-1). Change is automatic.
    pub recipients: &'a [(Fr, u64)],
    /// Per-output-slot (blinding, rho), length `N_SPLIT_OUTPUTS`, ordered:
    /// recipients…, change, then padding dummies.
    pub out_rand: &'a [(Fr, Fr); N_SPLIT_OUTPUTS],
}

impl SplitWitness {
    /// Build a split witness against live state: spend one owned note, pay each recipient,
    /// return change to the sender, and pad the remaining output slots with value-0 dummy
    /// notes (owned by the sender) so the on-chain footprint is always 8 outputs.
    pub fn build(h: &Hasher, p: SplitInputs) -> SplitWitness {
        assert!(
            !p.recipients.is_empty() && p.recipients.len() <= N_SPLIT_OUTPUTS - 1,
            "1..=7 recipients"
        );
        let owner_pk = h.owner_pk(&p.owner_sk);
        let nf0 = h.nullifier(&p.note_rho, &p.owner_sk);
        let nf1 = h.nullifier(&p.dummy_rho, &p.owner_sk);
        let (nf_old, nf_new, w0, w1) = build_two_insertions(h, p.prior_nullifiers, nf0, nf1);

        let real_note = SpendNote {
            value: p.note_value,
            blinding: p.note_blinding,
            epoch: p.note_epoch,
            rho: p.note_rho,
            leaf_index: p.note_leaf_index,
        };
        let real_in = real_input(h, &real_note, p.commitment_leaves, p.asp_leaves, owner_pk, w0);
        let dummy_in = dummy_input(p.epoch, p.dummy_rho, w1);

        let paid: u64 = p.recipients.iter().map(|(_, v)| *v).sum();
        let change_value = p.note_value - paid; // caller validates paid <= note_value

        // Build the 8 output slots: recipients, change, then value-0 dummies (to self).
        let mut outputs: Vec<OutputWitness> = Vec::with_capacity(N_SPLIT_OUTPUTS);
        for (i, (rpk, val)) in p.recipients.iter().enumerate() {
            outputs.push(OutputWitness {
                value: Fr::from_u64(*val),
                owner_pk: *rpk,
                blinding: p.out_rand[i].0,
                rho: p.out_rand[i].1,
            });
        }
        let change_slot = p.recipients.len();
        outputs.push(OutputWitness {
            value: Fr::from_u64(change_value),
            owner_pk,
            blinding: p.out_rand[change_slot].0,
            rho: p.out_rand[change_slot].1,
        });
        for i in (change_slot + 1)..N_SPLIT_OUTPUTS {
            outputs.push(OutputWitness {
                value: Fr::ZERO,
                owner_pk,
                blinding: p.out_rand[i].0,
                rho: p.out_rand[i].1,
            });
        }
        let outputs: [OutputWitness; N_SPLIT_OUTPUTS] =
            outputs.try_into().unwrap_or_else(|_| unreachable!("exactly 8 outputs"));

        let out_commitments: [Fr; N_SPLIT_OUTPUTS] = std::array::from_fn(|j| {
            outputs[j].commitment(h, &p.asset_tag, &p.epoch)
        });

        SplitWitness {
            domain_sep: p.domain_sep,
            asset_tag: p.asset_tag,
            epoch: p.epoch,
            commitment_root: real_in.cm.root,
            nullifier_old_root: nf_old,
            nullifier_new_root: nf_new,
            nullifiers: [nf0, nf1],
            out_commitments,
            asp_root: real_in.asp.root,
            owner_sk: p.owner_sk,
            inputs: [real_in, dummy_in],
            outputs,
        }
    }
}

/// Everything needed to build a real 1-real + 1-dummy `withdraw` against LIVE pool
/// state: spend one owned note, release `amount` to a public dest (bound by
/// `dest_bind`), keep `note_value - amount` as shielded change back to the spender.
pub struct WithdrawInputs<'a> {
    pub owner_sk: Fr,
    pub asset_tag: Fr,
    /// Current epoch — the public `epoch` input + change-note stamp.
    pub epoch: Fr,
    /// The spent note's mint epoch (recompute its commitment to match its leaf).
    pub note_epoch: Fr,
    pub domain_sep: Fr,
    pub note_value: u64,
    pub note_blinding: Fr,
    pub note_rho: Fr,
    pub note_leaf_index: usize,
    pub commitment_leaves: &'a [Fr],
    pub asp_leaves: &'a [Fr],
    pub prior_nullifiers: &'a [Fr],
    pub dummy_rho: Fr,
    pub amount: u64,
    pub dest_bind: Fr,
    pub change_blinding: Fr,
    pub change_rho: Fr,
}

impl WithdrawWitness {
    /// Build a withdraw witness against arbitrary live state — the stateful generator
    /// for withdraws (commitment + ASP paths over the live leaf sets, chained
    /// two-insertion accumulator witness over the live nullifier set).
    pub fn build(h: &Hasher, p: WithdrawInputs) -> WithdrawWitness {
        let owner_pk = h.owner_pk(&p.owner_sk);
        let nf0 = h.nullifier(&p.note_rho, &p.owner_sk);
        let nf1 = h.nullifier(&p.dummy_rho, &p.owner_sk);
        let (nf_old, nf_new, w0, w1) =
            build_two_insertions(h, p.prior_nullifiers, nf0, nf1);

        let real_note = SpendNote {
            value: p.note_value,
            blinding: p.note_blinding,
            epoch: p.note_epoch,
            rho: p.note_rho,
            leaf_index: p.note_leaf_index,
        };
        let real_in = real_input(h, &real_note, p.commitment_leaves, p.asp_leaves, owner_pk, w0);
        let dummy_in = dummy_input(p.epoch, p.dummy_rho, w1);

        let change = OutputWitness {
            value: Fr::from_u64(p.note_value - p.amount),
            owner_pk,
            blinding: p.change_blinding,
            rho: p.change_rho,
        };

        WithdrawWitness {
            domain_sep: p.domain_sep,
            asset_tag: p.asset_tag,
            epoch: p.epoch,
            commitment_root: real_in.cm.root,
            nullifier_old_root: nf_old,
            nullifier_new_root: nf_new,
            nullifiers: [nf0, nf1],
            change_commitment: change.commitment(h, &p.asset_tag, &p.epoch),
            asp_root: real_in.asp.root,
            amount: Fr::from_u64(p.amount),
            dest_bind: p.dest_bind,
            owner_sk: p.owner_sk,
            inputs: [real_in, dummy_in],
            change,
        }
    }

    /// Reproduces the Noir `withdraw::demo_witness_at` (spend 1000, release 700, keep
    /// 300 change, fresh pool) via [`WithdrawWitness::build`].
    pub fn demo(h: &Hasher, epoch: u32, domain_sep: Fr) -> WithdrawWitness {
        let asset_tag = Fr::from_u64(1);
        let epoch_f = Fr::from_u64(epoch as u64);
        let owner_sk = Fr::from_u64(12345);
        let owner_pk = h.owner_pk(&owner_sk);
        let in_commitment = h.commitment(
            &Fr::from_u64(1000),
            &asset_tag,
            &owner_pk,
            &Fr::from_u64(777),
            &epoch_f,
            &Fr::from_u64(111),
        );
        // dest_bind = Poseidon(0xde57, 0xbeef) — the demo's placeholder destination.
        let dest_bind = h.hash(&[Fr::from_u64(0xde57), Fr::from_u64(0xbeef)]);
        WithdrawWitness::build(
            h,
            WithdrawInputs {
                owner_sk,
                asset_tag,
                epoch: epoch_f,
                note_epoch: epoch_f,
                domain_sep,
                note_value: 1000,
                note_blinding: Fr::from_u64(777),
                note_rho: Fr::from_u64(111),
                note_leaf_index: 0,
                commitment_leaves: &[in_commitment],
                asp_leaves: &[owner_pk],
                prior_nullifiers: &[],
                dummy_rho: Fr::from_u64(0xdead),
                amount: 700,
                dest_bind,
                change_blinding: Fr::from_u64(444),
                change_rho: Fr::from_u64(555),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Known-good values from the Noir witgen (`circuits/transfer/Prover.toml`,
    // epoch 28 / pool 7 / net 42 → domain_sep 0x2eae4c…). If the native generator
    // matches these, it reproduces the circuit's witness construction.
    const DOMAIN_SEP_28: &str =
        "0x2eae4c361f605c06c766cb126a391a0f916308610ae8128f7e615e5e6b6c67ff";
    const COMMITMENT_ROOT: &str =
        "0x16c6e766b9ecd7bcbaede4a371f17104130d1e65794c63cb3b91a5f1323b608e";
    const NF_OLD_ROOT: &str =
        "0x0b37aab3b422b76af67794571198d0e42366b64b3ee779b72d72e81f918a4994";
    const NF_NEW_ROOT: &str =
        "0x0707f83270a0a589955561c94717d2bc057863c4c53e4816b7cae4e619f25aef";
    const ASP_ROOT: &str =
        "0x1610446d123b3be5a338712bcf508007d94184a71cb8045dd351cbd68a52b8dd";
    const OUT0_OWNER_PK: &str =
        "0x0a93fc7a00fc720755a3fbd3cb518bd8e8b44e5f6d6e905b04bf9158ddc01f05";
    const NF0: &str = "0x241b4b895b399d1a691d73f92fca35371be3a575176f4eb709cd1a0cab73e2aa";

    #[test]
    fn transfer_demo_matches_noir_witgen() {
        let h = Hasher::new();
        let w = TransferWitness::demo(&h, 28, Fr::from_hex(DOMAIN_SEP_28).unwrap());
        assert_eq!(w.commitment_root.to_hex(), COMMITMENT_ROOT, "commitment_root");
        assert_eq!(w.nullifier_old_root.to_hex(), NF_OLD_ROOT, "nf_old_root");
        assert_eq!(w.nullifier_new_root.to_hex(), NF_NEW_ROOT, "nf_new_root");
        assert_eq!(w.asp_root.to_hex(), ASP_ROOT, "asp_root");
        assert_eq!(w.nullifiers[0].to_hex(), NF0, "nullifier_0");
        assert_eq!(w.outputs[0].owner_pk.to_hex(), OUT0_OWNER_PK, "recipient owner_pk");
    }

    #[test]
    fn empty_accumulator_root_is_canonical() {
        // A fresh accumulator's root is the FROZEN empty-root the pool seeds.
        let h = Hasher::new();
        let (old_root, _, _, _) = build_two_insertions(
            &h,
            &[],
            h.nullifier(&Fr::from_u64(111), &Fr::from_u64(12345)),
            h.nullifier(&Fr::from_u64(0xdead), &Fr::from_u64(12345)),
        );
        assert_eq!(old_root.to_hex(), NF_OLD_ROOT);
    }

    #[test]
    fn prover_toml_has_expected_shape() {
        let h = Hasher::new();
        let w = TransferWitness::demo(&h, 28, Fr::from_hex(DOMAIN_SEP_28).unwrap());
        let toml = w.to_prover_toml();
        assert!(toml.contains("owner_sk = \"0x3039\""), "owner_sk 12345 = 0x3039");
        assert!(toml.contains("\n[[inputs]]\n"));
        assert!(toml.contains("is_dummy = true"));
        assert!(toml.contains("is_dummy = false"));
        assert!(toml.contains("[inputs.nf_low]"));
        assert!(toml.matches("[[outputs]]").count() == 2);
        // The real input's nf_low is the init tail leaf {0,0,0}.
        assert!(toml.contains("nf_new_index = \"0x01\""));
    }

    #[test]
    fn stateful_path_matches_single_leaf_helper() {
        // commitment_path over a 1-leaf set == the single_leaf_tree helper.
        let h = Hasher::new();
        let leaf = Fr::from_u64(42);
        let a = single_leaf_tree(&h, leaf);
        let b = commitment_path(&h, &[leaf], 0);
        assert_eq!(a.root, b.root);
        assert_eq!(a.siblings, b.siblings);
    }

    /// Build a split witness paying 3 recipients (250/250/250) from a 1000-note, change
    /// 250, padded to 8 outputs.
    fn split_demo(h: &Hasher) -> SplitWitness {
        let owner_sk = Fr::from_u64(12345);
        let owner_pk = h.owner_pk(&owner_sk);
        let asset_tag = Fr::from_u64(1);
        let epoch = Fr::from_u64(28);
        let in_cm = h.commitment(&Fr::from_u64(1000), &asset_tag, &owner_pk, &Fr::from_u64(777), &epoch, &Fr::from_u64(111));
        let recips = [
            (h.owner_pk(&Fr::from_u64(91)), 250u64),
            (h.owner_pk(&Fr::from_u64(92)), 250),
            (h.owner_pk(&Fr::from_u64(93)), 250),
        ];
        let out_rand: [(Fr, Fr); N_SPLIT_OUTPUTS] =
            std::array::from_fn(|i| (Fr::from_u64(100 + i as u64), Fr::from_u64(200 + i as u64)));
        SplitWitness::build(
            h,
            SplitInputs {
                owner_sk,
                asset_tag,
                epoch,
                note_epoch: epoch,
                domain_sep: Fr::from_hex(DOMAIN_SEP_28).unwrap(),
                note_value: 1000,
                note_blinding: Fr::from_u64(777),
                note_rho: Fr::from_u64(111),
                note_leaf_index: 0,
                commitment_leaves: &[in_cm],
                asp_leaves: &[owner_pk],
                prior_nullifiers: &[],
                dummy_rho: Fr::from_u64(0xdead),
                recipients: &recips,
                out_rand: &out_rand,
            },
        )
    }

    #[test]
    fn split_conserves_value_and_pads_to_eight() {
        let h = Hasher::new();
        let w = split_demo(&h);
        // 8 outputs, all out-commitments populated.
        assert_eq!(w.outputs.len(), N_SPLIT_OUTPUTS);
        assert_eq!(w.out_commitments.len(), N_SPLIT_OUTPUTS);
        // Conservation: 3*250 recipients + 250 change + 0*4 dummies == 1000.
        let out_sum: u64 = w.outputs.iter().map(|o| o.value.to_decimal().parse::<u64>().unwrap()).sum();
        assert_eq!(out_sum, 1000);
        // slots: 3 recipients, slot 3 = change (250), slots 4..8 = 0 (dummy, to self).
        assert_eq!(w.outputs[3].value.to_decimal(), "250");
        assert_eq!(w.outputs[3].owner_pk, h.owner_pk(&Fr::from_u64(12345)));
        for o in &w.outputs[4..] {
            assert_eq!(o.value.to_decimal(), "0");
        }
        // commitments match the output notes.
        for j in 0..N_SPLIT_OUTPUTS {
            assert_eq!(w.out_commitments[j], w.outputs[j].commitment(&h, &w.asset_tag, &w.epoch));
        }
    }

    #[test]
    fn split_prover_toml_has_n_outputs() {
        let h = Hasher::new();
        let toml = split_demo(&h).to_prover_toml();
        assert_eq!(toml.matches("[[outputs]]").count(), N_SPLIT_OUTPUTS);
        assert!(toml.contains("owner_sk = \"0x3039\""));
        assert!(toml.contains("[inputs.nf_low]"));
    }

    // ----- escrow witness parity (vs the Noir demos, captured 2026-06-23) -----
    // The escrow-specific public values of `notes::escrow::{contribute_demo, payout_demo}`
    // (epoch 5, domain_sep 0xabc). If the native builders reproduce these, they match the
    // circuit's derivation of the Pedersen + binding fields.

    use super::super::poseidon::DOMAIN_ESCROW_PAYEE;

    /// Reproduces the Noir `contribute_demo_at` via [`ContributeWitness::build`].
    fn contribute_demo(h: &Hasher) -> ContributeWitness {
        let asset_tag = Fr::from_u64(1);
        let epoch = Fr::from_u64(5);
        let owner_sk = Fr::from_u64(12345);
        let owner_pk = h.owner_pk(&owner_sk);
        let in_commitment = h.commitment(
            &Fr::from_u64(1000), &asset_tag, &owner_pk, &Fr::from_u64(777), &epoch, &Fr::from_u64(111),
        );
        ContributeWitness::build(
            h,
            ContributeInputs {
                owner_sk,
                asset_tag,
                epoch,
                note_epoch: epoch,
                domain_sep: Fr::from_u64(0xabc),
                note_value: 1000,
                note_blinding: Fr::from_u64(777),
                note_rho: Fr::from_u64(111),
                note_leaf_index: 0,
                commitment_leaves: &[in_commitment],
                asp_leaves: &[owner_pk],
                prior_nullifiers: &[],
                dummy_rho: Fr::from_u64(0xdead),
                amount: 700,
                blinding_r: Fr::from_u64(0xb11d),
                contrib_salt: Fr::from_u64(0x5a17),
                change_blinding: Fr::from_u64(444),
                change_rho: Fr::from_u64(555),
                p_old: super::super::pedersen::Point::identity(),
            },
        )
    }

    #[test]
    fn contribute_witness_matches_noir_demo() {
        let h = Hasher::new();
        let w = contribute_demo(&h);
        assert_eq!(w.c_raised_old.to_hex(),
            "0x0b63a53787021a4a962a452c2921b3663aff1ffd8d5510540f8e659e782956f1", "c_raised_old");
        assert_eq!(w.c_raised_new.to_hex(),
            "0x0467924615dd23f09f9b1ab6aaee224abe0ca3febf75aa9842df18d70422e449", "c_raised_new");
        assert_eq!(w.c_contrib.to_hex(),
            "0x0467924615dd23f09f9b1ab6aaee224abe0ca3febf75aa9842df18d70422e449", "c_contrib");
        assert_eq!(w.refund_bind.to_hex(),
            "0x2cb9678816a794efd76a759b84fb2b965cfbbe6a67ab2d4cb8f522f00758ffeb", "refund_bind");
        assert_eq!(w.change_commitment.to_hex(),
            "0x27f831ed3b6cd55e06303ffd1e3836e4a60119de767c1caeb859b96e737c02e3", "change_commitment");
        // Prover.toml carries the point table + the new escrow fields.
        let toml = w.to_prover_toml();
        assert!(toml.contains("[p_old]"));
        assert!(toml.contains("is_infinite = true"));
        assert!(toml.contains("c_raised_new ="));
    }

    #[test]
    fn payout_witness_matches_noir_demo() {
        let h = Hasher::new();
        let w = PayoutWitness::build(
            &h,
            PayoutInputs {
                domain_sep: Fr::from_u64(0xabc),
                asset_tag: Fr::from_u64(1),
                epoch: Fr::from_u64(5),
                floor: 500,
                domain_bind: DOMAIN_ESCROW_PAYEE,
                recipient_sk: Fr::from_u64(99),
                value: 700,
                blinding_r: Fr::from_u64(0xb11d),
                out_blinding: Fr::from_u64(222),
                out_rho: Fr::from_u64(333),
                salt: Fr::from_u64(0x9a17),
            },
        );
        assert_eq!(w.commitment_hash.to_hex(),
            "0x0467924615dd23f09f9b1ab6aaee224abe0ca3febf75aa9842df18d70422e449", "commitment_hash");
        assert_eq!(w.out_commitment.to_hex(),
            "0x2940974a422a3491d0d2872653520132b05a40b48e2570daeeaa6380e7ad7a17", "out_commitment");
        assert_eq!(w.recipient_bind.to_hex(),
            "0x1b1f9287484b321f50a6fd56b221a4d5fcb7e9eba6802fed96ec5a3005372a68", "recipient_bind");
    }

    // ----- channel close witness parity (vs the Noir `channel::close_demo`, captured 2026-06-24) -----

    /// Reproduces the Noir `channel::close_demo` via [`ChannelCloseWitness::build`] — including the
    /// Schnorr signature the native signer produces (sk=0x1234567, k=0x89abcdef over the period msg).
    /// If the publics match the circuit's, the whole sign+witness path is parity-correct.
    #[test]
    fn channel_close_witness_matches_noir_demo() {
        let h = Hasher::new();
        let sk = Fr::from_hex("0x1234567").unwrap();
        let k = Fr::from_hex("0x89abcdef").unwrap();
        let drawn = 600u64;
        let r_k = Fr::from_hex("0xd4a").unwrap();
        // msg = Poseidon2([channel_id, valid_after, c_k.x, c_k.y]); c_k = commit(drawn, r_k).
        let c_k = pedersen::commit(&Fr::from_u64(drawn), &r_k);
        let (ckx, cky) = pedersen::coords(&c_k);
        let msg = h.hash(&[Fr::from_u64(1), Fr::from_u64(50), ckx, cky]);
        let pk = pedersen::schnorr_pubkey(&sk);
        let sig = pedersen::schnorr_sign(&h, &sk, &k, &msg);

        let w = ChannelCloseWitness::build(
            &h,
            ChannelCloseInputs {
                domain_sep: Fr::from_u64(0xabc),
                asset_tag: Fr::from_u64(1),
                epoch: Fr::from_u64(5),
                valid_after_ledger: 50,
                channel_id: 1,
                cap: 1000,
                r_cap: Fr::from_hex("0xca9").unwrap(),
                drawn,
                r_k,
                pk,
                sig,
                merchant_pk: h.owner_pk(&Fr::from_hex("0x3e").unwrap()),
                m_salt: Fr::from_hex("0x3e17").unwrap(),
                merchant_blinding: Fr::from_u64(222),
                merchant_rho: Fr::from_u64(333),
                subscriber_pk: h.owner_pk(&Fr::from_hex("0x5b").unwrap()),
                s_salt: Fr::from_hex("0x5b17").unwrap(),
                subscriber_blinding: Fr::from_u64(444),
                subscriber_rho: Fr::from_u64(555),
            },
        );
        assert_eq!(w.cap_hash.to_hex(),
            "0x2c48a88806047b18b6867cf0e631230363d1fe9448c4acbb1e01a889edf061cc", "cap_hash");
        assert_eq!(w.auth_key.to_hex(),
            "0x2c39130bc310dab1eda97ffcccbd7501c76184929dffc76590e43f5b3bc3ebac", "auth_key");
        assert_eq!(w.merchant_out.to_hex(),
            "0x09b4963cc5f48a6dd0dce3d36d40842d5ed8e63c38bdc8e7a77151fdea00897c", "merchant_out");
        assert_eq!(w.subscriber_out.to_hex(),
            "0x0d62a2c142417b1c915340c7673af7f07f50b5b0b6629a04ddd43842f17966ca", "subscriber_out");
        assert_eq!(w.merchant_bind.to_hex(),
            "0x2e2d92ed22efd6667e8be87dd7fc6ff8fcde8f99e2ea9a669cf13a8c7496a651", "merchant_bind");
        assert_eq!(w.subscriber_bind.to_hex(),
            "0x058db364cf113a85a779cd11d6a3fca04b10378c53cb1f086cba56106001720e", "subscriber_bind");
        // The Prover.toml carries the signature point tables + the limb response.
        let toml = w.to_prover_toml();
        assert!(toml.contains("[pk]"));
        assert!(toml.contains("[sig_r]"));
        assert!(toml.contains("s_lo ="));
        assert!(toml.contains("channel_id = \"0x01\""));
    }
}
