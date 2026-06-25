#!/usr/bin/env bash
# Deploy the ozky cloud keeper to Cloud Run with scale-to-zero ($0 idle), and (optionally) a
# Cloud Scheduler cron that hits /tick so runs submit on schedule without the app open.
#
# Cost posture: --min-instances 0 ⇒ the service scales to zero when idle (nothing standing to bill).
# Work is request-driven (/tick), so no always-on CPU is needed. To stop ALL cost after testing:
#   gcloud run services delete ozky-keeper --region $REGION --project $PROJECT
#   gcloud scheduler jobs delete ozky-keeper-tick --location $REGION --project $PROJECT
#
# SECURITY: the relayer secret + bearer token are stored in Secret Manager and mounted via
# --set-secrets (NOT embedded in the inspectable service config). The values are piped to
# `gcloud secrets versions add` via stdin, never on a command line or in env-of-config.
#
# Usage:
#   POOL_ID=<pool C…> RELAYER_SECRET=<S…> KEEPER_TOKEN=<token> [CREATE_SCHEDULER=1] bash deploy_cloudrun.sh
set -euo pipefail

PROJECT="${PROJECT:-ozky-499920}"
REGION="${REGION:-us-east1}"
SERVICE="${SERVICE:-ozky-keeper}"
POOL_ID="${POOL_ID:?set POOL_ID to the pool contract id}"
RELAYER_SECRET="${RELAYER_SECRET:?set RELAYER_SECRET to the keeper relayer S… secret}"
KEEPER_TOKEN="${KEEPER_TOKEN:?set KEEPER_TOKEN to the per-user bearer token}"
RPC_URL="${RPC_URL:-https://soroban-testnet.stellar.org}"
PASSPHRASE="${PASSPHRASE:-Test SDF Network ; September 2015}"
TICK_SCHEDULE="${TICK_SCHEDULE:-*/5 * * * *}"
SEC_RELAYER="${SEC_RELAYER:-ozky-keeper-relayer}"
SEC_TOKEN="${SEC_TOKEN:-ozky-keeper-token}"
BUCKET="${BUCKET:-${PROJECT}-keeper-state}"

echo "### enable required APIs (idempotent)"
gcloud services enable run.googleapis.com cloudbuild.googleapis.com artifactregistry.googleapis.com \
  cloudscheduler.googleapis.com secretmanager.googleapis.com storage.googleapis.com --project "$PROJECT"

echo "### store relayer secret + token in Secret Manager (value via stdin)"
upsert_secret() { # name, value
  if gcloud secrets describe "$1" --project "$PROJECT" >/dev/null 2>&1; then
    printf '%s' "$2" | gcloud secrets versions add "$1" --project "$PROJECT" --data-file=- >/dev/null
  else
    printf '%s' "$2" | gcloud secrets create "$1" --project "$PROJECT" --replication-policy=automatic --data-file=- >/dev/null
  fi
}
upsert_secret "$SEC_RELAYER" "$RELAYER_SECRET"
upsert_secret "$SEC_TOKEN" "$KEEPER_TOKEN"

PROJNUM=$(gcloud projects describe "$PROJECT" --format='value(projectNumber)')
RUNTIME_SA="${PROJNUM}-compute@developer.gserviceaccount.com"
echo "### grant Secret Manager access to the Cloud Run runtime SA ($RUNTIME_SA)"
for s in "$SEC_RELAYER" "$SEC_TOKEN"; do
  gcloud secrets add-iam-policy-binding "$s" --project "$PROJECT" \
    --member "serviceAccount:$RUNTIME_SA" --role roles/secretmanager.secretAccessor >/dev/null
done

echo "### create state bucket (idempotent) + grant the runtime SA object access"
if ! gcloud storage buckets describe "gs://$BUCKET" --project "$PROJECT" >/dev/null 2>&1; then
  gcloud storage buckets create "gs://$BUCKET" --project "$PROJECT" --location "$REGION" --uniform-bucket-level-access
fi
gcloud storage buckets add-iam-policy-binding "gs://$BUCKET" --project "$PROJECT" \
  --member "serviceAccount:$RUNTIME_SA" --role roles/storage.objectAdmin >/dev/null

echo "### deploy from source (Cloud Build builds the Dockerfile, pushes, deploys)"
gcloud run deploy "$SERVICE" \
  --source . \
  --project "$PROJECT" \
  --region "$REGION" \
  --allow-unauthenticated \
  --min-instances 0 \
  --max-instances 2 \
  --memory 512Mi \
  --set-env-vars "^|^OZKY_POOL_CONTRACT=$POOL_ID|OZKY_RPC_URL=$RPC_URL|OZKY_NETWORK_PASSPHRASE=$PASSPHRASE|OZKY_KEEPER_BUCKET=$BUCKET" \
  --set-secrets "OZKY_RELAYER_SECRET=${SEC_RELAYER}:latest,OZKY_KEEPER_TOKEN=${SEC_TOKEN}:latest"

URL=$(gcloud run services describe "$SERVICE" --project "$PROJECT" --region "$REGION" \
  --format='value(status.url)')
echo "service URL: $URL"

echo "### smoke test"
sleep 3
echo "/health:" ; curl -s "$URL/health"; echo

if [ "${CREATE_SCHEDULER:-0}" = "1" ]; then
  echo "### create/replace Cloud Scheduler tick job ($TICK_SCHEDULE)"
  gcloud scheduler jobs delete ozky-keeper-tick --location "$REGION" --project "$PROJECT" --quiet 2>/dev/null || true
  gcloud scheduler jobs create http ozky-keeper-tick \
    --location "$REGION" --project "$PROJECT" \
    --schedule "$TICK_SCHEDULE" \
    --uri "$URL/tick" --http-method POST \
    --headers "Authorization=Bearer $KEEPER_TOKEN"
  echo "scheduler job ozky-keeper-tick created → POST $URL/tick on '$TICK_SCHEDULE'"
fi

echo ""
echo "Deployed with --min-instances 0 → scales to zero when idle (no standing cost)."
echo "Tear down:  gcloud run services delete $SERVICE --region $REGION --project $PROJECT"
echo "            gcloud scheduler jobs delete ozky-keeper-tick --location $REGION --project $PROJECT"
echo "            gcloud storage rm -r gs://$BUCKET --project $PROJECT   # keeper state (optional)"
