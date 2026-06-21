//! The Deposit flow (Phase A3) — the PUBLIC on-ramp into the shielded pool. Funds
//! arrive on the wallet's ordinary Stellar `G…` account (a normal payment from any
//! wallet/exchange); `deposit` then locks `amount` of the asset from that account
//! into the pool vault and mints a shielded note owned by this wallet.
//!
//! The minted note is encrypted to the wallet's OWN transmission key and published
//! on-chain, so a later [`super::scan`] rediscovers it as spendable (proven by the
//! live lifecycle). Builds the deposit proof with [`super::proving`], submits via the
//! native Rust submitter ([`super::chain::submit_deposit`], G14).

use super::config::PoolConfig;
use super::encrypt::{self, NotePlaintext};
use super::poseidon::{Fr, Hasher, SELECTOR_DEPOSIT};
use super::witness::DepositWitness;
use super::{chain, keys, proving, scan, CoreError};

/// Deposit `amount` of `asset` (a v1 code, e.g. "USDC") from the wallet's Stellar
/// account into the shielded pool, using the wallet stored in the OS keychain. Returns
/// the tx hash.
pub fn deposit(asset: &str, amount: u64) -> Result<String, CoreError> {
    let wallet = keys::current_wallet()?;
    let cfg = PoolConfig::load()?.with_asset(asset)?;
    deposit_with(&wallet, &cfg, amount)
}

/// Deposit with an explicit wallet + config (keychain-independent). Builds + proves the
/// deposit, mints a self-owned note, publishes its encrypted payload, and submits.
pub fn deposit_with(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    amount: u64,
) -> Result<String, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    let h = Hasher::new();
    let epoch = chain::current_epoch(&cfg.rpc_url)?;

    // Fresh note owned by this wallet; random blinding/rho keep it hiding + unique.
    let blinding = Fr::random();
    let rho = Fr::random();
    let domain_sep = h.domain_sep(&cfg.pool_id, &cfg.network_id, SELECTOR_DEPOSIT);
    let witness = DepositWitness::build(
        &h,
        domain_sep,
        cfg.asset_tag,
        Fr::from_u64(epoch as u64),
        amount,
        id.owner_pk,
        blinding,
        rho,
    );

    // Prove (verifies vs the frozen VK); proof bytes are submitted natively from memory.
    let bundle = proving::prove_deposit_witness(&witness)?;

    // Encrypt the note to ourselves so scan rediscovers it as spendable.
    let plaintext = NotePlaintext {
        value: amount,
        asset_tag: cfg.asset_tag,
        blinding,
        epoch,
        rho,
    };
    let enc = encrypt::encrypt_note(&plaintext.serialize(), &id.transmission_pub)?;
    let payload = chain::OutputPayload {
        enc_note: enc.enc_note,
        ephemeral_pub: enc.ephemeral_pub,
        view_tag: enc.view_tag,
    };

    chain::submit_deposit(
        cfg,
        wallet.stellar_secret(),
        wallet.stellar_address(),
        amount,
        &bundle.public_inputs,
        &bundle.proof,
        &payload,
    )
}
