# Concepts

ozky's engine is a UTXO shielded pool. A few primitives do all the work.

## Notes

Your balance is not an account number — it is a set of **notes**. A note is private data held
by its owner:

- `value` — the amount, in base units
- `asset_tag` — which asset (USDC, USDT, EURC); a note redeems only against its own vault
- `owner_pk` — the owner's public spending key
- `blinding` — random value that makes the commitment hiding
- `epoch` — the ledger window the note belongs to
- `rho` — a per-note nullifier seed

## Commitments

The on-chain leaf is a **Poseidon commitment** to the note:

```
commitment = Poseidon(value, asset_tag, owner_pk, blinding, epoch, rho)
```

Commitments are appended to an append-only **Merkle tree** (depth 20). The tree's root is
published on-chain.

## Nullifiers

To spend a note you publish a **nullifier**:

```
nullifier = Poseidon(rho, owner_sk)
```

It is deterministic (so double-spends are caught), unlinkable to the commitment, and only the
note's owner can produce it. **Nobody can spend your notes for you** — there is no "pull".

## View tags & epochs

A cheap **view tag** lets you scan the chain for notes addressed to you without trial-decrypting
everything. **Epochs** are deterministic ledger-sequence windows used for view-key scoping,
batching, and archiving — derived from the ledger, with no oracle or wall-clock.

## Keys

Everything derives from a single **12-word recovery phrase**: it produces your Stellar key and,
separately, a BN254-native ZK spending key plus a hierarchy of viewing keys scoped by
`(account, asset, epoch)`. The ZK keys are derived from — never equal to — the Stellar key.
