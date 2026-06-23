//! Channel close circuit (building block B, phase 2): merchant-pull subscription channel.
//!
//! `verify_channel_close` settles a one-way shielded payment channel. It opens the hidden `cap`
//! commitment and the subscriber-signed cumulative commitment `C_k = Commit(drawn, r_k)`, verifies
//! the subscriber's authorization signature IN-CIRCUIT (Schnorr over the embedded Grumpkin curve),
//! asserts `drawn <= cap`, and mints two shielded notes: `drawn` to the merchant and `cap - drawn`
//! back to the subscriber. The contract sees only opaque Poseidon handles + the verifier result, so
//! it does no curve math and no signature verification. See `claude-docs/channel_interface.md`.
//!
//! Schnorr over Grumpkin: the embedded curve's coordinates are BN254 `Fr` (Noir's `Field`) but its
//! SCALARS live in BN254 `Fq` (the larger base field). The challenge `e = Poseidon2(...)` is an `Fr`
//! element (< Fr < Fq), safe to lift via `EmbeddedCurveScalar::from_field`. The response `s` is
//! computed off-circuit in `Fq` and can land in `[Fr, Fq)`, so it is passed as the two limbs
//! `(s_lo, s_hi)` of an `EmbeddedCurveScalar` -- NEVER as a `Field` (which would reduce it mod Fr).
use crate::escrow::{bind, commit, point_hash, DOMAIN_ESCROW_REFUND};
use crate::{derive_owner_pk, Note};
use poseidon::poseidon2::Poseidon2;
use std::embedded_curve_ops::{multi_scalar_mul, EmbeddedCurvePoint, EmbeddedCurveScalar};

/// Domain tag for the merchant draw binding (ASCII "ozky_mrc"). Distinct from the escrow refund
/// domain reused for the subscriber binding, so neither output can be redirected to the other.
pub global DOMAIN_CHANNEL_MERCHANT: Field = 0x6f7a6b795f6d7263;

/// The Schnorr base point: `G0 = Commit(1, 0)`, the Pedersen value generator (already parity-pinned
/// in `claude-docs/escrow_parity.md` / the client's `pedersen` module). Grumpkin has prime order, so
/// any non-identity point generates the group; reusing G0 means the client needs no new generator.
pub global SCHNORR_G: EmbeddedCurvePoint = EmbeddedCurvePoint {
    x: 0x083e7911d835097629f0067531fc15cafd79a89beecb39903f69572c636f4a5a,
    y: 0x1a7f5efaad7f315c25a918f30cc8d7333fccab7ad7c90f14de81bcc528f9935d,
    is_infinite: false,
};

/// Verify a Schnorr signature `(R, s)` over `msg` under public key `pk = sk * G0`.
/// `e = Poseidon2([R.x, R.y, pk.x, pk.y, msg])`; the check is `s * G0 == R + e * pk`. `s` arrives as
/// limbs `(s_lo, s_hi)` (an `Fq` scalar that may exceed `Fr`); `e` is an `Fr` element lifted directly.
pub fn verify_schnorr(
    pk: EmbeddedCurvePoint,
    sig_r: EmbeddedCurvePoint,
    s_lo: Field,
    s_hi: Field,
    msg: Field,
) {
    let e = Poseidon2::hash([sig_r.x, sig_r.y, pk.x, pk.y, msg], 5);
    let s = EmbeddedCurveScalar::new(s_lo, s_hi);
    let e_scalar = EmbeddedCurveScalar::from_field(e);
    let lhs = multi_scalar_mul([SCHNORR_G], [s]);
    let e_pk = multi_scalar_mul([pk], [e_scalar]);
    let rhs = sig_r + e_pk;
    assert(lhs.x == rhs.x);
    assert(lhs.y == rhs.y);
}

pub struct CloseWitness {
    // public
    pub domain_sep: Field,
    pub asset_tag: Field,
    pub epoch: Field,
    pub cap_hash: Field,
    pub auth_key: Field,
    pub valid_after_ledger: Field,
    pub merchant_out: Field,
    pub subscriber_out: Field,
    pub merchant_bind: Field,
    pub subscriber_bind: Field,
    // private
    pub cap: Field,
    pub r_cap: Field,
    pub drawn: Field,
    pub r_k: Field,
    pub channel_id: Field,
    pub pk: EmbeddedCurvePoint,
    pub sig_r: EmbeddedCurvePoint,
    pub s_lo: Field,
    pub s_hi: Field,
    pub merchant_pk: Field,
    pub m_salt: Field,
    pub merchant_blinding: Field,
    pub merchant_rho: Field,
    pub subscriber_pk: Field,
    pub s_salt: Field,
    pub subscriber_blinding: Field,
    pub subscriber_rho: Field,
}

#[allow(clippy::too_many_arguments)]
pub fn verify_channel_close(
    domain_sep: Field,
    asset_tag: Field,
    epoch: Field,
    cap_hash: Field,
    auth_key: Field,
    valid_after_ledger: Field,
    merchant_out: Field,
    subscriber_out: Field,
    merchant_bind: Field,
    subscriber_bind: Field,
    cap: Field,
    r_cap: Field,
    drawn: Field,
    r_k: Field,
    channel_id: Field,
    pk: EmbeddedCurvePoint,
    sig_r: EmbeddedCurvePoint,
    s_lo: Field,
    s_hi: Field,
    merchant_pk: Field,
    m_salt: Field,
    merchant_blinding: Field,
    merchant_rho: Field,
    subscriber_pk: Field,
    s_salt: Field,
    subscriber_blinding: Field,
    subscriber_rho: Field,
) {
    assert(domain_sep != 0);
    epoch.assert_max_bit_size::<32>();
    cap.assert_max_bit_size::<64>();
    drawn.assert_max_bit_size::<64>();
    // drawn <= cap (both range-bound, so integer order); remainder stays in [0, cap].
    assert(!cap.lt(drawn));
    let remainder = cap - drawn;

    // Open the cap: cap_hash pins the point, which must be Commit(cap, r_cap).
    let c_cap = commit(cap, r_cap);
    assert(point_hash(c_cap) == cap_hash);

    // The subscriber-signed cumulative commitment for this period.
    let c_k = commit(drawn, r_k);

    // The auth-key handle binds the per-channel signing public key.
    assert(point_hash(pk) == auth_key);
    // The subscriber authorized exactly (channel_id, valid_after_ledger, C_k).
    let msg = Poseidon2::hash([channel_id, valid_after_ledger, c_k.x, c_k.y], 4);
    verify_schnorr(pk, sig_r, s_lo, s_hi, msg);

    // Mint the draw to the merchant (only the merchant key + salt opens merchant_bind).
    let m_note = Note {
        value: drawn,
        asset_tag,
        owner_pk: merchant_pk,
        blinding: merchant_blinding,
        epoch,
        rho: merchant_rho,
    };
    assert(m_note.commitment() == merchant_out);
    assert(bind(DOMAIN_CHANNEL_MERCHANT, merchant_pk, m_salt) == merchant_bind);

    // Mint the remainder back to the subscriber (reuses the escrow refund binding set at open).
    let s_note = Note {
        value: remainder,
        asset_tag,
        owner_pk: subscriber_pk,
        blinding: subscriber_blinding,
        epoch,
        rho: subscriber_rho,
    };
    assert(s_note.commitment() == subscriber_out);
    assert(bind(DOMAIN_ESCROW_REFUND, subscriber_pk, s_salt) == subscriber_bind);
}

impl CloseWitness {
    pub fn run(self) {
        verify_channel_close(
            self.domain_sep,
            self.asset_tag,
            self.epoch,
            self.cap_hash,
            self.auth_key,
            self.valid_after_ledger,
            self.merchant_out,
            self.subscriber_out,
            self.merchant_bind,
            self.subscriber_bind,
            self.cap,
            self.r_cap,
            self.drawn,
            self.r_k,
            self.channel_id,
            self.pk,
            self.sig_r,
            self.s_lo,
            self.s_hi,
            self.merchant_pk,
            self.m_salt,
            self.merchant_blinding,
            self.merchant_rho,
            self.subscriber_pk,
            self.s_salt,
            self.subscriber_blinding,
            self.subscriber_rho,
        );
    }
}

// ----------------------------- demo witness + tests -----------------------------
//
// The positive vector below (signing key, nonce point R, and response s) is produced by the native
// Rust signer (`core/channel.rs::tests::print_schnorr_parity_vectors`) using the SAME Poseidon2 and
// the SAME Grumpkin base G0 as this circuit, because a valid Schnorr signature needs Fq scalar-field
// arithmetic the circuit cannot do on itself. The Noir test then proves the circuit ACCEPTS it, which
// is what pins client<->circuit signature parity (the Pedersen/Poseidon-style reference-vector dance).

/// A deterministic close: cap=1000, drawn=600, remainder=400, channel_id=1, valid_after=50. The
/// signing material is the parity vector; `pk = sk*G0`, `R = k*G0`, `s = (k + e*sk) mod Fq`.
pub fn close_demo() -> CloseWitness {
    let asset_tag = 1;
    let epoch = 5;
    let domain_sep = 0xabc;
    let cap = 1000;
    let r_cap = 0xca9;
    let drawn = 600;
    let r_k = 0xd4a;
    let channel_id = 1;
    let valid_after_ledger = 50;

    // --- signature parity vector (from core::pedersen::tests::print_schnorr_parity_vectors,
    //     signing key sk=0x1234567, nonce k=0x89abcdef; pk = sk*G0, R = k*G0, s = (k+e*sk) mod Fq) ---
    let pk = EmbeddedCurvePoint {
        x: 0x102f436ca51e10edbcc14790feebb4d5f4859b103b735d38c5391829fc67833e,
        y: 0x01ee2d6fb91923147be9d69c48c1487eb75562f0a02de04ca52d3f2ec1d302b0,
        is_infinite: false,
    };
    let sig_r = EmbeddedCurvePoint {
        x: 0x0cedf0ec95adaf1df3e425efc2382bf4cec5c2b663110a21e014c5869ebd6726,
        y: 0x2fc28988d4a06930b7aa9fe2825916b8e842a873aac34eb88fc425c72791307d,
        is_infinite: false,
    };
    let s_lo = 0xa7cfbb9e6051930fa3f7f5bd66c9d202;
    let s_hi = 0x1512fadc1e5a6772f33b6ab3c677c1cc;
    // --- end parity vector ---

    let merchant_sk = 0x3e;
    let merchant_pk = derive_owner_pk(merchant_sk);
    let m_salt = 0x3e17;
    let merchant_blinding = 222;
    let merchant_rho = 333;

    let subscriber_sk = 0x5b;
    let subscriber_pk = derive_owner_pk(subscriber_sk);
    let s_salt = 0x5b17;
    let subscriber_blinding = 444;
    let subscriber_rho = 555;

    let c_cap = commit(cap, r_cap);
    let m_note = Note {
        value: drawn,
        asset_tag,
        owner_pk: merchant_pk,
        blinding: merchant_blinding,
        epoch,
        rho: merchant_rho,
    };
    let s_note = Note {
        value: cap - drawn,
        asset_tag,
        owner_pk: subscriber_pk,
        blinding: subscriber_blinding,
        epoch,
        rho: subscriber_rho,
    };

    CloseWitness {
        domain_sep,
        asset_tag,
        epoch,
        cap_hash: point_hash(c_cap),
        auth_key: point_hash(pk),
        valid_after_ledger,
        merchant_out: m_note.commitment(),
        subscriber_out: s_note.commitment(),
        merchant_bind: bind(DOMAIN_CHANNEL_MERCHANT, merchant_pk, m_salt),
        subscriber_bind: bind(DOMAIN_ESCROW_REFUND, subscriber_pk, s_salt),
        cap,
        r_cap,
        drawn,
        r_k,
        channel_id,
        pk,
        sig_r,
        s_lo,
        s_hi,
        merchant_pk,
        m_salt,
        merchant_blinding,
        merchant_rho,
        subscriber_pk,
        s_salt,
        subscriber_blinding,
        subscriber_rho,
    }
}

/// Demo close public values, for the Rust witness-builder parity test (CH4). The signature publics
/// (cap_hash from r_cap, auth_key from pk) plus the message inputs are what the client reproduces.
#[test]
fn print_close_demo_publics() {
    let w = close_demo();
    let cap_hash = w.cap_hash;
    let auth_key = w.auth_key;
    let m_out = w.merchant_out;
    let s_out = w.subscriber_out;
    let m_bind = w.merchant_bind;
    let s_bind = w.subscriber_bind;
    println(f"close.cap_hash = {cap_hash}");
    println(f"close.auth_key = {auth_key}");
    println(f"close.merchant_out = {m_out}");
    println(f"close.subscriber_out = {s_out}");
    println(f"close.merchant_bind = {m_bind}");
    println(f"close.subscriber_bind = {s_bind}");
}

/// Print the message + commitment the signer must sign over, so the Rust signer (CH4) can produce a
/// matching signature: msg = Poseidon2([channel_id, valid_after_ledger, C_k.x, C_k.y]).
#[test]
fn print_close_signing_material() {
    let drawn = 600;
    let r_k = 0xd4a;
    let c_k = commit(drawn, r_k);
    let ckx = c_k.x;
    let cky = c_k.y;
    let msg = Poseidon2::hash([1, 50, c_k.x, c_k.y], 4);
    println(f"close.c_k.x = {ckx}");
    println(f"close.c_k.y = {cky}");
    println(f"close.msg = {msg}");
    // The cap commitment the client also opens at close.
    let c_cap = commit(1000, 0xca9);
    let capx = c_cap.x;
    let capy = c_cap.y;
    println(f"close.c_cap.x = {capx}");
    println(f"close.c_cap.y = {capy}");
}

/// The whole point of CH2: the circuit ACCEPTS a signature the native Rust signer produced (over the
/// same Poseidon2 + Grumpkin G0). A pass here pins client<->circuit Schnorr parity.
#[test]
fn close_end_to_end() {
    close_demo().run();
}

#[test(should_fail)]
fn close_rejects_drawn_over_cap() {
    let mut w = close_demo();
    // drawn must be <= cap; bumping drawn past cap (with the same commitments) must fail.
    w.drawn = w.cap + 1;
    w.run();
}

#[test(should_fail)]
fn close_rejects_tampered_signature() {
    let mut w = close_demo();
    w.s_lo = w.s_lo + 1; // not a valid response for (R, e, pk)
    w.run();
}

#[test(should_fail)]
fn close_rejects_wrong_auth_key() {
    let mut w = close_demo();
    w.auth_key = w.auth_key + 1; // not point_hash(pk)
    w.run();
}

#[test(should_fail)]
fn close_rejects_wrong_message() {
    let mut w = close_demo();
    // A signature for valid_after=50 must not validate for a different period (the signed message
    // binds valid_after_ledger), even though the contract would also gate this publicly.
    w.valid_after_ledger = 60;
    w.run();
}

#[test(should_fail)]
fn close_rejects_wrong_merchant_bind() {
    let mut w = close_demo();
    w.merchant_bind = w.merchant_bind + 1;
    w.run();
}

#[test(should_fail)]
fn close_rejects_subscriber_value_tamper() {
    let mut w = close_demo();
    // Re-pointing subscriber_out to a note of the wrong remainder breaks conservation (the circuit
    // mints exactly cap - drawn to the subscriber).
    let bad = Note {
        value: 999, // remainder must be cap - drawn = 400
        asset_tag: w.asset_tag,
        owner_pk: w.subscriber_pk,
        blinding: w.subscriber_blinding,
        epoch: w.epoch,
        rho: w.subscriber_rho,
    };
    w.subscriber_out = bad.commitment();
    w.run();
}
