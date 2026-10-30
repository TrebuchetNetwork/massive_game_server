# Arena TV — instant live broadcast spectator experience

Date: 2026-10-30
Status: approved direction (user: spectator experience → instant live broadcast → template commentary; interviews waived, decisions delegated)

## Goal

A first-time visitor opening `space.selfware.design` sees a **real, live fight within 10 seconds** — the actual exhibition match rendered by the actual game client, with a cinematic auto-camera, real scoreboard/kill feed, and template commentary bound strictly to server events. The broadcast is the top of the funnel into playing (duel/mobile blitz) and into the model league pages.

Non-goals: video streaming infrastructure (HLS/relay), LLM live commentary, changes to game simulation or league evaluation, 3D graphics overhaul.

## Architecture

Landing page (`static_client/index.html`) embeds the real client in a broadcast frame:
`client.html?spectate=1&broadcast=1` in a same-origin iframe. Read-only: no input, no join-as-player. Click-through opens the full client (play or duel). What is rendered is the server's authoritative spectator state — never a replay, never scripted.

New client modules (all under `static_client/client_logic/`, plain JS, CSP-safe, unit-testable in `node --test` like the existing modules):

- `BroadcastDirector.js` — camera target selection + camera motion. Pure scoring/hysteresis core + thin glue over the existing camera API.
- `BroadcastOverlay.js` — DOM overlay: team scores, match clock, kill feed, momentum bar, commentary ticker, LIVE label with honest context (exhibition, fighter count).
- `commentary_templates.js` — event→line templates, variety pools, per-type cooldowns, dedup. Every number/name in a line comes from the event payload or snapshot; no invented stats.

Server-side changes are intentionally minimal:

1. **CSP:** `frame-ancestors 'self'` for `client.html` responses only (currently `none`, which blocks same-origin iframes). All other paths keep `none`.
2. **Broadcast spectator cap:** max 32 concurrent broadcast spectators server-side (env-tunable `MGS_BROADCAST_MAX_SPECTATORS`), counted separately from players. Over-cap joins get a clean rejection the client maps to the highlights fallback. No simulation changes.

## Director behavior

Inputs each tick/frame: visible player states (position, hp, team, name/model label), recent damage events (5s decay), recent kills (10s), exhibition roster top fighter id.

Target score per visible fighter (weights tuned in tests):
`score = 3·recentDamage + 8·recentKills + 2·lowHpDrama(hp<30% and enemy within 600px) + 1.5·closeCombat(nearest enemy <400px) + 0.5·isExhibitionLeader`

Camera rules:
- **Dwell:** keep current target ≥4s; switch only when challenger score > current × 1.35 sustained 1.5s.
- **Motion:** eased lerp (critically damped), max camera speed so fast ship motion never produces snap cuts; kill-zoom ×1.15 for 1.2s on a kill by/on the current target.
- **Quiet periods:** follow exhibition leader with slow drift; never idle on empty space while any fighter is visible.
- Deterministic given the same event stream (seeded tie-break by player id) — testable.

## Overlay & commentary

- Top: team scores + match clock from snapshot; LIVE badge with real context.
- Kill feed: last 5 kill events, 8s TTL, attacker → victim with model display names.
- Momentum: rolling 30s team damage share bar, from real damage events.
- Commentary: streaks (3/5/8 kills), multi-kills, low-HP escapes, lead changes, duel start/end, champion kill. Per-type cooldown 20s, global line dwell 4s, dedup identical lines within 60s. Variety pools (≥3 phrasings per event type) rotate deterministically.

## Fallbacks & lifecycle

- WebRTC failure or over-cap → swap frame to today's rendered highlight webm with label "live full/unavailable — showing highlights". No fake LIVE badge.
- No active exhibition round → same highlights path with "exhibition idle" note.
- Hidden tab: director stops camera writes, overlay throttles to 1Hz; resume cleanly on visible.
- Audio off until user gesture; mobile broadcast canvas at reduced resolution and 30fps.

## Testing

- Unit: director scoring/dwell/hysteresis/zoom on synthetic snapshots+events; commentary template mapping, cooldown, dedup, no-field-no-number invariant; overlay render from fixture state.
- Integration/e2e (Playwright): landing loads → broadcast connects → overlay numbers equal server snapshot values; camera target changes after injected kill; over-cap/failure fallback shows highlights with honest label; CSP allows same-origin frame, still blocks external.
- Load: reuse `scripts/scale` probes with N broadcast spectators to validate the 32 cap headroom (tick p99 within existing budget).
- No changes to league/state/eval paths; existing suites must stay green (`cargo test --lib`, `test:ui`, arena script tests).

## Rollout

Single feature flag `?tv=0` on the landing page to disable the broadcast frame (reverts to static hero). Default on. Ship behind one PR sequence: server CSP+cap → director+overlay modules+tests → landing integration+e2e → load check + docs (MAINTENANCE note for the new env var).
