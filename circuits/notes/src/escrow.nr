//! Escrow circuits (building block B): hidden-sum invoice / multi-payer escrow.
//!
//! `verify_escrow_contribute` is withdraw-shaped (spend one owned note, shielded change) but
//! instead of releasing a public amount it folds the HIDDEN amount into a running Pedersen
//! commitment via the embedded (Grumpkin) curve: P_new = P_old + Commit(amount, r). The
//! contract only ever sees Poseidon(x, y) handles of these points, so it does no curve math.
//!
//! `verify_escrow_payout` opens a Pedersen commitment to a hidden value V, asserts V >= floor,
//! and mints a note of V to a recipient bound by `recipient_bind`. Release and refund share it
//! (release opens the running commitment with floor=target; refund opens a contribution with
//! floor=0). See claude-docs/escrow_interface.md.
use crate::transfer::{
    build_two_insertions, InputWitness, make_input, N_INPUTS, OutputWitness, single_leaf_tree,
    spend_inputs, zero_ladder,
};
use crate::{derive_owner_pk, nullifier, Note};
use poseidon::poseidon2::Poseidon2;
use std::embedded_curve_ops::EmbeddedCurvePoint;
use std::hash::pedersen_commitment;

// Domain tags for the payout bindings (ASCII; distinct so a refund proof can't pass as a
// release). Placeholders finalized before VK generation, like the other DOMAIN_* tags.
pub global DOMAIN_ESCROW_PAYEE: Field = 0x6f7a6b795f706179; // "ozky_pay"
pub global DOMAIN_ESCROW_REFUND: Field = 0x6f7a6b795f726566; // "ozky_ref"

/// Pedersen commitment Commit(v, r) = v*G0 + r*G1 over the embedded curve. Homomorphic across
/// the 2-slot layout: Commit(v0,r0) + Commit(v1,r1) == Commit(v0+v1, r0+r1).
pub fn commit(value: Field, blinding: Field) -> EmbeddedCurvePoint {
    pedersen_commitment([value, blinding])
}

/// On-chain handle for a commitment point: Poseidon2(x, y). Opaque to the contract.
pub fn point_hash(p: EmbeddedCurvePoint) -> Field {
    Poseidon2::hash([p.x, p.y], 2)
}

/// recipient_bind = Poseidon(domain, recipient_pk, salt). Only the holder of the bound key
/// (knowing salt) can mint a payout to it.
pub fn bind(domain: Field, recipient_pk: Field, salt: Field) -> Field {
    Poseidon2::hash([domain, recipient_pk, salt], 3)
}

/// The seed every escrow's running commitment starts from: the hash of the identity point
/// (an empty sum). The contract's `init_c_raised` must equal this.
pub fn empty_raised_hash() -> Field {
    point_hash(EmbeddedCurvePoint::point_at_infinity())
}

// ----------------------------- contribute -----------------------------

pub struct ContributeWitness {
    // public
    pub domain_sep: Field,
    pub asset_tag: Field,
    pub epoch: Field,
    pub commitment_root: Field,
    pub nullifier_old_root: Field,
    pub nullifier_new_root: Field,
    pub nullifiers: [Field; N_INPUTS],
    pub change_commitment: Field,
    pub asp_root: Field,
    pub c_raised_old: Field,
    pub c_raised_new: Field,
    pub c_contrib: Field,
    pub refund_bind: Field,
    // private
    pub owner_sk: Field,
    pub inputs: [InputWitness; N_INPUTS],
    pub change: OutputWitness,
    pub amount: Field,
    pub blinding_r: Field,
    pub contrib_salt: Field,
    pub p_old: EmbeddedCurvePoint,
}

#[allow(clippy::too_many_arguments)]
pub fn verify_escrow_contribute(
    domain_sep: Field,
    asset_tag: Field,
    epoch: Field,
    commitment_root: Field,
    nullifier_old_root: Field,
    nullifier_new_root: Field,
    nullifiers: [Field; N_INPUTS],
    change_commitment: Field,
    asp_root: Field,
    c_raised_old: Field,
    c_raised_new: Field,
    c_contrib: Field,
    refund_bind: Field,
    owner_sk: Field,
    inputs: [InputWitness; N_INPUTS],
    change: OutputWitness,
    amount: Field,
    blinding_r: Field,
    contrib_salt: Field,
    p_old: EmbeddedCurvePoint,
) {
    assert(domain_sep != 0);
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
    // conservation: spent inputs == hidden contribution + shielded change.
    assert(in_sum == amount + change.value);

    // Homomorphic fold of the hidden amount into the running commitment.
    let c = commit(amount, blinding_r);
    let p_new = p_old + c;
    assert(point_hash(p_old) == c_raised_old);
    assert(point_hash(p_new) == c_raised_new);
    assert(point_hash(c) == c_contrib);

    // Only the contributor (knowing contrib_salt + owner_sk) can later refund.
    assert(bind(DOMAIN_ESCROW_REFUND, owner_pk, contrib_salt) == refund_bind);
}

impl ContributeWitness {
    pub fn run(self) {
        verify_escrow_contribute(
            self.domain_sep,
            self.asset_tag,
            self.epoch,
            self.commitment_root,
            self.nullifier_old_root,
            self.nullifier_new_root,
            self.nullifiers,
            self.change_commitment,
            self.asp_root,
            self.c_raised_old,
            self.c_raised_new,
            self.c_contrib,
            self.refund_bind,
            self.owner_sk,
            self.inputs,
            self.change,
            self.amount,
            self.blinding_r,
            self.contrib_salt,
            self.p_old,
        );
    }
}

// ----------------------------- payout (release & refund) -----------------------------

pub struct PayoutWitness {
    // public
    pub domain_sep: Field,
    pub asset_tag: Field,
    pub epoch: Field,
    pub commitment_hash: Field,
    pub floor: Field,
    pub out_commitment: Field,
    pub recipient_bind: Field,
    // private
    pub domain_bind: Field,
    pub recipient_sk: Field,
    pub value: Field,
    pub blinding_r: Field,
    pub out_blinding: Field,
    pub out_rho: Field,
    pub salt: Field,
}

#[allow(clippy::too_many_arguments)]
pub fn verify_escrow_payout(
    domain_sep: Field,
    asset_tag: Field,
    epoch: Field,
    commitment_hash: Field,
    floor: Field,
    out_commitment: Field,
    recipient_bind: Field,
    domain_bind: Field,
    recipient_sk: Field,
    value: Field,
    blinding_r: Field,
    out_blinding: Field,
    out_rho: Field,
    salt: Field,
) {
    assert(domain_sep != 0);
    epoch.assert_max_bit_size::<32>();
    value.assert_max_bit_size::<64>();
    floor.assert_max_bit_size::<64>();

    // Open the committed value: commitment_hash pins the point, which must be Commit(value, r).
    let c = commit(value, blinding_r);
    assert(point_hash(c) == commitment_hash);
    // Threshold: value >= floor (both range-bound, so this is integer order).
    assert(!value.lt(floor));

    // Mint the payout note of `value` to the recipient.
    let recipient_pk = derive_owner_pk(recipient_sk);
    let note = Note { value, asset_tag, owner_pk: recipient_pk, blinding: out_blinding, epoch, rho: out_rho };
    assert(note.commitment() == out_commitment);

    // Recipient binding: payee_bind (release) or refund_bind (refund); the matching domain is
    // forced by the original binding the contract supplies as `recipient_bind`.
    assert(bind(domain_bind, recipient_pk, salt) == recipient_bind);
}

impl PayoutWitness {
    pub fn run(self) {
        verify_escrow_payout(
            self.domain_sep,
            self.asset_tag,
            self.epoch,
            self.commitment_hash,
            self.floor,
            self.out_commitment,
            self.recipient_bind,
            self.domain_bind,
            self.recipient_sk,
            self.value,
            self.blinding_r,
            self.out_blinding,
            self.out_rho,
            self.salt,
        );
    }
}

// ----------------------------- demo witnesses + tests -----------------------------

/// A deterministic first contribution: spend 1000, contribute 700 (hidden), 300 change. The
/// running commitment starts at the identity, so p_new = Commit(700, r). Parameterized by
/// `epoch`/`domain_sep` to bind a live pool; spends the note `deposit::demo_witness_at` mints.
pub fn contribute_demo_at(epoch: Field, domain_sep: Field) -> ContributeWitness {
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

    let real_in = make_input(1000, 777, epoch, 111, false, cm_ir, cm_sib, asp_ir, asp_sib, w0);
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

    let amount = 700;
    let blinding_r = 0xb11d;
    let contrib_salt = 0x5a17;
    let p_old = EmbeddedCurvePoint::point_at_infinity();
    let c = commit(amount, blinding_r);
    let p_new = p_old + c;

    ContributeWitness {
        domain_sep,
        asset_tag,
        epoch,
        commitment_root,
        nullifier_old_root: nf_old,
        nullifier_new_root: nf_new,
        nullifiers: [nf0, nf1],
        change_commitment: change.commitment(asset_tag, epoch),
        asp_root,
        c_raised_old: point_hash(p_old),
        c_raised_new: point_hash(p_new),
        c_contrib: point_hash(c),
        refund_bind: bind(DOMAIN_ESCROW_REFUND, owner_pk, contrib_salt),
        owner_sk,
        inputs: [real_in, dummy_in],
        change,
        amount,
        blinding_r,
        contrib_salt,
        p_old,
    }
}

pub fn contribute_demo() -> ContributeWitness {
    contribute_demo_at(5, 0xabc)
}

/// A deterministic release: open Commit(700, r) to value 700, floor=target=500 (met), mint a
/// 700 note to the payee. Parameterized by `epoch`/`domain_sep`.
pub fn payout_demo_at(epoch: Field, domain_sep: Field) -> PayoutWitness {
    let asset_tag = 1;
    let value = 700;
    let blinding_r = 0xb11d;
    let recipient_sk = 99;
    let recipient_pk = derive_owner_pk(recipient_sk);
    let salt = 0x9a17;
    let out_blinding = 222;
    let out_rho = 333;

    let c = commit(value, blinding_r);
    let note = Note { value, asset_tag, owner_pk: recipient_pk, blinding: out_blinding, epoch, rho: out_rho };

    PayoutWitness {
        domain_sep,
        asset_tag,
        epoch,
        commitment_hash: point_hash(c),
        floor: 500,
        out_commitment: note.commitment(),
        recipient_bind: bind(DOMAIN_ESCROW_PAYEE, recipient_pk, salt),
        domain_bind: DOMAIN_ESCROW_PAYEE,
        recipient_sk,
        value,
        blinding_r,
        out_blinding,
        out_rho,
        salt,
    }
}

pub fn payout_demo() -> PayoutWitness {
    payout_demo_at(5, 0xabc)
}

#[test]
fn contribute_end_to_end() {
    contribute_demo().run();
}

#[test(should_fail)]
fn contribute_rejects_value_mismatch() {
    let mut w = contribute_demo();
    w.amount = 701; // 1000 != 701 + 300
    w.run();
}

#[test(should_fail)]
fn contribute_rejects_tampered_running_hash() {
    let mut w = contribute_demo();
    w.c_raised_new = w.c_raised_new + 1; // not the hash of the actual folded point
    w.run();
}

#[test]
fn payout_end_to_end() {
    payout_demo().run();
}

#[test(should_fail)]
fn payout_rejects_value_below_floor() {
    let mut w = payout_demo();
    // Re-open a commitment to 400 with floor still 500: 400 < 500 must fail.
    let value = 400;
    let c = commit(value, w.blinding_r);
    let note = Note {
        value,
        asset_tag: w.asset_tag,
        owner_pk: derive_owner_pk(w.recipient_sk),
        blinding: w.out_blinding,
        epoch: w.epoch,
        rho: w.out_rho,
    };
    w.value = value;
    w.commitment_hash = point_hash(c);
    w.out_commitment = note.commitment();
    w.run();
}

#[test(should_fail)]
fn payout_rejects_wrong_recipient_bind() {
    let mut w = payout_demo();
    w.recipient_bind = w.recipient_bind + 1; // not bind(domain, recipient_pk, salt)
    w.run();
}

/// The two homomorphic folds compose: contributing v0 then v1 yields a running commitment whose
/// hash equals Commit(v0+v1, r0+r1)'s hash (the payee can open the total).
#[test]
fn running_commitment_is_homomorphic() {
    let p0 = EmbeddedCurvePoint::point_at_infinity();
    let c0 = commit(700, 0xb11d);
    let p1 = p0 + c0;
    let c1 = commit(250, 0x1234);
    let p2 = p1 + c1;
    // Opening with the summed value + summed blinding reproduces the same point.
    let opened = commit(700 + 250, 0xb11d + 0x1234);
    assert(point_hash(p2) == point_hash(opened));
}
