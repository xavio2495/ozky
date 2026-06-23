//! Client-side Pedersen commitment over the **Grumpkin** curve, matching Noir's
//! `std::hash::pedersen_commitment` (escrow building block B). The escrow circuits fold a hidden
//! amount into a running commitment `P = v*G0 + r*G1`; the on-chain handle is
//! `Poseidon2([P.x, P.y])`. This module reproduces that point math in native Rust so the witness
//! this core builds matches the circuit — the Pedersen analogue of [`super::poseidon`].
//!
//! Why this is ordinary (not research): Grumpkin's BASE field is exactly BN254's scalar field
//! (our [`Fr`]), so point coordinates are `Fr` and curve arithmetic is field arithmetic mod the
//! BN254 scalar modulus. Grumpkin has `a = 0`, so addition needs no curve constant. The two
//! generators `G0 = commit(1,0)`, `G1 = commit(0,1)` are captured from Noir in
//! `claude-docs/escrow_parity.md` and hardcoded below; the parity test pins the whole thing.

use super::poseidon::{Fr, Hasher};
use num_bigint::BigUint;

/// BN254 scalar field modulus = Grumpkin base field modulus (coordinate arithmetic).
fn modulus() -> BigUint {
    BigUint::parse_bytes(
        b"21888242871839275222246405745257275088548364400416034343698204186575808495617",
        10,
    )
    .unwrap()
}

fn fadd(a: &BigUint, b: &BigUint, p: &BigUint) -> BigUint {
    (a + b) % p
}
fn fsub(a: &BigUint, b: &BigUint, p: &BigUint) -> BigUint {
    // a, b are reduced (< p), so a + p - b stays non-negative.
    (a + p - b) % p
}
fn fmul(a: &BigUint, b: &BigUint, p: &BigUint) -> BigUint {
    (a * b) % p
}
/// Multiplicative inverse via Fermat: a^(p-2) mod p.
fn finv(a: &BigUint, p: &BigUint) -> BigUint {
    a.modpow(&(p - 2u32), p)
}

/// An affine Grumpkin point (coordinates in BN254 Fr). `inf` is the identity; matches Noir's
/// `EmbeddedCurvePoint::point_at_infinity()` whose `(x, y)` read as `(0, 0)`.
#[derive(Clone, Debug)]
pub struct Point {
    pub x: BigUint,
    pub y: BigUint,
    pub inf: bool,
}

impl Point {
    pub fn identity() -> Point {
        Point { x: BigUint::ZERO, y: BigUint::ZERO, inf: true }
    }

    fn from_hex_xy(x_hex: &str, y_hex: &str) -> Point {
        let parse = |h: &str| {
            BigUint::parse_bytes(h.strip_prefix("0x").unwrap_or(h).as_bytes(), 16).unwrap()
        };
        Point { x: parse(x_hex), y: parse(y_hex), inf: false }
    }

    /// Point addition (a = 0 Grumpkin). Handles identity, doubling, and P + (-P).
    pub fn add(&self, other: &Point, p: &BigUint) -> Point {
        if self.inf {
            return other.clone();
        }
        if other.inf {
            return self.clone();
        }
        let slope = if self.x == other.x {
            // Same x: either doubling, or P + (-P) = identity.
            if (fadd(&self.y, &other.y, p)) == BigUint::ZERO {
                return Point::identity();
            }
            // doubling: (3x^2) / (2y)   (a = 0)
            let num = fmul(&BigUint::from(3u32), &fmul(&self.x, &self.x, p), p);
            let den = fmul(&BigUint::from(2u32), &self.y, p);
            fmul(&num, &finv(&den, p), p)
        } else {
            // (y2 - y1) / (x2 - x1)
            let num = fsub(&other.y, &self.y, p);
            let den = fsub(&other.x, &self.x, p);
            fmul(&num, &finv(&den, p), p)
        };
        // x3 = slope^2 - x1 - x2
        let x3 = fsub(&fsub(&fmul(&slope, &slope, p), &self.x, p), &other.x, p);
        // y3 = slope*(x1 - x3) - y1
        let y3 = fsub(&fmul(&slope, &fsub(&self.x, &x3, p), p), &self.y, p);
        Point { x: x3, y: y3, inf: false }
    }

    /// Scalar multiplication `k * self` (double-and-add over the big-endian scalar bits).
    pub fn mul(&self, k: &BigUint, p: &BigUint) -> Point {
        let mut acc = Point::identity();
        let mut addend = self.clone();
        let mut n = k.clone();
        let zero = BigUint::ZERO;
        let two = BigUint::from(2u32);
        while n > zero {
            if (&n % 2u32) == BigUint::from(1u32) {
                acc = acc.add(&addend, p);
            }
            addend = addend.add(&addend, p);
            n /= &two;
        }
        acc
    }
}

fn g0() -> Point {
    Point::from_hex_xy(
        "0x083e7911d835097629f0067531fc15cafd79a89beecb39903f69572c636f4a5a",
        "0x1a7f5efaad7f315c25a918f30cc8d7333fccab7ad7c90f14de81bcc528f9935d",
    )
}
fn g1() -> Point {
    Point::from_hex_xy(
        "0x054aa86a73cb8a34525e5bbed6e43ba1198e860f5f3950268f71df4591bde402",
        "0x209dcfbf2cfb57f9f6046f44d71ac6faf87254afc7407c04eb621a6287cac126",
    )
}

fn fr_to_biguint(f: &Fr) -> BigUint {
    BigUint::from_bytes_be(&f.0)
}

fn biguint_to_fr(v: &BigUint) -> Fr {
    let bytes = v.to_bytes_be();
    let mut b = [0u8; 32];
    // Left-pad to 32 bytes big-endian (a coordinate is always < the 254-bit modulus).
    b[32 - bytes.len()..].copy_from_slice(&bytes);
    Fr(b)
}

/// Pedersen commitment `commit(value, blinding) = value*G0 + blinding*G1`. Homomorphic:
/// `commit(v0,r0) + commit(v1,r1) == commit(v0+v1, r0+r1)` (the escrow running-sum fold).
pub fn commit(value: &Fr, blinding: &Fr) -> Point {
    let p = modulus();
    let a = g0().mul(&fr_to_biguint(value), &p);
    let b = g1().mul(&fr_to_biguint(blinding), &p);
    a.add(&b, &p)
}

/// Add two commitment points (the running-sum fold step).
pub fn add(a: &Point, b: &Point) -> Point {
    a.add(b, &modulus())
}

/// A point's `(x, y)` as `Fr` (big-endian), for Prover.toml `EmbeddedCurvePoint` serialization.
/// The identity reads as `(0, 0)` (matching Noir's `point_at_infinity`).
pub fn coords(pt: &Point) -> (Fr, Fr) {
    (biguint_to_fr(&pt.x), biguint_to_fr(&pt.y))
}

/// Reconstruct a point from `Fr` coordinates (e.g. the running commitment read from chain).
/// `inf` marks the identity; a fresh escrow's stored `(0,0)` is the identity.
pub fn point_from_coords(x: &Fr, y: &Fr, inf: bool) -> Point {
    Point { x: fr_to_biguint(x), y: fr_to_biguint(y), inf }
}

/// On-chain handle for a point: `Poseidon2([x, y])` (an identity point hashes its `(0,0)`
/// coordinates, matching the circuit's `point_hash(point_at_infinity())`).
pub fn point_hash(h: &Hasher, pt: &Point) -> Fr {
    h.hash(&[biguint_to_fr(&pt.x), biguint_to_fr(&pt.y)])
}

/// The seed every fresh escrow's running commitment starts from: `point_hash(identity)`.
/// Must equal the contract's `escrow::init_c_raised` and the circuit's `empty_raised_hash`.
pub fn empty_raised_hash(h: &Hasher) -> Fr {
    point_hash(h, &Point::identity())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference vectors from claude-docs/escrow_parity.md (captured from the Noir circuit).
    const EMPTY_RAISED_HASH: &str =
        "0x0b63a53787021a4a962a452c2921b3663aff1ffd8d5510540f8e659e782956f1";
    const COMMIT_700_X: &str =
        "0x10e4cbe00548da97f3816bbfd7c18661f3da5072d6967fa5b01ffc8c07279c27";
    const COMMIT_700_Y: &str =
        "0x1ced01105d21a42e1d073d67f84a8254a42aad7a322a28981c08ef072c82e079";
    const POINT_HASH_700: &str =
        "0x0467924615dd23f09f9b1ab6aaee224abe0ca3febf75aa9842df18d70422e449";

    #[test]
    fn commit_matches_noir_vector() {
        // commit(700, 0xb11d) must equal the point Noir printed (exercises scalar-mul + add).
        let c = commit(&Fr::from_u64(700), &Fr::from_hex("0xb11d").unwrap());
        assert_eq!(biguint_to_fr(&c.x).to_hex(), COMMIT_700_X, "commit.x");
        assert_eq!(biguint_to_fr(&c.y).to_hex(), COMMIT_700_Y, "commit.y");
    }

    #[test]
    fn point_hash_matches_noir_vector() {
        let h = Hasher::new();
        let c = commit(&Fr::from_u64(700), &Fr::from_hex("0xb11d").unwrap());
        assert_eq!(point_hash(&h, &c).to_hex(), POINT_HASH_700);
    }

    #[test]
    fn empty_hash_matches_contract_seed() {
        let h = Hasher::new();
        assert_eq!(empty_raised_hash(&h).to_hex(), EMPTY_RAISED_HASH);
    }

    #[test]
    fn pedersen_is_homomorphic() {
        // commit(v0,r0) + commit(v1,r1) == commit(v0+v1, r0+r1) — the escrow fold property.
        let h = Hasher::new();
        let v0 = Fr::from_u64(700);
        let r0 = Fr::from_hex("0xb11d").unwrap();
        let v1 = Fr::from_u64(250);
        let r1 = Fr::from_hex("0x1234").unwrap();
        let folded = add(&commit(&v0, &r0), &commit(&v1, &r1));
        let direct = commit(&Fr::from_u64(950), &Fr::from_hex("0xc351").unwrap()); // 0xb11d + 0x1234
        assert_eq!(point_hash(&h, &folded).to_hex(), point_hash(&h, &direct).to_hex());
    }

    #[test]
    fn identity_is_add_neutral() {
        let p = modulus();
        let c = commit(&Fr::from_u64(700), &Fr::from_hex("0xb11d").unwrap());
        let viaid = Point::identity().add(&c, &p);
        assert_eq!(viaid.x, c.x);
        assert_eq!(viaid.y, c.y);
    }
}
