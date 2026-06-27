#!/usr/bin/env bash
# Deploy the ozky onboarding funder to Cloud Run with scale-to-zero ($0 idle). This is the
# cheaper, repo-consistent alternative to GKE (deploy_gke.sh) for a request-driven funder.
#
# SECURITY: the funder key + optional token are stored in Secret Manager and mounted via
# --set-secrets (never embedded in the inspectable service config), piped via stdin.
#
# Usage:
#   FUNDER_SECRET=<S…> [FUNDER_TOKEN=<token>] [FUND_AMOUNT=100000000] bash deploy_cloudrun.sh
set -euo pipefail

PROJECT="${PROJECT:-ozky-499920}"
REGION="${REGION:-us-east1}"
SERVICE="${SERVICE:-ozky-funder}"
FUNDER_SECRET="${FUNDER_SECRET:?set FUNDER_SECRET to the funder S… secret (a funded account)}"
FUNDER_TOKEN="${FUNDER_TOKEN:-}"
RPC_URL="${RPC_URL:-https://soroban-testnet.stellar.org}"
PASSPHRASE="${PASSPHRASE:-Test SDF Network ; September 2015}"
FUND_AMOUNT="${FUND_AMOUNT:-100000000}" # 10 XLM in stroops
SEC_FUNDER="${SEC_FUNDER:-ozky-funder-secret}"
SEC_TOKEN="${SEC_TOKEN:-ozky-funder-token}"

echo "### enable required APIs (idempotent)"
gcloud services enable run.googleapis.com cloudbuild.googleapis.com artifactregistry.googleapis.com \
  secretmanager.googleapis.com --project "$PROJECT"

upsert_secret() { # name, value
  if gcloud secrets describe "$1" --project "$PROJECT" >/dev/null 2>&1; then
    printf '%s' "$2" | gcloud secrets versions add "$1" --project "$PROJECT" --data-file=- >/dev/null
  else
    printf '%s' "$2" | gcloud secrets create "$1" --project "$PROJECT" --replication-policy=automatic --data-file=- >/dev/null
  fi
}
echo "### store funder secret in Secret Manager (value via stdin)"
upsert_secret "$SEC_FUNDER" "$FUNDER_SECRET"

PROJNUM=$(gcloud projects describe "$PROJECT" --format='value(projectNumber)')
RUNTIME_SA="${PROJNUM}-compute@developer.gserviceaccount.com"
gcloud secrets add-iam-policy-binding "$SEC_FUNDER" --project "$PROJECT" \
  --member "serviceAccount:$RUNTIME_SA" --role roles/secretmanager.secretAccessor >/dev/null

SECRET_FLAGS="OZKY_FUNDER_SECRET=${SEC_FUNDER}:latest"
if [ -n "$FUNDER_TOKEN" ]; then
  upsert_secret "$SEC_TOKEN" "$FUNDER_TOKEN"
  gcloud secrets add-iam-policy-binding "$SEC_TOKEN" --project "$PROJECT" \
    --member "serviceAccount:$RUNTIME_SA" --role roles/secretmanager.secretAccessor >/dev/null
  SECRET_FLAGS="${SECRET_FLAGS},OZKY_FUNDER_TOKEN=${SEC_TOKEN}:latest"
fi

echo "### deploy from source (Cloud Build builds the Dockerfile, pushes, deploys)"
gcloud run deploy "$SERVICE" \
  --source . \
  --project "$PROJECT" \
  --region "$REGION" \
  --allow-unauthenticated \
  --min-instances 0 \
  --max-instances 2 \
  --memory 512Mi \
  --set-env-vars "^|^OZKY_RPC_URL=$RPC_URL|OZKY_NETWORK_PASSPHRASE=$PASSPHRASE|OZKY_FUND_AMOUNT=$FUND_AMOUNT" \
  --set-secrets "$SECRET_FLAGS"

URL=$(gcloud run services describe "$SERVICE" --project "$PROJECT" --region "$REGION" \
  --format='value(status.url)')
echo "service URL: $URL"
sleep 3
echo "/health:" ; curl -s "$URL/health"; echo
echo
echo "Set the app config to: OZKY_FUNDER_URL=$URL/fund"
[ -n "$FUNDER_TOKEN" ] && echo "                       OZKY_FUNDER_TOKEN=$FUNDER_TOKEN"
echo "Deployed with --min-instances 0 → scales to zero when idle (no standing cost)."
echo "Tear down:  gcloud run services delete $SERVICE --region $REGION --project $PROJECT"
