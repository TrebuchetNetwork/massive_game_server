# OpenRouter Model Arena Seasons

`run_top10_season.mjs` builds and rates a reproducible season from OpenRouter's
weekly top ten text models.

The runner freezes the provider ranking, gives every model the same versioned
fighter prompt, rejects synthetic fallback code, compiles the response to WASM,
then evaluates all 45 unordered pairings with fixed seeds in both A/B
orientations and runs every model faction together in shared-world events. A completed season is atomically published to
`data/arena_ratings.json` for the public game UI.

## Integrity rules

- Rust source is limited to 51,200 bytes (50 KiB).
- Every fighter must report `simulated=false` and `compiled=true`.
- Every evaluated runtime must be WASM; explicit fallback, trap, and fuel-error
  counters fail the season even if diagnostic warnings are truncated.
- The top-ten provider IDs and canonical slugs are captured at ranking time.
- The request omits `temperature` so every contestant uses its provider default;
  the `provider_default` policy and completion-token cap are frozen in season provenance.
  The cap is sent to OpenRouter using its broadly advertised outer `max_tokens` field.
- Every request sets OpenRouter `provider.sort=throughput` and
  `provider.require_parameters=true`. The routing policy is frozen in status,
  generation checkpoints, weekly state, and
  published methodology; the resolved provider is archived per fighter.
- The frozen `capability_minimum_v1` reasoning rule is applied equally from each
  model's captured OpenRouter metadata: optional reasoning is disabled, models
  that require reasoning receive their lowest advertised effort, and models
  without reasoning support receive no reasoning parameter. `reasoning.exclude=true`
  prevents hidden reasoning text from entering source. Each resolved per-model
  setting is checked on resume and publication. Prompt version `arena-rust-v3.1.0`
  requires an immediate, compact, complete source file. Reported usage, finish
  reason, resolved provider, and cost remain archived per fighter for auditability.
- Every response uses the frozen `sse_v1` transport: bounded incremental SSE parsing,
  a terminal `finish_reason=stop`, a final usage chunk, and `data: [DONE]` are all
  required. Truncated, malformed, filtered, tool-call, or provider-error streams are
  rejected without echoing source or reasoning text into diagnostics.
- Every pairing runs both sides against the same fixed seed set.
- Collaboration is accepted only from the v2 team sandbox's direct teammate
  support telemetry. The runner will not infer collaboration from repeated
  one-on-one fights.
- Partial runs are checkpointed under `artifacts/arena/seasons/<season-id>` and
  are not published as active ratings.
- Resumed generation, battle, and world checkpoints are bound to the exact
  prompt hash, source hash, compiled WASM byte length and SHA-256, provider
  routing and response-transport policies, collaboration ABI, and
  simulator-rules version. A recompiled artifact is checkpointed before any
  result for its previous bytes can be reused.

## Ratings

- **Personal (40% overall):** 70% solo arena verdict, 30% normalized personal
  score production.
- **Team (35% overall):** 65% team verdict, 35% normalized objective share,
  averaged across CTF, KOTH, and TDM.
- **Collaboration (25% overall):** 75% direct teammate-support share, 25% team
  result conversion.
- **World:** normalized placement points from simultaneous ten-faction events,
  where each model controls a three-fighter squad.
- **Strategy:** 75% duel-overall and 25% world rating. Epoch rank and weekly
  tour points use this combined score.

Ratings are expressed from 0 to 100. Mode values are normalized within each
head-to-head leg before averaging, so KOTH point scale cannot swamp CTF or TDM.

The default four seeds, side swaps, and 10-player teams produce 1,440 duel API
legs, 11,160 deterministic duel engagements, and four all-model world events.
OpenRouter is called only
for fighter generation, not for match simulation.

## Run

The game server—not merely the runner shell—must start with an OpenRouter key.
Use a secret file so the credential is not placed in shell history:

```bash
export OPENROUTER_API_KEY_FILE=/secure/path/openrouter-key
# Restart the game server so it inherits OPENROUTER_API_KEY_FILE.

export ARENA_ADMIN_BEARER_TOKEN_FILE=/secure/path/arena-admin-token
node scripts/arena/run_top10_season.mjs \
  --ranking-file scripts/arena/snapshots/openrouter_top_weekly_2026-07-23.json
```

The runner first calls the protected `/api/arena/code/status` endpoint and stops
before registration if the live process has no provider key, does not expose the
v2 collaboration ABI, uses a different source limit, or cannot prove the prompt
hash.

Useful commands:

```bash
# Inspect the current provider ranking and exact workload without writing files.
node scripts/arena/run_top10_season.mjs --dry-run

# Freeze a ranking only.
node scripts/arena/run_top10_season.mjs --snapshot-only

# Generate/compile all ten fighters, then stop before simulation.
node scripts/arena/run_top10_season.mjs --generate-only

# Resume checkpoints after an interrupted run (the default behavior).
node scripts/arena/run_top10_season.mjs \
  --ranking-file scripts/arena/snapshots/openrouter_top_weekly_2026-07-23.json

# Verify the pure rating logic.
node --test scripts/arena/season_scoring.test.mjs

# Verify weekly UTC boundaries and deterministic rotating seed packs.
node --test scripts/arena/weekly_supervisor.test.mjs
```

Configuration:

- `ARENA_API_BASE` (default `http://127.0.0.1:8080`)
- `ARENA_SEEDS` (default `104729,130363,155921,181081`)
- `ARENA_TEAM_SIZE` (default `10`)
- `ARENA_GENERATION_CONCURRENCY` (default `2`)
- `ARENA_SIMULATION_CONCURRENCY` (default `6`)
- `ARENA_GENERATION_ATTEMPTS` (default `3`)
- `MGS_ARENA_RATINGS_PATH` (default `data/arena_ratings.json`)

For the first monitored provider run, use one request at a time and one attempt
per model (`ARENA_GENERATION_CONCURRENCY=1`, `ARENA_GENERATION_ATTEMPTS=1`).
The weekly supervisor supplies bounded backoff around failed runs while completed
fighter checkpoints prevent successful models from being regenerated.

## Continuous weekly league

`weekly_supervisor.mjs` turns the one-season runner into a continuous UTC ISO
week league. It is deliberately not installed, enabled, or started by this
repository. Run it under the process supervisor used by the deployment only
after both credential files and the arena server are ready:

```bash
export OPENROUTER_API_KEY_FILE=/secure/path/openrouter-key
# Start or restart the game server so it inherits the OpenRouter key file.
export ARENA_ADMIN_BEARER_TOKEN_FILE=/secure/path/arena-admin-token
node scripts/arena/weekly_supervisor.mjs
```

For each `YYYY-Www` week the supervisor:

1. obtains an exact top-weekly-ten candidate through the normal season runner;
2. generates the ten Rust/WASM fighters once, then freezes that ranking only
   after generation succeeds;
3. repeatedly calls the runner with `--evaluate-only --no-publish` and a new
   deterministic seed pack;
4. accepts an epoch only after all 45 pairings, four modes, fixed seeds, A/B
   side swaps, and simultaneous all-model world events pass integrity checks; and
5. atomically publishes cumulative standings. A failed or interrupted epoch
   leaves the previously published standings untouched and resumes from runner
   checkpoints with exponential backoff.

The weekly league scores each completed epoch like a tennis tour. Ranks receive
`1000, 700, 500, 360, 250, 180, 120, 80, 50, 30` points. Standings sort by
cumulative season points, epoch wins, then cumulative strategy rating. Personal,
team, collaboration, duel-overall, world, and combined strategy ratings are
averaged across committed epochs, while W/L/D records and raw personal score,
team-objective, collaboration, world points, world round wins, eliminations,
deaths, and world-collaboration totals accumulate for auditability. A model is
never rewarded for a partial epoch.

The durable contract lives under
`artifacts/arena/weekly-supervisor/<YYYY-Www>/`:

- `ranking.json` is the immutable weekly roster; `candidate-ranking.json`
  exists only before successful generation.
- `state.json` is the atomic, contiguous epoch ledger with seed packs, points,
  archive hashes, the pinned simulator/prompt contract, exact per-model WASM
  bindings, and retry state.
- `epochs/epoch-NNNNNN.json` archives each verified balanced epoch.
- the published ratings artifact includes a `league` object and roster fields
  `season_points`, `epochs_played`, `epoch_wins`, `best_epoch_rank`, and
  `last_epoch_rank`, alongside cumulative world and strategy measurements.

Only one supervisor may own the state directory. SIGINT/SIGTERM stops the active
runner gracefully, state and JSON publication use rename-based atomic writes,
and the loop always waits after success or uses bounded exponential retry
backoff after failure. A midweek change to the roster, compiled fighter bytes,
team size, modes, rating weights, prompt hash, provider routing policy, source
limit, or simulator-rules version rejects the epoch instead of mixing
incompatible results. Credential
values and credential-file paths are redacted from child output and stored errors.

Supervisor configuration:

- `ARENA_WEEKLY_STATE_DIR` (default `artifacts/arena/weekly-supervisor`)
- `ARENA_WEEKLY_SEEDS_PER_EPOCH` (default `4`, range `1..64`, frozen per week)
- `ARENA_WEEKLY_EPOCH_INTERVAL_MS` (default `60000`, minimum `10000`)
- `ARENA_WEEKLY_RETRY_MIN_MS` (default `30000`)
- `ARENA_WEEKLY_RETRY_MAX_MS` (default `900000`)
- `MGS_ARENA_RATINGS_PATH` (default `data/arena_ratings.json`)

`benchmark_10v10.sh` remains as a small two-model diagnostic. It is not a fair
season evaluator and does not publish multidimensional ratings.

## Mixed-team chemistry

`continuous/chemistry.mjs` answers "which models work best together" with real
battles — something same-model squad battles cannot measure. It runs a
deterministic schedule of mixed-squad team battles through the additive
`/api/arena/matches/simulate_mixed_team_battle` endpoint: each match splits the
top-N league roster into two mixed squads (default 5v5 from the top 10), every
fighter driven by its own model's WASM. The schedule guarantees every model
pair shares a squad at least `--k` times (default 2, 6 matches for 10 models);
the response attributes eliminations, deaths, and scores per fighter, and the
runner aggregates per-pair win rates against the win rate expected from the
models' solo ratings (logistic over mean squad Elo, rating → Elo as
`(rating − 50) × 8`). Pairs with fewer than 3 games together are marked
provisional. Results persist to
`artifacts/arena/continuous/chemistry/<YYYY-MM-DD>.json` and are published by
`build_model_pages.mjs` as a per-model "Works best with" section and a league
chemistry pair table. The frozen weekly/league evaluation path is untouched —
mixed mode is additive.

```bash
# Preview the schedule without executing battles.
node scripts/arena/continuous/chemistry.mjs --dry-run

# One chemistry round (K=2) against the local server.
export ARENA_ADMIN_BEARER_TOKEN_FILE=/secure/path/arena-admin-token
node scripts/arena/continuous/chemistry.mjs --track L2 --k 2

# Verify the schedule/aggregation logic.
node --test scripts/arena/continuous/test/chemistry.test.mjs
```
