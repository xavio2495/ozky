# Cloud runtimes

ozky is a desktop wallet, but a few small **off-device services** sit alongside it. None of
them can spend your funds or see your amounts — each is deliberately scoped so a compromise
risks at most a small fee float, never user value. All are testnet today.

## Relayer — fee abstraction

A **pre-funded relayer** submits and fee-pays the interior operations (transfer, withdraw,
etc.). You hold **no XLM** and are never linked as the fee-payer of your own shielded
transactions. The relayer sees a proof and public inputs — never your keys or amounts.

## Funder — onboarding

A tiny HTTP service (`funder-service`) that turns a brand-new wallet address into a funded
Stellar account. A fresh account doesn't exist on-chain until something runs `CreateAccount`,
and the wallet has no XLM yet — so a server-held funded key does it once.

- `POST /fund {"address":"G…"}` → `CreateAccount(10 XLM)`, **idempotent** (an existing
  account returns `200` without re-funding) and **serialized**.
- Holds **only the funder key** (a small XLM float) — never any user key material. The app
  then establishes its USDC/EURC trustlines locally, paid by the new XLM.

Deploys to **Cloud Run** (scale-to-zero, \$0 idle).

## Keeper — headless payroll

A managed submitter (`keeper-service`) for **scheduled payroll** that runs even when the app
is closed. The wallet **pushes pre-proved runs**; a Cloud Scheduler cron hits `/tick` and the
service submits any due bundle via this user's **dedicated relayer**.

- Holds **no `owner_sk` and no `notes_key`** — a pushed bundle carries no key material and no
  plaintext amounts. A leak risks only the relayer's fee float, never funds.
- A submitted proof's nullifier is consumed, so there is **no replay**.
- Routes (bearer-token protected): `POST /push`, `GET /status`, `DELETE /run/<id>`,
  `POST /tick`. Cloud Run, `--min-instances 0`.

## Indexer — speed layer

A pure **speed / availability** layer (`indexer`) over Stellar RPC. It watches the pool's
events and serves the data clients need to scan for notes and build spends — but it is
**never on the correctness or liveness path**.

- Everything it serves (`/scan`, `/path/<leaf>`, `/nullifiers`, `/nullifier_root`,
  non-membership witnesses) is **re-derivable from raw chain events**.
- Reconstructed roots are compared to the contract's own published `roots` event on every
  response, and tree hashing is parity-locked to the circuit's Poseidon2.
- **Offline recovery is proven**: with the indexer down, a client rebuilds the identical
  root and Merkle path from chain events alone.

## Prover sidecar — local, not cloud

For completeness: proving runs **on your device**, not in the cloud. The native
**`ozky-prover`** sidecar (bb.js + noir_js WASM) proves byte-identically to the container
`bb`, so there is no Docker dependency for end users and your witness never leaves the
machine.

> The relayer, funder, and keeper all act only on **already-proved, key-free** material —
> consistent with the trust model in **[Concepts](/docs/concepts)**.
