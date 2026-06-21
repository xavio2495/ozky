#![no_std]
//! ozky shielded-pool contract (Phase Z4). The vault + private ledger: per-asset
//! SAC vaults, a contract-maintained append-only Poseidon commitment tree, the
//! proof-driven nullifier accumulator root, domain-separated anti-replay, and the
//! deposit/transfer/withdraw entrypoints that call the (per-circuit) verifier.
//!
//! Public edges move real tokens (`deposit` pulls SAC in, `withdraw` releases SAC
//! out); the interior `transfer` moves no tokens. The shielded==vaulted invariant
//! is held by the amount-binding deposit/withdraw proofs plus in-circuit value
//! conservation.

mod assets;
mod config;
mod domain;
mod inputs;
mod nullifier;
mod poseidon;
mod tree;
mod verifier;

use config::Config;
use soroban_sdk::{
    contract, contracterror, contractimpl, symbol_short, token, Address, Bytes, BytesN, Env,
    IntoVal, Symbol, U256, Vec,
};

/// Epoch length in ledgers (FROZEN, handoff): `epoch = ledger_seq / 110_000`.
const LEDGER_PER_EPOCH: u64 = 110_000;

/// Domain tag for the withdraw destination binding: `dest_bind = Poseidon(DOMAIN_DEST,
/// dest_ed25519_pubkey)` (ASCII "ozky_dst"). MUST match the client's `DOMAIN_DEST`.
const DOMAIN_DEST: u64 = 0x6f7a6b795f647374;

#[contracterror]
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    /// Commitment tree has reached its 2^depth leaf capacity.
    TreeFull = 1,
    /// `nullifier_old_root` does not equal the stored accumulator root.
    NullifierRootMismatch = 2,
    /// The verifier contract rejected the proof (or the call failed).
    VerificationFailed = 3,
    /// `public_inputs` is the wrong length for the circuit.
    BadPublicInputs = 4,
    /// `domain_sep` public input does not bind this pool/network/selector.
    BadDomainSep = 5,
    /// `epoch` public input does not equal the current epoch.
    BadEpoch = 6,
    /// `asset_tag` public input mismatched the call, or the asset is unregistered.
    AssetMismatch = 7,
    /// `commitment_root` public input is not within the rolling window.
    StaleCommitmentRoot = 8,
    /// `asp_root` public input does not equal the stored approved-set root.
    BadAspRoot = 9,
    /// The public `amount` field does not equal the token-edge `amount` argument.
    AmountMismatch = 10,
    /// Constructor already ran.
    AlreadyInitialized = 11,
    /// `dest_bind` public input did not equal `Poseidon(DOMAIN_DEST, dest)` recomputed
    /// from the actual withdraw destination (or `dest` is not a classic account). (G13)
    BadDestBind = 12,
    /// `from` is not on the policy contract's deposit allow-list.
    DepositNotAllowed = 13,
}

#[contract]
pub struct Pool;

#[contractimpl]
impl Pool {
    /// Deploy-time configuration (immutable identity + verifiers + policy; mutable
    /// asp_root cache).
    pub fn __constructor(
        env: Env,
        pool_id: U256,
        network_id: U256,
        deposit_verifier: Address,
        transfer_verifier: Address,
        withdraw_verifier: Address,
        split_verifier: Address,
        policy: Address,
        asp_root: U256,
        admin: Address,
    ) -> Result<(), Error> {
        if config::is_set(&env) {
            return Err(Error::AlreadyInitialized);
        }
        config::set(
            &env,
            &Config {
                pool_id,
                network_id,
                deposit_verifier,
                transfer_verifier,
                withdraw_verifier,
                split_verifier,
                policy,
                asp_root,
                admin,
            },
        );
        nullifier::init(&env);
        Ok(())
    }

    /// Register an asset's SAC + decimals (admin only).
    pub fn register_asset(env: Env, asset_tag: U256, sac: Address, decimals: u32) {
        config::get(&env).admin.require_auth();
        assets::register(&env, &asset_tag, &sac, decimals);
    }

    /// Update the ASP approved-set root (admin only; → governance before mainnet).
    pub fn set_asp_root(env: Env, new_root: U256) {
        let mut cfg = config::get(&env);
        cfg.admin.require_auth();
        cfg.asp_root = new_root;
        config::set(&env, &cfg);
    }

    /// Pull the canonical approved-set root from the policy contract into the cached
    /// `asp_root` (permissionless — it only mirrors the policy's value; spends still
    /// verify the proof's `asp_root` against this cache, and clients self-check the
    /// reconstructed members against it). Call after the policy enrolls a member so the
    /// hot interior path sees the updated set.
    pub fn sync_asp_root(env: Env) {
        let mut cfg = config::get(&env);
        let root = env.invoke_contract::<U256>(
            &cfg.policy,
            &Symbol::new(&env, "asp_root"),
            Vec::new(&env),
        );
        cfg.asp_root = root;
        config::set(&env, &cfg);
    }

    /// Public deposit: lock `amount` of `asset_tag` from `from` into the vault and
    /// mint the proven shielded note (`out_commitment`).
    /// Public inputs: [domain_sep, asset_tag, epoch, amount, out_commitment].
    pub fn deposit(
        env: Env,
        from: Address,
        asset_tag: U256,
        amount: i128,
        public_inputs: Bytes,
        proof: Bytes,
        enc_note: Bytes,
        ephemeral_pub: BytesN<32>,
        view_tag: u32,
    ) -> Result<u32, Error> {
        from.require_auth();
        let cfg = config::get(&env);
        // Public-edge ASP gate (spec §8): `from` must be on the policy allow-list.
        if !policy_is_allowed(&env, &cfg.policy, &from) {
            return Err(Error::DepositNotAllowed);
        }
        let f = inputs::read_fields(&env, &public_inputs, inputs::DEPOSIT_N)?;

        let info = check_common(
            &env,
            &cfg,
            &f,
            domain::SELECTOR_DEPOSIT,
            &asset_tag,
        )?;
        require_amount(&env, &f.get(3).unwrap(), amount)?;

        verifier::verify(&env, &cfg.deposit_verifier, public_inputs, proof)?;

        // Pull real tokens into the vault, then append the minted commitment.
        token::TokenClient::new(&env, &info.sac).transfer(
            &from,
            &env.current_contract_address(),
            &amount,
        );
        let out_commitment = f.get(4).unwrap();
        let leaf = tree::append(&env, &out_commitment)?;

        env.events().publish(
            (symbol_short!("commit"), leaf),
            (out_commitment, enc_note, ephemeral_pub, view_tag),
        );
        emit_roots(&env);
        Ok(leaf)
    }

    /// Private interior transfer: 2-in / 2-out, no token movement.
    /// Public inputs: [domain_sep, asset_tag, epoch, commitment_root,
    /// nullifier_old_root, nullifier_new_root, nf0, nf1, out_cm0, out_cm1, asp_root].
    pub fn transfer(
        env: Env,
        asset_tag: U256,
        public_inputs: Bytes,
        proof: Bytes,
        enc_notes: Vec<Bytes>,
        ephemeral_pubs: Vec<BytesN<32>>,
        view_tags: Vec<u32>,
    ) -> Result<(), Error> {
        let cfg = config::get(&env);
        let f = inputs::read_fields(&env, &public_inputs, inputs::TRANSFER_N)?;

        check_common(&env, &cfg, &f, domain::SELECTOR_TRANSFER, &asset_tag)?;
        require_recent_root(&env, &f.get(3).unwrap())?;
        require_asp_root(&cfg, &f.get(10).unwrap())?;
        require_nullifier_base(&env, &f.get(4).unwrap())?;

        verifier::verify(&env, &cfg.transfer_verifier, public_inputs, proof)?;

        nullifier::set_root(&env, &f.get(5).unwrap());
        emit_nullifiers(&env, &f.get(6).unwrap(), &f.get(7).unwrap());
        append_and_emit(&env, &f.get(8).unwrap(), &enc_notes, &ephemeral_pubs, &view_tags, 0)?;
        append_and_emit(&env, &f.get(9).unwrap(), &enc_notes, &ephemeral_pubs, &view_tags, 1)?;
        emit_roots(&env);
        Ok(())
    }

    /// Private interior split: 2-in / 6-out (up to 5 recipients + change), no token
    /// movement. Same shape as `transfer` with its own selector + verifier; unused
    /// outputs are dummy (value 0) so the recipient count is hidden.
    /// Public inputs: [domain_sep, asset_tag, epoch, commitment_root, nullifier_old_root,
    /// nullifier_new_root, nf0, nf1, out_cm0..out_cm5, asp_root] (15 fields).
    pub fn split(
        env: Env,
        asset_tag: U256,
        public_inputs: Bytes,
        proof: Bytes,
        enc_notes: Vec<Bytes>,
        ephemeral_pubs: Vec<BytesN<32>>,
        view_tags: Vec<u32>,
    ) -> Result<(), Error> {
        let cfg = config::get(&env);
        let f = inputs::read_fields(&env, &public_inputs, inputs::SPLIT_N)?;

        check_common(&env, &cfg, &f, domain::SELECTOR_SPLIT, &asset_tag)?;
        require_recent_root(&env, &f.get(3).unwrap())?;
        require_asp_root(&cfg, &f.get(14).unwrap())?;
        require_nullifier_base(&env, &f.get(4).unwrap())?;

        verifier::verify(&env, &cfg.split_verifier, public_inputs, proof)?;

        nullifier::set_root(&env, &f.get(5).unwrap());
        emit_nullifiers(&env, &f.get(6).unwrap(), &f.get(7).unwrap());
        // Batch-append all 6 output commitments (root window + index written once, to
        // stay within the per-tx CPU budget), then emit each leaf's note event.
        let mut commits = Vec::new(&env);
        for k in 0..6u32 {
            commits.push_back(f.get(8 + k).unwrap());
        }
        let leaves = tree::append_many(&env, &commits)?;
        for k in 0..6u32 {
            let leaf = leaves.get(k).unwrap();
            env.events().publish(
                (symbol_short!("commit"), leaf),
                (
                    commits.get(k).unwrap(),
                    enc_notes.get(k),
                    ephemeral_pubs.get(k),
                    view_tags.get(k),
                ),
            );
        }
        emit_roots(&env);
        Ok(())
    }

    /// Admin: upgrade the contract wasm in place (future features without redeploy).
    /// Testnet: dev-controlled; -> governance/multisig before mainnet (handoff §8).
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        config::get(&env).admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }

    /// Public withdraw: spend shielded note(s), release `amount` of `asset_tag` to
    /// `dest`, and re-commit the shielded change.
    /// Public inputs: [domain_sep, asset_tag, epoch, commitment_root,
    /// nullifier_old_root, nullifier_new_root, nf0, nf1, change_commitment, asp_root,
    /// amount, dest_bind].
    pub fn withdraw(
        env: Env,
        dest: Address,
        asset_tag: U256,
        amount: i128,
        public_inputs: Bytes,
        proof: Bytes,
    ) -> Result<u32, Error> {
        let cfg = config::get(&env);
        let f = inputs::read_fields(&env, &public_inputs, inputs::WITHDRAW_N)?;

        let info = check_common(&env, &cfg, &f, domain::SELECTOR_WITHDRAW, &asset_tag)?;
        require_recent_root(&env, &f.get(3).unwrap())?;
        require_asp_root(&cfg, &f.get(9).unwrap())?;
        require_amount(&env, &f.get(10).unwrap(), amount)?;
        // dest_bind binds the destination so a valid proof can't be redirected (G13):
        // recompute Poseidon(DOMAIN_DEST, dest's ed25519 key) from the real `dest` and
        // require it to equal the proof's `dest_bind` public input. (Closes the Z4 debt.)
        if f.get(11).unwrap() != compute_dest_bind(&env, &dest)? {
            return Err(Error::BadDestBind);
        }
        require_nullifier_base(&env, &f.get(4).unwrap())?;

        verifier::verify(&env, &cfg.withdraw_verifier, public_inputs, proof)?;

        nullifier::set_root(&env, &f.get(5).unwrap());
        emit_nullifiers(&env, &f.get(6).unwrap(), &f.get(7).unwrap());
        let leaf = tree::append(&env, &f.get(8).unwrap())?;
        env.events()
            .publish((symbol_short!("commit"), leaf), f.get(8).unwrap());

        // Release real tokens from the vault to the destination.
        token::TokenClient::new(&env, &info.sac).transfer(
            &env.current_contract_address(),
            &dest,
            &amount,
        );
        emit_roots(&env);
        Ok(leaf)
    }

    /// The current commitment-tree root.
    pub fn commitment_root(env: Env) -> U256 {
        tree::current_root(&env)
    }

    /// The current nullifier-accumulator root.
    pub fn nullifier_root(env: Env) -> U256 {
        nullifier::current_root(&env)
    }
}

// ----------------------------- shared validation -----------------------------

fn current_epoch(env: &Env) -> u64 {
    env.ledger().sequence() as u64 / LEDGER_PER_EPOCH
}

/// Recompute `dest_bind = Poseidon(DOMAIN_DEST, dest_ed25519_pubkey)` from the actual
/// withdraw destination (G13). `dest` must be a classic account (`G…`); its master-key
/// ed25519 bytes are the field preimage, matching the client's `Fr(pk.0)` (big-endian).
/// A contract destination (`C…`, no ed25519 key) is rejected.
fn compute_dest_bind(env: &Env, dest: &Address) -> Result<U256, Error> {
    use soroban_sdk::address_payload::AddressPayload;
    let key = match dest.to_payload().ok_or(Error::BadDestBind)? {
        AddressPayload::AccountIdPublicKeyEd25519(k) => k,
        AddressPayload::ContractIdHash(_) => return Err(Error::BadDestBind),
    };
    let dest_field = U256::from_be_bytes(env, &Bytes::from_array(env, &key.to_array()));
    let mut inputs = Vec::new(env);
    inputs.push_back(U256::from_u128(env, DOMAIN_DEST as u128));
    inputs.push_back(dest_field);
    Ok(poseidon::hash(env, &inputs))
}

/// Cross-contract read of the policy contract's deposit allow-list.
fn policy_is_allowed(env: &Env, policy: &Address, who: &Address) -> bool {
    let mut args: Vec<soroban_sdk::Val> = Vec::new(env);
    args.push_back(who.into_val(env));
    env.invoke_contract::<bool>(policy, &Symbol::new(env, "is_allowed"), args)
}

/// Checks common to all three entrypoints: domain separation, asset registration +
/// match, and epoch. Returns the asset's registry info.
fn check_common(
    env: &Env,
    cfg: &Config,
    f: &Vec<U256>,
    selector: u32,
    asset_tag: &U256,
) -> Result<assets::AssetInfo, Error> {
    let expected_domain = domain::compute_domain_sep(env, &cfg.pool_id, &cfg.network_id, selector);
    if f.get(0).unwrap() != expected_domain {
        return Err(Error::BadDomainSep);
    }
    if &f.get(1).unwrap() != asset_tag {
        return Err(Error::AssetMismatch);
    }
    let info = assets::get(env, asset_tag).ok_or(Error::AssetMismatch)?;
    if f.get(2).unwrap() != U256::from_u128(env, current_epoch(env) as u128) {
        return Err(Error::BadEpoch);
    }
    Ok(info)
}

fn require_recent_root(env: &Env, commitment_root: &U256) -> Result<(), Error> {
    if tree::root_is_recent(env, commitment_root) {
        Ok(())
    } else {
        Err(Error::StaleCommitmentRoot)
    }
}

fn require_asp_root(cfg: &Config, asp_root: &U256) -> Result<(), Error> {
    if &cfg.asp_root == asp_root {
        Ok(())
    } else {
        Err(Error::BadAspRoot)
    }
}

fn require_nullifier_base(env: &Env, old_root: &U256) -> Result<(), Error> {
    if &nullifier::current_root(env) == old_root {
        Ok(())
    } else {
        Err(Error::NullifierRootMismatch)
    }
}

fn require_amount(env: &Env, amount_field: &U256, amount: i128) -> Result<(), Error> {
    if amount < 0 {
        return Err(Error::AmountMismatch);
    }
    if amount_field == &U256::from_u128(env, amount as u128) {
        Ok(())
    } else {
        Err(Error::AmountMismatch)
    }
}

// ----------------------------- events (for the indexer) -----------------------------

fn append_and_emit(
    env: &Env,
    commitment: &U256,
    enc_notes: &Vec<Bytes>,
    ephemeral_pubs: &Vec<BytesN<32>>,
    view_tags: &Vec<u32>,
    i: u32,
) -> Result<(), Error> {
    let leaf = tree::append(env, commitment)?;
    env.events().publish(
        (symbol_short!("commit"), leaf),
        (
            commitment.clone(),
            enc_notes.get(i),
            ephemeral_pubs.get(i),
            view_tags.get(i),
        ),
    );
    Ok(())
}

fn emit_nullifiers(env: &Env, nf0: &U256, nf1: &U256) {
    let topic: Symbol = symbol_short!("nullif");
    env.events().publish((topic.clone(),), nf0.clone());
    env.events().publish((topic,), nf1.clone());
}

fn emit_roots(env: &Env) {
    env.events().publish(
        (symbol_short!("roots"),),
        (tree::current_root(env), nullifier::current_root(env)),
    );
}

#[cfg(test)]
mod testutils {
    use soroban_sdk::{contract, contractimpl, Address, Bytes, Env};

    /// A constructor-less contract used purely to obtain a storage/contract
    /// context in module unit tests (tree/nullifier/verifier logic). The real
    /// `Pool` requires constructor args; logic tests don't need a configured pool.
    #[contract]
    pub struct Harness;

    pub fn harness(env: &Env) -> Address {
        env.register(Harness, ())
    }

    /// Stub verifier that always accepts — lets entrypoint tests exercise the
    /// state transition (checks/append/nullifier/token) without a real proof. The
    /// real cryptographic accept/reject path is covered in `verifier::tests` and
    /// on testnet (Z4 round-trip).
    #[contract]
    pub struct OkVerifier;

    #[contractimpl]
    impl OkVerifier {
        pub fn verify_proof(_env: Env, _public_inputs: Bytes, _proof: Bytes) {}
    }
}

#[cfg(test)]
mod entrypoint_tests {
    use super::*;
    use crate::testutils::OkVerifier;
    use policy::{Policy, PolicyClient};
    use soroban_sdk::{
        testutils::Address as _,
        token::{StellarAssetClient, TokenClient},
        Address, Bytes, BytesN, Env,
    };

    fn field_blob(env: &Env, fields: &[U256]) -> Bytes {
        let mut blob = Bytes::new(env);
        for f in fields {
            let mut a = [0u8; 32];
            f.to_be_bytes().copy_into_slice(&mut a);
            blob.append(&Bytes::from_array(env, &a));
        }
        blob
    }

    struct Fixture {
        env: Env,
        pool_addr: Address,
        policy_addr: Address,
        admin: Address,
        from: Address,
        asset_tag: U256,
        sac: Address,
        pool_id: U256,
        network_id: U256,
    }

    impl Fixture {
        fn policy(&self) -> PolicyClient<'_> {
            PolicyClient::new(&self.env, &self.policy_addr)
        }
    }

    impl Fixture {
        fn pool(&self) -> PoolClient<'_> {
            PoolClient::new(&self.env, &self.pool_addr)
        }
    }

    fn setup() -> Fixture {
        let env = Env::default();
        env.mock_all_auths();
        env.cost_estimate().budget().reset_unlimited();

        let admin = Address::generate(&env);
        let verifier = env.register(OkVerifier, ());
        let pool_id = U256::from_u32(&env, 7);
        let network_id = U256::from_u32(&env, 42);
        let asp_root = U256::from_u32(&env, 0);

        // Policy contract: allow `from` to deposit.
        let from = Address::generate(&env);
        let policy_addr = env.register(Policy, (admin.clone(),));
        let policy = PolicyClient::new(&env, &policy_addr);
        policy.set_allowed(&from, &true);

        let pool_addr = env.register(
            Pool,
            (
                pool_id.clone(),
                network_id.clone(),
                verifier.clone(),
                verifier.clone(),
                verifier.clone(),
                verifier.clone(),
                policy_addr.clone(),
                asp_root,
                admin.clone(),
            ),
        );

        // A test SAC, registered as asset_tag = 1.
        let sac = env.register_stellar_asset_contract_v2(admin.clone());
        let sac_addr = sac.address();
        let asset_tag = U256::from_u32(&env, 1);
        PoolClient::new(&env, &pool_addr).register_asset(&asset_tag, &sac_addr, &6);

        // Fund `from` with 1000 base units.
        StellarAssetClient::new(&env, &sac_addr).mint(&from, &1000);

        Fixture {
            env,
            pool_addr,
            policy_addr,
            admin,
            from,
            asset_tag,
            sac: sac_addr,
            pool_id,
            network_id,
        }
    }

    fn deposit_inputs(f: &Fixture, amount: u128, out_commitment: u32) -> Bytes {
        let env = &f.env;
        let domain_sep = domain::compute_domain_sep(
            env,
            &f.pool_id,
            &f.network_id,
            domain::SELECTOR_DEPOSIT,
        );
        field_blob(
            env,
            &[
                domain_sep,
                f.asset_tag.clone(),
                U256::from_u32(env, 0), // epoch 0 (default ledger sequence)
                U256::from_u128(env, amount),
                U256::from_u32(env, out_commitment),
            ],
        )
    }

    #[test]
    fn deposit_locks_tokens_and_appends_commitment() {
        let f = setup();
        let env = &f.env;
        let pi = deposit_inputs(&f, 1000, 0xc0ffee);
        let proof = Bytes::new(env);
        let enc = Bytes::new(env);
        let eph = BytesN::from_array(env, &[0u8; 32]);

        let leaf = f
            .pool()
            .deposit(&f.from, &f.asset_tag, &1000, &pi, &proof, &enc, &eph, &0);
        assert_eq!(leaf, 0);

        // Real tokens moved from `from` into the pool vault.
        let token = TokenClient::new(env, &f.sac);
        assert_eq!(token.balance(&f.from), 0);
        assert_eq!(token.balance(&f.pool_addr), 1000);

        // The minted commitment defines the new commitment root: appending the same
        // leaf to a fresh tree must yield the pool's root.
        let want_leaf = U256::from_u32(env, 0xc0ffee);
        let solo = crate::testutils::harness(env);
        let solo_root = env.as_contract(&solo, || {
            tree::append(env, &want_leaf).unwrap();
            tree::current_root(env)
        });
        assert_eq!(f.pool().commitment_root(), solo_root);
    }

    #[test]
    fn deposit_rejects_amount_mismatch() {
        let f = setup();
        let env = &f.env;
        // Public `amount` field = 999 but the token-edge argument = 1000.
        let pi = deposit_inputs(&f, 999, 0xc0ffee);
        let proof = Bytes::new(env);
        let enc = Bytes::new(env);
        let eph = BytesN::from_array(env, &[0u8; 32]);

        let res = f
            .pool()
            .try_deposit(&f.from, &f.asset_tag, &1000, &pi, &proof, &enc, &eph, &0);
        assert_eq!(res, Err(Ok(Error::AmountMismatch)));
    }

    #[test]
    fn deposit_rejects_bad_domain_sep() {
        let f = setup();
        let env = &f.env;
        // Hand-built inputs with a bogus domain_sep.
        let pi = field_blob(
            env,
            &[
                U256::from_u32(env, 0xbad),
                f.asset_tag.clone(),
                U256::from_u32(env, 0),
                U256::from_u128(env, 1000),
                U256::from_u32(env, 0xc0ffee),
            ],
        );
        let proof = Bytes::new(env);
        let enc = Bytes::new(env);
        let eph = BytesN::from_array(env, &[0u8; 32]);
        let res = f
            .pool()
            .try_deposit(&f.from, &f.asset_tag, &1000, &pi, &proof, &enc, &eph, &0);
        assert_eq!(res, Err(Ok(Error::BadDomainSep)));
    }

    #[test]
    fn deposit_rejects_address_not_on_allowlist() {
        let f = setup();
        let env = &f.env;
        // A funded depositor who is NOT on the policy allow-list.
        let stranger = Address::generate(env);
        StellarAssetClient::new(env, &f.sac).mint(&stranger, &1000);
        let pi = deposit_inputs(&f, 1000, 0xc0ffee);
        let proof = Bytes::new(env);
        let enc = Bytes::new(env);
        let eph = BytesN::from_array(env, &[0u8; 32]);

        let res = f
            .pool()
            .try_deposit(&stranger, &f.asset_tag, &1000, &pi, &proof, &enc, &eph, &0);
        assert_eq!(res, Err(Ok(Error::DepositNotAllowed)));

        // Once allow-listed, the same deposit succeeds.
        f.policy().set_allowed(&stranger, &true);
        let leaf = f
            .pool()
            .deposit(&stranger, &f.asset_tag, &1000, &pi, &proof, &enc, &eph, &0);
        assert_eq!(leaf, 0);
        let _ = &f.admin; // admin retained for fixture completeness
    }

    /// Build valid `withdraw` public inputs for a fresh pool (recent commitment root +
    /// the stored nullifier base root), parameterized by `amount` and `dest_bind`.
    fn withdraw_inputs(f: &Fixture, amount: u128, dest_bind: U256) -> Bytes {
        let env = &f.env;
        let domain_sep =
            domain::compute_domain_sep(env, &f.pool_id, &f.network_id, domain::SELECTOR_WITHDRAW);
        field_blob(
            env,
            &[
                domain_sep,
                f.asset_tag.clone(),
                U256::from_u32(env, 0),       // epoch 0
                f.pool().commitment_root(),   // recent
                f.pool().nullifier_root(),    // == stored accumulator base
                U256::from_u32(env, 0xbeef),  // new nullifier root (any)
                U256::from_u32(env, 0xa),     // nf0
                U256::from_u32(env, 0xb),     // nf1
                U256::from_u32(env, 0xc0ffee), // change_commitment
                U256::from_u32(env, 0),       // asp_root (matches cfg)
                U256::from_u128(env, amount), // amount
                dest_bind,
            ],
        )
    }

    /// Deposit once so the commitment-tree root window is non-empty (a fresh pool has no
    /// "recent" root, which `require_recent_root` checks before the dest_bind check).
    fn prime_recent_root(f: &Fixture) {
        let env = &f.env;
        let pi = deposit_inputs(f, 1000, 0xc0ffee);
        f.pool().deposit(
            &f.from,
            &f.asset_tag,
            &1000,
            &pi,
            &Bytes::new(env),
            &Bytes::new(env),
            &BytesN::from_array(env, &[0u8; 32]),
            &0,
        );
    }

    #[test]
    fn withdraw_rejects_dest_bind_not_matching_destination() {
        use soroban_sdk::address_payload::AddressPayload;
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);

        // A classic account destination (G…). A proof whose `dest_bind` does NOT equal
        // Poseidon(DOMAIN_DEST, this dest) is REJECTED — a valid withdraw proof can't be
        // redirected to another address (G13). (`0xdead` is not the real binding.)
        let key = BytesN::from_array(env, &[0x22u8; 32]);
        let dest = AddressPayload::AccountIdPublicKeyEd25519(key).to_address(env);
        let pi_bad = withdraw_inputs(&f, 400, U256::from_u32(env, 0xdead));
        let res = f
            .pool()
            .try_withdraw(&dest, &f.asset_tag, &400, &pi_bad, &Bytes::new(env));
        assert_eq!(res, Err(Ok(Error::BadDestBind)));
        // Sanity: the correct binding for this dest is some specific non-`0xdead` value
        // (so the rejection above was a genuine mismatch, not a zero/degenerate check).
        assert_ne!(compute_dest_bind(env, &dest).unwrap(), U256::from_u32(env, 0xdead));
        // (End-to-end ACCEPTANCE — a matching dest_bind releasing real tokens to a
        // trustlined dest — is proven by the live testnet lifecycle in the Rust core,
        // which now fails if the client's dest_bind formula disagrees with this contract.)
    }

    #[test]
    fn compute_dest_bind_matches_independent_poseidon() {
        // The contract's dest_bind == Poseidon(DOMAIN_DEST, dest's ed25519 key), the SAME
        // formula the client (withdraw.rs) builds the proof's public input from. Computed
        // two ways here; both use the frozen-parity `poseidon::hash`.
        use soroban_sdk::address_payload::AddressPayload;
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        let key = [0x22u8; 32];
        let dest =
            AddressPayload::AccountIdPublicKeyEd25519(BytesN::from_array(&env, &key)).to_address(&env);

        let got = compute_dest_bind(&env, &dest).unwrap();

        let mut inputs = Vec::new(&env);
        inputs.push_back(U256::from_u128(&env, DOMAIN_DEST as u128));
        inputs.push_back(U256::from_be_bytes(&env, &Bytes::from_array(&env, &key)));
        assert_eq!(got, poseidon::hash(&env, &inputs));
        assert_ne!(got, U256::from_u32(&env, 0), "a real binding is non-zero");
    }

    #[test]
    fn withdraw_rejects_contract_destination() {
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);
        StellarAssetClient::new(env, &f.sac).mint(&f.pool_addr, &1000);
        // A contract address (C…) has no ed25519 master key, so it can't be a withdraw
        // destination under the dest_bind scheme.
        let dest = Address::generate(env);
        let pi = withdraw_inputs(&f, 400, U256::from_u32(env, 1));
        let res = f
            .pool()
            .try_withdraw(&dest, &f.asset_tag, &400, &pi, &Bytes::new(env));
        assert_eq!(res, Err(Ok(Error::BadDestBind)));
    }
}
