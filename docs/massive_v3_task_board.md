# Massive V3 Task Board (2026-02-15)

## Source Files Ingested
- `artifacts/massive_v3_recommendation/CODE_REVIEW_FEB15_2026.md`
- `artifacts/massive_v3_recommendation/CODE_REVIEW_LATEST.md`
- `artifacts/massive_v3_recommendation/IMPLEMENTATION_GUIDE.md`
- `artifacts/massive_v3_recommendation/LLM_BOT_ARENA_IMPLEMENTATION.md`
- `artifacts/massive_v3_recommendation/MEGA_BATTLE_EXECUTIVE_SUMMARY.md`
- `artifacts/massive_v3_recommendation/MEGA_BATTLE_GAME_DESIGN.md`
- `artifacts/massive_v3_recommendation/README_SINGLE_SERVER.md`
- `artifacts/massive_v3_recommendation/SINGLE_SERVER_PURE_RUST_OPTIMIZATIONS.md`
- `artifacts/massive_v3_recommendation/SYSTEM_ARCHITECTURE.md`
- `artifacts/massive_v3_recommendation/TREBUCHET_IMPROVEMENTS_MASTER.md`
- `artifacts/massive_v3_recommendation/massive_game_client_analysis.md`
- `artifacts/massive_v3_recommendation/massive_game_server_improvements.md`
- `artifacts/massive_v3_recommendation/scalability_analysis.md`
- `artifacts/massive_v3_recommendation/scalability_analysis_part1.md`
- `artifacts/massive_v3_recommendation/scalability_analysis_part2.md`

## V3 Focus Areas
| ID | Task | Status | Notes |
|---|---|---|---|
| V3-01 | Frontend visual polish pass | In Progress | Core combat clarity features are present in `static_client/client.html` (hitmarkers, streak medals, objective urgency, directional damage indicators, radial HUD, announcer cues). |
| V3-02 | Arena mode expansion (CTF/KOTH/TDM) | Done | Added canonical mode parsing/validation and mode-aware sandbox simulation wiring. |
| V3-03 | Bot intelligence uplift in arena runtime | Done (MVP) | Fallback bot policy is now mode-aware and score/health-aware, with improved tactical behavior by mode. |
| V3-04 | 1000-player path prep (single-machine scale) | In Progress | Phase-2 target (`100/120`) is met; this pass removed projectile hot-path allocations and hash-set removal overhead for lower single-machine CPU pressure. |

## Implemented In This Pass
- Added arena match mode model and parsing:
  - `server/src/operational/bot_sandbox.rs`
  - `ArenaMatchMode` supports canonical values and aliases:
    - `arena` (`duel`, `classic`)
    - `ctf` (`capture_the_flag`)
    - `koth` (`king_of_the_hill`)
    - `tdm` (`team_deathmatch`)
- Added mode-aware sandbox execution:
  - `BotSandbox::execute_match(...)`
  - `execute_duel(...)` now delegates to `execute_match(..., ArenaMatchMode::Arena, ...)`
  - Added objective simulation/output for non-arena modes:
    - CTF capture progress/captures
    - KOTH control accumulation
    - TDM elimination tracking
  - Added mode/objective fields in `BotMatchOutcome`:
    - `mode`
    - `objective_label`
    - `objective_a`
    - `objective_b`
- Added mode-aware fallback bot strategy:
  - Fallback policy now uses mode + score differential + health thresholds to choose `Attack`/`Defend`/`Charge`.
- Wired mode execution through arena service:
  - `server/src/operational/arena.rs`
  - Queue APIs now normalize/validate mode (reject unsupported values with `invalid_mode`).
  - `execute_next_match` now parses queued mode and calls `execute_match(...)`.
- Added worker log enrichment:
  - `server/src/main.rs`
  - Worker logs now include mode and objective metrics for executed matches.
- Added projectile hot-path single-machine optimization:
  - `server/src/server/instance.rs`
  - Removed per-projectile temporary ray-sample vectors in collision checks.
  - Reused per-chunk target buffers (`target_ids`, `target_xs`, `target_ys`) instead of reallocating per projectile.
  - Replaced hash-set based projectile removal with sorted linear filtering (`sort_unstable` + `dedup` + single pass).

## Validation
- `cargo test -p massive_game_server_core operational::bot_sandbox::tests:: -- --nocapture`
  - Passed (`4` tests)
- `cargo test -p massive_game_server_core operational::arena::tests:: -- --nocapture`
  - Passed (`5` tests)
- `cargo check -p massive_game_server_core`
  - Passed

## Remaining V3 Work
1. Stabilize repeated `100/120`+ tail runs with tighter p95 variance (especially early wave `open_channel_wait_ms` spikes).
2. Continue writer-side authoritative ECS ownership migration (full mutation ownership boundaries).
3. Reduce remaining full-state serialization copy points for larger launch waves.
4. Optional frontend polish sweep for any remaining v3 effects deltas not already in `static_client/client.html`.
