# Environment Variables Reference

Centralized reference for runtime/deploy environment variables used by `massive_game_server`.

## Core Runtime

- `MGS_HOST`: Bind host for HTTP/WebSocket server.
- `MGS_PORT`: Bind port for HTTP/WebSocket server.
- `MGS_ALLOWED_ORIGINS`: Allowed browser origins for signaling/HTTP.
- `MGS_REQUIRE_AUTH`: Require auth/session token for gameplay joins.
- `MGS_BEHIND_TLS_PROXY`: Enable trusted proxy behavior for TLS termination.
- `MGS_ALLOW_INSECURE_WS_PROXY_PROTO`: Allow downgraded forwarded proto handling.
- `MGS_DEV_MODE`: Enable relaxed/dev behavior.
- `MGS_DEV_TRUST_PRIVATE_PROXIES`: Trust private RFC1918 forwarded proxies in dev.

## Match/Gameplay

- `MGS_TARGET_BOT_COUNT`: Bot population target.
- `MGS_MAX_PLAYERS_PER_MATCH`: Match capacity.
- `MGS_MATCH_TYPE`: Match mode override (`full`, `quick`, `mobile_blitz`, `mobile_standard`).
- `MGS_TICK_RATE`: Simulation tick rate.
- `MGS_MAP_PATH`: Load map JSON from path.
- `MGS_MAP_TEMPLATE`: Procedural map template override.
- `MGS_MAP_SEED`: Procedural map seed.
- `MGS_MAP_TARGET_PLAYERS`: Procedural map target size.
- `MGS_MAP_MAX_WALLS`, `MGS_MAP_MAX_ZONES`, `MGS_MAP_MAX_PICKUPS`: Map safety caps.
- `MGS_DYNAMIC_MODE_TRANSITIONS`: Enable automatic mode transitions.
- `MGS_SPECTATOR_CAP`: Spectator count cap.

## WebRTC / Signaling

- `MGS_DISABLE_STUN`: Disable default STUN bootstrap.
- `MGS_STUN_URLS`: Comma-separated STUN URLs.
- `MGS_TURN_URLS`: Comma-separated TURN/TURNS URLs.
- `MGS_TURN_USERNAME`, `MGS_TURN_CREDENTIAL`: TURN credentials.
- `MGS_TURN_CREDENTIAL_TYPE`: TURN credential mode.
- `MGS_ICE_SERVERS`: Extra ICE server entries (`urls|username|credential`).
- `MGS_WS_KEEPALIVE_INTERVAL_SECS`: WebSocket signaling keepalive interval.
- `MGS_SIGNALING_SDP_CONCURRENCY`: Max concurrent SDP admissions.
- `MGS_JOIN_RATE_LIMIT_PER_SEC`, `MGS_JOIN_RATE_LIMIT_BURST`: Join rate limiting.
- `MGS_INPUT_RATE_LIMIT_PER_SEC`, `MGS_INPUT_RATE_LIMIT_BURST`: Input message rate limiting.
- `MGS_IP_RATE_LIMIT_PER_SEC`, `MGS_IP_RATE_LIMIT_BURST`: Signaling/IP request limiting.
- `MGS_SIGNALING_ICE_RATE_LIMIT_PER_SEC`, `MGS_SIGNALING_ICE_RATE_LIMIT_BURST`: ICE candidate rate limiting.
- `MGS_CHAT_COOLDOWN_MS`: Per-message chat cooldown.
- `MGS_CHAT_BURST_CAPACITY`: Chat burst budget per peer before refill throttling.
- `MGS_CHAT_BURST_WINDOW_MS`: Refill window for the per-peer chat burst budget.

## QUIC

- `MGS_QUIC_PRIMARY`: Prefer QUIC as primary transport.
- `MGS_QUIC_PRIMARY_ONLY`: QUIC-only mode.
- `MGS_QUIC_BIND_ADDR`: QUIC bind address.
- `MGS_QUIC_CERT_PATH`, `MGS_QUIC_KEY_PATH`: TLS cert/key for QUIC.
- `MGS_QUIC_REQUIRE_REAL_CERT`: Forbid self-signed fallback certs.
- `MGS_QUIC_ALLOW_SELF_SIGNED_TESTING`: Allow test self-signed certs.
- `MGS_QUIC_MAX_CONCURRENT_CONNECTIONS`: QUIC connection cap.
- `MGS_QUIC_MAX_BIDI`: QUIC bidirectional stream cap.
- `MGS_QUIC_MAX_STREAM_PAYLOAD_BYTES`: Per-stream payload cap.
- `MGS_QUIC_CONN_RATE_PER_SEC`, `MGS_QUIC_CONN_RATE_BURST`: QUIC connection rate limiter.
- `MGS_QUIC_OUTBOUND_MODE`: QUIC outbound send mode.

## Auth / Sessions / Admin

- `MGS_AUTH_USE_COOKIES`: Use cookie-based auth session transport. The server
  emits an HttpOnly `mgs_session` cookie and clears it on logout. When
  `MGS_BEHIND_TLS_PROXY=true`, the cookie is also marked `Secure`; otherwise
  Secure is omitted so localhost/plain-HTTP dev environments still work.
- `MGS_AUTH_SESSION_TTL_SECONDS`: Session TTL.
- `MGS_AUTH_OTP_TTL_SECONDS`: OTP expiry.
- `MGS_AUTH_RESEND_INTERVAL_SECONDS`: OTP resend interval.
- `MGS_AUTH_MAX_VERIFY_ATTEMPTS`: OTP verify attempt cap.
- `MGS_AUTH_TOKEN_RATE_LIMIT_PER_SEC`, `MGS_AUTH_TOKEN_RATE_LIMIT_BURST`: Auth token endpoint limiter.
- `MGS_ACCOUNT_DELETION_GRACE_PERIOD_HOURS`: GDPR deletion grace window.
- `MGS_GDPR_HASH_SALT`: Salt for GDPR hash anonymization.
- `MGS_AUTH_STORE_PATH`: File-backed auth store path.
- `MGS_REDIS_URL`: Shared Redis URL for auth persistence and other shared-state stores.
- `MGS_REDIS_AUTH_STORE_KEY`: Redis auth store key.
- `MGS_SMS_COMMAND`: External SMS send command.
- `MGS_SMS_DEV_MODE`: SMS dev-mode mock behavior.
- `MGS_ADMIN_BEARER_TOKEN` / `MGS_ADMIN_TOKEN`: Admin API bearer token.
- `MGS_ADMIN_ALLOWED_IPS` / `MGS_ADMIN_IP_ALLOWLIST`: Admin route IP allowlist.

## Arena / Code Generation

- `MGS_ARENA_STORE_PATH`: Arena persistent store path.
- `MGS_ARENA_REDIS_URL`: Optional Redis URL override for shared arena persistence.
- `MGS_REDIS_ARENA_STORE_KEY`: Redis key for the persisted arena store snapshot.
- `MGS_ARENA_WASM_DIR`: Arena wasm storage directory.
- `MGS_ARENA_WASM_MAX_BYTES`: Max uploaded wasm size.
- `MGS_ARENA_BOT_FUEL_PER_TICK`: Wasmtime fuel budget per bot tick.
- `MGS_ARENA_BOT_MAX_TICKS`: Max arena simulation ticks.
- `MGS_ARENA_WORKER_ENABLED`: Enable background arena worker loop.
- `MGS_ARENA_WORKER_INTERVAL_MS`: Arena worker polling interval.
- `MGS_ARENA_WORKER_MAX_TICKS`: Worker execution tick cap.
- `MGS_ARENA_SOURCE_DIR`: Source staging directory for generated bots.
- `MGS_BOT_SOURCE_MAX_BYTES`: Source-size cap for generated code.
- `MGS_CODEGEN_RUSTC_TIMEOUT_SECS`: `rustc` timeout.
- `MGS_CODEGEN_RUSTC_CPU_LIMIT_SECS`: `rustc` CPU limit.
- `MGS_CODEGEN_RUSTC_MEMORY_LIMIT_MB`: `rustc` memory limit.

## Performance / Scaling / Concurrency

- `MGS_SINGLE_MACHINE_OPT`: Enable single-machine tuned join/broadcast behavior.
- `MGS_SINGLE_MACHINE_MODE`: Single-machine mode toggle.
- `MGS_CPU_AFFINITY`: Enable CPU pinning.
- `MGS_NUMA_AWARE`: Enable NUMA-aware scheduling.
- `MGS_NUMA_NODE_MAP`: Explicit NUMA node map.
- `MGS_EVENT_QUEUE_MAX_EVENTS`: Event queue cap.
- `MGS_DIRECT_PACKET_QUEUE_CAP`: Direct packet queue cap.
- `MGS_AOI_UPDATE_DIVISOR`: AOI update cadence divisor.
- `MGS_SPEED_HACK_TOLERANCE`: Movement validation tolerance.
- `MGS_SPATIAL_INDEX_MODE`: Spatial index strategy selection.
- `MGS_SPATIAL_QUADTREE_MIN_ENTITIES`: Quadtree activation threshold.
- `MGS_SPATIAL_QUADTREE_REBUILD_MS`: Quadtree rebuild cadence.
- `MGS_CLUSTER_SHARDS`, `MGS_LOCAL_SHARD_ID`: Sharding configuration.
- `MGS_PLAYER_SHARDS`: Player shard count.
- `MGS_THREADS_GAME`, `MGS_THREADS_PHYSICS`, `MGS_THREADS_AI`, `MGS_THREADS_NETWORK`, `MGS_THREADS_IO`: Thread counts.

## Monitoring / Diagnostics / Logs

- `MGS_METRICS_ENABLED`: Enable Prometheus metrics endpoint.
- `MGS_METRICS_BIND_ADDR` / `MGS_PROMETHEUS_LISTEN`: Metrics bind address.
- `MGS_LOG_FORMAT`: Log output format (`json`, etc).
- `MGS_OTEL_ENABLED`: OpenTelemetry enable.
- `MGS_OTEL_EXPORTER_OTLP_ENDPOINT`: OTLP endpoint.
- `MGS_OTEL_EXPORTER_TIMEOUT_MS`: OTLP export timeout.
- `MGS_ALERT_EVAL_INTERVAL_SECONDS`: Internal alert evaluation cadence.
- `MGS_ALERTMANAGER_URL`, `MGS_ALERTMANAGER_WEBHOOK_URL`: Alert routing endpoints.
- `MGS_ALERT_MAX_FRAME_MS`, `MGS_ALERT_MAX_RSS_BYTES`, `MGS_ALERT_MAX_CONNECTED_PLAYERS`, `MGS_ALERT_MAX_AUTH_FAILURES_PER_MINUTE`: Alert thresholds.
- `MGS_ALERT_COOLDOWN_SECONDS`: Alert suppression cooldown.
- `MGS_ENV`: Environment label used by alerts/telemetry.

## Backup / Replay / Persistence

- `MGS_BACKUP_ENABLED`: Enable periodic backups.
- `MGS_BACKUP_DIR`: Backup output directory.
- `MGS_BACKUP_INTERVAL_SECONDS`: Backup interval.
- `MGS_BACKUP_RETENTION_COUNT`: Backup retention count.
- `MGS_BACKUP_REDIS_URL`: Optional Redis URL override for shared latest-backup metadata.
- `MGS_REDIS_BACKUP_KEY`: Redis key used for the shared latest-backup metadata record.
- `MGS_BACKUP_EXTRA_PATHS`: Additional paths to archive.
- `MGS_LIVE_REPLAY_ENABLED`: Enable live replay capture.
- `MGS_LIVE_REPLAY_CAPACITY`: Replay ring capacity.
- `MGS_LIVE_REPLAY_PLAYER_CAP`: Per-player replay cap.
- `MGS_LIVE_REPLAY_MATCH_PERSIST`: Persist match replay snapshots.
- `MGS_LIVE_REPLAY_MATCH_STORE_DIR`: Replay match storage path.
- `MGS_LIVE_REPLAY_MATCH_REDIS_URL`: Optional Redis URL override for shared persisted match metadata.
- `MGS_REDIS_LIVE_REPLAY_MATCH_KEY`: Redis base key for latest match summary and persisted match metadata.
- `MGS_LIVE_REPLAY_MATCH_RETENTION`: Retention for persisted matches.
- `MGS_LIVE_REPLAY_DISPUTE_PERSIST`: Persist dispute evidence.
- `MGS_LIVE_REPLAY_DISPUTE_STORE_PATH`: Dispute store path.
- `MGS_LIVE_REPLAY_DISPUTE_REDIS_URL`: Optional Redis URL override for shared dispute metadata persistence.
- `MGS_REDIS_LIVE_REPLAY_DISPUTE_KEY`: Redis base key for persisted dispute records and chain head.
- `MGS_LIVE_REPLAY_DISPUTE_SIGNING_KEY`: Signing key for dispute artifacts.

## Feature Flags / Config Loader

- `MGS_FEATURE_FLAGS`: Bootstrap feature flags from env.
- `MGS_FEATURE_FLAG_STORE_PATH`: Feature flag persistence path.
- `MGS_FEATURE_FLAGS_REDIS_URL`: Optional Redis URL override for shared feature flag persistence.
- `MGS_REDIS_FEATURE_FLAGS_KEY`: Redis key used for shared feature flag persistence.
- `MGS_CONFIG_PATH`: YAML config override path.

## Notes

- Some variables are test-only (`MGS_TEST_*`) and intentionally omitted from the operational list above.
- Defaults and exact parsing behavior are implemented in code; see `server/src/` for authoritative values.
- Quick full index command:

```bash
rg -o "MGS_[A-Z0-9_]+" server/src | sort -u
```
