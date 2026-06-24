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
mod channel;
mod config;
mod domain;
mod escrow;
mod inputs;
mod nullifier;
mod poseidon;
mod reserve;
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
    /// No escrow exists for the given id.
    NoSuchEscrow = 14,
    /// Escrow is not Open (already released, or otherwise closed to this operation).
    EscrowClosed = 15,
    /// `c_raised_old` public input does not equal the escrow's stored running commitment.
    BadRaisedRoot = 16,
    /// `commitment_hash` public input does not equal the expected escrow/contribution commitment.
    BadCommitmentHash = 17,
    /// `floor` public input does not equal the expected threshold (target, or 0).
    BadFloor = 18,
    /// `recipient_bind` public input does not equal the escrow's payee/refund binding.
    BadRecipientBind = 19,
    /// The escrow deadline has not yet passed (required for this operation).
    DeadlineNotPassed = 20,
    /// No contribution exists at the given (escrow id, index).
    NoSuchContribution = 21,
    /// This contribution was already refunded.
    AlreadyRefunded = 22,
    /// Refund is not allowed for this escrow's mode (only AllOrNothing refunds).
    RefundNotAllowed = 23,
    /// `mode` passed to `open_escrow` is not a valid escrow mode.
    BadMode = 24,
    /// The passed running point `(raised_x, raised_y)` does not hash to `c_raised_new`.
    BadRaisedPoint = 25,
    /// No channel exists for the given id.
    NoSuchChannel = 26,
    /// Channel is not Open (already closed or reclaimed).
    ChannelClosed = 27,
    /// `auth_key` public input does not equal the channel's stored signing-key handle.
    BadAuthKey = 28,
    /// `valid_after_ledger` public input is greater than the current ledger (drawing a future
    /// period early).
    ValidAfterNotReached = 29,
    /// Swap source and destination assets are the same.
    BadSwapAssets = 30,
    /// The AMM reserve cannot deliver the proof's `value_b` (price moved / insufficient liquidity),
    /// or a reserve-withdraw exceeds the available reserve.
    ReserveTooLow = 31,
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
        escrow_contribute_verifier: Address,
        escrow_payout_verifier: Address,
        channel_close_verifier: Address,
        swap_verifier: Address,
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
                escrow_contribute_verifier,
                escrow_payout_verifier,
                channel_close_verifier,
                swap_verifier,
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

    /// Open a hidden-sum escrow (permissionless). Returns the new escrow id. `payee_bind` hides
    /// the payee; the running commitment is seeded to the identity. (building block B)
    pub fn open_escrow(
        env: Env,
        asset_tag: U256,
        target: u64,
        deadline: u64,
        mode: u32,
        payee_bind: U256,
    ) -> Result<u64, Error> {
        if mode != escrow::MODE_ALL_OR_NOTHING && mode != escrow::MODE_KEEP_WHAT_YOU_RAISE {
            return Err(Error::BadMode);
        }
        // Asset must be registered (a payout can only land in a real vault asset).
        assets::get(&env, &asset_tag).ok_or(Error::AssetMismatch)?;
        let id = escrow::open(&env, asset_tag.clone(), target, deadline, mode, payee_bind.clone());
        env.events().publish(
            (symbol_short!("escopen"), id),
            (asset_tag, target, deadline, mode, payee_bind),
        );
        Ok(id)
    }

    /// Contribute to an escrow: spend one owned pool note (HIDDEN amount), fold it into the
    /// running commitment, record the refund claim. `payee_enc` carries (amount, r) encrypted to
    /// the payee so only they can open the total. Returns the contribution index. (building block B)
    /// Public inputs: [domain_sep, asset_tag, epoch, commitment_root, nullifier_old_root,
    /// nullifier_new_root, nf0, nf1, change_commitment, asp_root, c_raised_old, c_raised_new,
    /// c_contrib, refund_bind].
    #[allow(clippy::too_many_arguments)]
    pub fn escrow_contribute(
        env: Env,
        escrow_id: u64,
        asset_tag: U256,
        public_inputs: Bytes,
        proof: Bytes,
        change_enc: Bytes,
        change_eph: BytesN<32>,
        change_tag: u32,
        payee_enc: Bytes,
        raised_x: U256,
        raised_y: U256,
    ) -> Result<u32, Error> {
        let cfg = config::get(&env);
        let mut e = escrow::get(&env, escrow_id).ok_or(Error::NoSuchEscrow)?;
        if e.status != escrow::STATUS_OPEN {
            return Err(Error::EscrowClosed);
        }
        let f = inputs::read_fields(&env, &public_inputs, inputs::ESCROW_CONTRIBUTE_N)?;
        check_common(&env, &cfg, &f, domain::SELECTOR_ESCROW_CONTRIBUTE, &asset_tag)?;
        if asset_tag != e.asset_tag {
            return Err(Error::AssetMismatch);
        }
        require_recent_root(&env, &f.get(3).unwrap())?;
        require_asp_root(&cfg, &f.get(9).unwrap())?;
        require_nullifier_base(&env, &f.get(4).unwrap())?;
        // Running-commitment chaining (same old/new pattern as the nullifier accumulator).
        let c_raised_new = f.get(11).unwrap();
        if f.get(10).unwrap() != e.c_raised {
            return Err(Error::BadRaisedRoot);
        }
        // The passed running POINT (raised_x, raised_y) must hash to the proof's c_raised_new, so
        // the cached point a later contributor reads is bound to the verified commitment.
        let mut pt = Vec::new(&env);
        pt.push_back(raised_x.clone());
        pt.push_back(raised_y.clone());
        if poseidon::hash(&env, &pt) != c_raised_new {
            return Err(Error::BadRaisedPoint);
        }

        verifier::verify(&env, &cfg.escrow_contribute_verifier, public_inputs, proof)?;

        nullifier::set_root(&env, &f.get(5).unwrap());
        emit_nullifiers(&env, &f.get(6).unwrap(), &f.get(7).unwrap());
        let leaf = tree::append(&env, &f.get(8).unwrap())?;
        env.events().publish(
            (symbol_short!("commit"), leaf),
            (f.get(8).unwrap(), change_enc, change_eph, change_tag),
        );

        let idx = e.n_contrib;
        e.c_raised = c_raised_new;
        e.raised_x = raised_x;
        e.raised_y = raised_y;
        e.n_contrib += 1;
        escrow::set(&env, escrow_id, &e);
        escrow::set_contrib(
            &env,
            escrow_id,
            idx,
            &escrow::Contribution {
                c_contrib: f.get(12).unwrap(),
                refund_bind: f.get(13).unwrap(),
                refunded: false,
            },
        );
        // Emit the (amount,r)-to-payee blob so the payee can accumulate the true total.
        env.events()
            .publish((symbol_short!("escrcon"), escrow_id, idx), payee_enc);
        emit_roots(&env);
        Ok(idx)
    }

    /// Release the escrow to the payee. AllOrNothing: the proof must show raised >= target
    /// (floor=target), allowed any time. KeepWhatYouRaise: requires the deadline passed (floor=0).
    /// Mints one shielded payout note; marks the escrow Released. (building block B)
    /// Public inputs: [domain_sep, asset_tag, epoch, commitment_hash, floor, out_commitment,
    /// recipient_bind].
    #[allow(clippy::too_many_arguments)]
    pub fn escrow_release(
        env: Env,
        escrow_id: u64,
        public_inputs: Bytes,
        proof: Bytes,
        enc_note: Bytes,
        ephemeral_pub: BytesN<32>,
        view_tag: u32,
    ) -> Result<u32, Error> {
        let cfg = config::get(&env);
        let mut e = escrow::get(&env, escrow_id).ok_or(Error::NoSuchEscrow)?;
        if e.status != escrow::STATUS_OPEN {
            return Err(Error::EscrowClosed);
        }
        let floor = if e.mode == escrow::MODE_KEEP_WHAT_YOU_RAISE {
            if (env.ledger().sequence() as u64) <= e.deadline {
                return Err(Error::DeadlineNotPassed);
            }
            U256::from_u32(&env, 0)
        } else {
            U256::from_u128(&env, e.target as u128)
        };
        let leaf = escrow_payout(
            &env,
            &cfg,
            &e.asset_tag,
            &public_inputs,
            proof,
            &e.c_raised,
            &floor,
            &e.payee_bind,
            enc_note,
            ephemeral_pub,
            view_tag,
        )?;
        e.status = escrow::STATUS_RELEASED;
        escrow::set(&env, escrow_id, &e);
        emit_roots(&env);
        Ok(leaf)
    }

    /// Refund one contribution (AllOrNothing fail path only): requires the deadline passed, the
    /// escrow not released, and this contribution not already refunded. Mints the contribution's
    /// amount back to the bound contributor. (building block B)
    /// Public inputs: same payout layout as `escrow_release` (floor must be 0).
    #[allow(clippy::too_many_arguments)]
    pub fn escrow_refund(
        env: Env,
        escrow_id: u64,
        contrib_index: u32,
        public_inputs: Bytes,
        proof: Bytes,
        enc_note: Bytes,
        ephemeral_pub: BytesN<32>,
        view_tag: u32,
    ) -> Result<u32, Error> {
        let cfg = config::get(&env);
        let e = escrow::get(&env, escrow_id).ok_or(Error::NoSuchEscrow)?;
        if e.mode != escrow::MODE_ALL_OR_NOTHING {
            return Err(Error::RefundNotAllowed);
        }
        if e.status == escrow::STATUS_RELEASED {
            return Err(Error::EscrowClosed);
        }
        if (env.ledger().sequence() as u64) <= e.deadline {
            return Err(Error::DeadlineNotPassed);
        }
        let mut c =
            escrow::get_contrib(&env, escrow_id, contrib_index).ok_or(Error::NoSuchContribution)?;
        if c.refunded {
            return Err(Error::AlreadyRefunded);
        }
        let leaf = escrow_payout(
            &env,
            &cfg,
            &e.asset_tag,
            &public_inputs,
            proof,
            &c.c_contrib,
            &U256::from_u32(&env, 0),
            &c.refund_bind,
            enc_note,
            ephemeral_pub,
            view_tag,
        )?;
        c.refunded = true;
        escrow::set_contrib(&env, escrow_id, contrib_index, &c);
        emit_roots(&env);
        Ok(leaf)
    }

    /// Public escrow state for the UI.
    pub fn escrow(env: Env, escrow_id: u64) -> Result<escrow::Escrow, Error> {
        escrow::get(&env, escrow_id).ok_or(Error::NoSuchEscrow)
    }

    /// The id the next `open_escrow` will assign (monotonic). Lets the opener learn its escrow's
    /// id deterministically (the wallet submits one open at a time).
    pub fn next_escrow_id(env: Env) -> u64 {
        escrow::next_id(&env)
    }

    /// Open a subscription channel (subscriber). Spends one owned pool note of a HIDDEN `cap` via an
    /// escrow_contribute-shaped proof (selector 5, reused frozen VK): the proof's `c_contrib` is the
    /// cap commitment (provably commits to the vaulted amount) and its `refund_bind` becomes the
    /// channel's `subscriber_bind`. `merchant_bind` / `auth_key` / `expiry` are subscriber-supplied
    /// channel params; `merchant_enc` seals (cap, r_cap, salts, ramp) to the merchant. Returns the
    /// channel id. (building block B phase 2)
    /// Public inputs: the 14-field escrow_contribute layout.
    #[allow(clippy::too_many_arguments)]
    pub fn open_channel(
        env: Env,
        asset_tag: U256,
        public_inputs: Bytes,
        proof: Bytes,
        change_enc: Bytes,
        change_eph: BytesN<32>,
        change_tag: u32,
        merchant_bind: U256,
        auth_key: U256,
        expiry: u64,
        merchant_enc: Bytes,
    ) -> Result<u64, Error> {
        let cfg = config::get(&env);
        // A payout can only land in a real vault asset.
        assets::get(&env, &asset_tag).ok_or(Error::AssetMismatch)?;
        let f = inputs::read_fields(&env, &public_inputs, inputs::ESCROW_CONTRIBUTE_N)?;
        check_common(&env, &cfg, &f, domain::SELECTOR_ESCROW_CONTRIBUTE, &asset_tag)?;
        require_recent_root(&env, &f.get(3).unwrap())?;
        require_asp_root(&cfg, &f.get(9).unwrap())?;
        require_nullifier_base(&env, &f.get(4).unwrap())?;
        // A channel is a single fold from the standard seed, so the cap commitment IS the proof's
        // c_contrib (point_hash(Commit(cap, r_cap))). Bind c_raised_old to the seed so the cap was
        // committed onto a known starting point (defensive; the cap soundness is c_contrib +
        // in-circuit conservation, not the running commitment).
        if f.get(10).unwrap() != escrow::init_c_raised(&env) {
            return Err(Error::BadRaisedRoot);
        }

        verifier::verify(&env, &cfg.escrow_contribute_verifier, public_inputs, proof)?;

        // Spend side: advance the nullifier accumulator, append the shielded change note.
        nullifier::set_root(&env, &f.get(5).unwrap());
        emit_nullifiers(&env, &f.get(6).unwrap(), &f.get(7).unwrap());
        let leaf = tree::append(&env, &f.get(8).unwrap())?;
        env.events().publish(
            (symbol_short!("commit"), leaf),
            (f.get(8).unwrap(), change_enc, change_eph, change_tag),
        );

        let cap_commitment = f.get(12).unwrap(); // c_contrib
        let subscriber_bind = f.get(13).unwrap(); // refund_bind
        let id = channel::open(
            &env,
            asset_tag.clone(),
            cap_commitment,
            auth_key,
            merchant_bind,
            subscriber_bind,
            expiry,
        );
        // Emit the channel-open + the merchant secrets blob (cap, r_cap, salts, ramp) so the merchant
        // can scan, decrypt, and later close.
        env.events()
            .publish((symbol_short!("chanopen"), id), (asset_tag, expiry, merchant_enc));
        emit_roots(&env);
        Ok(id)
    }

    /// Close a channel (merchant). Verifies a channel_close proof (selector 7) that opens the cap +
    /// the subscriber-signed cumulative commitment, checks the subscriber's signature in-circuit,
    /// and proves conservation; the contract asserts the proof's cap_hash / auth_key / merchant_bind
    /// / subscriber_bind match the stored channel and that `valid_after_ledger` has elapsed, then
    /// mints `drawn` to the merchant and `cap - drawn` to the subscriber. (building block B phase 2)
    /// Public inputs: [domain_sep, asset_tag, epoch, cap_hash, auth_key, valid_after_ledger,
    /// merchant_out, subscriber_out, merchant_bind, subscriber_bind].
    #[allow(clippy::too_many_arguments)]
    pub fn close_channel(
        env: Env,
        channel_id: u64,
        public_inputs: Bytes,
        proof: Bytes,
        merchant_enc: Bytes,
        merchant_eph: BytesN<32>,
        merchant_tag: u32,
        subscriber_enc: Bytes,
        subscriber_eph: BytesN<32>,
        subscriber_tag: u32,
    ) -> Result<(), Error> {
        let cfg = config::get(&env);
        let mut ch = channel::get(&env, channel_id).ok_or(Error::NoSuchChannel)?;
        if ch.status != channel::STATUS_OPEN {
            return Err(Error::ChannelClosed);
        }
        let f = inputs::read_fields(&env, &public_inputs, inputs::CHANNEL_CLOSE_N)?;
        check_common(&env, &cfg, &f, domain::SELECTOR_CHANNEL_CLOSE, &ch.asset_tag)?;
        if f.get(3).unwrap() != ch.cap_commitment {
            return Err(Error::BadCommitmentHash);
        }
        if f.get(4).unwrap() != ch.auth_key {
            return Err(Error::BadAuthKey);
        }
        // The signed period must have elapsed: valid_after_ledger <= current ledger.
        let now = U256::from_u128(&env, env.ledger().sequence() as u128);
        if f.get(5).unwrap() > now {
            return Err(Error::ValidAfterNotReached);
        }
        if f.get(8).unwrap() != ch.merchant_bind {
            return Err(Error::BadRecipientBind);
        }
        if f.get(9).unwrap() != ch.subscriber_bind {
            return Err(Error::BadRecipientBind);
        }

        verifier::verify(&env, &cfg.channel_close_verifier, public_inputs, proof)?;

        // Mint both notes (value already vaulted at open — no token move): drawn -> merchant,
        // remainder -> subscriber.
        let m_leaf = tree::append(&env, &f.get(6).unwrap())?;
        env.events().publish(
            (symbol_short!("commit"), m_leaf),
            (f.get(6).unwrap(), merchant_enc, merchant_eph, merchant_tag),
        );
        let s_leaf = tree::append(&env, &f.get(7).unwrap())?;
        env.events().publish(
            (symbol_short!("commit"), s_leaf),
            (f.get(7).unwrap(), subscriber_enc, subscriber_eph, subscriber_tag),
        );

        ch.status = channel::STATUS_CLOSED;
        channel::set(&env, channel_id, &ch);
        emit_roots(&env);
        Ok(())
    }

    /// Reclaim the full cap (subscriber, expiry path): if the merchant never closed, after `expiry`
    /// the subscriber sweeps the whole vaulted cap back. Verifies an escrow_payout proof (selector 6,
    /// reused frozen VK) opening the cap commitment with floor 0, recipient_bind == subscriber_bind.
    /// (building block B phase 2)
    /// Public inputs: the 7-field escrow_payout layout (floor must be 0).
    #[allow(clippy::too_many_arguments)]
    pub fn channel_reclaim(
        env: Env,
        channel_id: u64,
        public_inputs: Bytes,
        proof: Bytes,
        enc_note: Bytes,
        ephemeral_pub: BytesN<32>,
        view_tag: u32,
    ) -> Result<u32, Error> {
        let cfg = config::get(&env);
        let mut ch = channel::get(&env, channel_id).ok_or(Error::NoSuchChannel)?;
        if ch.status != channel::STATUS_OPEN {
            return Err(Error::ChannelClosed);
        }
        if (env.ledger().sequence() as u64) <= ch.expiry {
            return Err(Error::DeadlineNotPassed);
        }
        let leaf = escrow_payout(
            &env,
            &cfg,
            &ch.asset_tag,
            &public_inputs,
            proof,
            &ch.cap_commitment,
            &U256::from_u32(&env, 0),
            &ch.subscriber_bind,
            enc_note,
            ephemeral_pub,
            view_tag,
        )?;
        ch.status = channel::STATUS_CLOSED;
        channel::set(&env, channel_id, &ch);
        emit_roots(&env);
        Ok(leaf)
    }

    /// Public channel state for the UI.
    pub fn channel(env: Env, channel_id: u64) -> Result<channel::Channel, Error> {
        channel::get(&env, channel_id).ok_or(Error::NoSuchChannel)
    }

    /// The id the next `open_channel` will assign (monotonic).
    pub fn next_channel_id(env: Env) -> u64 {
        channel::next_id(&env)
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

    /// Admin: seed/add AMM liquidity reserve for `asset_tag` (pulls `amount` real SAC tokens from
    /// the admin into the pool, increasing the reserve). Roadmap 2.5 Phase 2.
    pub fn seed_reserve(env: Env, asset_tag: U256, amount: i128) -> Result<(), Error> {
        let cfg = config::get(&env);
        cfg.admin.require_auth();
        let info = assets::get(&env, &asset_tag).ok_or(Error::AssetMismatch)?;
        if amount <= 0 {
            return Err(Error::AmountMismatch);
        }
        token::TokenClient::new(&env, &info.sac).transfer(
            &cfg.admin,
            &env.current_contract_address(),
            &amount,
        );
        reserve::set(&env, &asset_tag, reserve::get(&env, &asset_tag) + amount);
        Ok(())
    }

    /// Admin: withdraw `amount` of `asset_tag` reserve liquidity back out to `to`.
    pub fn withdraw_reserve(env: Env, asset_tag: U256, amount: i128, to: Address) -> Result<(), Error> {
        let cfg = config::get(&env);
        cfg.admin.require_auth();
        let info = assets::get(&env, &asset_tag).ok_or(Error::AssetMismatch)?;
        let bal = reserve::get(&env, &asset_tag);
        if amount <= 0 || amount > bal {
            return Err(Error::ReserveTooLow);
        }
        reserve::set(&env, &asset_tag, bal - amount);
        token::TokenClient::new(&env, &info.sac).transfer(
            &env.current_contract_address(),
            &to,
            &amount,
        );
        Ok(())
    }

    /// The AMM reserve balance for `asset_tag` (base units).
    pub fn reserve(env: Env, asset_tag: U256) -> i128 {
        reserve::get(&env, &asset_tag)
    }

    /// In-pool shielded swap (roadmap 2.5 Phase 2 - constant-product AMM). Spends an A-note,
    /// mints a B-note priced by the pool's reserves, and re-shields the A remainder. NO token
    /// movement - the swap re-labels value between notes and the reserve within the pool's vaults.
    /// Public inputs: [domain_sep, asset_a_tag, asset_b_tag, epoch, commitment_root,
    /// nullifier_old_root, nullifier_new_root, nf0, nf1, change_commitment, out_commitment_b,
    /// asp_root, value_a, value_b]. `enc_notes[0]/...[0]` describe the A change note, `[1]` the B note.
    pub fn shielded_swap(
        env: Env,
        asset_a_tag: U256,
        asset_b_tag: U256,
        public_inputs: Bytes,
        proof: Bytes,
        enc_notes: Vec<Bytes>,
        ephemeral_pubs: Vec<BytesN<32>>,
        view_tags: Vec<u32>,
    ) -> Result<(), Error> {
        let cfg = config::get(&env);
        let f = inputs::read_fields(&env, &public_inputs, inputs::SWAP_N)?;

        // Common checks (swap layout: asset_a at 1, asset_b at 2, epoch at 3).
        let expected_domain =
            domain::compute_domain_sep(&env, &cfg.pool_id, &cfg.network_id, domain::SELECTOR_SWAP);
        if f.get(0).unwrap() != expected_domain {
            return Err(Error::BadDomainSep);
        }
        if f.get(1).unwrap() != asset_a_tag || f.get(2).unwrap() != asset_b_tag {
            return Err(Error::AssetMismatch);
        }
        if asset_a_tag == asset_b_tag {
            return Err(Error::BadSwapAssets);
        }
        // Both assets must be registered (so the SAC balance backs the reserves).
        assets::get(&env, &asset_a_tag).ok_or(Error::AssetMismatch)?;
        assets::get(&env, &asset_b_tag).ok_or(Error::AssetMismatch)?;
        if f.get(3).unwrap() != U256::from_u128(&env, current_epoch(&env) as u128) {
            return Err(Error::BadEpoch);
        }
        require_recent_root(&env, &f.get(4).unwrap())?;
        require_asp_root(&cfg, &f.get(11).unwrap())?;
        require_nullifier_base(&env, &f.get(5).unwrap())?;

        verifier::verify(&env, &cfg.swap_verifier, public_inputs, proof)?;

        // Constant-product pricing: the proof's `value_b` must be deliverable at the current
        // reserves (also acts as the user's min-out floor; surplus stays with the reserve).
        let value_a = f.get(12).unwrap();
        let value_b = f.get(13).unwrap();
        let reserve_a = reserve::get(&env, &asset_a_tag);
        let reserve_b = reserve::get(&env, &asset_b_tag);
        let quote_b = amm_quote(&env, &value_a, reserve_a, reserve_b)?;
        if value_b > quote_b || value_b == U256::from_u32(&env, 0) {
            return Err(Error::ReserveTooLow);
        }
        let value_a_i = value_a.to_u128().ok_or(Error::ReserveTooLow)? as i128;
        let value_b_i = value_b.to_u128().ok_or(Error::ReserveTooLow)? as i128;
        reserve::set(&env, &asset_a_tag, reserve_a + value_a_i);
        reserve::set(&env, &asset_b_tag, reserve_b - value_b_i);

        nullifier::set_root(&env, &f.get(6).unwrap());
        emit_nullifiers(&env, &f.get(7).unwrap(), &f.get(8).unwrap());
        append_and_emit(&env, &f.get(9).unwrap(), &enc_notes, &ephemeral_pubs, &view_tags, 0)?;
        append_and_emit(&env, &f.get(10).unwrap(), &enc_notes, &ephemeral_pubs, &view_tags, 1)?;
        emit_roots(&env);
        Ok(())
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

/// Swap fee in basis points (0.30%), retained by the reserve (LP benefit).
const SWAP_FEE_BPS: u128 = 30;

/// Constant-product quote: how much B the reserves can deliver for `value_a` of A, after fee.
/// `quote_b = reserve_b * amount_in / (reserve_a + amount_in)` with
/// `amount_in = value_a * (10000 - fee) / 10000`. All in U256 (the product can exceed i128).
fn amm_quote(env: &Env, value_a: &U256, reserve_a: i128, reserve_b: i128) -> Result<U256, Error> {
    let bps = U256::from_u128(env, 10_000);
    let fee_factor = U256::from_u128(env, 10_000 - SWAP_FEE_BPS);
    let amount_in = value_a.mul(&fee_factor).div(&bps);
    let res_a = U256::from_u128(env, reserve_a as u128);
    let res_b = U256::from_u128(env, reserve_b as u128);
    let den = res_a.add(&amount_in);
    if den == U256::from_u32(env, 0) {
        return Err(Error::ReserveTooLow);
    }
    Ok(res_b.mul(&amount_in).div(&den))
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

/// Shared escrow payout (release & refund): validate the payout proof against the expected
/// commitment_hash / floor / recipient_bind (so a refund proof can't pass as a release, and only
/// the bound key-holder can mint), verify, then mint the payout note (value already vaulted — no
/// token move). Returns the new leaf index. Callers enforce the per-mode/per-state guard first.
#[allow(clippy::too_many_arguments)]
fn escrow_payout(
    env: &Env,
    cfg: &Config,
    asset_tag: &U256,
    public_inputs: &Bytes,
    proof: Bytes,
    commitment_hash: &U256,
    floor: &U256,
    recipient_bind: &U256,
    enc_note: Bytes,
    ephemeral_pub: BytesN<32>,
    view_tag: u32,
) -> Result<u32, Error> {
    let f = inputs::read_fields(env, public_inputs, inputs::ESCROW_PAYOUT_N)?;
    check_common(env, cfg, &f, domain::SELECTOR_ESCROW_PAYOUT, asset_tag)?;
    if &f.get(3).unwrap() != commitment_hash {
        return Err(Error::BadCommitmentHash);
    }
    if &f.get(4).unwrap() != floor {
        return Err(Error::BadFloor);
    }
    if &f.get(6).unwrap() != recipient_bind {
        return Err(Error::BadRecipientBind);
    }
    verifier::verify(env, &cfg.escrow_payout_verifier, public_inputs.clone(), proof)?;
    let leaf = tree::append(env, &f.get(5).unwrap())?;
    env.events().publish(
        (symbol_short!("commit"), leaf),
        (f.get(5).unwrap(), enc_note, ephemeral_pub, view_tag),
    );
    Ok(leaf)
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

    // ----------------------------- in-pool swap (roadmap 2.5 Phase 2) -----------------------------

    /// Register a second asset (tag 2) + seed both reserves, returning (asset_b_tag, sac_b).
    fn setup_swap_reserves(f: &Fixture, res_a: i128, res_b: i128) -> U256 {
        let env = &f.env;
        let sac_b = env.register_stellar_asset_contract_v2(f.admin.clone());
        let asset_b = U256::from_u32(env, 2);
        f.pool().register_asset(&asset_b, &sac_b.address(), &6);
        // Fund the admin and seed both reserves (pulls real SAC into the pool).
        StellarAssetClient::new(env, &f.sac).mint(&f.admin, &res_a);
        StellarAssetClient::new(env, &sac_b.address()).mint(&f.admin, &res_b);
        f.pool().seed_reserve(&f.asset_tag, &res_a);
        f.pool().seed_reserve(&asset_b, &res_b);
        asset_b
    }

    fn swap_inputs(f: &Fixture, asset_b: &U256, value_a: u128, value_b: u128) -> Bytes {
        let env = &f.env;
        let domain_sep =
            domain::compute_domain_sep(env, &f.pool_id, &f.network_id, domain::SELECTOR_SWAP);
        field_blob(
            env,
            &[
                domain_sep,
                f.asset_tag.clone(),         // asset_a_tag
                asset_b.clone(),             // asset_b_tag
                U256::from_u32(env, 0),      // epoch 0
                f.pool().commitment_root(),  // recent
                f.pool().nullifier_root(),   // == stored base
                U256::from_u32(env, 0xbeef), // new nullifier root
                U256::from_u32(env, 0xa),    // nf0
                U256::from_u32(env, 0xb),    // nf1
                U256::from_u32(env, 0xc0ffee), // change_commitment (A)
                U256::from_u32(env, 0xb0b),  // out_commitment_b (B)
                U256::from_u32(env, 0),      // asp_root matches cfg
                U256::from_u128(env, value_a),
                U256::from_u128(env, value_b),
            ],
        )
    }

    fn swap_payloads(env: &Env) -> (Vec<Bytes>, Vec<BytesN<32>>, Vec<u32>) {
        let enc = Vec::from_array(env, [Bytes::new(env), Bytes::new(env)]);
        let eph = Vec::from_array(
            env,
            [BytesN::from_array(env, &[0u8; 32]), BytesN::from_array(env, &[0u8; 32])],
        );
        let tags = Vec::from_array(env, [0u32, 0u32]);
        (enc, eph, tags)
    }

    #[test]
    fn swap_prices_and_updates_reserves() {
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);
        let asset_b = setup_swap_reserves(&f, 10_000, 10_000);

        // amount_in = 1000*9970/10000 = 997; quote_b = 10000*997/(10000+997) = 906 (floor).
        let pi = swap_inputs(&f, &asset_b, 1000, 900);
        let (enc, eph, tags) = swap_payloads(env);
        f.pool()
            .shielded_swap(&f.asset_tag, &asset_b, &pi, &Bytes::new(env), &enc, &eph, &tags);

        // reserve_A += value_a; reserve_B -= value_b.
        assert_eq!(f.pool().reserve(&f.asset_tag), 11_000);
        assert_eq!(f.pool().reserve(&asset_b), 9_100);
    }

    #[test]
    fn swap_rejects_value_b_above_quote() {
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);
        let asset_b = setup_swap_reserves(&f, 10_000, 10_000);

        // 950 > the 906 the reserves can deliver -> ReserveTooLow (acts as the slippage floor).
        let pi = swap_inputs(&f, &asset_b, 1000, 950);
        let (enc, eph, tags) = swap_payloads(env);
        let res = f.pool().try_shielded_swap(
            &f.asset_tag,
            &asset_b,
            &pi,
            &Bytes::new(env),
            &enc,
            &eph,
            &tags,
        );
        assert_eq!(res, Err(Ok(Error::ReserveTooLow)));
    }

    #[test]
    fn swap_rejects_same_asset() {
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);
        setup_swap_reserves(&f, 10_000, 10_000);
        // asset_a_tag == asset_b_tag both = tag 1.
        let pi = swap_inputs(&f, &f.asset_tag.clone(), 1000, 900);
        let (enc, eph, tags) = swap_payloads(env);
        let res = f.pool().try_shielded_swap(
            &f.asset_tag,
            &f.asset_tag,
            &pi,
            &Bytes::new(env),
            &enc,
            &eph,
            &tags,
        );
        assert_eq!(res, Err(Ok(Error::BadSwapAssets)));
    }

    #[test]
    fn withdraw_reserve_returns_liquidity() {
        let f = setup();
        let asset_b = setup_swap_reserves(&f, 5_000, 7_000);
        assert_eq!(f.pool().reserve(&asset_b), 7_000);
        f.pool().withdraw_reserve(&asset_b, &3_000, &f.admin);
        assert_eq!(f.pool().reserve(&asset_b), 4_000);
        // Over-withdraw is rejected.
        let res = f.pool().try_withdraw_reserve(&asset_b, &9_999, &f.admin);
        assert_eq!(res, Err(Ok(Error::ReserveTooLow)));
    }

    // ----------------------------- escrow (building block B) -----------------------------

    fn open_basic(f: &Fixture, mode: u32, target: u64, deadline: u64, payee_bind: u32) -> u64 {
        f.pool()
            .open_escrow(&f.asset_tag, &target, &deadline, &mode, &U256::from_u32(&f.env, payee_bind))
    }

    /// 14-field escrow_contribute public inputs (recent commitment root + stored nullifier base).
    fn contribute_inputs(
        f: &Fixture,
        c_raised_old: U256,
        c_raised_new: U256,
        c_contrib: U256,
        refund_bind: U256,
        change_commitment: u32,
    ) -> Bytes {
        let env = &f.env;
        let domain_sep = domain::compute_domain_sep(
            env,
            &f.pool_id,
            &f.network_id,
            domain::SELECTOR_ESCROW_CONTRIBUTE,
        );
        field_blob(
            env,
            &[
                domain_sep,
                f.asset_tag.clone(),
                U256::from_u32(env, 0),     // epoch 0
                f.pool().commitment_root(), // recent
                f.pool().nullifier_root(),  // == stored base
                U256::from_u32(env, 0xbeef), // new nullifier root
                U256::from_u32(env, 0xa),   // nf0
                U256::from_u32(env, 0xb),   // nf1
                U256::from_u32(env, change_commitment),
                U256::from_u32(env, 0),     // asp_root matches cfg
                c_raised_old,
                c_raised_new,
                c_contrib,
                refund_bind,
            ],
        )
    }

    /// 7-field escrow_payout public inputs (release/refund).
    fn payout_inputs(
        f: &Fixture,
        commitment_hash: U256,
        floor: U256,
        out_commitment: u32,
        recipient_bind: U256,
    ) -> Bytes {
        let env = &f.env;
        let domain_sep = domain::compute_domain_sep(
            env,
            &f.pool_id,
            &f.network_id,
            domain::SELECTOR_ESCROW_PAYOUT,
        );
        field_blob(
            env,
            &[
                domain_sep,
                f.asset_tag.clone(),
                U256::from_u32(env, 0),
                commitment_hash,
                floor,
                U256::from_u32(env, out_commitment),
                recipient_bind,
            ],
        )
    }

    /// Hash a running point `(px, py)` the way the contract does — so the stub-verifier tests can
    /// pass a `c_raised_new` that satisfies the on-chain `Poseidon(raised_x, raised_y)` check.
    fn pt_hash(f: &Fixture, px: u32, py: u32) -> U256 {
        let env = &f.env;
        let mut pt = Vec::new(env);
        pt.push_back(U256::from_u32(env, px));
        pt.push_back(U256::from_u32(env, py));
        poseidon::hash(env, &pt)
    }

    fn do_contribute(f: &Fixture, id: u64, pi: &Bytes, px: u32, py: u32) -> u32 {
        let env = &f.env;
        f.pool().escrow_contribute(
            &id,
            &f.asset_tag,
            pi,
            &Bytes::new(env),
            &Bytes::new(env),
            &BytesN::from_array(env, &[0u8; 32]),
            &0u32,
            &Bytes::new(env),
            &U256::from_u32(env, px),
            &U256::from_u32(env, py),
        )
    }

    /// One valid contribution: reads the escrow's current `c_raised` as the old root, folds to a
    /// chosen point `(px, py)` whose hash becomes the new root, and submits. Returns the index.
    fn contribute_step(f: &Fixture, id: u64, px: u32, py: u32, refund_bind: U256, change_cm: u32) -> u32 {
        let env = &f.env;
        let c_old = f.pool().escrow(&id).c_raised;
        let c_new = pt_hash(f, px, py);
        let pi = contribute_inputs(f, c_old, c_new, U256::from_u32(env, 0xc1), refund_bind, change_cm);
        do_contribute(f, id, &pi, px, py)
    }

    fn release(f: &Fixture, id: u64, pi: &Bytes) -> u32 {
        let env = &f.env;
        f.pool().escrow_release(
            &id,
            pi,
            &Bytes::new(env),
            &Bytes::new(env),
            &BytesN::from_array(env, &[0u8; 32]),
            &0u32,
        )
    }

    /// Flatten the `try_` result to `Result<u32, Error>` (Ok value / contract error), panicking
    /// on the unexpected conversion/host-error arms — keeps the assertions readable.
    fn try_release(f: &Fixture, id: u64, pi: &Bytes) -> Result<u32, Error> {
        let env = &f.env;
        match f.pool().try_escrow_release(
            &id,
            pi,
            &Bytes::new(env),
            &Bytes::new(env),
            &BytesN::from_array(env, &[0u8; 32]),
            &0u32,
        ) {
            Ok(Ok(v)) => Ok(v),
            Err(Ok(e)) => Err(e),
            other => panic!("unexpected try_escrow_release result: {other:?}"),
        }
    }

    fn try_refund(f: &Fixture, id: u64, idx: u32, pi: &Bytes) -> Result<u32, Error> {
        let env = &f.env;
        match f.pool().try_escrow_refund(
            &id,
            &idx,
            pi,
            &Bytes::new(env),
            &Bytes::new(env),
            &BytesN::from_array(env, &[0u8; 32]),
            &0u32,
        ) {
            Ok(Ok(v)) => Ok(v),
            Err(Ok(e)) => Err(e),
            other => panic!("unexpected try_escrow_refund result: {other:?}"),
        }
    }

    fn refund(f: &Fixture, id: u64, idx: u32, pi: &Bytes) -> u32 {
        let env = &f.env;
        f.pool().escrow_refund(
            &id,
            &idx,
            pi,
            &Bytes::new(env),
            &Bytes::new(env),
            &BytesN::from_array(env, &[0u8; 32]),
            &0u32,
        )
    }

    #[test]
    fn open_escrow_assigns_ids_and_seeds() {
        let f = setup();
        let id0 = open_basic(&f, escrow::MODE_ALL_OR_NOTHING, 1000, 100, 0x9001);
        let id1 = open_basic(&f, escrow::MODE_KEEP_WHAT_YOU_RAISE, 500, 100, 0x9002);
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        let e = f.pool().escrow(&id0);
        assert_eq!(e.status, escrow::STATUS_OPEN);
        assert_eq!(e.n_contrib, 0);
        assert_eq!(e.target, 1000);
        assert_eq!(e.c_raised, escrow::init_c_raised(&f.env));
    }

    #[test]
    fn open_escrow_rejects_bad_mode() {
        let f = setup();
        let res = f
            .pool()
            .try_open_escrow(&f.asset_tag, &1000u64, &100u64, &7u32, &U256::from_u32(&f.env, 1));
        assert_eq!(res, Err(Ok(Error::BadMode)));
    }

    #[test]
    fn escrow_contribute_chains_and_records() {
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);
        let id = open_basic(&f, escrow::MODE_ALL_OR_NOTHING, 1000, 100, 0x9001);

        let seed = escrow::init_c_raised(env);
        // first contribution: fold to point (0x100, 0x101); c_raised := hash(point).
        let idx = contribute_step(&f, id, 0x100, 0x101, U256::from_u32(env, 0xbb01), 0xaa);
        assert_eq!(idx, 0);
        let e = f.pool().escrow(&id);
        assert_eq!(e.c_raised, pt_hash(&f, 0x100, 0x101));
        assert_eq!(e.raised_x, U256::from_u32(env, 0x100));
        assert_eq!(e.raised_y, U256::from_u32(env, 0x101));
        assert_eq!(e.n_contrib, 1);

        // second contribution chains from the stored point to a new one.
        let idx2 = contribute_step(&f, id, 0x200, 0x201, U256::from_u32(env, 0xbb02), 0xbb);
        assert_eq!(idx2, 1);
        assert_eq!(f.pool().escrow(&id).c_raised, pt_hash(&f, 0x200, 0x201));

        // a contribute with a STALE c_raised_old (reusing the seed) is rejected.
        let c_new = pt_hash(&f, 0x300, 0x301);
        let pi_bad = contribute_inputs(&f, seed, c_new, U256::from_u32(env, 0xc3), U256::from_u32(env, 0xbb03), 0xcc);
        let res = f.pool().try_escrow_contribute(
            &id,
            &f.asset_tag,
            &pi_bad,
            &Bytes::new(env),
            &Bytes::new(env),
            &BytesN::from_array(env, &[0u8; 32]),
            &0u32,
            &Bytes::new(env),
            &U256::from_u32(env, 0x300),
            &U256::from_u32(env, 0x301),
        );
        assert_eq!(res, Err(Ok(Error::BadRaisedRoot)));

        // a contribute whose passed point does NOT hash to c_raised_new is rejected.
        let c_old_now = f.pool().escrow(&id).c_raised;
        let pi_mismatch = contribute_inputs(&f, c_old_now, pt_hash(&f, 0x400, 0x401), U256::from_u32(env, 0xc4), U256::from_u32(env, 0xbb04), 0xdd);
        let res2 = f.pool().try_escrow_contribute(
            &id,
            &f.asset_tag,
            &pi_mismatch,
            &Bytes::new(env),
            &Bytes::new(env),
            &BytesN::from_array(env, &[0u8; 32]),
            &0u32,
            &Bytes::new(env),
            &U256::from_u32(env, 0x999), // wrong point
            &U256::from_u32(env, 0x998),
        );
        assert_eq!(res2, Err(Ok(Error::BadRaisedPoint)));
    }

    #[test]
    fn escrow_release_all_or_nothing_validates_and_closes() {
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);
        let id = open_basic(&f, escrow::MODE_ALL_OR_NOTHING, 1000, 100, 0x9001);
        contribute_step(&f, id, 0x100, 0x101, U256::from_u32(env, 0xbb01), 0xaa);
        let c_raised = f.pool().escrow(&id).c_raised; // release opens the running commitment
        let payee_bind = U256::from_u32(env, 0x9001);
        let target = U256::from_u128(env, 1000);

        // wrong recipient_bind / floor / commitment_hash each rejected
        assert_eq!(
            try_release(&f, id, &payout_inputs(&f, c_raised.clone(), target.clone(), 0xfee1, U256::from_u32(env, 0xbad))),
            Err(Error::BadRecipientBind)
        );
        assert_eq!(
            try_release(&f, id, &payout_inputs(&f, c_raised.clone(), U256::from_u128(env, 999), 0xfee1, payee_bind.clone())),
            Err(Error::BadFloor)
        );
        assert_eq!(
            try_release(&f, id, &payout_inputs(&f, U256::from_u32(env, 0xdead), target.clone(), 0xfee1, payee_bind.clone())),
            Err(Error::BadCommitmentHash)
        );

        // correct release succeeds and closes the escrow
        let _leaf = release(&f, id, &payout_inputs(&f, c_raised.clone(), target.clone(), 0xfee1, payee_bind.clone()));
        assert_eq!(f.pool().escrow(&id).status, escrow::STATUS_RELEASED);

        // a second release is rejected (closed)
        assert_eq!(
            try_release(&f, id, &payout_inputs(&f, c_raised, target, 0xfee2, payee_bind)),
            Err(Error::EscrowClosed)
        );
    }

    #[test]
    fn escrow_refund_after_deadline_once() {
        use soroban_sdk::testutils::Ledger;
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);
        let id = open_basic(&f, escrow::MODE_ALL_OR_NOTHING, 1000, 100, 0x9001);
        let refund_bind = U256::from_u32(env, 0xbb01);
        // contribution stores c_contrib = 0xc1 (refund opens THAT, not the running commitment).
        contribute_step(&f, id, 0x100, 0x101, refund_bind.clone(), 0xaa);

        // before the deadline: refund rejected
        env.ledger().with_mut(|li| li.sequence_number = 50);
        assert_eq!(
            try_refund(&f, id, 0, &payout_inputs(&f, U256::from_u32(env, 0xc1), U256::from_u32(env, 0), 0xfee0, refund_bind.clone())),
            Err(Error::DeadlineNotPassed)
        );

        // after the deadline: refund of contribution 0 succeeds, second is rejected
        env.ledger().with_mut(|li| li.sequence_number = 200);
        let _ = refund(&f, id, 0, &payout_inputs(&f, U256::from_u32(env, 0xc1), U256::from_u32(env, 0), 0xfee0, refund_bind.clone()));
        assert_eq!(
            try_refund(&f, id, 0, &payout_inputs(&f, U256::from_u32(env, 0xc1), U256::from_u32(env, 0), 0xfee5, refund_bind)),
            Err(Error::AlreadyRefunded)
        );
    }

    #[test]
    fn escrow_refund_rejected_for_keep_what_you_raise() {
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);
        let id = open_basic(&f, escrow::MODE_KEEP_WHAT_YOU_RAISE, 1000, 100, 0x9001);
        let res = try_refund(
            &f,
            id,
            0,
            &payout_inputs(&f, U256::from_u32(env, 0xc1), U256::from_u32(env, 0), 0x1, U256::from_u32(env, 0x1)),
        );
        assert_eq!(res, Err(Error::RefundNotAllowed));
    }

    #[test]
    fn escrow_release_keep_what_you_raise_needs_deadline() {
        use soroban_sdk::testutils::Ledger;
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);
        let id = open_basic(&f, escrow::MODE_KEEP_WHAT_YOU_RAISE, 1000, 100, 0x9001);
        contribute_step(&f, id, 0x100, 0x101, U256::from_u32(env, 0x1), 0xaa);
        let c_raised = f.pool().escrow(&id).c_raised;
        let payee_bind = U256::from_u32(env, 0x9001);

        // before deadline: keep-what-you-raise release needs the deadline passed (floor=0)
        env.ledger().with_mut(|li| li.sequence_number = 50);
        assert_eq!(
            try_release(&f, id, &payout_inputs(&f, c_raised.clone(), U256::from_u32(env, 0), 0xfee1, payee_bind.clone())),
            Err(Error::DeadlineNotPassed)
        );

        // after deadline: succeeds at floor=0 (any amount)
        env.ledger().with_mut(|li| li.sequence_number = 200);
        let _ = release(&f, id, &payout_inputs(&f, c_raised, U256::from_u32(env, 0), 0xfee1, payee_bind));
        assert_eq!(f.pool().escrow(&id).status, escrow::STATUS_RELEASED);
    }

    // ----------------------------- channel (building block B phase 2) -----------------------------

    /// Open a channel via the entrypoint: an escrow_contribute-shaped proof whose c_raised_old is the
    /// seed, c_contrib is the cap commitment, refund_bind is the subscriber binding. Returns the id.
    fn open_channel(
        f: &Fixture,
        cap_commitment: U256,
        subscriber_bind: U256,
        merchant_bind: U256,
        auth_key: U256,
        expiry: u64,
    ) -> u64 {
        let env = &f.env;
        let seed = escrow::init_c_raised(env);
        let pi = contribute_inputs(f, seed, U256::from_u32(env, 0xceee), cap_commitment, subscriber_bind, 0xaa);
        f.pool().open_channel(
            &f.asset_tag,
            &pi,
            &Bytes::new(env),
            &Bytes::new(env),
            &BytesN::from_array(env, &[0u8; 32]),
            &0u32,
            &merchant_bind,
            &auth_key,
            &expiry,
            &Bytes::new(env),
        )
    }

    /// 10-field channel_close public inputs.
    fn close_inputs(
        f: &Fixture,
        cap_hash: U256,
        auth_key: U256,
        valid_after: u64,
        merchant_out: u32,
        subscriber_out: u32,
        merchant_bind: U256,
        subscriber_bind: U256,
    ) -> Bytes {
        let env = &f.env;
        let domain_sep =
            domain::compute_domain_sep(env, &f.pool_id, &f.network_id, domain::SELECTOR_CHANNEL_CLOSE);
        field_blob(
            env,
            &[
                domain_sep,
                f.asset_tag.clone(),
                U256::from_u32(env, 0), // epoch 0
                cap_hash,
                auth_key,
                U256::from_u128(env, valid_after as u128),
                U256::from_u32(env, merchant_out),
                U256::from_u32(env, subscriber_out),
                merchant_bind,
                subscriber_bind,
            ],
        )
    }

    fn try_close(f: &Fixture, id: u64, pi: &Bytes) -> Result<(), Error> {
        let env = &f.env;
        match f.pool().try_close_channel(
            &id,
            pi,
            &Bytes::new(env),
            &Bytes::new(env),
            &BytesN::from_array(env, &[0u8; 32]),
            &0u32,
            &Bytes::new(env),
            &BytesN::from_array(env, &[0u8; 32]),
            &0u32,
        ) {
            Ok(Ok(())) => Ok(()),
            Err(Ok(e)) => Err(e),
            other => panic!("unexpected try_close_channel result: {other:?}"),
        }
    }

    fn try_reclaim(f: &Fixture, id: u64, pi: &Bytes) -> Result<u32, Error> {
        let env = &f.env;
        match f.pool().try_channel_reclaim(
            &id,
            pi,
            &Bytes::new(env),
            &Bytes::new(env),
            &BytesN::from_array(env, &[0u8; 32]),
            &0u32,
        ) {
            Ok(Ok(v)) => Ok(v),
            Err(Ok(e)) => Err(e),
            other => panic!("unexpected try_channel_reclaim result: {other:?}"),
        }
    }

    #[test]
    fn open_channel_records_state() {
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);
        let cap = U256::from_u32(env, 0xca9);
        let sub_bind = U256::from_u32(env, 0x5b01);
        let mer_bind = U256::from_u32(env, 0x3e01);
        let auth = U256::from_u32(env, 0xa07);
        let id = open_channel(&f, cap.clone(), sub_bind.clone(), mer_bind.clone(), auth.clone(), 100);
        assert_eq!(id, 0);
        assert_eq!(f.pool().next_channel_id(), 1);
        let ch = f.pool().channel(&id);
        assert_eq!(ch.status, channel::STATUS_OPEN);
        assert_eq!(ch.cap_commitment, cap);
        assert_eq!(ch.subscriber_bind, sub_bind);
        assert_eq!(ch.merchant_bind, mer_bind);
        assert_eq!(ch.auth_key, auth);
        assert_eq!(ch.expiry, 100);
    }

    #[test]
    fn open_channel_rejects_non_seed_start() {
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);
        // c_raised_old != the seed -> the cap wasn't committed onto the known starting point.
        let pi = contribute_inputs(
            &f,
            U256::from_u32(env, 0xdead),
            U256::from_u32(env, 0xceee),
            U256::from_u32(env, 0xca9),
            U256::from_u32(env, 0x5b01),
            0xaa,
        );
        let res = f.pool().try_open_channel(
            &f.asset_tag,
            &pi,
            &Bytes::new(env),
            &Bytes::new(env),
            &BytesN::from_array(env, &[0u8; 32]),
            &0u32,
            &U256::from_u32(env, 0x3e01),
            &U256::from_u32(env, 0xa07),
            &100u64,
            &Bytes::new(env),
        );
        assert_eq!(res, Err(Ok(Error::BadRaisedRoot)));
    }

    #[test]
    fn close_channel_validates_binds_and_mints_two_notes() {
        use soroban_sdk::testutils::Ledger;
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);
        let cap = U256::from_u32(env, 0xca9);
        let sub_bind = U256::from_u32(env, 0x5b01);
        let mer_bind = U256::from_u32(env, 0x3e01);
        let auth = U256::from_u32(env, 0xa07);
        let id = open_channel(&f, cap.clone(), sub_bind.clone(), mer_bind.clone(), auth.clone(), 100);
        env.ledger().with_mut(|li| li.sequence_number = 50);

        // wrong cap_hash / auth_key / valid_after(future) / binds each rejected.
        assert_eq!(
            try_close(&f, id, &close_inputs(&f, U256::from_u32(env, 0xbad), auth.clone(), 10, 0xd1, 0xd2, mer_bind.clone(), sub_bind.clone())),
            Err(Error::BadCommitmentHash)
        );
        assert_eq!(
            try_close(&f, id, &close_inputs(&f, cap.clone(), U256::from_u32(env, 0xbad), 10, 0xd1, 0xd2, mer_bind.clone(), sub_bind.clone())),
            Err(Error::BadAuthKey)
        );
        assert_eq!(
            try_close(&f, id, &close_inputs(&f, cap.clone(), auth.clone(), 99, 0xd1, 0xd2, mer_bind.clone(), sub_bind.clone())),
            Err(Error::ValidAfterNotReached)
        );
        assert_eq!(
            try_close(&f, id, &close_inputs(&f, cap.clone(), auth.clone(), 10, 0xd1, 0xd2, U256::from_u32(env, 0xbad), sub_bind.clone())),
            Err(Error::BadRecipientBind)
        );
        assert_eq!(
            try_close(&f, id, &close_inputs(&f, cap.clone(), auth.clone(), 10, 0xd1, 0xd2, mer_bind.clone(), U256::from_u32(env, 0xbad))),
            Err(Error::BadRecipientBind)
        );

        // a valid close mints both notes and closes the channel.
        try_close(&f, id, &close_inputs(&f, cap.clone(), auth.clone(), 10, 0xdaa1, 0xdaa2, mer_bind.clone(), sub_bind.clone())).unwrap();
        assert_eq!(f.pool().channel(&id).status, channel::STATUS_CLOSED);

        // a second close is rejected (closed).
        assert_eq!(
            try_close(&f, id, &close_inputs(&f, cap, auth, 10, 0xdaa3, 0xdaa4, mer_bind, sub_bind)),
            Err(Error::ChannelClosed)
        );
    }

    #[test]
    fn channel_reclaim_after_expiry_only() {
        use soroban_sdk::testutils::Ledger;
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);
        let cap = U256::from_u32(env, 0xca9);
        let sub_bind = U256::from_u32(env, 0x5b01);
        let id = open_channel(&f, cap.clone(), sub_bind.clone(), U256::from_u32(env, 0x3e01), U256::from_u32(env, 0xa07), 100);

        // before expiry: reclaim rejected.
        env.ledger().with_mut(|li| li.sequence_number = 50);
        assert_eq!(
            try_reclaim(&f, id, &payout_inputs(&f, cap.clone(), U256::from_u32(env, 0), 0xfee0, sub_bind.clone())),
            Err(Error::DeadlineNotPassed)
        );

        // after expiry: reclaim (floor 0, cap commitment, subscriber_bind) succeeds and closes.
        env.ledger().with_mut(|li| li.sequence_number = 200);
        try_reclaim(&f, id, &payout_inputs(&f, cap.clone(), U256::from_u32(env, 0), 0xfee0, sub_bind.clone())).unwrap();
        assert_eq!(f.pool().channel(&id).status, channel::STATUS_CLOSED);

        // a second reclaim is rejected (closed).
        assert_eq!(
            try_reclaim(&f, id, &payout_inputs(&f, cap, U256::from_u32(env, 0), 0xfee1, sub_bind)),
            Err(Error::ChannelClosed)
        );
    }

    #[test]
    fn channel_close_then_reclaim_blocked() {
        use soroban_sdk::testutils::Ledger;
        let f = setup();
        let env = &f.env;
        prime_recent_root(&f);
        let cap = U256::from_u32(env, 0xca9);
        let sub_bind = U256::from_u32(env, 0x5b01);
        let mer_bind = U256::from_u32(env, 0x3e01);
        let auth = U256::from_u32(env, 0xa07);
        let id = open_channel(&f, cap.clone(), sub_bind.clone(), mer_bind.clone(), auth.clone(), 100);
        env.ledger().with_mut(|li| li.sequence_number = 50);
        try_close(&f, id, &close_inputs(&f, cap.clone(), auth, 10, 0xdaa1, 0xdaa2, mer_bind, sub_bind.clone())).unwrap();

        // once closed, even past expiry the subscriber cannot reclaim (first-mover close wins).
        env.ledger().with_mut(|li| li.sequence_number = 200);
        assert_eq!(
            try_reclaim(&f, id, &payout_inputs(&f, cap, U256::from_u32(env, 0), 0xfee0, sub_bind)),
            Err(Error::ChannelClosed)
        );
    }
}
