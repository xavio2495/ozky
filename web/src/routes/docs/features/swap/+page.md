# Shielded swap & pay

Move between stablecoins inside the pool — **no public edge**. The earlier edge-swap (route
out to the Stellar DEX and back) was superseded and removed; swaps are now a fully in-pool
shielded AMM.

## Shielded swap (in-pool AMM)

Exchange one shielded asset for another (e.g. XLM → USDC) against an in-pool
**constant-product** market (`x · y = k`). A **shielded_swap** circuit spends your input
note and produces the output note in the target asset, all in **one atomic transaction**.

- **Trader identity is hidden**; the **trade amount is public** (the reserve update reveals
  size, not who).
- Reserves are **admin-seeded**, with **no oracle** — price comes only from the curve.
- Value conservation is enforced in-circuit and verified on-chain.

This was live-verified on testnet (a 14-public-input proof verified on-chain, conservation
exact).

## Cross-asset pay

Pay a recipient in an asset you don't hold by **composing** a transfer with an in-pool swap —
the wallet sources from one note and delivers the target asset. Because the swap stays
in-pool, this avoids the public DEX leg the old edge-swap had.

## Limits (testnet)

Open-LP reserves with fees, amount-hidden batched swaps, and multi-hop routing are deferred.
Today reserves are admin-seeded and the swapped amount is public.

> Settles through the **[pool](/docs/contracts/pool)** like every other shielded action.
