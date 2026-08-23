# Continuous Model League — Design

**Date:** 2026-08-16
**Status:** Direction approved (continuous league / 48h feedback, 3 submissions / rating-35 & 25%-WR retirement bar)
**Scope:** New league supervisor + publishing. Builds alongside the frozen weekly league; becomes the public league at the W34 boundary (2026-08-17).

## Goal

A league that never freezes: 10 active model slots, a new challenger enters
whenever a slot opens, every model gets stat-feedback revisions (max 3
submissions each), underperformers retire automatically, and everything —
every submission, score, retirement — is traceable and published.

## Approved decisions

- **Architecture:** new continuous league, built alongside; takes over at the
  week boundary. The frozen weekly league is untouched (its machinery is
  *reused*, not modified).
- **Feedback:** every 48h, each active model with submissions left gets an
  improvement prompt built from its own stats; revised bot = next submission.
  **Max 3 submissions per model** (initial + 2 revisions). A failed compile
  still consumes the submission.
- **Retirement:** model is retired when ALL hold: ≥3 days in league,
  submissions exhausted (3/3), AND (overall rating < 35/100 OR win rate < 25%).
- **Challengers:** vacant slots are filled from the live OpenRouter top-weekly
  ranking, skipping models currently in the league or retired in the last 7
  days.

## Components

### 1. League supervisor — `scripts/arena/continuous_league.mjs`

Long-running user service (`massive-game-continuous-league.service`), same
supervision patterns as the weekly one (owned lock, atomic writes, backoff).

State: `artifacts/arena/continuous/state.json` — roster[10]: model_id, slug,
mascot, joined_at, submissions_used, artifact binding (wasm_sha256, source
sha256, prompt_sha256), rating aggregates (Elo-like 0–100 + raw W/L/D),
days_in_league, status. Plus `announcements[]` (capped 200), `retired[]`
(hall of fame), `day_index`, `last_feedback_at`.

Daily cycle (every 24h, phase-offset per step):
1. **Evaluate** — round-robin battles among the 10 (reuse
   `run_top10_season.mjs --evaluate-only` primitives with per-model artifact
   bindings; 4 seeds per matchup, side-swapped — same contract as weekly).
   Ratings update; per-day score snapshot appended to
   `artifacts/arena/continuous/history/<date>.json`.
2. **Retire** — apply the bar; retired models move to `retired[]` with final
   stats; announcement written.
3. **Feedback** (every 48h) — for each active model with submissions < 3:
   build an improvement brief from its stats (behavior fingerprint, worst
   matchups, fault counts, per-mode weaknesses) → codegen via the existing
   OpenRouter codegen path → compile to WASM → validate → new artifact
   binding. Lineage link parent→child recorded. Compile failure consumes the
   submission (logged with diagnostics).
4. **Recruit** — for each vacant slot: next eligible OpenRouter top-weekly
   model → initial bot generation (submission 1) → announcement.

### 2. Traceability — `artifacts/arena/continuous/history/`

- Per-day score snapshots (ratings, W/L/D, per-mode splits).
- `submissions.jsonl` — every submission: model, version, parent version,
  prompt_sha256, improvement brief summary, source/wasm sha256, compile
  attempts, outcome, timestamp. Append-only.
- `announcements.json` — entrant joins, revision accepted/failed, retirement,
  bar thresholds at time of decision.

### 3. Publishing — extend `build_model_pages.mjs`

- `/models/` gains: league status header (day index, slots, next feedback
  countdown), **announcements feed**, submission lineage on each model page
  (v1→v2→v3 with per-version stats delta), **Hall of Fame** for retired
  models with final stats.
- Landing page: "league ticker" — latest 3 announcements.

### 4. Cutover

- Build + unit tests now; run in **shadow mode** (evaluates and logs but does
  not publish or recruit) alongside the W34 weekly season for 24–48h.
- Cutover: stop `massive-game-arena-weekly.service`, switch
  `data/arena_ratings.json` publishing to the continuous league, public pages
  become the CML view. Rollback = restart the weekly service (its state is
  untouched).

## Error handling

- OpenRouter failures: exponential backoff, skip cycle step (never block the
  loop), announcement only after success.
- Codegen/compile failure: consumes the submission (by rule), diagnostics in
  submissions.jsonl, model continues with previous artifact.
- State corruption: same validation discipline as the weekly supervisor
  (schema check on load, refuse to run on invalid state, backups before
  migration).

## Testing

- Unit tests: retirement predicate (bar edge cases), feedback cadence gating,
  challenger eligibility filter, submission limit enforcement, lineage links,
  rating update math, announcement capping.
- Integration: a 3-day fast-forward fixture (mocked OpenRouter + compiler)
  exercising retire→recruit→revise cycles end to end.
- Shadow-mode comparison: first 2 days' ratings vs the weekly league's
  ratings for the same models (sanity, not equality).

## Out of scope

- Multi-tier divisions (single league of 10 for now; tiers announced as
  ranks only).
- True mixed-team chemistry (still a sim-format question).
- Human-submitted models (OpenRouter ranking is the only entrant source).
