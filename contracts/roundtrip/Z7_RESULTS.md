# Z7 — Protocol integration on testnet (results)

**Status: PASSED 2026-06-20** (testnet, Protocol 27, epoch 28). Full lifecycle from a
scripted client (`z7_integration.sh`), all components exercised together, all
invariants held. Transcript: `z7_transcript.txt`.

## What ran (one scripted client, real on-chain proofs)

| Step | Result |
|------|--------|
| Deploy 3 verifiers (frozen VKs) + policy (asp_root + allow-list) + viewkeys | ✅ |
| **Private transfer #1** (pool P1): deposit 1000 → transfer (2-in/2-out, ASP-gated) | ✅ proof verified on-chain |
| **Double-spend**: replay transfer #1 | ✅ rejected (nullifier root advanced) |
| **Private transfer #2** (pool P2): deposit → transfer | ✅ proof verified on-chain |
| **Withdraw** (pool W): deposit → withdraw 700 to dest, 300 shielded change | ✅ invariant: vaulted 300 == change 300; dest +700 |
| **ASP gating** | ✅ deposits allow-list-gated by policy; transfers/withdraw prove `owner_pk ∈ asp_root` |
| **Disclosure**: register scoped view key → grant to auditor → auditor re-derives commitment | ✅ `is_disclosed=true`; re-derived commitment == on-chain leaf |
| **Indexer in the loop** (P1): `/scan`, `/path/0`, `/nullifier_root` | ✅ all reconstruct/serve chain state; roots match published |

## Provable disclosure (the headline)

The auditor, given the disclosed note opening (off-chain), re-derives
`commitment = 0x0db85f2d…0316fc` (decimal `6205823…076348`). This is **byte-identical**
to P1's on-chain leaf-0 `commit` event AND to the commitment the indexer serves at
`/scan` leaf 0 — provable disclosure verified against chain, no reliance on ledger
plaintext.

## Timings (container, 8 threads; `proving_times.txt`)

| Circuit | prove (ms) | verify (ms) | gates | public inputs |
|---------|-----------|-------------|-------|---------------|
| deposit  | 1988 | 410 | 2,969  | 5  |
| transfer | 5544 | 136 | 24,576 | 11 |
| withdraw | 3406 | 120 | 24,420 | 12 |

Indexer cold-start ingest of a 3-commitment / 2-nullifier pool: well under the 16 s
settle used in the script (event fetch + decode is sub-second; the rest is RPC
round-trips). On-chain `verify_proof` succeeded within the default Soroban resource
budget for all three circuits.

## Contracts deployed this run (testnet)

- transfer#1 pool P1: `CBQHI5VOGFVYIXP4IIDBDIKRJVX3RO4TUQ255YNPL62RKABHG7EIHO4C`
- transfer#2 pool P2: `CBSSPJOZDKHMCGDTJC43DCBAGIF7IIVYTZFQWEUTDQT2DYD63I6UOX5E`
- withdraw pool W: `CDNT27MYS64XPLMSYUB5SU2IM7I7KRCVIURM3MSA2GONBFKULJ5NQJ3O`
- policy: `CDULHVI6XGCED3WXM5DIB6IURV2N4ZASDCM75EKJ5BK7OKHB4ISTUMDW`
- viewkeys: `CATJWFZCNXWEVATMZRFOCUG4V7QGG2BSUUE53F7CKUOMZN6HJMXIIU2E`

## Scope note (honest)

Each spend runs on a **fresh pool** (the note sits at leaf 0), so the current witgen
(single-leaf membership path, empty-accumulator base) verifies on-chain without a
stateful witness generator. The two private transfers are therefore independent
flows, not a single shared anonymity set with transfer #2 spending transfer #1's
output. That sequential-chain case needs stateful witness generation against the
live multi-leaf tree + non-empty nullifier accumulator — the **A2 Rust core's job**
(the indexer already reconstructs the trees/witnesses; A2 consumes them at proving
time). This is the one piece deferred from a "single pool, many ops" demo, and it is
A2, not Z7.
