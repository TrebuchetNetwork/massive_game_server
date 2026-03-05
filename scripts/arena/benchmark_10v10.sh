#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ARTIFACT_DIR="$ROOT_DIR/artifacts/arena"
mkdir -p "$ARTIFACT_DIR"

API_BASE="${ARENA_API_BASE:-http://127.0.0.1:18082}"
ADMIN_BEARER_TOKEN="${ARENA_ADMIN_BEARER_TOKEN:-}"
REQUIRE_REAL_PROVIDER="${ARENA_REQUIRE_REAL_PROVIDER:-1}"
TEAM_SIZE="${ARENA_TEAM_SIZE:-10}"
ROUNDS="${ARENA_ROUNDS:-3}"
MODE="${ARENA_MODE:-tdm}"
MAX_TICKS="${ARENA_MAX_TICKS:-240}"
SEED="${ARENA_SEED:-42}"

MODEL_A_PROVIDER_MODEL="${ARENA_MODEL_A_PROVIDER_MODEL:-openai/gpt-4o-mini}"
MODEL_B_PROVIDER_MODEL="${ARENA_MODEL_B_PROVIDER_MODEL:-anthropic/claude-3.5-sonnet}"
MODEL_A_OBJECTIVE="${ARENA_MODEL_A_OBJECTIVE:-high-pressure flanking and objective denial}"
MODEL_B_OBJECTIVE="${ARENA_MODEL_B_OBJECTIVE:-defensive teamplay with timed counter-pushes}"
PROMPT_STYLE_A="${ARENA_MODEL_A_PROMPT_STYLE:-aggressive}"
PROMPT_STYLE_B="${ARENA_MODEL_B_PROMPT_STYLE:-adaptive}"

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required" >&2
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 1
fi

if [[ "$REQUIRE_REAL_PROVIDER" == "1" && -z "${OPENROUTER_API_KEY:-}" ]]; then
  echo "OPENROUTER_API_KEY is not set. Set it before running real-provider benchmark mode." >&2
  exit 1
fi
if [[ "$REQUIRE_REAL_PROVIDER" != "1" && -z "${OPENROUTER_API_KEY:-}" ]]; then
  echo "[arena] OPENROUTER_API_KEY not set; allowing template fallback (ARENA_REQUIRE_REAL_PROVIDER=0)." >&2
fi

ts="$(date -u +%Y%m%d_%H%M%S)"
MODEL_A_ID="${ARENA_MODEL_A_ID:-arena_model_a_${ts}}"
MODEL_B_ID="${ARENA_MODEL_B_ID:-arena_model_b_${ts}}"
OUT_JSON="$ARTIFACT_DIR/arena_10v10_${ts}.json"

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

post_json() {
  local path="$1"
  local payload="$2"
  local output="$3"
  local status
  local -a auth_header=()
  if [[ -n "$ADMIN_BEARER_TOKEN" ]]; then
    auth_header=(-H "authorization: Bearer $ADMIN_BEARER_TOKEN")
  fi
  status="$(curl -sS -o "$output" -w "%{http_code}" \
    -H "content-type: application/json" \
    "${auth_header[@]}" \
    -X POST \
    "$API_BASE$path" \
    -d "$payload")"
  if [[ "$status" -lt 200 || "$status" -ge 300 ]]; then
    echo "request failed: POST $path status=$status" >&2
    cat "$output" >&2 || true
    exit 1
  fi
}

echo "[arena] registering models..."
post_json "/api/arena/models/register" \
  "{\"model_id\":\"$MODEL_A_ID\",\"model_name\":\"$MODEL_A_ID\",\"provider\":\"openrouter\",\"version\":\"latest\",\"active\":true}" \
  "$tmp_dir/register_a.json"
post_json "/api/arena/models/register" \
  "{\"model_id\":\"$MODEL_B_ID\",\"model_name\":\"$MODEL_B_ID\",\"provider\":\"openrouter\",\"version\":\"latest\",\"active\":true}" \
  "$tmp_dir/register_b.json"

echo "[arena] generating + compiling strategies..."
post_json "/api/arena/code/generate_and_compile" \
  "{\"model_id\":\"$MODEL_A_ID\",\"model\":\"$MODEL_A_PROVIDER_MODEL\",\"objective\":\"$MODEL_A_OBJECTIVE\",\"prompt_style\":\"$PROMPT_STYLE_A\",\"overwrite\":true}" \
  "$tmp_dir/generate_a.json"
post_json "/api/arena/code/generate_and_compile" \
  "{\"model_id\":\"$MODEL_B_ID\",\"model\":\"$MODEL_B_PROVIDER_MODEL\",\"objective\":\"$MODEL_B_OBJECTIVE\",\"prompt_style\":\"$PROMPT_STYLE_B\",\"overwrite\":true}" \
  "$tmp_dir/generate_b.json"

compiled_a="$(jq -r '.data.compile.compiled // false' "$tmp_dir/generate_a.json")"
compiled_b="$(jq -r '.data.compile.compiled // false' "$tmp_dir/generate_b.json")"
simulated_a="$(jq -r '.data.generated.simulated // true' "$tmp_dir/generate_a.json")"
simulated_b="$(jq -r '.data.generated.simulated // true' "$tmp_dir/generate_b.json")"
if [[ "$compiled_a" != "true" || "$compiled_b" != "true" ]]; then
  echo "compile failed for one or both models" >&2
  jq '{a:.data.compile,b:input.data.compile}' "$tmp_dir/generate_a.json" "$tmp_dir/generate_b.json" >&2
  exit 1
fi
if [[ "$REQUIRE_REAL_PROVIDER" == "1" ]]; then
  if [[ "$simulated_a" != "false" || "$simulated_b" != "false" ]]; then
    echo "provider generation fell back to template (simulated=true) while real provider is required" >&2
    exit 1
  fi
fi

echo "[arena] running ${TEAM_SIZE}v${TEAM_SIZE} team simulation..."
post_json "/api/arena/matches/simulate_team_battle" \
  "{\"model_a_id\":\"$MODEL_A_ID\",\"model_b_id\":\"$MODEL_B_ID\",\"mode\":\"$MODE\",\"team_size\":$TEAM_SIZE,\"rounds\":$ROUNDS,\"max_ticks\":$MAX_TICKS,\"seed\":$SEED}" \
  "$tmp_dir/simulate.json"

expected_engagements=$((TEAM_SIZE * ROUNDS))
actual_engagements="$(jq -r '.data.simulation.total_engagements // 0' "$tmp_dir/simulate.json")"
if [[ "$actual_engagements" -ne "$expected_engagements" ]]; then
  echo "unexpected engagement count: expected=$expected_engagements got=$actual_engagements" >&2
  exit 1
fi

jq -n \
  --arg generated_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg api_base "$API_BASE" \
  --arg model_a_id "$MODEL_A_ID" \
  --arg model_b_id "$MODEL_B_ID" \
  --arg model_a_provider_model "$MODEL_A_PROVIDER_MODEL" \
  --arg model_b_provider_model "$MODEL_B_PROVIDER_MODEL" \
  --argjson register_a "$(cat "$tmp_dir/register_a.json")" \
  --argjson register_b "$(cat "$tmp_dir/register_b.json")" \
  --argjson generate_a "$(cat "$tmp_dir/generate_a.json")" \
  --argjson generate_b "$(cat "$tmp_dir/generate_b.json")" \
  --argjson simulation "$(cat "$tmp_dir/simulate.json")" \
  '{
    generated_at: $generated_at,
    api_base: $api_base,
    model_a: { id: $model_a_id, provider_model: $model_a_provider_model },
    model_b: { id: $model_b_id, provider_model: $model_b_provider_model },
    register_a: $register_a,
    register_b: $register_b,
    generate_a: $generate_a,
    generate_b: $generate_b,
    simulation: $simulation
  }' > "$OUT_JSON"

echo "[arena] completed. artifact: $OUT_JSON"
jq '{artifact:$artifact,winner:.simulation.data.simulation.winner_model_id,draw:.simulation.data.simulation.draw,total_engagements:.simulation.data.simulation.total_engagements,duration_ms:.simulation.data.simulation.duration_ms}' \
  --arg artifact "$OUT_JSON" \
  "$OUT_JSON"
