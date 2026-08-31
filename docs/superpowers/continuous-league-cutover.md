# Continuous Model League — Cutover Runbook

**Date:** 2026-08-24 · **Status:** LIVE since 2026-08-26 (weekly supervisor disabled, rollback path below)
**League supervisor:** `scripts/arena/continuous_league.mjs` (schema v2, multi-track amendment)
**Amendment spec:** `docs/superpowers/specs/2026-08-24-continuous-league-multitrack-amendment.md`

The continuous league runs in **shadow mode** alongside the weekly league,
as **four parallel intervention tracks** with **40 models per track** in a
4-division pyramid (premier/challenger/contender/prospect, 10 each).
Divisions are derived state — recomputed from ratings every cycle, so
promotion/relegation needs no bookkeeping. Battles run within divisions:
4 division-seasons per track per day, 16 per full cycle (same 1440-battle
density per division as a weekly epoch).

| Track | Generation | Compile recovery | Gameplay feedback |
|---|---|---|---|
| L0 Zero-Shot | once | none (1 compile attempt; failed compile = no entry) | never |
| L1 Compile-Fix | once | 3 compile attempts | never |
| L2 Two-Iteration | once | 3 compile attempts | 2 stat-feedback revisions, ≥48h apart |
| L3 Weekly Feedback | once | 3 compile attempts | revision every 7 days, cap 8 |

Feedback briefs are raw measured stats only ("Here are your measured
results." + fingerprint/per-mode/matchups/faults) — no coaching language
(`brief.mjs` enforces this by construction).

Shadow state lives in `artifacts/arena/continuous-shadow/` (never touches
the live `artifacts/arena/continuous/`); it evaluates daily and skips
feedback/recruit (no paid codegen). Cutover flips the continuous league to
live mode and retires the weekly supervisor.

## Current deployment

| Piece | What |
|---|---|
| Weekly (live) | `massive-game-arena-weekly.service` — `weekly_supervisor.mjs` |
| Continuous shadow | `massive-game-continuous-league-shadow.timer` — daily 05:23 local, oneshot `--shadow --once` (runs all four tracks sequentially) |
| Shadow state | `artifacts/arena/continuous-shadow/state.json` (schema v2, `tracks` map) |
| Per-track data | `continuous-shadow/tracks/<TRACK>/{fighters,rankings,history,revision-journal}/` |
| Submission ledger | `continuous-shadow/submissions.jsonl` — shared, append-only, records carry `track` and `stint` (the stint's `joined_at`, so a re-recruited model's lineage never collides with its previous stint) |
| Eval seasons | `artifacts/arena/seasons/continuous-<league_id>-<TRACK>-day<N>-<division>/` |
| Top-40 seeding | `node scripts/arena/continuous/bootstrap_top40.mjs [--force]` — fills the roster to 40/track from the live ranking (one codegen per new model, artifact copied to all 4 tracks; existing entries kept) |
| Env | `~/.config/massive-game-server/arena-weekly.env` (API base, admin token file) |

## Monitoring

- Shadow cycle logs: `journalctl --user -u massive-game-continuous-league-shadow.service`
- Weekly supervisor: `journalctl --user -u massive-game-arena-weekly.service`
- Sanity: every track's `day_index` increments daily; every roster model has
  288 matches per track-day (its division's 1440-battle round-robin);
  history snapshots record each entry's division for that day; per-track
  `announcements[]` carry `track`; `tracks/L2|L3/submissions` activity
  follows each track's cadence (L2: ≥48h, L3: 7d; L0/L1 never revise).

## Pre-cutover checklist (shadow health, 2+ days)

1. `systemctl --user list-timers massive-game-continuous-league-shadow.timer` — runs daily, no failures.
2. `journalctl --user -u massive-game-continuous-league-shadow.service --since "2 days ago"` — each run ends with `cycle complete`; a single track's failure appears as `LX: cycle failed: ...` and must not stop the other tracks; no `manual rebind required` errors.
3. Compare shadow ratings against the weekly league's ratings for the same
   models (`data/arena_ratings.json`) — sanity, not equality (different
   seeds, same fighters). Cross-track: identical v1 artifacts mean day-0
   ratings per model should be broadly similar across tracks.
4. Publishing overlay for the 4-track shape merged and its tests green
   (`node --test scripts/arena/*.test.mjs 'scripts/arena/continuous/test/*.test.mjs'`).

## Cutover steps

1. **Freeze the weekly league:**
   `systemctl --user stop massive-game-arena-weekly.service && systemctl --user disable massive-game-arena-weekly.service`
   (Its state under `artifacts/arena/weekly-supervisor/` is untouched — rollback path.)
2. **Seed the live league from the shadow league** (keeps shadow ratings/history):
   `cp -a artifacts/arena/continuous-shadow artifacts/arena/continuous`
   (or re-run `node scripts/arena/continuous/bootstrap_from_weekly.mjs` against a
   fresh live dir if a clean restart from rating 50 is preferred — decide before flip).
3. **Create + enable the live units** — copy the shadow units to
   `massive-game-continuous-league.{service,timer}`, dropping `--shadow` from
   `ExecStart` (live state dir is `artifacts/arena/continuous/`), then
   `systemctl --user daemon-reload && systemctl --user enable --now massive-game-continuous-league.timer`.
   Use `--track <ID>` for single-track operator runs when debugging.
4. **First live cycle:** runs at the next 05:23 tick (or start the oneshot
   service manually once). Open slots recruit from the live OpenRouter
   ranking **per track** (paid codegen — up to `4 × (10 - roster)`
   generations if seeded partial; L0 entries get a single compile attempt,
   L1+ get three). L2/L3 feedback rounds follow their cadences from
   `last_feedback_at`. Verify `artifacts/arena/continuous/state.json` and
   the shared `submissions.jsonl` (`track` field on every record).
5. **Publishing:** once the 4-track overlay is live, regenerate model pages
   (`node scripts/arena/build_model_pages.mjs`) and verify `/models/` shows
   the per-track standings (4 tables), the experiment matrix (model ×
   track), the announcements feed with track ids, and the Hall of Fame per
   track. Point the public ratings at the continuous league output per the
   publishing wiring.
6. **Decommission shadow units** once the live league is confirmed healthy:
   `systemctl --user disable --now massive-game-continuous-league-shadow.timer`.

## Rollback

`systemctl --user disable --now massive-game-continuous-league.timer &&
systemctl --user enable --now massive-game-arena-weekly.service`

The weekly supervisor resumes from its frozen state exactly where it
stopped; the continuous league's state dirs are left intact for inspection.

## Failure notes

- `server contract changed, manual rebind required` in a cycle log: the
  server's generation/revision prompt changed; fighters generated under the
  old prompt need re-generation (a maintenance task, not an automatic step).
  Scoped to the affected track only — other tracks continue.
- Recruit/feedback skip logs (`recruit: skipped, live ranking unavailable`)
  are transient OpenRouter failures; the cycle still persists evaluate results.
- A schema-v1 `state.json` is discarded on load (clean-slate migration);
  per-track fighters/history under `tracks/` are unaffected.
