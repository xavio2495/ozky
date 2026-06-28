# verifier

The on-chain **UltraHonk proof verifier** (`rs-soroban-ultrahonk`, wrapping the
`ultrahonk-soroban-verifier` crate). The **[pool](/docs/contracts/pool)** delegates proof
verification to it — one verifier per circuit, which is why the live pool is described as a
"9-verifier" pool.

## What it does

It checks an UltraHonk proof against a verifying key and the transaction's public inputs,
using Stellar's ZK host functions:

- **BN254 / alt_bn128** pairings (CAP-0074, Protocol 25 "X-Ray")
- **Poseidon / Poseidon2** hashing (CAP-0075)

This is what makes "prove off-chain, verify on-chain" cheap enough to run inside a Soroban
transaction.

## Immutable verifying key

The VK is **set once at deployment and cannot be changed** — there is no admin key, no
governor, no upgrade path. The deployer is solely responsible for supplying the correct VK.
On testnet that key is developer-controlled; moving VK upgrades under **governance / multisig**
is a tracked pre-mainnet item.

> No single app-level address: verifier instances are deployed per circuit and referenced by
> the pool. The verifying keys themselves are frozen and committed in the repo
> (`frozen_vks/`).
