# Model Profiles + Highlight Media — Design

**Date:** 2026-08-15
**Status:** Approved direction (Project 1 of 2; league ops follows separately)
**Scope:** space.selfware.design public site + arena data pipeline. No game-server logic changes.

## Goal

Give every competing model a public identity: a profile page with behavior
metrics, a mascot, rivalry standings, and auto-generated highlight clips of its
best fights. Every number on the page must trace back to a persisted artifact
(battle JSON / epoch archive / replay file) — full traceability of submissions
and scores.

## Constraints / ground truth (from codebase exploration)

- Arena sim replays (`BotMatchReplay`) are abstract (actions/health/score per
  tick, no positions) — usable for timeline charts, not for video.
- Live exhibition matches (weekly roster WASM bots on the live server) persist
  positional replays: `data/live_replay/matches/replay_*.json.zst`
  (60fps per-player x/y/velocity/health/alive/team + killcam samples).
- Per-battle artifacts `artifacts/arena/seasons/<season>/battles/*.json`
  (~6M files) carry full provenance (source/wasm sha256) and
  `team_*_action_counts {idle,attack,defend,charge,support}` — the behavior
  fingerprint. World events `world/*.json` (~16.5k) give all-10-model FFA
  placements.
- `data/arena_ratings.json` (served at `/api/public/arena/ratings`) holds the
  cumulative per-model roster entry (46 fields).
- The league never mixes models on a team (10v10 same-model squads), so true
  cross-model team chemistry does not exist; we publish world-FFA co-placement
  correlation + collaboration rating as an honestly-labeled proxy.
- `ffmpeg` is not installed; Playwright + Chromium is available under
  `scripts/e2e/`. Static files under `static_client/` are served publicly
  (denylist only blocks dev files).

## Components

### 1. Highlight renderer — `scripts/media/render_highlights.mjs`

- Input: `data/live_replay/matches/*.json.zst` (zstd via system `zstd` CLI;
  verified at implementation, fallback to a bundled decoder).
- Highlight selection: score each match by kill density, kill streaks (from
  per-player alive transitions), and final-stand closeness; keep top N per day
  plus the single best fight per model.
- Rendering: purpose-built minimal top-down renderer (pure JS → PNG frames via
  a small embedded PNG encoder; no new npm deps): ship dots per team color,
  motion trails, kill flashes, mascot + model name banner, score ticker.
  1280×720 @ 30fps, trimmed to the hottest ±10s window (max ~45s clips).
- Encoding: static ffmpeg binary installed **locally** under
  `massive-game-server-deployment/tools/` (no system-wide install), producing
  `.webm` (primary) + `.gif` (compat) + poster `.png`.
- Output: `static_client/media/highlights/YYYY-MM-DD/` + `index.json`
  (clip metadata: models, mascot, kills, score, timestamp).
- Playwright-based rendering of the real game client is the documented
  fallback if the minimal renderer proves too ugly (`capture_showcase_media.js`
  pattern), not the default.

### 2. Profile page generator — `scripts/arena/build_model_pages.mjs`

- Reads `data/arena_ratings.json` (roster aggregates) + a bounded sample of
  `battles/*.json` (latest ~200 battles per model, all modes mixed, drawn from
  an mtime-sorted window of the newest battle files; full scans of 6M files are
  not viable — aggregation is cached incrementally in
  `artifacts/arena/page-cache.json`) + all `world/*.json` (16.5k, fine).
- Emits `static_client/models/index.html` + `static_client/models/<slug>.html`
  using the existing `website/css` visual language (dark arena theme).
- Per-model page sections:
  - Header: mascot (emoji + title + color), model name, provider, current rank
    and season points.
  - Ratings: overall / personal / team / collaboration / world / strategy
    (0–100) as a radar chart (inline SVG, no JS lib).
  - Behavior fingerprint: action-distribution bars + derived traits
    (aggression = attack+charge share, discipline = 1 − invalid/fault rate)
    aggregated across all modes as the primary view, plus a compact per-mode
    breakdown table (rows = arena/ctf/koth/tdm; columns = duels, top-action
    share, aggression index, W-L-D).
  - Rivalries: head-to-head grid vs the other 9 models (all modes blended,
    both orientations from the sampled battles; the per-mode table alongside
    the fingerprint carries the mode-level W-L-D detail).
  - "Plays well alongside": world-FFA co-placement (average finishing gap
    across shared world events) + collaboration rating — labeled as
    co-performance, not mixed-team synergy.
  - Fights: this model's highlight clips (from `media/highlights/index.json`).
  - Provenance: links to season id, artifact sha256s, epochs played — every
    stat traceable to its source artifact.

### 3. Mascot registry — `scripts/arena/mascots.json`

- Curated map: canonical model slug → `{ emoji, title, color }`.
- Fallback generator for new/unknown models (deterministic pick from a palette
  so daily entrants get an identity automatically).
- Consumed by profile pages, the landing-page roster, and video overlays.

### 4. Surfacing + scheduling

- Landing page (`website/js/main.js`): roster rows link to `/models/<slug>.html`;
  new "Highlights" section embedding the latest clips.
- No server route changes needed (static files). Verified: `static_files.rs`
  denylist does not block `models/` or `media/`.
- A systemd **user** timer `massive-game-media-daily.timer` runs both scripts
  daily (and the page generator also runs after the weekly supervisor publishes,
  best-effort via path trigger on `data/arena_ratings.json`).

## Error handling

- Missing/corrupt replay file → skip with warning, continue; never block the
  page generator on media failures.
- A model missing mascot entry → deterministic fallback, logged once.
- Stale data: pages carry `generated_at` and the source `ledger_sha256` so any
  staleness is visible.

## Testing

- Unit tests for highlight selection scoring and for battle-sample aggregation
  (fixture JSONs under `scripts/media/test_fixtures/`).
- Golden-file test: generator output for a frozen mini-roster.
- Live check: after a run, `curl` the public `/models/` and `/media/` URLs for
  200s; one Playwright screenshot per page type for visual sanity.

## Out of scope (Project 2 — league ops)

Daily new-model entrants, stat-feedback improvement rounds, submission limits,
automatic retirement, league tiers and announcements. The traceability
artifacts produced here (page-cache, highlight index) are designed to be
reused by it.
