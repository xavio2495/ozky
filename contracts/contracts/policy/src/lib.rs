#![no_std]
//! Policy / ASP contract (Z1 spec §11, §8; build plan Z5). Kept separate from the
//! pool so compliance logic can evolve without touching the shielded-pool audit
//! surface. It owns two things:
//!
//!  1. The **ASP approved-set root** (`asp_root`) — the Merkle root of approved
//!     spending keys. Transfers/withdrawals prove `owner_pk ∈ asp_root` *in-circuit*
//!     (the pool checks the proof's `asp_root` public input against the value it was
//!     told to enforce). This contract is the governance home for that root and its
//!     authorized update path.
//!  2. The **public deposit allow-list** — `from` addresses permitted to deposit
//!     (the public edge gate, spec §8). The pool calls `is_allowed(from)` on deposit.
//!
//! Testnet: a single `admin` controls updates (FROZEN debt → governance/multisig
//! before mainnet, handoff §8). The denied-set root (ASP Option C) is a mainnet item
//! and intentionally absent here.

use soroban_sdk::{contract, contracterror, contractevent, contractimpl, contracttype, Address, Env, U256};

#[contracterror]
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
}

#[contracttype]
#[derive(Clone)]
enum Key {
    Admin,
    AspRoot,
    /// Per-address deposit allow-list flag.
    Allowed(Address),
}

#[contractevent(topics = ["asp_root"], data_format = "single-value")]
pub struct AspRootUpdated {
    pub root: U256,
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

#[contractimpl]
impl Policy {
    pub fn __constructor(env: Env, admin: Address, asp_root: U256) -> Result<(), Error> {
        if env.storage().instance().has(&Key::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&Key::Admin, &admin);
        env.storage().instance().set(&Key::AspRoot, &asp_root);
        Ok(())
    }

    /// Update the ASP approved-set root (admin only).
    pub fn set_asp_root(env: Env, new_root: U256) {
        admin(&env).require_auth();
        env.storage().instance().set(&Key::AspRoot, &new_root);
        AspRootUpdated { root: new_root }.publish(&env);
    }

    /// The current approved-set root.
    pub fn asp_root(env: Env) -> U256 {
        env.storage().instance().get(&Key::AspRoot).unwrap()
    }

    /// Add/remove an address from the deposit allow-list (admin only).
    pub fn set_allowed(env: Env, who: Address, allowed: bool) {
        admin(&env).require_auth();
        if allowed {
            env.storage().persistent().set(&Key::Allowed(who.clone()), &true);
        } else {
            env.storage().persistent().remove(&Key::Allowed(who.clone()));
        }
        AllowListUpdated { who, allowed }.publish(&env);
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

    fn setup() -> (Env, Address, PolicyClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let root0 = U256::from_u32(&env, 100);
        let id = env.register(Policy, (admin.clone(), root0));
        let client = PolicyClient::new(&env, &id);
        let user = Address::generate(&env);
        (env, admin, client, user)
    }

    #[test]
    fn asp_root_set_and_get() {
        let (env, _admin, c, _u) = setup();
        assert_eq!(c.asp_root(), U256::from_u32(&env, 100));
        c.set_asp_root(&U256::from_u32(&env, 999));
        assert_eq!(c.asp_root(), U256::from_u32(&env, 999));
    }

    #[test]
    fn allow_list_defaults_closed_then_toggles() {
        let (_env, _admin, c, user) = setup();
        assert!(!c.is_allowed(&user));
        c.set_allowed(&user, &true);
        assert!(c.is_allowed(&user));
        c.set_allowed(&user, &false);
        assert!(!c.is_allowed(&user));
    }
}
