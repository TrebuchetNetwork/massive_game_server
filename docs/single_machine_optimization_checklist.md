# Single-Machine Optimization Checklist

This checklist is for high-density single-host runs (10v10 up to large bot/bench mixes).

## Runtime Profile

Use the single-machine launch profile:

```bash
MGS_SINGLE_MACHINE_OPT=1 \
MGS_CPU_AFFINITY=1 \
MGS_TARGET_BOT_COUNT=0 \
RUST_LOG=massive_game_server_core=warn,warp=warn,webrtc=warn \
./target/release/massive_game_server_core
```

## Implemented Phase-2 Items

- Lock-free snapshot + SoA read layout:
  - `server/src/concurrent/atomic_snapshot.rs`
  - published once per broadcast and consumed by initial/delta serialization.
- SIMD spatial/AOI path:
  - `server/src/core/simd.rs`
  - used by `server/src/concurrent/spatial_index.rs` for nearby players/projectiles.
- Zero-copy-friendly serialization + packet batching:
  - shared chat serialization once per tick in `prepare_shared_broadcast_data`.
  - per-client chat dispatch clones `Bytes` (no re-serialization) with bounded batched send.
- SIMD physics/collision path:
  - projectile ray sample collision checks against players use SIMD first-hit queries.

## Kernel + OS Tuning

Run dry-check:

```bash
./scripts/setup-system.sh --check
```

Apply on Linux root shell:

```bash
sudo ./scripts/setup-system.sh --apply
```

Checklist:

- `net.core.somaxconn >= 65535`
- `net.core.netdev_max_backlog >= 250000`
- `net.core.rmem_max >= 134217728`
- `net.core.wmem_max >= 134217728`
- `net.ipv4.ip_local_port_range = 10000 65535`
- `net.ipv4.tcp_fin_timeout <= 15`
- `net.ipv4.tcp_tw_reuse = 1`
- `kernel.numa_balancing = 0`
- `vm.nr_hugepages >= 256`
- `transparent_hugepage=never`
- `hugetlbfs` mounted at `/dev/hugepages`
- CPU governor set to `performance`
- `irqbalance` active
- `ulimit -n >= 1048576`

## CPU Affinity + NUMA

- Enable `MGS_CPU_AFFINITY=1` for thread-pool pinning.
- On multi-socket hosts, use `numactl`:

```bash
numactl --cpunodebind=0 --membind=0 ./target/release/massive_game_server_core
```

## Monitoring Checklist

Record process-level metrics (CPU, RSS/swap, threads, FDs, context switches, IO bytes, page faults):

```bash
./scripts/monitor.sh <pid>
```

Defaults:

- interval: `MGS_MONITOR_INTERVAL_SEC=1`
- duration: `MGS_MONITOR_DURATION_SEC=120`
- output: `artifacts/monitoring/single_machine_monitor_<pid>_<timestamp>.csv`

Always capture these artifacts for each benchmark run:

- multi-client JSON result (`artifacts/scale/*.json`)
- server log for the run (`artifacts/scale/*.log`)
- monitor CSV (`artifacts/monitoring/*.csv`)
