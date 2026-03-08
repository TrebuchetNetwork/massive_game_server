#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
NAMESPACE="${MGS_KIND_NAMESPACE:-mgs-smoke}"
IMAGE_TAG="${MGS_KIND_IMAGE_TAG:-massive-game-server:kind}"
LOCAL_BASE_URL="${MGS_KIND_BASE_URL:-http://127.0.0.1:18090}"
LOCAL_WS_URL="${MGS_KIND_WS_URL:-ws://127.0.0.1:18090/ws}"
PORT_FORWARD_PORT="${MGS_KIND_PORT_FORWARD_PORT:-18090}"
PORT_FORWARD_PID=""
PORT_FORWARD_LOG=""

wait_for_ready_replicas() {
  local expected="$1"
  local ready=""
  for _ in $(seq 1 120); do
    ready="$(kubectl -n "${NAMESPACE}" get deployment massive-game-server -o jsonpath='{.status.readyReplicas}' 2>/dev/null || true)"
    if [[ "${ready}" == "${expected}" ]]; then
      return 0
    fi
    sleep 2
  done
  echo "kind smoke: expected ${expected} ready replicas, got '${ready}'" >&2
  return 1
}

cleanup_port_forward() {
  if [[ -n "${PORT_FORWARD_PID}" ]] && kill -0 "${PORT_FORWARD_PID}" >/dev/null 2>&1; then
    kill "${PORT_FORWARD_PID}" >/dev/null 2>&1 || true
    wait "${PORT_FORWARD_PID}" >/dev/null 2>&1 || true
  fi
}

start_port_forward() {
  cleanup_port_forward
  PORT_FORWARD_LOG="$(mktemp /tmp/mgs-kind-port-forward.XXXXXX.log)"
  kubectl -n "${NAMESPACE}" port-forward service/massive-game-server "${PORT_FORWARD_PORT}:8080" \
    >"${PORT_FORWARD_LOG}" 2>&1 &
  PORT_FORWARD_PID=$!
  for _ in $(seq 1 30); do
    if curl -fsS "${LOCAL_BASE_URL}/healthz" >/dev/null 2>&1 \
      && curl -fsS "${LOCAL_BASE_URL}/readyz" >/dev/null 2>&1; then
      return 0
    fi
    sleep 2
  done
  echo "kind smoke: local port-forward never became ready" >&2
  cat "${PORT_FORWARD_LOG}" >&2 || true
  return 1
}

trap cleanup_port_forward EXIT

cd "${ROOT_DIR}"

kubectl create namespace "${NAMESPACE}" --dry-run=client -o yaml | kubectl apply -f -
kubectl -n "${NAMESPACE}" apply -f k8s/deployment.yaml
kubectl -n "${NAMESPACE}" apply -f k8s/service.yaml
kubectl -n "${NAMESPACE}" apply -f k8s/networkpolicy.yaml
kubectl -n "${NAMESPACE}" apply -f k8s/hpa.yaml
kubectl -n "${NAMESPACE}" apply -f k8s/pdb.yaml

kubectl -n "${NAMESPACE}" set image deployment/massive-game-server server="${IMAGE_TAG}"
kubectl -n "${NAMESPACE}" set env deployment/massive-game-server \
  MGS_BEHIND_TLS_PROXY=0 \
  MGS_DISABLE_STUN=1 \
  MGS_MATCH_DURATION_OVERRIDE_SECS=15 \
  MGS_REQUIRE_AUTH=0 \
  MGS_TARGET_BOT_COUNT=0
kubectl -n "${NAMESPACE}" set resources deployment/massive-game-server \
  -c server \
  --requests=cpu=250m,memory=256Mi \
  --limits=cpu=1000m,memory=1Gi

kubectl -n "${NAMESPACE}" rollout status deployment/massive-game-server --timeout=300s
wait_for_ready_replicas 2

kubectl -n "${NAMESPACE}" get service massive-game-server >/dev/null
kubectl -n "${NAMESPACE}" get hpa massive-game-server >/dev/null
kubectl -n "${NAMESPACE}" get pdb massive-game-server >/dev/null

start_port_forward

pushd scripts/e2e >/dev/null
E2E_BASE_URL="${LOCAL_BASE_URL}" \
E2E_SERVER_SKIP=1 \
E2E_WS_URL="${LOCAL_WS_URL}" \
  npx playwright test \
  tests/connect.spec.js \
  tests/runtime.spec.js \
  --project=chromium \
  --workers=1 \
  --reporter=list
popd >/dev/null

kubectl -n "${NAMESPACE}" rollout restart deployment/massive-game-server
kubectl -n "${NAMESPACE}" rollout status deployment/massive-game-server --timeout=300s
start_port_forward

POD_TO_DELETE="$(kubectl -n "${NAMESPACE}" get pods -l app=massive-game-server -o jsonpath='{.items[0].metadata.name}')"
kubectl -n "${NAMESPACE}" delete pod "${POD_TO_DELETE}" --wait=true
wait_for_ready_replicas 2
start_port_forward

curl -fsS "${LOCAL_BASE_URL}/healthz" >/dev/null
curl -fsS "${LOCAL_BASE_URL}/readyz" >/dev/null

echo "kind smoke completed successfully"
