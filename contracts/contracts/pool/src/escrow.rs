//! Escrow (building block B): hidden-sum invoice / multi-payer escrow as pure accounting
//! INSIDE the pool — value never leaves the vault. Contributions fold a hidden amount into an
//! opaque running Pedersen commitment (`c_raised`, stored as `Poseidon(x,y)` bytes the contract
//! never interprets); release/refund mint shielded notes proven against that commitment. The
//! contract does NO elliptic-curve math — all value/threshold logic is in-circuit. State machine
//! and public-input layouts: `claude-docs/escrow_interface.md`.

use soroban_sdk::{contracttype, Bytes, Env, U256};

/// Release requires `raised >= target`; under target at the deadline, contributors refund.
/// (`u32` not `u8` — Soroban contract types/params don't support `u8`.)
pub const MODE_ALL_OR_NOTHING: u32 = 0;
/// Payee sweeps whatever was raised after the deadline; no refunds.
pub const MODE_KEEP_WHAT_YOU_RAISE: u32 = 1;

pub const STATUS_OPEN: u32 = 0;
pub const STATUS_RELEASED: u32 = 1;

/// Seed POINT for a fresh escrow's running commitment: the fixed generator `G1 = Commit(0, 1)`,
/// NOT the identity. bb 0.87's `embedded_curve_add` blackbox rejects the point-at-infinity as an
/// input (it on-curve-checks regardless of `is_infinite`), so the first contribution can't fold
/// onto identity in a real proof. Seeding with G1 keeps every `p_old` a valid on-curve point; the
/// `Commit(0, 1)` offset is absorbed at release as `blinding = ΣR + 1`. The contribute circuit
/// takes `p_old` as input (no seed hardcoded), so the VK is unchanged — only this constant moved.
const SEED_X: [u8; 32] = [
    0x05, 0x4a, 0xa8, 0x6a, 0x73, 0xcb, 0x8a, 0x34, 0x52, 0x5e, 0x5b, 0xbe, 0xd6, 0xe4, 0x3b, 0xa1,
    0x19, 0x8e, 0x86, 0x0f, 0x5f, 0x39, 0x50, 0x26, 0x8f, 0x71, 0xdf, 0x45, 0x91, 0xbd, 0xe4, 0x02,
];
const SEED_Y: [u8; 32] = [
    0x20, 0x9d, 0xcf, 0xbf, 0x2c, 0xfb, 0x57, 0xf9, 0xf6, 0x04, 0x6f, 0x44, 0xd7, 0x1a, 0xc6, 0xfa,
    0xf8, 0x72, 0x54, 0xaf, 0xc7, 0x40, 0x7c, 0x04, 0xeb, 0x62, 0x1a, 0x62, 0x87, 0xca, 0xc1, 0x26,
];
/// `point_hash(G1) = Poseidon2([G1.x, G1.y])` — the seed's running-commitment hash (E6 parity).
const SEED_RAISED_HASH: [u8; 32] = [
    0x27, 0x3a, 0x06, 0xc5, 0xfa, 0x48, 0xd9, 0x5f, 0x4b, 0xd3, 0x17, 0xe8, 0xd3, 0xf3, 0x26, 0x89,
    0x1d, 0xda, 0xfe, 0x83, 0x65, 0xb2, 0x17, 0x16, 0xb1, 0xf4, 0x34, 0xcc, 0x63, 0xb8, 0x35, 0x4d,
];

pub fn init_c_raised(env: &Env) -> U256 {
    U256::from_be_bytes(env, &Bytes::from_array(env, &SEED_RAISED_HASH))
}

#[contracttype]
#[derive(Clone)]
pub struct Escrow {
    pub asset_tag: U256,
    pub target: u64,
    pub deadline: u64,
    pub mode: u32,
    pub payee_bind: U256,
    /// Poseidon(x,y) of the running Pedersen commitment to the hidden total (opaque).
    pub c_raised: U256,
    /// The running commitment POINT coordinates (x, y), cached so the NEXT contributor can read
    /// it to fold (the chain stores the hash for the chaining check; the point is needed as a
    /// fold witness). Verified at contribute: `Poseidon(x, y) == c_raised`. Seeded to `G1` at open.
    pub raised_x: U256,
    pub raised_y: U256,
    pub n_contrib: u32,
    pub status: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct Contribution {
    /// Poseidon(x,y) of this contribution's Pedersen commitment (refund handle, opaque).
    pub c_contrib: U256,
    /// Poseidon(DOMAIN_REFUND, contributor_pk, salt) — only the contributor can refund.
    pub refund_bind: U256,
    pub refunded: bool,
}

#[contracttype]
#[derive(Clone)]
pub enum EscrowKey {
    /// Next escrow id to assign (monotonic).
    Next,
    /// Escrow record by id.
    Escrow(u64),
    /// Contribution record by (escrow id, contribution index).
    Contrib(u64, u32),
}

pub fn next_id(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&EscrowKey::Next)
        .unwrap_or(0u64)
}

/// Create a fresh escrow, assign it the next id, seed its running commitment. Returns the id.
pub fn open(
    env: &Env,
    asset_tag: U256,
    target: u64,
    deadline: u64,
    mode: u32,
    payee_bind: U256,
) -> u64 {
    let id = next_id(env);
    let e = Escrow {
        asset_tag,
        target,
        deadline,
        mode,
        payee_bind,
        c_raised: init_c_raised(env),
        raised_x: U256::from_be_bytes(env, &Bytes::from_array(env, &SEED_X)),
        raised_y: U256::from_be_bytes(env, &Bytes::from_array(env, &SEED_Y)),
        n_contrib: 0,
        status: STATUS_OPEN,
    };
    set(env, id, &e);
    env.storage().persistent().set(&EscrowKey::Next, &(id + 1));
    id
}

pub fn get(env: &Env, id: u64) -> Option<Escrow> {
    env.storage().persistent().get(&EscrowKey::Escrow(id))
}

pub fn set(env: &Env, id: u64, e: &Escrow) {
    env.storage().persistent().set(&EscrowKey::Escrow(id), e);
}

pub fn get_contrib(env: &Env, id: u64, index: u32) -> Option<Contribution> {
    env.storage()
        .persistent()
        .get(&EscrowKey::Contrib(id, index))
}

pub fn set_contrib(env: &Env, id: u64, index: u32, c: &Contribution) {
    env.storage()
        .persistent()
        .set(&EscrowKey::Contrib(id, index), c);
}
