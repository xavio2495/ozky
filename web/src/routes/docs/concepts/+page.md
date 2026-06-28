# Concepts

ozky is a **UTXO shielded pool** for stablecoins on Stellar/Soroban — not account
balances. This page covers the zero-knowledge primitives it is built on and **where each
one shows up in the app**. It is testnet-first and unaudited.

## Notes — the unit of value

Your balance is a set of private **notes**, never a public number. A note is private data
held by its owner — `value`, `asset_tag`, `owner_pk`, a random `blinding`, the `epoch`,
and a per-note seed `rho`. On-chain a note exists only as its commitment.

_Where in the app:_ the **Wallet** page shows your spendable balance by scanning and
summing the notes only you can decrypt.

## Commitments & the Merkle tree

The on-chain leaf is a **Poseidon commitment** to the note:

```
commitment = Poseidon(value, asset_tag, owner_pk, blinding, epoch, rho)
```

Commitments are appended to an append-only **Merkle tree** (depth 20). The root is the
public anchor for "all value that exists in the pool."

_Where in the app:_ every action that creates value — **Deposit**, change from a
**Send**, a **Swap** output — appends a new commitment.

## Nullifiers — spending without linking

To spend a note you publish a **nullifier** and prove in zero-knowledge that the note is
in the tree:

```
nullifier = Poseidon(rho, owner_sk)
```

It is deterministic (double-spends are caught), unlinkable to the commitment, and only the
owner can produce it — there is no "pull". So spends can't be linked to deposits.

_Where in the app:_ **Send**, **Withdraw**, **Swap**, and **Consolidate** spend notes and
publish nullifiers; the **Transactions** page reads your local history, not the chain graph.

## Proving — Noir / UltraHonk

Proofs are written as **Noir** circuits and proved with **UltraHonk**, fully
**client-side** in the native Rust core, off the UI thread. The chain only ever sees a
proof and its public inputs — never your keys or amounts.

_Where in the app:_ proving runs in the `ozky-prover` sidecar; the UI just shows progress.

## View tags & scanning

Incoming notes are found by trial-decryption accelerated with cheap **view tags**, so you
scan the chain for your notes without decrypting everything. Scanning runs in the Rust core.

_Where in the app:_ receiving a payment "just appears" in **Wallet** once the scanner
detects it.

## Keys — separate from your Stellar key

Everything derives from one **12-word BIP39 phrase**, but the in-circuit **`owner_sk`**
(BN254-native) is **never** the Ed25519 Stellar key — they are kept separate by design.
A **BIP32-style view-key tree** derives scoped, revocable read-only keys per
`account / asset / epoch`.

_Where in the app:_ **Settings** manages the phrase; the **Auditor** page hands out scoped
view keys. See **[Auditor disclosure](/docs/features/disclosure)**.

## Compliance — approved-set membership

Every transfer proves **in-circuit** that its funds trace to a depositor in an
**Association Set Provider (ASP)** approved set — without revealing which one. Shielded
funds are provably clean while the graph stays private.

_Where in the app:_ surfaced as a status on **Send/Withdraw**; see
**[ASP compliance](/docs/features/compliance)**.

## Epochs

Note encryption and view-key scoping are partitioned by **epoch**
(`LEDGER_PER_EPOCH = 110_000`) — deterministic ledger-sequence windows, no oracle or
wall-clock. Epochs bound disclosure windows and keep scanning bounded.

## The edges

Privacy is strongest **inside** the pool. **Deposits, withdrawals, and bridges** are public
Stellar legs — ozky labels them and offers a denomination/timing policy to reduce
correlation, framed as _speed vs. privacy_. (Swaps are now a fully in-pool
**[shielded AMM](/docs/features/swap)** — no public DEX edge.) A pre-funded **relayer** pays
XLM fees so you never touch a public XLM account.

> Next: **[Contracts](/docs/contracts)** — the on-chain layer that verifies all of this.
