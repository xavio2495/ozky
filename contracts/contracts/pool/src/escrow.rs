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

/// Seed for a fresh escrow's running commitment: the hash of the Pedersen identity point.
/// = `point_hash(EmbeddedCurvePoint::point_at_infinity())` from the Noir circuit's
/// `escrow::empty_raised_hash` (E3 parity-pinned). The contribute circuit folds the first
/// contribution onto this exact value, so its `c_raised_old` for the first contribute equals it.
pub fn init_c_raised(env: &Env) -> U256 {
    const EMPTY_RAISED_HASH: [u8; 32] = [
        0x0b, 0x63, 0xa5, 0x37, 0x87, 0x02, 0x1a, 0x4a, 0x96, 0x2a, 0x45, 0x2c, 0x29, 0x21, 0xb3,
        0x66, 0x3a, 0xff, 0x1f, 0xfd, 0x8d, 0x55, 0x10, 0x54, 0x0f, 0x8e, 0x65, 0x9e, 0x78, 0x29,
        0x56, 0xf1,
    ];
    U256::from_be_bytes(env, &Bytes::from_array(env, &EMPTY_RAISED_HASH))
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
