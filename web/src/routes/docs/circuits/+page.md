# Circuits

The zero-knowledge logic lives in **Noir** circuits, proved client-side with **UltraHonk**.
They are **not separate contracts** — each circuit is verified on-chain by the
**[pool](/docs/contracts/pool)** (the live pool is a "9-verifier" pool, registering one
verifier per circuit). So a feature like escrow or swap is a **circuit + a pool entrypoint**,
not a new deployed contract.

The flow is always the same: **prove off-chain, verify on-chain.** The Rust core builds a
witness, the prover produces an UltraHonk proof, the relayer submits it, and the pool's
matching **[verifier](/docs/contracts/verifier)** checks it with BN254 + Poseidon host
functions.

## The circuits

| Circuit                      | What it proves                                                                                                | Feature                                               |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| **deposit**                  | A public on-ramp commits the right value into a note.                                                         | [Deposit & withdraw](/docs/features/deposit-withdraw) |
| **transfer** / **transfer4** | Spend up to 4 owned notes → 2 outputs; membership + nullifiers + value conservation.                          | [Shielded send](/docs/features/shielded-send)         |
| **withdraw**                 | Release public tokens for a spent note, bound to the destination (`dest_bind`).                               | [Deposit & withdraw](/docs/features/deposit-withdraw) |
| **split**                    | 1-in / N-out (padded to 6) — standard denominations and multi-recipient payouts.                              | [Consolidate & split](/docs/features/notes)           |
| **shielded_swap**            | Swap one shielded asset for another against an in-pool constant-product market.                               | [Shielded swap & pay](/docs/features/swap)            |
| **escrow_contribute**        | Fund an escrow privately; the running sum stays hidden (Pedersen over Grumpkin).                              | [Escrow](/docs/features/escrow)                       |
| **escrow_payout**            | Claim a funded escrow once terms are met, with refund on expiry.                                              | [Escrow](/docs/features/escrow)                       |
| **channel_close**            | Settle a merchant-pull channel's final balance; offline draws authorized by in-circuit Schnorr over Grumpkin. | [Payment channels](/docs/features/channels)           |

A shared **notes** library defines the note/commitment/nullifier format used across all of
them, so they agree byte-for-byte with the contract and indexer.

## Frozen verifying keys

Each circuit's verifying key is **frozen and committed in the repo** (`frozen_vks/`) and set
immutably on its on-chain verifier at deployment. The client checks its proofs against the
same frozen VKs before submitting, so off-chain and on-chain verification can never diverge.

> The on-chain side of this is the **[pool](/docs/contracts/pool)** and the
> **[verifier](/docs/contracts/verifier)**; the user-facing side is **[Features](/docs/features)**.
