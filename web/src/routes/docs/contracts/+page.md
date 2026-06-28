# Contracts

The on-chain layer is a small set of **Soroban** contracts (workspace pins
`soroban-sdk = "26"`), deployed on **Stellar testnet**. They hold the shielded state and
verify the zero-knowledge proofs produced client-side. Proofs are generated **off-chain**
(Noir / UltraHonk) and **verified on-chain** using Stellar's BN254 and Poseidon host
functions.

There are **three deployed contracts** plus an embedded **UltraHonk verifier**:

| Contract                                 | Role                                                                                                                                             |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| **[pool](/docs/contracts/pool)**         | The vault + private ledger: per-asset SAC vaults, the commitment tree, the nullifier accumulator, and the deposit/transfer/withdraw entrypoints. |
| **[policy](/docs/contracts/policy)**     | The ASP approved set and its depth-20 Poseidon root — kept separate from the pool so compliance can evolve independently.                        |
| **[viewkeys](/docs/contracts/viewkeys)** | A thin record keeper for published view keys and disclosure grants (the auditable trail).                                                        |
| **[verifier](/docs/contracts/verifier)** | The on-chain UltraHonk proof verifier, per-circuit, with an immutable verifying key.                                                             |

## How a spend flows

```
UI action  →  Rust core builds witness  →  ozky-prover (UltraHonk) proves
           →  relayer submits to pool
           →  pool calls the per-circuit verifier (BN254 + Poseidon host fns)
           →  nullifiers recorded, commitments appended, vault settles
```

You never submit from a public XLM account — a pre-funded **relayer** pays fees (see
**[Cloud runtimes](/docs/cloud)**).

> The **[circuits](/docs/circuits)** these contracts verify (deposit, transfer/transfer4,
> withdraw, split, escrow, shielded_swap, channel_close) are Noir/UltraHonk circuits — each a
> pool entrypoint, not a separate contract. Each maps to a **[feature](/docs/features)**.
