//! Asset swap — Phase 2, the IN-POOL shielded AMM (roadmap 2.5; spec `claude-docs/swap_pool_interface.md`).
//!
//! Converts a shielded balance of asset A into a shielded balance of asset B entirely INSIDE the
//! pool — no public-account hop, no public DEX. One atomic transaction: spend an A-note, mint a
//! B-note priced by the pool's on-chain constant-product reserves, and re-shield the A remainder.
//!
//! The trade AMOUNT is public on-chain (the AMM prices + checks solvency on public reserves), but
//! the trader's IDENTITY is hidden (it's a shielded note spend) and unlinkable to any deposit /
//! withdraw — strictly more private than the Phase 1 edge swap this replaces. Both output notes'
//! ciphertexts are published on-chain, so a later [`super::scan`] rediscovers them as spendable.

use super::config::{asset_by_code, PoolConfig};
use super::encrypt::{self, NotePlaintext};
use super::poseidon::{Fr, Hasher, SELECTOR_SWAP};
use super::witness::{SwapInputs, SwapWitness};
use super::{chain, keys, proving, scan, CoreError};

/// Swap fee in basis points (0.30%) — MUST match the pool contract's `SWAP_FEE_BPS`.
const SWAP_FEE_BPS: u128 = 30;

/// A constant-product swap quote read from the live on-chain reserves.
#[derive(serde::Serialize)]
pub struct SwapQuote {
    /// Estimated destination amount at the current reserves, in base units.
    pub dest_amount: u64,
    /// The source reserve (base units) — for display / price context.
    pub reserve_from: i128,
    /// The destination reserve (base units).
    pub reserve_to: i128,
}

/// The result of an in-pool swap: one atomic transaction.
#[derive(serde::Serialize)]
pub struct SwapReceipt {
    pub tx_hash: String,
    pub from: String,
    pub to: String,
    /// Source amount swapped, in base units.
    pub sent: u64,
    /// Destination amount minted, in base units.
    pub received: u64,
}

/// Constant-product quote (mirrors the contract's `amm_quote`): how much B `amount` of A buys at
/// the given reserves, after fee. `quote_b = reserve_b * amount_in / (reserve_a + amount_in)`.
fn amm_quote(amount: u64, reserve_a: i128, reserve_b: i128) -> u64 {
    let amount_in = amount as u128 * (10_000 - SWAP_FEE_BPS) / 10_000;
    let den = reserve_a as u128 + amount_in;
    if den == 0 {
        return 0;
    }
    (reserve_b as u128 * amount_in / den) as u64
}

/// Quote swapping `amount` (base units) of `from` into `to` against the pool's live reserves.
pub fn quote(from: &str, to: &str, amount: u64) -> Result<SwapQuote, CoreError> {
    let (from_info, to_info) = resolve_pair(from, to)?;
    let cfg = PoolConfig::load()?;
    let reserve_from = chain::read_reserve(&cfg, &Fr::from_u64(from_info.tag))?;
    let reserve_to = chain::read_reserve(&cfg, &Fr::from_u64(to_info.tag))?;
    Ok(SwapQuote {
        dest_amount: amm_quote(amount, reserve_from, reserve_to),
        reserve_from,
        reserve_to,
    })
}

/// Swap `amount` (base units) of `from` into shielded `to`, accepting up to `slippage_bps` basis
/// points below the live quote. One atomic in-pool transaction. Uses the keychain wallet.
pub fn swap(from: &str, to: &str, amount: u64, slippage_bps: u32) -> Result<SwapReceipt, CoreError> {
    let wallet = keys::current_wallet()?;
    let cfg = PoolConfig::load()?;
    swap_with(&wallet, &cfg, from, to, amount, slippage_bps)
}

/// Swap with an explicit wallet + config (keychain-independent), so live tests can drive it from a
/// derived wallet. Same constant-product in-pool swap as [`swap`].
pub fn swap_with(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    from: &str,
    to: &str,
    amount: u64,
    slippage_bps: u32,
) -> Result<SwapReceipt, CoreError> {
    if slippage_bps > 10_000 {
        return Err(CoreError::Chain("slippage tolerance cannot exceed 100%".into()));
    }
    let (from_info, to_info) = resolve_pair(from, to)?;
    let asset_a_tag = Fr::from_u64(from_info.tag);
    let asset_b_tag = Fr::from_u64(to_info.tag);

    // Price the swap against the live reserves; value_b is the minimum the user will accept.
    let reserve_a = chain::read_reserve(cfg, &asset_a_tag)?;
    let reserve_b = chain::read_reserve(cfg, &asset_b_tag)?;
    let quote_b = amm_quote(amount, reserve_a, reserve_b);
    let value_b = (quote_b as u128 * (10_000 - slippage_bps as u128) / 10_000) as u64;
    if value_b == 0 {
        return Err(CoreError::Chain(
            "swap rounds to zero — amount too small or reserve too thin".into(),
        ));
    }

    // Self-swap: the minted B-note + its ciphertext go back to this wallet.
    let id = scan::wallet_identity(wallet)?;
    swap_core(
        wallet,
        cfg,
        from_info,
        to_info,
        amount,
        value_b,
        id.owner_pk,
        id.transmission_pub,
    )
}

/// The shared in-pool swap path (build `SwapWitness` → prove → encrypt → submit `shielded_swap`),
/// parameterized by `value_a` (X spent into the reserve), `value_b` (Y minted), and the OUT owner —
/// the B-note's `owner_pk` + the transmission key its ciphertext is encrypted to. For a self-swap
/// the OUT owner is this wallet; for a cross-asset PAY it is the recipient (the X change always
/// returns to this wallet). Spends one owned X-note covering `value_a` (the swap circuit is 2-in).
#[allow(clippy::too_many_arguments)]
fn swap_core(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    from_info: &super::config::AssetInfo,
    to_info: &super::config::AssetInfo,
    value_a: u64,
    value_b: u64,
    out_owner_pk: Fr,
    out_transmission_pub: [u8; 32],
) -> Result<SwapReceipt, CoreError> {
    let asset_a_tag = Fr::from_u64(from_info.tag);
    let asset_b_tag = Fr::from_u64(to_info.tag);
    let cfg_a = cfg.with_asset(from_info.code)?;
    let id = scan::wallet_identity(wallet)?;
    let h = Hasher::new();
    let epoch = chain::current_epoch(&cfg.rpc_url)?;

    // Live state + an owned A-note that covers `value_a`.
    let state = chain::pool_state(&cfg_a)?;
    let commitment_leaves = chain::commitment_leaves_from(&state.commits)?;
    let asp_leaves = chain::approved_set(&cfg_a)?;
    let local = super::notes::load(wallet)?;
    let note = scan::owned_notes(&id, &state, &local, 0)?
        .into_iter()
        .find(|n| n.value >= value_a && n.asset_tag == asset_a_tag)
        .ok_or_else(|| {
            CoreError::Proving(format!("no single owned {} note covers {value_a}", from_info.code))
        })?;
    if !asp_leaves.contains(&id.owner_pk) {
        return Err(CoreError::Proving(
            "wallet not enrolled in this pool's ASP approved set (cannot prove membership)".into(),
        ));
    }

    // Fresh openings for the two minted notes (A change + B output), reused in both the witness
    // and the published ciphertext so a later scan rediscovers exactly these commitments.
    let change_blinding = Fr::random();
    let change_rho = Fr::random();
    let out_blinding = Fr::random();
    let out_rho = Fr::random();

    let witness = SwapWitness::build(
        &h,
        SwapInputs {
            owner_sk: id.owner_sk,
            asset_a_tag,
            asset_b_tag,
            epoch: Fr::from_u64(epoch as u64),
            note_epoch: Fr::from_u64(note.epoch as u64),
            domain_sep: h.domain_sep(&cfg.pool_id, &cfg.network_id, SELECTOR_SWAP),
            note_value: note.value,
            note_blinding: note.blinding,
            note_rho: note.rho,
            note_leaf_index: note.leaf_index as usize,
            commitment_leaves: &commitment_leaves,
            asp_leaves: &asp_leaves,
            prior_nullifiers: &state.nullifiers,
            dummy_rho: Fr::random(),
            value_a,
            value_b,
            change_blinding,
            change_rho,
            out_owner_pk,
            out_blinding,
            out_rho,
        },
    );

    let bundle = proving::prove_swap_witness(&witness)?;

    // Change A-note → self; minted B-note → the OUT transmission key (self, or the recipient).
    let change_payload = payload_for(
        &id,
        NotePlaintext {
            value: note.value - value_a,
            asset_tag: asset_a_tag,
            blinding: change_blinding,
            epoch,
            rho: change_rho,
        },
    )?;
    let out_pt = NotePlaintext {
        value: value_b,
        asset_tag: asset_b_tag,
        blinding: out_blinding,
        epoch,
        rho: out_rho,
    };
    let out_enc = encrypt::encrypt_note(&out_pt.serialize(), &out_transmission_pub)?;
    let out_payload = chain::OutputPayload {
        enc_note: out_enc.enc_note,
        ephemeral_pub: out_enc.ephemeral_pub,
        view_tag: out_enc.view_tag,
    };

    let tx_hash = chain::submit_swap(
        cfg,
        cfg.submit_source(wallet.stellar_secret()),
        &asset_a_tag,
        &asset_b_tag,
        &bundle.public_inputs,
        &bundle.proof,
        &[change_payload, out_payload],
    )?;

    Ok(SwapReceipt {
        tx_hash,
        from: from_info.code.into(),
        to: to_info.code.into(),
        sent: value_a,
        received: value_b,
    })
}

// ----------------------------- cross-asset pay -----------------------------

/// A cross-asset-pay quote: the X the sender will spend to deliver `dest_amount` of Y at the
/// current reserves (BEFORE the slippage buffer that [`pay_with`] adds on top).
#[derive(serde::Serialize)]
pub struct PayQuote {
    /// Estimated source (X) cost in base units to deliver the requested Y.
    pub source_cost: u64,
    /// The source reserve (base units).
    pub reserve_from: i128,
    /// The destination reserve (base units).
    pub reserve_to: i128,
}

/// Reverse of [`amm_quote`]: the MINIMUM X (`value_a`) to spend so the constant-product AMM yields
/// at least `dest_amount` of Y at the given reserves (both divisions ceiled, so the result never
/// under-buys). Errors if the destination reserve can't deliver `dest_amount`.
fn reverse_quote(dest_amount: u64, reserve_a: i128, reserve_b: i128) -> Result<u64, CoreError> {
    if dest_amount == 0 {
        return Err(CoreError::Chain("destination amount must be greater than zero".into()));
    }
    let reserve_a = reserve_a.max(0) as u128;
    let reserve_b = reserve_b.max(0) as u128;
    if reserve_b <= dest_amount as u128 {
        return Err(CoreError::Chain(
            "destination amount exceeds the pool reserve for that asset (try a smaller amount)".into(),
        ));
    }
    // amount_in >= dest_amount * reserve_a / (reserve_b - dest_amount)   (ceil)
    let num = dest_amount as u128 * reserve_a;
    let den = reserve_b - dest_amount as u128;
    let amount_in = num.div_ceil(den);
    // value_a >= amount_in * 10000 / (10000 - fee)   (ceil)
    let value_a = (amount_in * 10_000).div_ceil(10_000 - SWAP_FEE_BPS).max(1);
    u64::try_from(value_a).map_err(|_| CoreError::Chain("source cost overflows u64".into()))
}

/// Quote the source (X) cost to deliver `dest_amount` of `to` (Y), paying in `from` (X), against the
/// pool's live reserves. Read-only.
pub fn pay_quote(from: &str, to: &str, dest_amount: u64) -> Result<PayQuote, CoreError> {
    let (from_info, to_info) = resolve_pair(from, to)?;
    let cfg = PoolConfig::load()?;
    let reserve_from = chain::read_reserve(&cfg, &Fr::from_u64(from_info.tag))?;
    let reserve_to = chain::read_reserve(&cfg, &Fr::from_u64(to_info.tag))?;
    Ok(PayQuote {
        source_cost: reverse_quote(dest_amount, reserve_from, reserve_to)?,
        reserve_from,
        reserve_to,
    })
}

/// Cross-asset pay: deliver exactly `dest_amount` of `to` (Y) to the holder of `recipient_code`,
/// paying in `from` (X) from this wallet. One atomic in-pool `shielded_swap` whose minted B-note +
/// ciphertext go to the recipient; the X change returns to the sender. Uses the keychain wallet.
pub fn pay(
    recipient_code: &str,
    from: &str,
    to: &str,
    dest_amount: u64,
    slippage_bps: u32,
) -> Result<SwapReceipt, CoreError> {
    let wallet = keys::current_wallet()?;
    let cfg = PoolConfig::load()?;
    pay_with(&wallet, &cfg, recipient_code, from, to, dest_amount, slippage_bps)
}

/// Cross-asset pay with an explicit wallet + config (keychain-independent), so payroll/tests can
/// drive it. Reverse-quotes the X cost for `dest_amount` of Y, over-spends X by `slippage_bps` so
/// the contract's `value_b <= quote_b` check survives an adverse reserve move, and routes the minted
/// Y-note to the recipient. Same single-X-note limit as [`swap_with`] (the swap circuit is 2-in).
#[allow(clippy::too_many_arguments)]
pub fn pay_with(
    wallet: &keys::WalletKeys,
    cfg: &PoolConfig,
    recipient_code: &str,
    from: &str,
    to: &str,
    dest_amount: u64,
    slippage_bps: u32,
) -> Result<SwapReceipt, CoreError> {
    if slippage_bps > 10_000 {
        return Err(CoreError::Chain("slippage tolerance cannot exceed 100%".into()));
    }
    let (from_info, to_info) = resolve_pair(from, to)?;
    let (recipient_owner_pk, recipient_tpub) = super::send::parse_payment_code(recipient_code)?;

    let reserve_a = chain::read_reserve(cfg, &Fr::from_u64(from_info.tag))?;
    let reserve_b = chain::read_reserve(cfg, &Fr::from_u64(to_info.tag))?;
    let base_cost = reverse_quote(dest_amount, reserve_a, reserve_b)?;
    // Over-spend X by the slippage tolerance so the on-chain quote still covers `dest_amount`.
    let value_a = ((base_cost as u128 * (10_000 + slippage_bps as u128)).div_ceil(10_000)) as u64;

    swap_core(
        wallet,
        cfg,
        from_info,
        to_info,
        value_a,
        dest_amount,
        recipient_owner_pk,
        recipient_tpub,
    )
}

/// Encrypt a note plaintext to the wallet's own transmission key as an [`chain::OutputPayload`].
fn payload_for(id: &scan::WalletIdentity, note: NotePlaintext) -> Result<chain::OutputPayload, CoreError> {
    let enc = encrypt::encrypt_note(&note.serialize(), &id.transmission_pub)?;
    Ok(chain::OutputPayload {
        enc_note: enc.enc_note,
        ephemeral_pub: enc.ephemeral_pub,
        view_tag: enc.view_tag,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amm_quote_matches_contract_formula() {
        // Mirrors the pool contract's `amm_quote`: amount_in = 1000*9970/10000 = 997;
        // quote = 10000*997/(10000+997) = 9970000/10997 = 906 (floor). Parity with the
        // contract test `swap_prices_and_updates_reserves`.
        assert_eq!(amm_quote(1000, 10_000, 10_000), 906);
        // Empty reserve -> zero out.
        assert_eq!(amm_quote(1000, 0, 0), 0);
        // Larger pool, smaller price impact.
        assert_eq!(amm_quote(1000, 1_000_000, 1_000_000), 996);
    }

    #[test]
    fn reverse_quote_inverts_amm_quote() {
        // Forward: amm_quote(1000, 10k, 10k) == 906. Reverse(906) must spend enough X that the
        // forward quote of that spend still delivers >= 906 — and lands back near 1000.
        let va = reverse_quote(906, 10_000, 10_000).unwrap();
        assert_eq!(va, 1000);
        assert!(amm_quote(va, 10_000, 10_000) >= 906);

        // Deep pool: the reverse cost still covers the target after the forward AMM.
        let va2 = reverse_quote(996, 1_000_000, 1_000_000).unwrap();
        assert!(amm_quote(va2, 1_000_000, 1_000_000) >= 996);

        // Asking for >= the whole destination reserve is impossible; zero is rejected.
        assert!(reverse_quote(10_000, 10_000, 10_000).is_err());
        assert!(reverse_quote(20_000, 10_000, 10_000).is_err());
        assert!(reverse_quote(0, 10_000, 10_000).is_err());
    }
}

/// Resolve + validate the asset pair (known assets, and A != B).
fn resolve_pair(
    from: &str,
    to: &str,
) -> Result<(&'static super::config::AssetInfo, &'static super::config::AssetInfo), CoreError> {
    let from_info =
        asset_by_code(from).ok_or_else(|| CoreError::Chain(format!("unknown asset '{from}'")))?;
    let to_info =
        asset_by_code(to).ok_or_else(|| CoreError::Chain(format!("unknown asset '{to}'")))?;
    if from_info.tag == to_info.tag {
        return Err(CoreError::Chain("source and destination assets are the same".into()));
    }
    Ok((from_info, to_info))
}
