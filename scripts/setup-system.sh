#!/usr/bin/env bash
set -euo pipefail

MODE="check"
if [[ "${1:-}" == "--apply" ]]; then
  MODE="apply"
elif [[ "${1:-}" == "--check" || -z "${1:-}" ]]; then
  MODE="check"
else
  echo "Usage: $0 [--check|--apply]"
  exit 1
fi

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Single-machine kernel tuning is Linux-specific. Current OS: $(uname -s)"
  echo "Use this checklist as reference; no sysctl changes were made."
  exit 0
fi

if [[ "$MODE" == "apply" && "${EUID:-$(id -u)}" -ne 0 ]]; then
  echo "--apply requires root (run with sudo)."
  exit 1
fi

declare -A TARGET_SYSCTL=(
  ["net.core.somaxconn"]="65535"
  ["net.core.netdev_max_backlog"]="250000"
  ["net.core.rmem_max"]="134217728"
  ["net.core.wmem_max"]="134217728"
  ["net.ipv4.udp_rmem_min"]="16384"
  ["net.ipv4.udp_wmem_min"]="16384"
  ["net.ipv4.udp_mem"]="262144 524288 1048576"
  ["net.ipv4.ip_local_port_range"]="10000 65535"
  ["net.ipv4.tcp_fin_timeout"]="15"
  ["net.ipv4.tcp_tw_reuse"]="1"
  ["kernel.numa_balancing"]="0"
  ["vm.max_map_count"]="1048576"
  ["vm.swappiness"]="1"
  ["fs.file-max"]="2097152"
)

print_header() {
  echo "== Massive Game Server: Single Machine Tuning =="
  echo "mode=$MODE host=$(hostname) kernel=$(uname -r)"
}

check_or_apply_sysctl() {
  local key="$1"
  local target="$2"
  local current
  current="$(sysctl -n "$key" 2>/dev/null || echo "<missing>")"
  if [[ "$MODE" == "apply" ]]; then
    sysctl -w "$key=$target" >/dev/null
    echo "[applied] $key=$target (was: $current)"
  else
    if [[ "$current" == "$target" ]]; then
      echo "[ok] $key=$current"
    else
      echo "[todo] $key current='$current' target='$target'"
    fi
  fi
}

check_thp() {
  local thp_file="/sys/kernel/mm/transparent_hugepage/enabled"
  if [[ -f "$thp_file" ]]; then
    local thp
    thp="$(cat "$thp_file")"
    echo "[info] transparent_hugepage: $thp"
    if [[ "$MODE" == "apply" && "$thp" != *"[never]"* ]]; then
      echo never > "$thp_file"
      echo "[applied] transparent_hugepage=never"
    fi
  else
    echo "[skip] transparent hugepage control file not present"
  fi
}

check_hugepages() {
  local hp_file="/proc/sys/vm/nr_hugepages"
  if [[ -f "$hp_file" ]]; then
    local current_hp
    current_hp="$(cat "$hp_file")"
    if [[ "$MODE" == "check" ]]; then
      if [[ "$current_hp" -ge 256 ]]; then
        echo "[ok] vm.nr_hugepages=$current_hp"
      else
        echo "[todo] vm.nr_hugepages=$current_hp (target >= 256 for single-machine high load)"
      fi
    else
      if [[ "$current_hp" -lt 256 ]]; then
        echo 256 > "$hp_file"
        echo "[applied] vm.nr_hugepages=256 (was: $current_hp)"
      else
        echo "[ok] vm.nr_hugepages already $current_hp"
      fi
    fi
  fi
}

check_cpu_governor() {
  local gov_file
  gov_file="$(find /sys/devices/system/cpu -name scaling_governor 2>/dev/null | head -n 1 || true)"
  if [[ -z "$gov_file" ]]; then
    echo "[skip] CPU governor controls unavailable"
    return
  fi
  local current
  current="$(cat "$gov_file")"
  if [[ "$MODE" == "check" ]]; then
    if [[ "$current" == "performance" ]]; then
      echo "[ok] cpu_governor=performance"
    else
      echo "[todo] cpu_governor=$current (target: performance)"
    fi
    return
  fi
  while IFS= read -r file; do
    echo performance > "$file" || true
  done < <(find /sys/devices/system/cpu -name scaling_governor 2>/dev/null)
  echo "[applied] cpu_governor=performance"
}

check_irqbalance() {
  if ! command -v systemctl >/dev/null 2>&1; then
    echo "[skip] systemctl unavailable; cannot verify irqbalance"
    return
  fi
  local status
  status="$(systemctl is-active irqbalance 2>/dev/null || true)"
  if [[ "$status" == "active" ]]; then
    echo "[ok] irqbalance=active"
  elif [[ "$MODE" == "apply" ]]; then
    if systemctl enable --now irqbalance >/dev/null 2>&1; then
      echo "[applied] irqbalance enabled and started"
    else
      echo "[todo] unable to enable irqbalance automatically"
    fi
  else
    echo "[todo] irqbalance status='$status' (target: active)"
  fi
}

check_hugetlb_mount() {
  if grep -qE '\s/dev/hugepages\s' /proc/mounts 2>/dev/null; then
    echo "[ok] hugetlbfs mounted at /dev/hugepages"
    return
  fi
  if [[ "$MODE" == "apply" ]]; then
    mkdir -p /dev/hugepages
    if mount -t hugetlbfs nodev /dev/hugepages >/dev/null 2>&1; then
      echo "[applied] mounted hugetlbfs at /dev/hugepages"
    else
      echo "[todo] failed to mount hugetlbfs at /dev/hugepages"
    fi
  else
    echo "[todo] hugetlbfs not mounted at /dev/hugepages"
  fi
}

check_numa() {
  if ! command -v numactl >/dev/null 2>&1; then
    echo "[skip] numactl not installed"
    return
  fi
  local nodes
  nodes="$(numactl --hardware 2>/dev/null | awk '/available:/ {print $2}' | head -n 1 || true)"
  if [[ -z "$nodes" ]]; then
    echo "[skip] unable to query NUMA topology"
    return
  fi
  if [[ "$nodes" -gt 1 ]]; then
    echo "[info] numa_nodes=$nodes (recommend --cpunodebind/--membind launch pinning)"
  else
    echo "[ok] numa_nodes=$nodes"
  fi
}

check_limits() {
  local nofile_soft
  nofile_soft="$(ulimit -n || true)"
  echo "[info] current shell nofile soft limit: $nofile_soft"
  if [[ "${nofile_soft:-0}" -lt 1048576 ]]; then
    echo "[todo] raise nofile limit (ulimit -n 1048576 and /etc/security/limits.conf)"
  fi
}

print_runtime_hints() {
  cat <<'EOF'
[hints] Runtime launch profile:
  export MGS_SINGLE_MACHINE_OPT=1
  export MGS_CPU_AFFINITY=1
  export RUST_LOG=massive_game_server_core=warn,warp=warn,webrtc=warn

[hints] Optional NUMA pinning (dual-socket):
  numactl --cpunodebind=0 --membind=0 ./target/release/massive_game_server_core

[hints] Persist sysctl values:
  write the same keys to /etc/sysctl.d/99-mgs-single-machine.conf and run sysctl --system
EOF
}

print_header
echo "[info] logical_cpus=$(nproc) mem_kb=$(awk '/MemTotal/ {print $2}' /proc/meminfo)"

for key in "${!TARGET_SYSCTL[@]}"; do
  check_or_apply_sysctl "$key" "${TARGET_SYSCTL[$key]}"
done

check_hugepages
check_thp
check_cpu_governor
check_irqbalance
check_hugetlb_mount
check_numa
check_limits
print_runtime_hints
