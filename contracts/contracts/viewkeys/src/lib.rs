#![no_std]
//! View-key registry + disclosure grant trail (Z1 spec §11 D6; build plan Z5).
//!
//! This contract is a THIN on-chain record keeper, by design. The actual
//! cryptographic disclosure is off-chain (D6): an auditor handed the viewing +
//! detection keys for a scope re-derives note contents and the commitment, then
//! verifies against on-chain commitments — no contract involvement. The view-key
//! hierarchy derivation (BIP32-style ECDH→HKDF→AEAD) lives in the Rust core
//! (Phases A1/A2). What MUST be on-chain is the auditable trail: which scoped
//! viewing keys an owner published, and which disclosure grants they made — so a
//! grant (and its later revocation) is provable and timestamped.
//!
//! Scope is hierarchical (`account / asset_tag / epoch`, FROZEN handoff): a grant
//! is for exactly one scope and gives no path to other accounts/assets/epochs.

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, BytesN, Env,
    U256,
};

#[contracterror]
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    /// No viewing key registered for this (owner, scope).
    KeyNotRegistered = 1,
}

/// Hierarchical disclosure scope (FROZEN: account / asset / epoch).
#[contracttype]
#[derive(Clone, PartialEq)]
pub struct ViewScope {
    pub account: u32,
    pub asset_tag: U256,
    pub epoch: u32,
}

/// A registered scoped viewing key pair (public halves only — secrets never touch
/// chain). `viewing_pub` decrypts note payloads; `detection_pub` drives view-tag
/// scanning for the scope.
#[contracttype]
#[derive(Clone)]
pub struct ScopedKey {
    pub viewing_pub: BytesN<32>,
    pub detection_pub: BytesN<32>,
}

// Storage keys flatten the scope into the tuple rather than nesting the
// `ViewScope` struct: a nested struct key serializes with its field-name symbols
// and pushes the composite over the 250-byte ledger-key limit. Order: account,
// asset_tag, epoch (FROZEN scope order).
#[contracttype]
#[derive(Clone)]
enum Key {
    /// Registered viewing key for (owner, account, asset_tag, epoch).
    ViewKey(Address, u32, U256, u32),
    /// Disclosure grant (owner, auditor, account, asset_tag, epoch) -> ledger seq.
    Grant(Address, Address, u32, U256, u32),
}

fn vk_key(owner: &Address, s: &ViewScope) -> Key {
    Key::ViewKey(owner.clone(), s.account, s.asset_tag.clone(), s.epoch)
}

fn grant_key(owner: &Address, auditor: &Address, s: &ViewScope) -> Key {
    Key::Grant(
        owner.clone(),
        auditor.clone(),
        s.account,
        s.asset_tag.clone(),
        s.epoch,
    )
}

#[contractevent(topics = ["vk_reg"], data_format = "map")]
pub struct ViewKeyRegistered {
    #[topic]
    pub owner: Address,
    pub scope: ViewScope,
}

#[contractevent(topics = ["disclose"], data_format = "map")]
pub struct DisclosureGranted {
    #[topic]
    pub owner: Address,
    #[topic]
    pub auditor: Address,
    pub scope: ViewScope,
}

#[contractevent(topics = ["revoke"], data_format = "map")]
pub struct DisclosureRevoked {
    #[topic]
    pub owner: Address,
    #[topic]
    pub auditor: Address,
    pub scope: ViewScope,
}

#[contract]
pub struct ViewKeys;

#[contractimpl]
impl ViewKeys {
    /// Register (or rotate) the scoped viewing key pair for `owner` at `scope`.
    /// Only the owner can register their own keys.
    pub fn register_view_key(
        env: Env,
        owner: Address,
        scope: ViewScope,
        viewing_pub: BytesN<32>,
        detection_pub: BytesN<32>,
    ) {
        owner.require_auth();
        env.storage().persistent().set(
            &vk_key(&owner, &scope),
            &ScopedKey {
                viewing_pub,
                detection_pub,
            },
        );
        ViewKeyRegistered { owner, scope }.publish(&env);
    }

    /// The registered scoped key pair, if any (the bytes an auditor needs to know
    /// which public keys a disclosure refers to).
    pub fn view_key(env: Env, owner: Address, scope: ViewScope) -> Option<ScopedKey> {
        env.storage().persistent().get(&vk_key(&owner, &scope))
    }

    /// Grant `auditor` a disclosure for `scope`. Requires a registered key for the
    /// scope (the auditor re-derives off-chain from the corresponding secret the
    /// owner hands over out-of-band). Records an auditable, revocable grant.
    pub fn disclose(
        env: Env,
        owner: Address,
        auditor: Address,
        scope: ViewScope,
    ) -> Result<(), Error> {
        owner.require_auth();
        if !env.storage().persistent().has(&vk_key(&owner, &scope)) {
            return Err(Error::KeyNotRegistered);
        }
        let when = env.ledger().sequence();
        env.storage()
            .persistent()
            .set(&grant_key(&owner, &auditor, &scope), &when);
        DisclosureGranted {
            owner,
            auditor,
            scope,
        }
        .publish(&env);
        Ok(())
    }

    /// Revoke a previously granted disclosure (owner only).
    pub fn revoke(env: Env, owner: Address, auditor: Address, scope: ViewScope) {
        owner.require_auth();
        env.storage()
            .persistent()
            .remove(&grant_key(&owner, &auditor, &scope));
        DisclosureRevoked {
            owner,
            auditor,
            scope,
        }
        .publish(&env);
    }

    /// Whether `auditor` currently holds a disclosure grant from `owner` for `scope`.
    pub fn is_disclosed(env: Env, owner: Address, auditor: Address, scope: ViewScope) -> bool {
        env.storage()
            .persistent()
            .has(&grant_key(&owner, &auditor, &scope))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, U256};

    fn scope(env: &Env) -> ViewScope {
        ViewScope {
            account: 0,
            asset_tag: U256::from_u32(env, 1),
            epoch: 28,
        }
    }

    fn setup() -> (Env, ViewKeysClient<'static>, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(ViewKeys, ());
        let client = ViewKeysClient::new(&env, &id);
        (env.clone(), client, Address::generate(&env), Address::generate(&env))
    }

    #[test]
    fn register_then_read_view_key() {
        let (env, c, owner, _auditor) = setup();
        let s = scope(&env);
        let vp = BytesN::from_array(&env, &[1u8; 32]);
        let dp = BytesN::from_array(&env, &[2u8; 32]);
        c.register_view_key(&owner, &s, &vp, &dp);
        let got = c.view_key(&owner, &s).unwrap();
        assert_eq!(got.viewing_pub, vp);
        assert_eq!(got.detection_pub, dp);
    }

    #[test]
    fn disclose_requires_registered_key_then_grants_and_revokes() {
        let (env, c, owner, auditor) = setup();
        let s = scope(&env);

        // No key yet → disclose fails.
        assert_eq!(
            c.try_disclose(&owner, &auditor, &s),
            Err(Ok(Error::KeyNotRegistered))
        );

        // Register, then grant.
        let vp = BytesN::from_array(&env, &[1u8; 32]);
        let dp = BytesN::from_array(&env, &[2u8; 32]);
        c.register_view_key(&owner, &s, &vp, &dp);
        assert!(!c.is_disclosed(&owner, &auditor, &s));
        c.disclose(&owner, &auditor, &s);
        assert!(c.is_disclosed(&owner, &auditor, &s));

        // Revoke → no path remains.
        c.revoke(&owner, &auditor, &s);
        assert!(!c.is_disclosed(&owner, &auditor, &s));
    }

    #[test]
    fn grant_is_scoped_no_leak_to_other_epoch() {
        let (env, c, owner, auditor) = setup();
        let s28 = scope(&env);
        let s29 = ViewScope { account: 0, asset_tag: U256::from_u32(&env, 1), epoch: 29 };
        let vp = BytesN::from_array(&env, &[1u8; 32]);
        let dp = BytesN::from_array(&env, &[2u8; 32]);
        c.register_view_key(&owner, &s28, &vp, &dp);
        c.disclose(&owner, &auditor, &s28);
        // A grant for epoch 28 gives no disclosure for epoch 29.
        assert!(c.is_disclosed(&owner, &auditor, &s28));
        assert!(!c.is_disclosed(&owner, &auditor, &s29));
    }
}
