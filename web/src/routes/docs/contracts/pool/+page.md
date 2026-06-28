# pool

The **shielded-pool** contract — the vault and the private ledger. It is the core of the
protocol: every shielded action settles here.

**Deployed (testnet):**
[`CCCULLPYVOFZF5WWVJNSF2HGBY3SMZWO25BPEOBXWMYM3VRM6YVVXUOF`](https://stellar.expert/explorer/testnet/contract/CCCULLPYVOFZF5WWVJNSF2HGBY3SMZWO25BPEOBXWMYM3VRM6YVVXUOF)

## What it owns

- **Per-asset SAC vaults** — XLM, USDC, EURC are registered on the live pool (USDT is
  defined but not yet live). Deposits pull real tokens in; withdrawals release them out.
- **Append-only commitment tree** — a contract-maintained depth-20 Poseidon Merkle tree of
  note commitments.
- **Nullifier accumulator** — the proof-driven root that marks notes spent, with
  domain-separated anti-replay.

## Entrypoints

- **deposit** — pulls SAC tokens into the vault and appends a commitment (a public on-ramp).
- **transfer** — moves no tokens; spends interior notes and appends outputs. `transfer4`
  spends up to 4 owned notes (2 outputs); `split` fans 1 note out to many (padded to 6).
- **withdraw** — verifies a proof and releases SAC to a public `G…` destination.
  **`dest_bind` is enforced on-chain** — a withdrawal can't be redirected, and contract
  destinations are rejected.
- **swap** — `shielded_swap` against in-pool constant-product reserves.
- **escrow** — `escrow_contribute` / `escrow_payout` (hidden-sum, open → contribute →
  release / refund).
- **channel** — open and `channel_close` for a merchant-pull payment channel.

These are **entrypoints of the pool, not separate contracts** — escrow, swap, channels, and
split are each a **[circuit](/docs/circuits)** plus a pool entrypoint. For each one the pool
calls the matching **[verifier](/docs/contracts/verifier)** (the live pool is a "9-verifier"
pool, one per circuit) and checks the **[policy](/docs/contracts/policy)** gate where
required.

## Invariant

The **shielded == vaulted** invariant holds: amount-binding deposit/withdraw proofs plus
in-circuit value conservation guarantee that what's in the vaults always equals what's
shielded. Double-spends are rejected; a withdrawal's destination balance is exact.

> Built against the FROZEN note/commitment/nullifier spec — see **[Concepts](/docs/concepts)**.
