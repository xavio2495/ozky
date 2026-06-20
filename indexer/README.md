# ozky indexer (Z6)

A pure speed/availability layer over Stellar RPC. It watches the shielded-pool
contract's events and serves clients the data they need to scan for their notes and
build spends — **never on the correctness or liveness path**. Everything it serves is
re-derivable from raw chain events, so a client can recover with the indexer offline
(proven by `offline_drill.sh`).

## Endpoints

| Endpoint | Serves |
|----------|--------|
| `GET /health` | liveness |
| `GET /status` | counts + latest commitment/nullifier roots + last ledger |
| `GET /scan?from=<leaf>` | commitments (with `enc_note`, `ephemeral_pub`, `view_tag`) from a leaf index — the note-scan stream |
| `GET /nullifiers` | all published nullifiers |
| `GET /path/<leaf>` | depth-20 Merkle authentication path for a commitment leaf, with `root_matches_published` self-check |
| `GET /nullifier_root` | reconstructed accumulator root vs the contract's published one |
| `GET /nonmembership/<nullifier>` | indexed-tree non-membership witness (low leaf + path) for an unspent nullifier |

## How it stays off the correctness path

- **Re-derivable.** State is built only from `commit`/`nullif`/`roots` events fetched
  via `getEvents`. No privileged data source.
- **Self-checking.** Reconstructed commitment and nullifier roots are compared to the
  contract's own published `roots` event on every relevant response
  (`root_matches_published`). A served path/witness that doesn't match the chain root
  is flagged rather than trusted.
- **Parity-locked.** Tree hashing uses `soroban-poseidon` — the exact Poseidon2 the
  circuit/contract use — guarded by a startup self-test against the frozen reference
  vector `Poseidon2([1,2]) = 0x0386…ed7383`. So a served Merkle path is byte-for-byte
  acceptable to the spend circuit.
- **Offline recovery.** `offline_drill.sh` shows the indexer's served root equals the
  contract's published root decoded independently from raw RPC — i.e. with the indexer
  down, a client rebuilds the identical root/path from chain events alone.

## Run locally

```bash
POOL_ID=<pool contract id> cargo run --release
# then: curl localhost:8080/status
```

Env: `POOL_ID` (required), `RPC_URL` (default testnet), `PORT` (default 8080),
`POLL_SECS` (default 6), `LOOKBACK` (default 120000 ledgers).

## Deploy (Cloud Run, scale-to-zero)

```bash
POOL_ID=<pool contract id> bash deploy_cloudrun.sh
```

Cloud Run with `--min-instances 0` scales to zero when idle → **$0 standing cost**,
nothing to shut down. Each cold start does a blocking initial ingest from chain, so
correctness survives eviction. (GKE was the original plan but bills a control plane +
nodes continuously; Cloud Run fits the "shut down when not in use" requirement.)
