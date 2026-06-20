//! Nullifier accumulator - indexed Merkle tree (Z1 spec section 4b, decision D3).
//!
//! Spent nullifiers live in a sorted linked list embedded in a Merkle tree. Only the
//! root is stored on-chain; a spend proves NON-membership against the root (via a
//! "low leaf" that brackets the new nullifier) and computes the post-insertion root.
//! This keeps nullifier storage flat (one root) regardless of how many are spent.
use crate::merkle::{compute_root, TREE_DEPTH};
use poseidon::poseidon2::Poseidon2;

/// Unfilled slots hash as the zero field (matches the empty-leaf convention).
pub global EMPTY_LEAF: Field = 0;

/// An indexed-tree leaf: a node in the value-sorted linked list.
pub struct IndexedLeaf {
    pub value: Field,
    pub next_index: Field,
    pub next_value: Field,
}

impl IndexedLeaf {
    pub fn hash(self) -> Field {
        Poseidon2::hash([self.value, self.next_index, self.next_value], 3)
    }
}

/// Prove `target` is NOT in the tree: a `low` leaf is a member and brackets `target`
/// (`low.value < target < low.next_value`, or `low` is the tail with `next_value == 0`).
pub fn assert_non_membership(
    target: Field,
    low: IndexedLeaf,
    low_is_right: [bool; TREE_DEPTH],
    low_siblings: [Field; TREE_DEPTH],
    root: Field,
) {
    // (1) the low leaf is in the tree
    assert(compute_root(low.hash(), low_is_right, low_siblings) == root);
    // (2) low.value < target
    assert(low.value.lt(target));
    // (3) target < low.next_value, unless low is the current tail (next_value == 0)
    let is_tail = low.next_value == 0;
    assert(is_tail | target.lt(low.next_value));
}

/// Insert `target` (a proven non-member) at the empty slot `new_index`, returning the
/// new root. Two-step: re-point the low leaf at the new leaf, then write the new leaf
/// (which inherits the low leaf's old successor) into the empty slot.
pub fn insert(
    target: Field,
    low: IndexedLeaf,
    low_is_right: [bool; TREE_DEPTH],
    low_siblings: [Field; TREE_DEPTH],
    old_root: Field,
    new_index: Field,
    new_is_right: [bool; TREE_DEPTH],
    new_siblings: [Field; TREE_DEPTH],
) -> Field {
    // Non-membership against the current root.
    assert_non_membership(target, low, low_is_right, low_siblings, old_root);

    // Step 1: low leaf now points at the new leaf.
    let low_updated = IndexedLeaf { value: low.value, next_index: new_index, next_value: target };
    let mid_root = compute_root(low_updated.hash(), low_is_right, low_siblings);

    // Step 2: the target slot must currently be empty (against mid_root), then write
    // the new leaf, which inherits the low leaf's previous successor.
    assert(compute_root(EMPTY_LEAF, new_is_right, new_siblings) == mid_root);
    let new_leaf = IndexedLeaf { value: target, next_index: low.next_index, next_value: low.next_value };
    compute_root(new_leaf.hash(), new_is_right, new_siblings)
}

#[test]
fn non_membership_accepts_in_range() {
    let low = IndexedLeaf { value: 10, next_index: 5, next_value: 20 };
    let siblings = [3; TREE_DEPTH];
    let path = [false; TREE_DEPTH];
    let root = compute_root(low.hash(), path, siblings);
    assert_non_membership(15, low, path, siblings, root); // 10 < 15 < 20
}

#[test]
fn non_membership_accepts_above_tail() {
    let tail = IndexedLeaf { value: 10, next_index: 0, next_value: 0 };
    let siblings = [3; TREE_DEPTH];
    let path = [false; TREE_DEPTH];
    let root = compute_root(tail.hash(), path, siblings);
    assert_non_membership(99, tail, path, siblings, root); // > tail, next_value == 0
}

#[test(should_fail)]
fn non_membership_rejects_in_existing_range_gap() {
    // target below the low leaf's value must be rejected.
    let low = IndexedLeaf { value: 10, next_index: 5, next_value: 20 };
    let siblings = [3; TREE_DEPTH];
    let path = [false; TREE_DEPTH];
    let root = compute_root(low.hash(), path, siblings);
    assert_non_membership(5, low, path, siblings, root); // 5 < low.value
}

#[test(should_fail)]
fn non_membership_rejects_above_range() {
    let low = IndexedLeaf { value: 10, next_index: 5, next_value: 20 };
    let siblings = [3; TREE_DEPTH];
    let path = [false; TREE_DEPTH];
    let root = compute_root(low.hash(), path, siblings);
    assert_non_membership(25, low, path, siblings, root); // 25 > next_value, not tail
}

#[test]
fn leaf_hash_is_binding() {
    let a = IndexedLeaf { value: 10, next_index: 5, next_value: 20 };
    let b = IndexedLeaf { value: 10, next_index: 5, next_value: 21 };
    assert(a.hash() != b.hash());
}
