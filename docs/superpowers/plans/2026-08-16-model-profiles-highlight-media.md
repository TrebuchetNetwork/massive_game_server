# Model Profiles + Highlight Media Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Public per-model pages (mascot, ratings, behavior fingerprint, rivalries, clips) and auto-generated fight highlight videos on space.selfware.design.

**Architecture:** Two offline generators (Node, no new prod deps) reading persisted arena artifacts + live-replay match files, emitting static HTML + WebM/GIF into `static_client/` (served publicly by warp). Spec: `docs/superpowers/specs/2026-08-15-model-profiles-highlight-media-design.md`.

**Tech Stack:** Node 22 (built-ins only: node:zlib for PNG encoding), system `zstd` CLI (present at /usr/bin/zstd), ffmpeg via npm `imageio-ffmpeg` (vendored static binary, isolated install in scripts/media/).

**Repo root:** `/home/habitat/massive-game-server-deployment/massive_game_server` (game server repo). Launcher: `/home/habitat/massive-game-server-deployment/run-server-with-turn.js`.

**Global rules:** no git mutations by implementers (controller commits); never restart anything except where a task says so; verification gates per task must pass.

---

### Task 0: Enable live-replay persistence

**Files:**
- Modify: `/home/habitat/massive-game-server-deployment/run-server-with-turn.js` (env block ~line 93)

- [ ] Add `env.MGS_LIVE_REPLAY_ENABLED = 'true';` next to the other `MGS_` assignments. (Server default is false; `persist_match_replay_snapshot` writes `data/live_replay/matches/replay_*.json.zst` on match end.)
- [ ] Restart: `systemctl --user restart massive-game-server.service`; verify `curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8080/healthz` = 200 and public healthz 200.
- [ ] Verify persistence: exhibition matches cycle every few minutes (MobileBlitz = 180s matches). Poll `ls data/live_replay/matches/ | head` every 60s for up to 10 min until at least one `replay_*.json.zst` appears. Sanity-check content: `zstd -dc <file> | head -c 400` is JSON with `frames` array containing per-player `x`,`y`,`health`,`alive`,`team_id`.
- [ ] If nothing appears after 10 min, read `server/src/server/instance/match_summary.rs:110` + `replay.rs` to find what triggers persistence and report NEEDS_CONTEXT with findings.

### Task 1: Media tooling bootstrap

**Files:**
- Create: `scripts/media/package.json` (+ `npm install imageio-ffmpeg`)

- [ ] `mkdir -p scripts/media && cd scripts/media && npm init -y && npm install imageio-ffmpeg` — vendored static ffmpeg binary, isolated to this dir. Verify: `node -e "console.log(require('imageio-ffmpeg').path)"` then `<that path> -version | head -1`.
- [ ] Add `scripts/media/lib/` dir convention for shared code used by later tasks.

### Task 2: Highlight renderer

**Files:**
- Create: `scripts/media/lib/replay.mjs` (zst decode + frame model), `scripts/media/lib/select.mjs` (highlight scoring), `scripts/media/lib/raster.mjs` (pure-JS PNG frame writer), `scripts/media/render_highlights.mjs` (CLI)
- Test: `scripts/media/test/select.test.mjs`, `scripts/media/test/raster.test.mjs` (node:test)

- [ ] `replay.mjs`: `loadReplay(path)` → spawn `zstd -dc`, parse JSON, return `{ frames, players, durationMs }`. Frames are ~60fps; downsample to 15fps for video.
- [ ] `select.mjs`: score a replay for "watchability": kill events (alive→dead transitions) per minute, kill clusters (≥3 kills in 5s window), lead swaps in team scores if present, final-stand closeness. Return top-N windows `{ startMs, endMs, reason, score }` (10–45s each). Unit tests with synthetic frame arrays.
- [ ] `raster.mjs`: render one frame to a 1280×720 RGB buffer → PNG via `node:zlib` deflate (hand-rolled PNG chunks: IHDR/IDAT/IEND, CRC32). Draw: dark themed background (void palette: #05070f base), walls skipped (not in replay data), players as team-colored chevrons with rotation from velocity, motion trails (last 8 positions fading), kill flash ring on death events, HUD strip with model names + mascots + match clock. Unit test: PNG magic bytes + decodable size.
- [ ] `render_highlights.mjs`: scan `data/live_replay/matches/*.json.zst`, select top clips, render frames, encode with imageio-ffmpeg: `webm` (vp9/vp8, 15fps) + `gif` (320px wide) + poster png. Output `static_client/media/highlights/YYYY-MM-DD/<match>-<n>.{webm,gif,png}` + update `static_client/media/highlights/index.json` (array: file, models, reason, score, timestamp). Keep last 14 days; delete older dirs.
- [ ] Verify: run against real replay files from Task 0; read back one GIF/PNG visually (the controller will view it too).

### Task 3: Mascot registry

**Files:**
- Create: `scripts/arena/mascots.json`, `scripts/media/lib/mascots.mjs`

- [ ] `mascots.json`: curated entries for the current W33 roster (read `data/arena_ratings.json` roster model_ids) → `{ emoji, title, color }` (e.g. deepseek-v4-pro → 🐋 "Abyss", glm-5.2 → 🦉 "Sage"...). Include all 10 current models + 5 generic fallbacks.
- [ ] `mascots.mjs`: `mascotFor(modelId)` — exact/canonical-slug match, else deterministic fallback (hash of modelId into fallback list). Unit test: known model hits curated entry; unknown model gets stable fallback.

### Task 4: Profile page generator

**Files:**
- Create: `scripts/arena/build_model_pages.mjs`, `scripts/arena/model_page.css` (copied into static_client/models/)
- Test: `scripts/arena/test/build_model_pages.test.mjs`

- [ ] Inputs: `data/arena_ratings.json` (roster aggregates), bounded sample of `artifacts/arena/seasons/<season>/battles/*.json` (latest 200 per model — scan by mtime, cache aggregates in `artifacts/arena/page-cache.json`), `artifacts/arena/seasons/<season>/world/*.json` (co-placement), mascots from Task 3.
- [ ] Output `static_client/models/index.html` + `static_client/models/<slug>.html`: dark arena theme matching the landing page (read `static_client/website/css/styles.css` first for palette/fonts); sections per spec: header (mascot/name/rank/points), ratings radar (inline SVG), behavior fingerprint bars (action distribution from sampled battles), rivalry grid W/L vs other 9, "plays well alongside" (world co-placement + collaboration rating, labeled as proxy), fights (clips from media/highlights/index.json for this model), provenance footer (season id, generated_at, ledger sha256 prefix).
- [ ] Slug rule: canonical_slug if present else sanitized model_id; index lists all 10 sorted by rank.
- [ ] Tests: golden-file snapshot for a frozen mini-roster fixture (2 models); slug function; rivalry aggregation math.
- [ ] Verify: run it; `curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8080/models/index.html` = 200 (server serves working tree); controller screenshots the pages.

### Task 5: Landing page integration

**Files:**
- Modify: `static_client/index.html`, `static_client/website/js/main.js`, maybe `static_client/website/css/styles.css`

- [ ] Roster rows (renderSeasonRoster in main.js) link each model to `/models/<slug>.html`; add mascot emoji before model name (fetch mascots via a tiny generated `static_client/models/mascots.json` copy from Task 3/4 output).
- [ ] New "Highlights" section on the landing page: fetch `/media/highlights/index.json`, show the 3 newest clips as looping muted `<video>` (webm) with gif fallback; empty-state hidden when no clips.
- [ ] Verify: `curl` 200s; controller screenshot of landing.

### Task 6: Daily scheduling

**Files:**
- Create: `~/.config/systemd/user/massive-game-media-daily.{service,timer}`

- [ ] Oneshot service running `node scripts/arena/build_model_pages.mjs && node scripts/media/render_highlights.mjs` in repo dir; timer daily at 04:17 local. `systemctl --user daemon-reload && systemctl --user enable --now massive-game-media-daily.timer`; verify `systemctl --user list-timers` shows it; run the service once manually and check exit 0.

### Task 7: Final verification

- [ ] All node:test suites in scripts/media + scripts/arena pass.
- [ ] Public 200s: `/models/index.html`, one model page, `/media/highlights/index.json`, one clip file.
- [ ] Screenshot review (controller reads PNGs): landing highlights section, one model page, one highlight GIF frame.
- [ ] No regression: `scripts/e2e/user-journey.js` 15/15; arena supervisor log clean.

## Self-review notes

- Spec coverage: highlights→T0-2, mascots→T3, profiles→T4, surfacing/scheduling→T5-6, verification→T7. Team chemistry proxy + provenance in T4 per spec.
- Replay files only exist after Task 0; T2 develops against real files.
- PNG encoder is hand-rolled intentionally (zero deps); ffmpeg is the only vendored binary, isolated in scripts/media/node_modules.
