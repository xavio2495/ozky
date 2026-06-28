# Payment channels

A **merchant-pull subscription channel** — a one-way payment channel for repeated draws by a
single payee, settled with a single on-chain close. It's the honest, key-safe substitute for
a standing "pull": the payer authorizes draws cryptographically, in advance, up to a cap.

## How it works

You **open** a channel backed by a shielded note up to a cap. Off-chain, an in-circuit
**Schnorr-over-Grumpkin** signature authorizes each offline draw the merchant takes — no
chain transaction per draw, and the merchant can never exceed the authorized amount.

Two ways it ends:

- **open → close** — a **channel_close** circuit proves the final balance and settles it back
  into the pool in one shielded action.
- **open → expiry → reclaim** — if it lapses, the payer reclaims the remaining balance.

Both paths were live-verified on testnet.

## Why it matters

- **Cheaper** — one open, one close, regardless of how many draws happen in between.
- **More private** — the chain sees a single settlement, not every individual draw.
- **Key-safe** — the merchant holds an authorization, never the ability to spend your notes.

> For one-off conditional releases rather than repeated draws, use
> **[escrow](/docs/features/escrow)** instead.
