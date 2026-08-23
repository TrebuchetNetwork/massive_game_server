# Continuous Model League Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A never-frozen league: 10 slots, daily evaluate→retire→feedback→recruit cycle, 3 submissions per model, auto-retirement (rating<35 or WR<25% after 3 days & exhausted submissions), full traceability, published on the site.

**Architecture:** New supervisor `scripts/arena/continuous_league.mjs` reusing the weekly machinery (run_top10_season.mjs as a child for battles/generation, same lock/atomic/validation discipline). Spec: `docs/superpowers/specs/2026-08-16-continuous-model-league-design.md`.

**Repo root:** `/home/habitat/massive-game-server-deployment/massive_game_server`
**Env source for runs:** `~/.config/massive-game-server/arena-weekly.env` (ARENA_API_BASE, admin token, etc.) — same as the weekly service uses.

**Key reusable primitives (verify signatures before use):**
- `run_top10_season.mjs --dry-run` → JSON plan incl. live OpenRouter top-weekly ranking (challenger source).
- `run_top10_season.mjs --ranking-file <f> --season-id <id> --generate-only` → codegen+compile bots for ranking entries (artifact checkpoints in `artifacts/arena/seasons/<id>/generations/<model>.json`).
- `run_top10_season.mjs --ranking-file <f> --season-id <id> --evaluate-only --no-publish` with `ARENA_SEEDS` env → battles; output `artifacts/arena/seasons/<id>/season.json`.
- `scripts/arena/weekly_supervisor.mjs` — reference for: owned lock, atomicWriteJson (fsync version now), backoff, validation discipline. Do NOT import it (it runs its own loop); copy small helpers into `scripts/arena/continuous/`.
- `scripts/arena/mascots.mjs` `mascotFor(modelId)` for entrant identities.

**Rules for implementers:** NO git mutations; NO service restarts except Task 6; another agent works in `server/` + deployment root — stay in `scripts/arena/`, `scripts/media/`, `static_client/`, `docs/`.

---

### Task 1: Skeleton, state schema, validation, IO helpers

**Files:**
- Create: `scripts/arena/continuous/state.mjs` (schema + validateState + atomic read/write), `scripts/arena/continuous/league.mjs` (pure league logic: retirement predicate, cadence gates, challenger filter, rating update), `scripts/arena/continuous_league.mjs` (CLI skeleton: `--once`, `--shadow`, no loop yet)
- Test: `scripts/arena/continuous/test/state.test.mjs`, `league.test.mjs`

- [ ] State schema: `{ schema_version: 1, league_id, day_index, roster: [ { model_id, slug, mascot, joined_at, submissions_used, artifact: {wasm_sha256, source_sha256, prompt_sha256, version, parent_version}, rating, wins, losses, draws, matches, days_in_league, status } ], retired: [], announcements: [], last_feedback_at, created_at, updated_at }`. validateState checks every field + roster ≤ 10 + submissions_used ∈ 0..3.
- [ ] Pure logic + unit tests: `shouldRetire(model, now)` (≥3 days AND submissions_used==3 AND (rating<35 OR winrate<0.25)); `feedbackDue(state, now)` (48h); `eligibleChallengers(ranking, state)` (skip roster + retired<7d); `applyBattleRatings(roster, seasonJson)` (rating recompute 0–100 from W/L/D + points, deterministic); `nextVersion(model)`.
- [ ] `node --test 'scripts/arena/continuous/test/*.test.mjs'` green.

### Task 2: Evaluation + retirement + recruit cycle

**Files:**
- Modify: `scripts/arena/continuous_league.mjs` (implement `--once` cycle steps 1-2-4: evaluate, retire, recruit)
- Create: `scripts/arena/continuous/runner.mjs` (child-process wrapper for run_top10_season.mjs with env from arena-weekly.env, timeouts, sanitized errors — model on weekly_supervisor.mjs `runRunner`)

- [ ] Evaluate: build a ranking-file from current roster (10 entries in the runner's expected shape — copy from a W33 candidate-plan.json), season-id `continuous-<league_id>-day<day_index>`, run evaluate-only, read season.json, `applyBattleRatings`, append `artifacts/arena/continuous/history/<date>.json` snapshot.
- [ ] Retire: `shouldRetire` per model → move to `retired[]` with final stats + `announcements[]` entry `{type:'retirement', model, reason, stats, at}`.
- [ ] Recruit: for each open slot, next eligible from `--dry-run` ranking → `--generate-only` for a ranking-file containing just the new model(s) (investigate whether the runner supports partial rosters; if it requires exactly 10, generate via the server's `/api/arena/code/generate_and_compile` admin endpoint directly — read `weekly_supervisor.mjs` generation flow first and reuse the same API contract) → roster entry with submissions_used=1, version=1, announcement `{type:'entrant'}`.
- [ ] `--shadow` flag: full cycle but skips recruit side-effects and never writes outside `artifacts/arena/continuous/`.
- [ ] Integration test with a stubbed runner.mjs (inject fake runner results): seed roster of 10, fast-forward: day 1 evaluate → ratings move; day 3+ bar model retires; slot refills; announcements ordered.

### Task 3: Feedback/revision rounds

**Files:**
- Modify: `scripts/arena/continuous_league.mjs` (feedback step), `scripts/arena/continuous/brief.mjs` (improvement brief builder)
- Test: brief builder unit tests

- [ ] `brief.mjs`: from a model's state + latest sampled battles (reuse `build_model_pages.mjs` sampling approach, read-only) produce an improvement brief: behavior fingerprint, worst 3 matchups by loss rate, fault/trap counts, per-mode weakness, concise instruction text (< 2KB).
- [ ] Feedback step (every 48h): for each active model with submissions_used < 3: call the same codegen endpoint the weekly generation uses, with the brief + current source as base (read how run_top10_season.mjs requests generation; reuse exact API contract), compile, validate, write new artifact checkpoint; `submissions_used += 1`, `version += 1`, `parent_version` linked; append to `artifacts/arena/continuous/submissions.jsonl` `{model, version, parent, prompt_sha256, brief_sha256, source_sha256, wasm_sha256, compile_attempts, outcome, at}`. Compile failure = submission consumed, keep old artifact, outcome:'compile_failed'.
- [ ] Tests: brief contents sane from fixture stats; submission limit enforced (a 3/3 model is never revised); lineage links correct.

### Task 4: Publishing integration

**Files:**
- Modify: `scripts/arena/build_model_pages.mjs` (league status header, announcements feed, lineage section, hall of fame), `static_client/website/js/main.js` (landing ticker)

- [ ] Read `artifacts/arena/continuous/state.json` + `submissions.jsonl` when present (absent = current behavior unchanged).
- [ ] `/models/index.html`: league header (day, active slots, next feedback countdown), announcements feed (latest 20), Hall of Fame section for `retired[]` with final stats. Model pages: submission lineage v1→v2→v3 with per-version W/L/D delta.
- [ ] Landing: "league ticker" — latest 3 announcements under the roster (fail-silent).
- [ ] Tests: fixture state → golden sections render; absent state → no new sections.

### Task 5: Shadow run + systemd service + cutover runbook

**Files:**
- Create: `~/.config/systemd/user/massive-game-continuous-league.{service,timer}` (or a long-running service — decide: long-running service with internal 24h marks, mirroring the weekly supervisor's reliability model)
- Create: `docs/superpowers/continuous-league-cutover.md` (runbook)

- [ ] Install service in `--shadow` mode; run 24-48h; inspect state.json + announcements for sanity.
- [ ] Runbook: cutover steps (stop weekly service, switch publish path, restart) and rollback.

### Task 6: Final verification + commit

- [ ] All test suites green; journey 15/15; public pages 200 with league sections (once cutover).
- [ ] Controller commits + pushes.

## Self-review notes

- Spec coverage: daily cycle→T2, feedback→T3, traceability→T1+T2+T3 (submissions.jsonl/history/announcements), publishing→T4, shadow/cutover→T5.
- Biggest unknown: whether the runner/generation endpoints support partial-roster generation — Task 2 has the investigation step built in; the weekly revision flow (epoch 336) is the fallback pattern to copy.
