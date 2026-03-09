# Massive Game Server Production Super Plan

## Purpose
This document is the authoritative roadmap for production readiness work on the current codebase. It replaces informal report-sprawl as the source of truth for engineering sequencing.

## Current Truth
### Confirmed and actionable
- Kubernetes Stage A needed honest single-replica gameplay exposure instead of the old 2-replica TCP-only shape.
- WebSocket connection capping needed reservation-based enforcement instead of `peers.len()` admission.
- The landing page needed to stop depending on remote CDNs so it could live under the same strict CSP posture as the rest of the app.
- Tick duration needed exact nanosecond-derived timing instead of millisecond truncation.

### Confirmed stale findings
- SMS command injection: already fixed in `main`.
- ObjectPool race condition: already fixed in `main`.
- Delta bitmask truncation: already fixed in `main`.
- Several previously reported missing gameplay feedback systems were already implemented before this roadmap.

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
