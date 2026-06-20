//! Commitment-tree Merkle membership - Z1 spec section 4a.
//!
//! Binary Poseidon2 Merkle tree, depth 20 (FROZEN). The node hash defined here is
//! the CANONICAL definition; the Z4 pool contract must reproduce it exactly
//! (Noir<->Soroban Poseidon parity).
use poseidon::poseidon2::Poseidon2;

pub global TREE_DEPTH: u32 = 20;

/// Internal node hash for the commitment tree.
pub fn hash_node(left: Field, right: Field) -> Field {
    Poseidon2::hash([left, right], 2)
}

/// Recompute the Merkle root from a leaf and its authentication path.
/// `path_is_right[i] == true` means the current node sits on the RIGHT at level `i`
/// (its sibling is on the left); `false` means it is the left child.
pub fn compute_root(
    leaf: Field,
    path_is_right: [bool; TREE_DEPTH],
    siblings: [Field; TREE_DEPTH],
) -> Field {
    let mut current = leaf;
    for i in 0..TREE_DEPTH {
        let (l, r) = if path_is_right[i] {
            (siblings[i], current)
        } else {
            (current, siblings[i])
        };
        current = hash_node(l, r);
    }
    current
}

/// Constrain that `leaf` is a member of the tree with the given `root`.
pub fn assert_membership(
    leaf: Field,
    path_is_right: [bool; TREE_DEPTH],
    siblings: [Field; TREE_DEPTH],
    root: Field,
) {
    assert(compute_root(leaf, path_is_right, siblings) == root);
}

#[test]
fn membership_roundtrip() {
    let leaf = 42;
    let mut siblings = [0; TREE_DEPTH];
    for i in 0..TREE_DEPTH {
        siblings[i] = (i as Field) + 1;
    }
    let path_is_right = [false; TREE_DEPTH];
    let root = compute_root(leaf, path_is_right, siblings);
    // A path that recomputes `root` is accepted.
    assert_membership(leaf, path_is_right, siblings, root);
}

#[test]
fn membership_left_right_differ() {
    // Same leaf + siblings but flipped position must yield a different root.
    let leaf = 42;
    let mut siblings = [0; TREE_DEPTH];
    for i in 0..TREE_DEPTH {
        siblings[i] = (i as Field) + 1;
    }
    let as_left = compute_root(leaf, [false; TREE_DEPTH], siblings);
    let as_right = compute_root(leaf, [true; TREE_DEPTH], siblings);
    assert(as_left != as_right);
}

#[test(should_fail)]
fn membership_rejects_wrong_root() {
    let leaf = 42;
    let siblings = [7; TREE_DEPTH];
    let path_is_right = [false; TREE_DEPTH];
    let root = compute_root(leaf, path_is_right, siblings);
    assert_membership(leaf, path_is_right, siblings, root + 1);
}
