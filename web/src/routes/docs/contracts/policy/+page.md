# policy

The **ASP / compliance** contract. It is kept **separate from the pool** on purpose, so
compliance logic can evolve without touching the shielded-pool audit surface.

**Deployed (testnet):**
[`CCXRKEM3MUJBFXJOC6VMU7OUFJWSNO76LJPSTSQIODSSLZ2AMNTZG2CP`](https://stellar.expert/explorer/testnet/contract/CCXRKEM3MUJBFXJOC6VMU7OUFJWSNO76LJPSTSQIODSSLZ2AMNTZG2CP)

## What it owns

The **ASP approved set** — an ordered list of approved spending keys (`owner_pk`s) and its
depth-20 Poseidon Merkle **root** (`asp_root`). The contract recomputes the root on every
enrollment, so it always matches the circuit's view of the set.

## How it's used

- Transfers and withdrawals prove **`owner_pk ∈ asp_root` in-circuit** — membership without
  revealing which member.
- Each enrollment emits an **`asp_mem`** event, so a client can reconstruct the set from the
  chain (indexer-free), build its own membership path, and self-check it against the root.
- The shared set has **more than one** member, so membership leaks nothing about identity.

## On mainnet

This contract is the hook for **denied-set non-membership** (freeze bad actors at withdrawal
time) and for moving the approved-set root and policy parameters under
**governance / multisig** control.

> The user-facing side of this is **[ASP compliance](/docs/features/compliance)**.
