# Continuous Model League — Multi-Track Amendment

**Date:** 2026-08-24
**Amends:** `2026-08-16-continuous-model-league-design.md`
**Status:** Approved (same models in all tracks; raw-stats-only feedback)

## What changes

The single continuous league becomes **four parallel intervention tracks**.
Every roster model fields a bot in EVERY track; we observe which models win
each track and how much feedback helps each model.

### Tracks

| Track | Generation | Compile recovery | Gameplay feedback |
|---|---|---|---|
| **L0 Zero-Shot** | standard prompt, once | none — a failed compile means no L0 entry | never |
| **L1 Compile-Fix** | standard prompt, once | up to 3 compile attempts with raw error output | never |
| **L2 Two-Iteration** | standard prompt | up to 3 compile attempts | 2 stat-feedback revisions, ≥48h apart |
| **L3 Weekly Feedback** | standard prompt | up to 3 compile attempts | stat-feedback revision every 7 days, ongoing (cap 8 revisions/season) |

### Neutrality rule (hard requirement)

Feedback briefs contain ONLY raw measured stats: action distribution, W/L/D
per mode and per matchup, fault counts, placement history. **No imperative or
coaching language** — no "improve X", no "you lose because", no suggested
strategies. The model alone decides what to change. This replaces the current
`brief.mjs` instruction paragraph with a pure stats document plus one neutral
framing line: "Here are your measured results." (No directive attached.)

### Retirement per track

The approved bar applies per track: ≥3 days in track AND submissions
exhausted for that track AND (rating < 35 OR WR < 25%). A retired model's
slot reopens in that track; other tracks are unaffected. L0/L1 exhaust
submissions immediately (1/1), so their bar is effectively rating-based
after day 3.

### Publishing

- League page gets per-track standings (4 tables) plus an **experiment
  matrix**: model × track, rating per cell, so cross-track improvement
  per model is visible at a glance.
- Announcements carry the track id.
- Model pages show lineage per track.

## Cost note (accepted)

4 tracks × 10 models: ~40 initial generations, 4× daily battles, L2/L3
revision rounds. OpenRouter tokens and server CPU scale accordingly.

## Implementation notes

- One supervisor, parameterized by track configs — NOT four copies. State
  schema v2: top-level `tracks: { L0: {...}, L1: {...}, L2: {...}, L3: {...} }`,
  each with its own roster/retired/announcements/day_index; shared
  `submissions.jsonl` gains a `track` field.
- The existing single-track shadow state is discarded (shadow starts fresh
  as 4 tracks, bootstrapped from the weekly roster per track — L0/L1/L2/L3
  all begin from the SAME compiled v1 artifacts, then diverge by policy).
- Cutover runbook updated accordingly.
