# Arena Benchmark E2E

Runs a full API-level pipeline:
- register two arena models
- generate + compile Rust WASM strategies via OpenRouter-backed `/api/arena/code/generate_and_compile`
- execute `/api/arena/matches/simulate_team_battle` as `10v10` (configurable)
- validate engagement counts and save artifact JSON

## Requirements
- Server running with code-generation routes enabled.
- `OPENROUTER_API_KEY` exported in the environment of the server process for real-provider generation.
- `curl` and `jq` installed.

## Run
```bash
chmod +x scripts/arena/benchmark_10v10.sh
OPENROUTER_API_KEY=... \
ARENA_API_BASE=http://127.0.0.1:18082 \
scripts/arena/benchmark_10v10.sh
```

Artifact output:
- `artifacts/arena/arena_10v10_<timestamp>.json`

## Useful Options
- `ARENA_TEAM_SIZE` (default `10`)
- `ARENA_ROUNDS` (default `3`)
- `ARENA_MODE` (default `tdm`, supports `arena|ctf|koth|tdm`)
- `ARENA_REQUIRE_REAL_PROVIDER` (default `1`; set `0` to allow local-template fallback)
- `ARENA_MODEL_A_PROVIDER_MODEL` / `ARENA_MODEL_B_PROVIDER_MODEL`
