#!/usr/bin/env bash
# E6 live-run driver — shielded escrow (building block B) on testnet through the REAL
# app code path. Two stages, both wrapping #[ignore]d Rust integration tests:
#
#   STAGE 1 (deploy_escrow_pool): migrate to a NEW escrow-capable pool — deploy all SIX
#     verifiers (deposit/transfer/withdraw/split + escrow_contribute/escrow_payout from
#     the frozen VKs), withdraw owned notes from the OLD pool, re-deposit into the new
#     one. Prints the new contract IDs.  ->  paste them into ozky.config.json.
#
#   STAGE 2 (escrow_lifecycle_on_testnet): on the configured escrow pool, run
#     open -> contribute -> release (all-or-nothing) and open -> contribute -> refund.
#     Proves the 14-PI escrow_contribute + 7-PI escrow_payout proofs verify within budget.
#
# Run from the repo root (Git Bash). Needs Docker (ZK container) + network +
# OZKY_DEPLOY_MNEMONIC + ozky.config.json. Run STAGE 1, update config, then STAGE 2.
#   STAGE=1 OZKY_DEPLOY_MNEMONIC="..." contracts/roundtrip/e6_escrow_testnet.sh
#   STAGE=2 OZKY_DEPLOY_MNEMONIC="..." contracts/roundtrip/e6_escrow_testnet.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
STAGE="${STAGE:-1}"

echo "### build contract wasm (pool incl. escrow + verifiers) into the container target"
docker compose -f compose.zk.yaml run --rm zk bash -c \
  'cd contracts && stellar contract build' >/dev/null

echo "### warm the bb CRS volume on the escrow_contribute circuit (avoids a mid-proof re-download)"
docker compose -f compose.zk.yaml run --rm zk bash -c \
  'cd circuits/escrow_contribute && nargo compile >/dev/null 2>&1 && nargo execute >/dev/null 2>&1 && \
   bb prove --scheme ultra_honk --oracle_hash keccak --bytecode_path target/escrow_contribute.json \
     --witness_path target/escrow_contribute.gz --output_path target --output_format bytes_and_fields >/dev/null 2>&1' || true

cd ozky/src-tauri
if [ "$STAGE" = "1" ]; then
  echo "### STAGE 1/2 — deploy the escrow-capable pool + migrate funds (deploy_escrow_pool)"
  cargo test --lib -- --ignored --test-threads=1 --nocapture deploy_escrow_pool
  echo "STAGE 1 done — paste the printed OZKY_* IDs into ozky.config.json, then run STAGE=2."
else
  echo "### STAGE 2/2 — live escrow lifecycle (open/contribute/release + refund)"
  cargo test --lib -- --ignored --test-threads=1 --nocapture escrow_lifecycle_on_testnet
  echo "E6 escrow lifecycle OK"
fi
