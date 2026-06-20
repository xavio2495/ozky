#!/usr/bin/env bash
# A3 live-run driver — the full Send lifecycle on testnet through the REAL app code
# path (not pre-staged witgen). Wraps the self-contained Rust integration test
# `core::send::tests::send_lifecycle_on_testnet`, which:
#   fund -> deploy verifiers/policy/pool (asp_root bound to a throwaway test wallet's
#   owner_pk) -> register native asset -> deposit a wallet-owned note (deposit proof
#   built by the core, verified vs the frozen VK) -> SEND 600 via the core send flow
#   (scan -> stateful witness -> prove -> submit) -> scan and assert the 600 + 400
#   outputs are ours and the spent note is gone.
#
# Unlike run_testnet.sh / z7_integration.sh (which used pre-staged proofs on fresh
# pools), this exercises the app's own witness generation + proving + submission.
#
# Run from the repo root (Git Bash). Needs Docker (ZK container) + network.
set -euo pipefail
cd "$(dirname "$0")/../.."

echo "### 1/3 build contract wasm (pool/policy/verifier) into the container target"
docker compose -f compose.zk.yaml run --rm zk bash -c \
  'cd contracts && stellar contract build' >/dev/null

echo "### 2/3 warm the bb CRS volume (one-time; avoids a mid-proof re-download)"
docker compose -f compose.zk.yaml run --rm zk bash -c \
  'cd circuits/transfer && nargo compile >/dev/null 2>&1 && nargo execute >/dev/null 2>&1 && \
   bb prove --scheme ultra_honk --oracle_hash keccak --bytecode_path target/transfer.json \
     --witness_path target/transfer.gz --output_path target --output_format bytes_and_fields >/dev/null 2>&1' || true

echo "### 3/3 run the live Send lifecycle (native cargo test, shells into the container)"
cd ozky/src-tauri
cargo test --lib -- --ignored --test-threads=1 --nocapture send_lifecycle_on_testnet

echo "A3 live Send lifecycle OK"
