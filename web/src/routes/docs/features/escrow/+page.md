# Escrow

Private, conditional payments with a **contribute-then-payout** shape — invoices, group pay,
deposits. This is the honest substitute for a "pull": **no one can spend your notes for you**,
so escrow models the same outcome with explicit, provable steps. It is also the
**multi-user single-payment** primitive (many payers → one payee).

## How it works

A **hidden-sum** escrow (Pedersen commitments over Grumpkin) backed by two circuits:

- **escrow_contribute** — one or many payers fund an escrow privately; the running sum stays
  hidden.
- **escrow_payout** — the payee claims the funded amount once terms are met; unclaimed funds
  **refund on expiry**.

Contributions and the payout are shielded; the escrow's terms (amount, expiry) gate the
release. Lifecycle: **open → contribute → release / refund**. Live on testnet.

## When to use it

- A payee who needs to be paid by several people (split a bill, fund a pool).
- A payment that should only settle if a condition holds, with a guaranteed refund path.

> For repeated draws by one payee rather than a one-off release, use
> **[payment channels](/docs/features/channels)** instead.
