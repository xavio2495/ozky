#!/usr/bin/env bash
# Deploy the ozky indexer to Cloud Run with scale-to-zero ($0 idle).
#
# Cost posture: Cloud Run bills per request-time only; with --min-instances=0 the
# service scales to zero when idle, so there is nothing to "shut down" — idle cost is
# $0. State is rebuilt from chain on each cold start (blocking initial ingest in
# main.rs), so scale-to-zero is safe. This is why Cloud Run, not GKE (GKE bills a
# control plane + nodes continuously).
#
# Usage: POOL_ID=<pool contract id> bash deploy_cloudrun.sh
set -euo pipefail

PROJECT="${PROJECT:-ozky-499920}"
REGION="${REGION:-us-east1}"
SERVICE="${SERVICE:-ozky-indexer}"
POOL_ID="${POOL_ID:?set POOL_ID to the pool contract id the indexer should watch}"
RPC_URL="${RPC_URL:-https://soroban-testnet.stellar.org}"

echo "### enable required APIs (idempotent)"
gcloud services enable run.googleapis.com cloudbuild.googleapis.com \
  artifactregistry.googleapis.com --project "$PROJECT"

echo "### deploy from source (Cloud Build builds the Dockerfile, pushes, deploys)"
gcloud run deploy "$SERVICE" \
  --source . \
  --project "$PROJECT" \
  --region "$REGION" \
  --allow-unauthenticated \
  --min-instances 0 \
  --max-instances 2 \
  --memory 512Mi \
  --set-env-vars "POOL_ID=$POOL_ID,RPC_URL=$RPC_URL"

URL=$(gcloud run services describe "$SERVICE" --project "$PROJECT" --region "$REGION" \
  --format='value(status.url)')
echo "service URL: $URL"

echo "### smoke test (a request triggers a cold start + initial chain ingest)"
sleep 3
echo "/health:" ; curl -s "$URL/health"; echo
echo "/status:" ; curl -s "$URL/status"; echo

echo ""
echo "Deployed with --min-instances 0 → scales to zero when idle (no standing cost)."
echo "To remove entirely later:  gcloud run services delete $SERVICE --region $REGION --project $PROJECT"
