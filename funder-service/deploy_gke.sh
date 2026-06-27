#!/usr/bin/env bash
# Deploy the ozky onboarding funder to GKE (Autopilot): build + push the image to Artifact
# Registry, ensure a cluster, store the funder key in a k8s Secret, and apply the Deployment
# + LoadBalancer Service. Prints the external IP the app should POST /fund to.
#
# COST NOTE: GKE Autopilot bills for the running pod (a small always-on cost) plus a regional
# LoadBalancer. For a request-driven funder, Cloud Run (deploy_cloudrun.sh) scales to zero and
# is cheaper — use that unless you specifically need GKE.
#
# SECURITY: the funder key is a server-held S… with a small XLM float (no user key material).
# It is stored in a k8s Secret (created from stdin, never on a command line).
#
# Usage:
#   FUNDER_SECRET=<S…> [FUNDER_TOKEN=<token>] [FUND_AMOUNT=100000000] bash deploy_gke.sh
set -euo pipefail

PROJECT="${PROJECT:-ozky-499920}"
REGION="${REGION:-us-east1}"
CLUSTER="${CLUSTER:-ozky-autopilot}"
REPO="${REPO:-ozky}"
IMAGE_NAME="${IMAGE_NAME:-ozky-funder-service}"
FUNDER_SECRET="${FUNDER_SECRET:?set FUNDER_SECRET to the funder S… secret (a funded account)}"
FUNDER_TOKEN="${FUNDER_TOKEN:-}"
RPC_URL="${RPC_URL:-https://soroban-testnet.stellar.org}"
PASSPHRASE="${PASSPHRASE:-Test SDF Network ; September 2015}"
FUND_AMOUNT="${FUND_AMOUNT:-100000000}" # 10 XLM in stroops

IMAGE="${REGION}-docker.pkg.dev/${PROJECT}/${REPO}/${IMAGE_NAME}:latest"

echo "### enable required APIs (idempotent)"
gcloud services enable container.googleapis.com artifactregistry.googleapis.com \
  cloudbuild.googleapis.com --project "$PROJECT"

echo "### ensure Artifact Registry repo '$REPO'"
if ! gcloud artifacts repositories describe "$REPO" --location "$REGION" --project "$PROJECT" >/dev/null 2>&1; then
  gcloud artifacts repositories create "$REPO" --repository-format=docker \
    --location "$REGION" --project "$PROJECT"
fi

echo "### build + push image via Cloud Build → $IMAGE"
gcloud builds submit --tag "$IMAGE" --project "$PROJECT" .

echo "### ensure GKE Autopilot cluster '$CLUSTER'"
if ! gcloud container clusters describe "$CLUSTER" --region "$REGION" --project "$PROJECT" >/dev/null 2>&1; then
  gcloud container clusters create-auto "$CLUSTER" --region "$REGION" --project "$PROJECT"
fi
gcloud container clusters get-credentials "$CLUSTER" --region "$REGION" --project "$PROJECT"

echo "### upsert the funder Secret (values via stdin)"
kubectl create secret generic ozky-funder-secret \
  --from-literal=funder-secret="$FUNDER_SECRET" \
  --from-literal=funder-token="$FUNDER_TOKEN" \
  --dry-run=client -o yaml | kubectl apply -f -

echo "### apply manifests"
export IMAGE RPC_URL PASSPHRASE FUND_AMOUNT
envsubst '${IMAGE} ${RPC_URL} ${PASSPHRASE} ${FUND_AMOUNT}' < k8s/deployment.yaml | kubectl apply -f -
kubectl apply -f k8s/service.yaml
kubectl rollout status deployment/ozky-funder --timeout=180s

echo "### wait for the LoadBalancer external IP"
for _ in $(seq 1 30); do
  IP=$(kubectl get service ozky-funder -o jsonpath='{.status.loadBalancer.ingress[0].ip}' 2>/dev/null || true)
  [ -n "${IP:-}" ] && break
  sleep 5
done
echo "funder external IP: ${IP:-<pending — re-check: kubectl get service ozky-funder>}"
echo
echo "Set the app config to: OZKY_FUNDER_URL=http://${IP:-<IP>}/fund"
[ -n "$FUNDER_TOKEN" ] && echo "                       OZKY_FUNDER_TOKEN=$FUNDER_TOKEN"
echo
echo "Tear down:  kubectl delete -f k8s/service.yaml; kubectl delete deployment ozky-funder"
echo "            gcloud container clusters delete $CLUSTER --region $REGION --project $PROJECT"
