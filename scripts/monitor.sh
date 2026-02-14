#!/usr/bin/env bash
set -euo pipefail

INTERVAL_SEC="${MGS_MONITOR_INTERVAL_SEC:-1}"
DURATION_SEC="${MGS_MONITOR_DURATION_SEC:-120}"
TARGET_PID="${1:-}"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "This monitor script currently supports Linux /proc metrics only."
  exit 0
fi

if [[ -z "$TARGET_PID" ]]; then
  TARGET_PID="$(pgrep -f massive_game_server_core | head -n 1 || true)"
fi

if [[ -z "$TARGET_PID" ]]; then
  echo "No server PID detected. Pass PID explicitly: $0 <pid>"
  exit 1
fi

if ! kill -0 "$TARGET_PID" 2>/dev/null; then
  echo "PID $TARGET_PID is not running."
  exit 1
fi

OUT_DIR="artifacts/monitoring"
mkdir -p "$OUT_DIR"
STAMP="$(date +%Y%m%d_%H%M%S)"
OUT_FILE="$OUT_DIR/single_machine_monitor_${TARGET_PID}_${STAMP}.csv"

echo "timestamp,pid,cpu_pct,rss_mb,threads,open_fds,voluntary_ctxt_switches,nonvoluntary_ctxt_switches" > "$OUT_FILE"

SAMPLES=$(( DURATION_SEC / INTERVAL_SEC ))
if [[ "$SAMPLES" -lt 1 ]]; then
  SAMPLES=1
fi

echo "Monitoring PID=$TARGET_PID interval=${INTERVAL_SEC}s duration=${DURATION_SEC}s -> $OUT_FILE"

for ((i=0; i<SAMPLES; i++)); do
  if ! kill -0 "$TARGET_PID" 2>/dev/null; then
    echo "Process exited during monitor run."
    break
  fi

  TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  PS_LINE="$(ps -p "$TARGET_PID" -o %cpu=,rss=,nlwp= | awk '{$1=$1;print}')"
  CPU_PCT="$(echo "$PS_LINE" | awk '{print $1}')"
  RSS_KB="$(echo "$PS_LINE" | awk '{print $2}')"
  THREADS="$(echo "$PS_LINE" | awk '{print $3}')"
  RSS_MB="$(awk -v kb="$RSS_KB" 'BEGIN { printf "%.2f", kb / 1024.0 }')"

  FD_COUNT="$(ls "/proc/$TARGET_PID/fd" 2>/dev/null | wc -l | tr -d ' ')"
  VOL_CTX="$(awk '/voluntary_ctxt_switches/ {print $2}' "/proc/$TARGET_PID/status" 2>/dev/null || echo 0)"
  NONVOL_CTX="$(awk '/nonvoluntary_ctxt_switches/ {print $2}' "/proc/$TARGET_PID/status" 2>/dev/null || echo 0)"

  echo "$TS,$TARGET_PID,$CPU_PCT,$RSS_MB,$THREADS,$FD_COUNT,$VOL_CTX,$NONVOL_CTX" >> "$OUT_FILE"
  sleep "$INTERVAL_SEC"
done

echo "Monitoring complete: $OUT_FILE"
