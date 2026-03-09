# Massive Game Server Production Super Plan

## Purpose
This document is the authoritative roadmap for production readiness work on the current codebase. It replaces informal report-sprawl as the source of truth for engineering sequencing.

## Current Truth
### Confirmed and actionable
- Kubernetes Stage B is still incomplete: multi-replica gameplay remains deferred until shared artifact storage and the remaining persistence surfaces are externalized.
- Backup artifacts and replay payload files are still file-system backed; only metadata/state slices have been externalized so far.
- Client lifecycle cleanup is still ongoing, but the highest-risk reconnect/reset leaks have already been reduced with targeted tests.

### Confirmed stale findings
- SMS command injection: already fixed in `main`.
- ObjectPool race condition: already fixed in `main`.
- Delta bitmask truncation: already fixed in `main`.
- Several previously reported missing gameplay feedback systems were already implemented before this roadmap.

### Already implemented in `main`
- Exact nanosecond-derived tick timing with regression coverage.
- Self-hosted landing page assets under strict page-specific CSP.
- Reservation-based WebSocket connection capping.
- Kubernetes Stage A single-replica UDP gameplay manifests plus kind browser smoke coverage.
- Shared Redis persistence for feature flags, arena state, live replay disputes, and live replay match metadata.
- Release-edge alert delivery verification plus severity routing.
- QUIC framed-write coalescing and additional FlatBuffer builder reuse.

### Intentionally deferred
- Full multi-replica Kubernetes gameplay is deferred until shared persistence/state externalization lands.
- Large-file extraction refactors are deferred until the correctness and deployment blockers are green.
- Deep performance refactors remain benchmark-driven work after Phase 1/2 stability is established.

## Phase 0
- Maintain this file as the current source of truth.
- Keep third-party or agent-generated reports out of the execution path unless they are reconciled against the live repo.
- Consolidate reusable integration-test helpers in `server/tests/common/helpers.rs`.

## Phase 1
### Exact tick timing
- Use exact nanosecond-derived tick duration.
- Expose authoritative tick seconds as `f32`/`f64` constants.
- Keep regression tests proving 60 ticks represent one second.

### Landing page and CSP
- Keep `/index.html` fully self-hosted.
- Keep static HTML CSP explicit per page.
- Prevent regressions with browser-backed edge tests.

### WebSocket cap enforcement
- Reserve capacity before upgrade completion.
- Reject excess upgrades deterministically under concurrency.
- Cover with concurrent integration tests.

### Kubernetes Stage A
- Single replica by default.
- Explicit UDP exposure for gameplay.
- ConfigMap/Secret driven runtime configuration.
- Browser smoke should validate actual data-channel gameplay, not only page load.

## Phase 2
- Remove crash-on-bad-input patterns from network boundaries.
- Improve release-path observability and alert metadata.
- Continue client lifecycle cleanup and structured logging.
- Add explicit protocol version negotiation and rejection.

## Phase 3
- Optimize AI hot loops.
- Reduce projectile/wall collision cost.
- Pool FlatBuffer builders and scratch buffers.
- Coalesce QUIC writes.

## Phase 4
- Keep PR gates deterministic and small.
- Keep release-edge as the authoritative browser gate.
- Extend kind gating in staged steps, not by pretending Stage B is already valid.
- Push heavy cross-browser, soak, and hybrid stress checks to scheduled workflows.
