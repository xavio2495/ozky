#!/usr/bin/env bash
# Z4 done-criterion: deposit -> transfer -> withdraw round-trips on testnet against
# the real verifiers; a replayed nullifier (double-spend) is rejected; the
# shielded==vaulted invariant holds.
#
# Run inside the ZK container (one invocation; testnet state persists, local keys
# do not). Proofs/VKs are pre-staged under contracts/roundtrip/{deposit,transfer,
# withdraw}/ by witgen at epoch 28 (testnet's current epoch), pool_id=7,
# network_id=42 — so each proof's domain_sep/epoch/roots bind to the pools below.
#
# Asset = native XLM via its SAC (asset_tag=1), avoiding classic issuance/mint.
# Two fresh pools share the three verifier deployments:
#   Pool A: deposit -> transfer -> replay transfer (must be rejected)
#   Pool B: deposit -> withdraw (releases 700, keeps 300 change)
set -euo pipefail

NET=testnet
RT=/workspace/contracts/roundtrip
POOL_WASM=/workspace/contracts/target/wasm32v1-none/release/pool.wasm
VER_WASM=/workspace/contracts/target/wasm32v1-none/release/rs_soroban_ultrahonk.wasm
POLICY_WASM=/workspace/contracts/target/wasm32v1-none/release/policy.wasm
POOL_ID=7
NETWORK_ID=42
ASP_ROOT=9979624481174071864301327747278505457583196166631345769412112690562003548381
ASSET_TAG=1
EPH=0000000000000000000000000000000000000000000000000000000000000000

# Strip surrounding quotes/whitespace from CLI scalar output (i128 prints quoted).
clean() { tr -d '"[:space:]'; }
bal() { stellar contract invoke --id "$SAC" --source admin --network "$NET" -- balance --id "$1" | clean; }

stellar network add $NET --rpc-url https://soroban-testnet.stellar.org \
  --network-passphrase "Test SDF Network ; September 2015" 2>/dev/null || true

echo "### keys"
stellar keys generate admin --network $NET --fund --overwrite >/dev/null 2>&1
stellar keys generate dest  --network $NET --fund --overwrite >/dev/null 2>&1
ADMIN=$(stellar keys address admin)
DEST=$(stellar keys address dest)
echo "admin=$ADMIN dest=$DEST"

echo "### native SAC id (asset_tag $ASSET_TAG -> native XLM)"
SAC=$(stellar contract id asset --asset native --network $NET)
# Ensure the native SAC wrapper is instantiated on testnet (no-op if already).
stellar contract asset deploy --asset native --source admin --network $NET >/dev/null 2>&1 || true
echo "SAC=$SAC"

echo "### deploy 3 verifiers (frozen VKs)"
VDEP=$(stellar contract deploy --wasm $VER_WASM --source admin --network $NET -- --vk_bytes-file-path /workspace/contracts/frozen_vks/deposit/vk)
VTRA=$(stellar contract deploy --wasm $VER_WASM --source admin --network $NET -- --vk_bytes-file-path /workspace/contracts/frozen_vks/transfer/vk)
VWIT=$(stellar contract deploy --wasm $VER_WASM --source admin --network $NET -- --vk_bytes-file-path /workspace/contracts/frozen_vks/withdraw/vk)
echo "deposit_verifier=$VDEP"
echo "transfer_verifier=$VTRA"
echo "withdraw_verifier=$VWIT"

echo "### deploy policy (asp_root + deposit allow-list) and allow admin to deposit"
POLICY=$(stellar contract deploy --wasm $POLICY_WASM --source admin --network $NET -- \
  --admin $ADMIN --asp_root $ASP_ROOT)
echo "policy=$POLICY"
stellar contract invoke --id $POLICY --source admin --network $NET -- \
  set_allowed --who $ADMIN --allowed true >/dev/null
echo "admin allow-listed for deposit"

deploy_pool() {
  stellar contract deploy --wasm $POOL_WASM --source admin --network $NET -- \
    --pool_id $POOL_ID --network_id $NETWORK_ID \
    --deposit_verifier $VDEP --transfer_verifier $VTRA --withdraw_verifier $VWIT \
    --policy $POLICY --asp_root $ASP_ROOT --admin $ADMIN
}

echo ""
echo "========================= POOL A: deposit -> transfer -> replay ========================="
POOLA=$(deploy_pool)
echo "poolA=$POOLA"
stellar contract invoke --id $POOLA --source admin --network $NET -- \
  register_asset --asset_tag $ASSET_TAG --sac $SAC --decimals 7 >/dev/null
echo "registered asset"

echo "--- deposit 1000 (locks XLM, mints note) ---"
stellar contract invoke --id $POOLA --source admin --network $NET --send yes -- \
  deposit --from $ADMIN --asset_tag $ASSET_TAG --amount 1000 \
  --public_inputs-file-path $RT/deposit/public_inputs \
  --proof-file-path $RT/deposit/proof \
  --enc_note deadbeef --ephemeral_pub $EPH --view_tag 0
echo "poolA vault balance: $(bal $POOLA)"

echo "--- transfer (spends the note; 2 outputs) ---"
stellar contract invoke --id $POOLA --source admin --network $NET --send yes -- \
  transfer --asset_tag $ASSET_TAG \
  --public_inputs-file-path $RT/transfer/public_inputs \
  --proof-file-path $RT/transfer/proof \
  --enc_notes '[]' --ephemeral_pubs '[]' --view_tags '[]'
echo "transfer OK; nullifier_root=$(stellar contract invoke --id $POOLA --source admin --network $NET -- nullifier_root)"

echo "--- replay transfer (double-spend MUST be rejected) ---"
if stellar contract invoke --id $POOLA --source admin --network $NET --send yes -- \
  transfer --asset_tag $ASSET_TAG \
  --public_inputs-file-path $RT/transfer/public_inputs \
  --proof-file-path $RT/transfer/proof \
  --enc_notes '[]' --ephemeral_pubs '[]' --view_tags '[]' >/dev/null 2>&1; then
  echo "!!! DOUBLE-SPEND ACCEPTED — FAIL"; exit 1
else
  echo "double-spend rejected (expected) OK"
fi

echo ""
echo "========================= POOL B: deposit -> withdraw ========================="
POOLB=$(deploy_pool)
echo "poolB=$POOLB"
stellar contract invoke --id $POOLB --source admin --network $NET -- \
  register_asset --asset_tag $ASSET_TAG --sac $SAC --decimals 7 >/dev/null

echo "--- deposit 1000 ---"
stellar contract invoke --id $POOLB --source admin --network $NET --send yes -- \
  deposit --from $ADMIN --asset_tag $ASSET_TAG --amount 1000 \
  --public_inputs-file-path $RT/deposit/public_inputs \
  --proof-file-path $RT/deposit/proof \
  --enc_note deadbeef --ephemeral_pub $EPH --view_tag 0
VAULT0=$(bal $POOLB)
DEST0=$(bal $DEST)
echo "poolB vault after deposit: $VAULT0 ; dest before: $DEST0"

echo "--- withdraw 700 to dest (keeps 300 shielded change) ---"
stellar contract invoke --id $POOLB --source admin --network $NET --send yes -- \
  withdraw --dest $DEST --asset_tag $ASSET_TAG --amount 700 \
  --public_inputs-file-path $RT/withdraw/public_inputs \
  --proof-file-path $RT/withdraw/proof
VAULT1=$(bal $POOLB)
DEST1=$(bal $DEST)
echo "poolB vault after withdraw: $VAULT1 ; dest after: $DEST1"

echo ""
echo "========================= INVARIANT ========================="
echo "vaulted real units (pool B) = $VAULT1   (expect 300 = shielded change)"
echo "dest received = $(( DEST1 - DEST0 ))     (expect 700)"
if [ "$VAULT1" = "300" ] && [ "$(( DEST1 - DEST0 ))" = "700" ]; then
  echo "INVARIANT HOLDS: shielded(300) == vaulted(300); 700 released. Z4 round-trip OK"
else
  echo "!!! INVARIANT VIOLATED — FAIL"; exit 1
fi
