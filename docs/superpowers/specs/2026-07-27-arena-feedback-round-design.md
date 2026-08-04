# Design: Mid-Season Feedback Round (Arena Revision)

Date: 2026-07-27 · Status: awaiting review · Approach: A (server-side revision route)

## Goal

Once per 7-day season, each of the 10 weekly OpenRouter models gets exactly one chance to improve its fighting bot, informed by its own mid-season performance stats. Improvement is measured head-to-head within the same season (pre/post strategy-rating delta), because standings continue seamlessly across the revision boundary.

## Why (context established this session)

- The weekly league (`massive-game-arena-weekly.service` → `scripts/arena/weekly_supervisor.mjs` → `scripts/arena/run_top10_season.mjs`) freezes 10 gen-1 bots at week start and never lets models learn: W30 ran 1,680 epochs with static bots; Claude Opus 4.8 won 71% of epochs (96% in W31 so far). A feedback round makes the league measure *learning*, not just first-shot coding.
- Generation is deliberately frozen server-side: `/api/arena/code/generate` ignores request-level `objective`/`prompt_style` ("the uniform season prompt is immutable") and pins `prompt_version` + `prompt_sha256` into every checkpoint's competition contract. There is no channel to feed stats into gen-1 generation — a revision path must be built explicitly.
- 7-day season ≈ 650–750 epochs at 13–16 min each (validated live after the epoch-96 fix). Mid-season ≈ epoch 336 (Wednesday).

## Non-goals (YAGNI)

- No multiple revision rounds, no per-mode prompts, no new telemetry, no changes to the (flat) server Elo board, no season-end evolution round (rejected in favor of mid-season).

## Architecture

### 1. Server: `POST /api/arena/code/revise` (`server/src/operational/code_generation.rs`)

- New admin-gated route (same `requires_admin_auth` bearer as the rest of `/api/arena`).
- Body: `{ model, previous_source, stats_digest, reasoning_mode?, reasoning_effort? }`.
- New frozen revision template `ARENA_UNIFORM_REVISION_PROMPT` + `ARENA_REVISION_PROMPT_VERSION` (e.g. `arena-rust-revision-v1.0.0`), hashed by `revision_prompt_sha256()`. The template contains fixed slots; `previous_source` and `stats_digest` are interpolated as data. The hashed template stays constant, preserving the frozen-contract audit model.
- Reuses the existing OpenRouter SSE transport, provider routing (`sort=throughput`, `require_parameters`), reasoning policy normalization, source validation (`validate_source_impl`), and audit fields (resolved model, provider, response id, usage, cost).
- Response mirrors `GenerateBotCodeResponse` but carries the revision prompt version/sha and echoes `stats_digest_sha256` for audit.
- `/api/arena/code/status` is extended with `revision_prompt_version` + `revision_prompt_sha256` so the runner pins the revision contract from the status endpoint exactly like the generation contract — never trusting values self-reported by a generation response.
- Limits: `previous_source` ≤ existing `max_source_bytes`; `stats_digest` ≤ 8 KB body-enforced.

### 2. Runner: `--revise-only` mode (`scripts/arena/run_top10_season.mjs`)

- Mutually exclusive with `--generate-only`/`--evaluate-only`/`--rehydrate-only`; requires `--ranking-file` + `--season-id` like the other modes.
- For each of the 10 frozen entrants:
  1. Build the stats digest (below) from the season's own artifacts.
  2. Journal the attempt *before* the provider call (same pattern as the migration attempt journal), so a crash mid-round cannot cause a second provider call — one chance means one call.
  3. Call `/api/arena/code/revise` with the entrant's gen-1 source + digest.
  4. Validate the response against the **revision contract**: revision prompt version/sha; all other rules identical to generation (resolved model == frozen entrant, terminal usage, finish_reason=stop, source passes validation).
  5. Compile via `/api/arena/code/compile` (`overwrite: true`) and validate the WASM (bytes, sha, export check server-side).
  6. Atomically swap the fighter: write the revision checkpoint (schema v2 fields plus `revision_of: <gen1 source_sha256>`, `stats_digest_sha256`, `revision_epoch`), then atomic-rename it over `generations/<model_id>.json`. The epoch loop's `trustArchivedArtifact` path picks up revised fighters with no further changes.
- Any failure at any step for a model → gen-1 checkpoint untouched, chance consumed, error sha256 recorded. Per-model isolation: one model's failure never blocks the other nine.
- Successful-but-unswapped responses are archived so crash recovery can complete the swap without a new provider call.

### 3. Stats digest builder (runner-side, pure function)

Bounded (≤ 4 KB), deterministic JSON, built per model from `season.json` (latest epoch's roster entry) + supervisor `state.json` epoch ledger:

- per-mode personal / team / collaboration ratings (arena, ctf, koth, tdm)
- world rating, strategy rating, current rank
- epoch wins so far, rank trajectory over the last 10 epochs
- top-3 opponents' strategy ratings (context, not their code)

`sha256` of the digest is recorded in the revision checkpoint and the supervisor state. No new telemetry is collected — all inputs already exist.

### 4. Supervisor scheduling (`scripts/arena/weekly_supervisor.mjs`)

- Constant `REVISION_EPOCH_INDEX = 336` (≈ Wednesday 12:00 UTC at observed epoch cadence).
- After epoch 336 commits and before epoch 337 starts: run the runner child with `--revise-only` (same frozen ranking file and season id as epochs).
- Record in `state.json`: `revision: { epoch_index, started_at, completed_at, entries: [{ model_id, status: improved|kept_gen1|failed, source_sha256_before, source_sha256_after?, stats_digest_sha256, error_sha256? }] }` — hash-auditable like the frozen roster.
- Runs exactly once per season: if `revision` is present and complete, skip. Epoch loop is sequential, so epochs naturally pause during the round (~10 provider calls + compiles, ≈ 5–10 min).
- Epoch archives after the boundary are implicitly post-revision (`epoch.index > revision.epoch_index`); pre/post improvement deltas are computable from existing epoch standings.

## Error handling

- OpenRouter timeout/error → keep gen-1, consumed, recorded.
- Compile failure → keep gen-1, consumed, `error_sha256` recorded (mirrors `persistCompileFailure` semantics).
- Revision contract drift (unexpected prompt version/sha from server) → abort the whole round before any swap; epoch loop continues with gen-1 bots.
- Supervisor crash mid-round → per-model journal + archived responses allow resume without extra provider calls; already-swapped models are skipped.
- Server restart during the round → child fails, supervisor backoff applies, resume logic above.

## Testing

- **Rust** (`code_generation.rs` tests): revision route requires admin auth; template sha stable; `stats_digest` > 8 KB rejected; source validation enforced on response; revision contract fields present in `/api/arena/code/status`.
- **Runner** (`run_top10_season.test.mjs`): revision contract validation (accept revision sha, reject gen-1 sha for revised checkpoints and vice versa); stats digest builder determinism + ≤ 4 KB bound; per-model swap atomicity; failure keeps gen-1; journal prevents a second provider call after simulated crash.
- **Supervisor** (`weekly_supervisor.test.mjs`): triggers once at epoch 336; skipped when `revision` record complete; epochs resume after; failed round does not wedge the epoch loop.
- **Live**: `e2e/smoke.cjs` (in-game smoke, added this session) green against the restarted server; one manual `--revise-only --dry-run`-style validation pass on W31 artifacts if a dry-run flag is cheap (otherwise covered by unit tests).

## Rollout

1. Implement server route → `cargo test -p massive_game_server_core` → `cargo build --release` (~4.5 min).
2. Restart `massive-game-server.service` (via `run-server-with-turn.js` wrapper) — brief game downtime, TURN creds re-injected.
3. Implement runner + supervisor → `node --test scripts/arena/*.test.mjs` (51 existing + new).
4. Restart `massive-game-arena-weekly.service`. W31 (currently ~epoch 103) reaches the revision point at epoch 336 (~Wednesday) automatically; no backfill, no manual trigger.
5. Watch the first revision round in the journal; verify `state.json` revision record and post-revision epochs in standings.

## Risks / notes

- Revised bots change battle outcomes mid-season; competitive fairness is preserved because all 10 models get the same single chance at the same epoch with the same digest structure. Epoch archives remain append-only and hash-pinned.
- The revision prompt is a second frozen template to maintain; both templates are compiled into the server binary, so template changes require a rebuild — same as today.
- Toolchain drift (observed 2026-07-27 with rustc 1.97.1) does not affect the revision path: revised artifacts are compiled and used directly; no byte-compare against staging-renamed artifacts is involved.
