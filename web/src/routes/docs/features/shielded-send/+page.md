# Shielded send

The core action: pay anyone in the pool with **amount, sender, and receiver hidden**
on-chain.

## What happens

You paste a recipient's `ozky…` code and an amount. The Rust core does coin selection —
the **transfer4** circuit spends up to **4 owned notes** — builds a witness, and proves it:
it shows the input notes are in the Merkle tree, publishes their **nullifiers**, and creates
two output commitments — one for the recipient, one for your **change**. A relayer submits
it; the **[pool](/docs/contracts/pool)** verifies and records it.

## What an observer sees

Indistinguishable commitments and nullifiers. Not who paid whom, not how much. Your local
**Transactions** history is the only record of the payment — it is not reconstructable from
the chain.

## Notes

- Change returns to you automatically as a fresh note.
- The recipient's wallet finds the payment by **view-tag scanning** — it simply appears.
- Funds prove **ASP** membership in-circuit, so they're provably clean.

> Spends publish nullifiers — see **[Concepts](/docs/concepts)**. To get value into the
> pool first, see **[Deposit & withdraw](/docs/features/deposit-withdraw)**.
