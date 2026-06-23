//! Merchant-pull subscription channel (building block B, phase 2): a one-way shielded payment
//! channel as pure accounting INSIDE the pool — value never leaves the vault. The subscriber
//! OPENS a channel by spending one owned note of a hidden `cap` (reusing the escrow_contribute
//! proof, whose `c_contrib` IS the cap commitment), then pre-signs an off-chain ramp of cumulative
//! authorizations. The merchant CLOSES once with the highest elapsed authorization: a channel_close
//! proof opens the cap + the signed cumulative commitment, verifies the subscriber's signature
//! IN-CIRCUIT, and mints two shielded notes (drawn -> merchant, remainder -> subscriber). If the
//! merchant never closes, after `expiry` the subscriber RECLAIMS the full cap (reusing the escrow
//! payout proof). The contract does NO elliptic-curve math and NO signature verification — all
//! value/threshold/authorization logic is in-circuit; the contract only compares opaque hashes and
//! enforces the state machine. State machine + public-input layouts: `claude-docs/channel_interface.md`.

use soroban_sdk::{contracttype, Env, U256};

pub const STATUS_OPEN: u32 = 0;
pub const STATUS_CLOSED: u32 = 1;

#[contracttype]
#[derive(Clone)]
pub struct Channel {
    pub asset_tag: U256,
    /// Poseidon(x,y) of C_cap = Commit(cap, r_cap) — the hidden maximum chargeable amount (opaque).
    /// Equal to the open proof's `c_contrib`; the close opens it, the reclaim opens it (floor 0).
    pub cap_commitment: U256,
    /// Poseidon(pk_chan.x, pk_chan.y) — the subscriber's per-channel signing pubkey. The close
    /// proof verifies a Schnorr signature under this key in-circuit; the contract only compares.
    pub auth_key: U256,
    /// Poseidon(DOMAIN_CHANNEL_MERCHANT, merchant_pk, m_salt) — only the merchant can receive the draw.
    pub merchant_bind: U256,
    /// The reclaim/remainder binding (== the open proof's `refund_bind`); only the subscriber can
    /// receive the remainder (close) or the full cap (reclaim).
    pub subscriber_bind: U256,
    /// Ledger sequence after which the subscriber may reclaim the full cap if not yet closed (PUBLIC).
    pub expiry: u64,
    pub status: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum ChannelKey {
    /// Next channel id to assign (monotonic).
    Next,
    /// Channel record by id.
    Channel(u64),
}

pub fn next_id(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&ChannelKey::Next)
        .unwrap_or(0u64)
}

/// Create a fresh channel from a verified open, assign it the next id. Returns the id.
pub fn open(
    env: &Env,
    asset_tag: U256,
    cap_commitment: U256,
    auth_key: U256,
    merchant_bind: U256,
    subscriber_bind: U256,
    expiry: u64,
) -> u64 {
    let id = next_id(env);
    let ch = Channel {
        asset_tag,
        cap_commitment,
        auth_key,
        merchant_bind,
        subscriber_bind,
        expiry,
        status: STATUS_OPEN,
    };
    set(env, id, &ch);
    env.storage().persistent().set(&ChannelKey::Next, &(id + 1));
    id
}

pub fn get(env: &Env, id: u64) -> Option<Channel> {
    env.storage().persistent().get(&ChannelKey::Channel(id))
}

pub fn set(env: &Env, id: u64, ch: &Channel) {
    env.storage().persistent().set(&ChannelKey::Channel(id), ch);
}
