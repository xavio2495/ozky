#![no_std]
//! Policy / ASP contract (Z1 spec §11, §8; build plan Z5; FEATURE_SET G1-G2). Kept
//! separate from the pool so compliance logic can evolve without touching the
//! shielded-pool audit surface. It owns:
//!
//!  1. The **ASP approved set** — an ordered list of approved spending keys
//!     (`owner_pk`s) and its depth-20 Poseidon Merkle **root** (`asp_root`).
//!     Transfers/withdrawals prove `owner_pk ∈ asp_root` *in-circuit*; this contract
//!     is the source of truth for the set and recomputes the root on every enrollment
//!     (so it always matches the circuit's). Each enrollment emits an `asp_mem` event
//!     so a client can reconstruct the set from chain (indexer-free) and build its
//!     membership path, self-checking against this root.
//!  2. The **public deposit allow-list** — `from` addresses permitted to deposit
//!     (the public edge gate, spec §8). The pool calls `is_allowed(from)` on deposit.
//!
//! `enroll(owner_pk, who)` does both at once: approve the spending key (for private
//! transfers/withdrawals) AND allow the funding address (for deposits) — the single
//! onboarding step for a new wallet.
//!
//! Testnet: a single `admin` controls enrollment (FROZEN debt → governance/multisig
//! before mainnet, handoff §8). The denied-set root (ASP Option C) is a mainnet item.

mod merkle;

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Env, Vec, U256,
};

#[contracterror]
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    /// `owner_pk` is already in the approved set.
    AlreadyApproved = 3,
}

#[contracttype]
#[derive(Clone)]
enum Key {
    Admin,
    /// Cached current approved-set Merkle root.
    AspRoot,
    /// Ordered list of approved `owner_pk`s (leaf order = enrollment order).
    Members,
    /// Per-address deposit allow-list flag.
    Allowed(Address),
}

#[contractevent(topics = ["asp_root"], data_format = "single-value")]
pub struct AspRootUpdated {
    pub root: U256,
}

/// Emitted on each enrollment so clients reconstruct the approved set (leaf order) and
/// build membership paths against `asp_root` — no indexer required.
#[contractevent(topics = ["asp_mem"], data_format = "single-value")]
pub struct AspMember {
    #[topic]
    pub index: u32,
    pub owner_pk: U256,
}

#[contractevent(topics = ["allow"], data_format = "map")]
pub struct AllowListUpdated {
    #[topic]
    pub who: Address,
    pub allowed: bool,
}

#[contract]
pub struct Policy;

fn admin(env: &Env) -> Address {
    env.storage().instance().get(&Key::Admin).unwrap()
}

fn members(env: &Env) -> Vec<U256> {
    env.storage()
        .instance()
        .get(&Key::Members)
        .unwrap_or_else(|| Vec::new(env))
}

/// Append a member + recompute the root + emit events. NO auth check — callers that
/// are public entrypoints `require_auth` once first (so `enroll` doesn't double-auth).
fn do_approve(env: &Env, owner_pk: U256) -> Result<u32, Error> {
    let mut ms = members(env);
    if ms.iter().any(|m| m == owner_pk) {
        return Err(Error::AlreadyApproved);
    }
    let index = ms.len();
    ms.push_back(owner_pk.clone());
    let root = merkle::root_of(env, &ms);
    env.storage().instance().set(&Key::Members, &ms);
    env.storage().instance().set(&Key::AspRoot, &root);
    AspMember { index, owner_pk }.publish(env);
    AspRootUpdated { root }.publish(env);
    Ok(index)
}

/// Set the deposit allow-list flag + emit. NO auth check (see [`do_approve`]).
fn do_set_allowed(env: &Env, who: Address, allowed: bool) {
    if allowed {
        env.storage().persistent().set(&Key::Allowed(who.clone()), &true);
    } else {
        env.storage().persistent().remove(&Key::Allowed(who.clone()));
    }
    AllowListUpdated { who, allowed }.publish(env);
}

#[contractimpl]
impl Policy {
    /// Deploy with an `admin` and an empty approved set (root = empty depth-20 tree).
    pub fn __constructor(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&Key::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&Key::Admin, &admin);
        let empty: Vec<U256> = Vec::new(&env);
        let root = merkle::root_of(&env, &empty);
        env.storage().instance().set(&Key::Members, &empty);
        env.storage().instance().set(&Key::AspRoot, &root);
        Ok(())
    }

    /// Approve a spending key: append `owner_pk` to the set, recompute + store the
    /// root, and emit `asp_mem` (admin only). Rejects a duplicate.
    pub fn approve_member(env: Env, owner_pk: U256) -> Result<u32, Error> {
        admin(&env).require_auth();
        do_approve(&env, owner_pk)
    }

    /// Onboard a wallet in one step: approve its spending key AND allow its funding
    /// address to deposit (admin only). Single auth (then internal no-auth helpers).
    pub fn enroll(env: Env, owner_pk: U256, who: Address) -> Result<u32, Error> {
        admin(&env).require_auth();
        let index = do_approve(&env, owner_pk)?;
        do_set_allowed(&env, who, true);
        Ok(index)
    }

    /// The current approved-set root.
    pub fn asp_root(env: Env) -> U256 {
        env.storage().instance().get(&Key::AspRoot).unwrap()
    }

    /// The number of approved members.
    pub fn member_count(env: Env) -> u32 {
        members(&env).len()
    }

    /// The approved set (leaf order). Convenience for clients/tests; the canonical
    /// reconstruction path is the `asp_mem` event stream.
    pub fn members(env: Env) -> Vec<U256> {
        members(&env)
    }

    /// Add/remove an address from the deposit allow-list (admin only).
    pub fn set_allowed(env: Env, who: Address, allowed: bool) {
        admin(&env).require_auth();
        do_set_allowed(&env, who, allowed);
    }

    /// Whether `who` may deposit (public edge gate).
    pub fn is_allowed(env: Env, who: Address) -> bool {
        env.storage()
            .persistent()
            .get(&Key::Allowed(who))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, Env, U256};

    fn setup() -> (Env, PolicyClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        env.cost_estimate().budget().reset_unlimited();
        let admin = Address::generate(&env);
        let id = env.register(Policy, (admin.clone(),));
        let client = PolicyClient::new(&env, &id);
        let user = Address::generate(&env);
        (env, client, user)
    }

    #[test]
    fn empty_set_then_members_change_root() {
        let (env, c, _u) = setup();
        let empty_root = c.asp_root();
        assert_eq!(c.member_count(), 0);

        let i0 = c.approve_member(&U256::from_u32(&env, 12345));
        assert_eq!(i0, 0);
        let root1 = c.asp_root();
        assert_ne!(root1, empty_root, "root changes when a member is added");
        assert_eq!(c.member_count(), 1);

        let i1 = c.approve_member(&U256::from_u32(&env, 67890));
        assert_eq!(i1, 1);
        assert_ne!(c.asp_root(), root1, "root changes again");
        assert_eq!(c.member_count(), 2);
        assert_eq!(c.members().len(), 2);
    }

    #[test]
    fn duplicate_member_is_rejected() {
        let (env, c, _u) = setup();
        c.approve_member(&U256::from_u32(&env, 7));
        let res = c.try_approve_member(&U256::from_u32(&env, 7));
        assert_eq!(res, Err(Ok(Error::AlreadyApproved)));
    }

    #[test]
    fn enroll_approves_and_allows() {
        let (env, c, user) = setup();
        assert!(!c.is_allowed(&user));
        c.enroll(&U256::from_u32(&env, 42), &user);
        assert!(c.is_allowed(&user), "funding address allow-listed");
        assert_eq!(c.member_count(), 1, "spending key approved");
    }

    #[test]
    fn allow_list_defaults_closed_then_toggles() {
        let (_env, c, user) = setup();
        assert!(!c.is_allowed(&user));
        c.set_allowed(&user, &true);
        assert!(c.is_allowed(&user));
        c.set_allowed(&user, &false);
        assert!(!c.is_allowed(&user));
    }
}
