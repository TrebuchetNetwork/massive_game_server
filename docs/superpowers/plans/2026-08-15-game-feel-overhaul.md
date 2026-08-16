# Game-Feel Overhaul Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the live game (space.selfware.design) feel modern: smooth wall movement, no camera bugs, haptics, new ship/wall/world visuals, working audio polish, no perf regression.

**Architecture:** Client is PixiJS (`static_client/client.html` + `client_logic/*.js`, WebGL, pre-rendered sprite textures). Server physics in Rust (`server/src/server/instance/`). Spec: `docs/superpowers/specs/2026-08-15-game-feel-overhaul-design.md`.

**Tech Stack:** Rust (server), vanilla JS + PixiJS (client), Node e2e (Playwright) for verification.

**Repo root:** `/home/habitat/massive-game-server-deployment/massive_game_server`

**Global verification (run after EVERY task):**
- `cd <repo> && cargo test -p massive_game_server_core --lib 2>&1 | tail -2` (when server touched)
- `cd <repo>/scripts/e2e && node user-journey.js` must end with `15/15 checks passed`
- Screenshot review: the journey writes `/tmp/user-journey-game.png` — read it.

---

### Task 1: Wall sliding (server physics)

**Files:**
- Modify: `server/src/server/instance/player_physics.rs` (collision resolution, currently ~line 324-378: circle-AABB closest-point + full position revert + velocity zeroing at 369-372)
- Test: `server/tests/integration/walls_integration.rs` (add), unit tests in `player_physics.rs` if a suitable module exists

- [ ] **Step 1: Write the failing test** (integration, pattern follows `server/tests/integration/anti_cheat.rs` helpers `setup_test_server`/`add_player`):

```rust
#[tokio::test(flavor = "multi_thread")]
async fn player_slides_along_wall_instead_of_stopping() {
    let server = setup_test_server();
    // Place player next to a horizontal wall, moving diagonally into it.
    let pid = add_player(&server, "slider", 1, 100.0, 96.0); // adjust to a real wall from the default map
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.velocity_x = 120.0; // tangential
        ps.velocity_y = 60.0;  // into the wall
    }
    server.run_physics_update(0.016).await;
    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(ps.velocity_x.abs() > 100.0, "tangential speed must be preserved, got {}", ps.velocity_x);
    assert!(ps.velocity_y.abs() < 1.0, "normal component must be removed, got {}", ps.velocity_y);
}
```

- [ ] **Step 2:** `cargo test -p massive_game_server_core --test walls_integration player_slides 2>&1 | tail -5` → FAIL (velocity zeroed today)
- [ ] **Step 3:** Implement: in the collision branch, compute wall normal from closest-point vector; `v_tangent = v − (v·n)n`; set position to contact point (not full revert) and velocity to `v_tangent`. Keep the `wall_slam` system event when the removed normal speed exceeds the existing slam threshold. Do not touch `last_valid_position` handling (fixed separately, already in tree).
- [ ] **Step 4:** test passes; also `cargo test -p massive_game_server_core --test anti_cheat --test input_and_combat 2>&1 | tail -3` all pass
- [ ] **Step 5:** e2e: `scripts/e2e/tests/player_wall_collision.spec.js` via `npx playwright test tests/player_wall_collision.spec.js` (uses local debug server — see `scripts/e2e/run.sh` for build/run pattern)

### Task 2: Screen-shake stacking fix (client)

**Files:**
- Modify: `static_client/client_logic/GameRenderer.js:489-506` (`applyScreenShake`), call sites at `:1127-1129`, `:1609-1610`, `client_logic/CombatFeedback.js:1043,1186`

- [ ] **Step 1:** Read current implementation + call sites; note signature `applyScreenShake(intensity, durationMs?)`.
- [ ] **Step 2:** Replace with trauma model: `this._shakeTrauma = Math.min(1, (this._shakeTrauma||0) + intensity)`; each frame in the render loop compute `offset = perlinishRandom() * trauma^2 * MAX_PX`, decay `trauma -= dt/duration`. Remove the private rAF chain and the position capture/restore entirely. Keep the public method name/signature so call sites are unchanged.
- [ ] **Step 3:** Verify: run a match in the local debug server (pattern: `E2E_SERVER_CMD=target/debug/massive_game_server_core`), trigger overlapping shakes (dash + get hit), screenshot shows no permanent camera offset; `node scripts/e2e/user-journey.js` stays 15/15.

### Task 3: Mobile haptics

**Files:**
- Modify: `static_client/client_logic/InputManager.js` (touch fire, ~`:1062-1141`), `static_client/client_logic/CombatFeedback.js` (hit taken ~`:559-596`, death event)
- Setting exists: `mobileHaptics` toggle (`client.html:450`)

- [ ] **Step 1:** Add `function vibrate(pattern){ if (settings.mobileHaptics && navigator.vibrate) navigator.vibrate(pattern); }` helper in `InputManager.js` (exported or passed via ctx, matching the file's existing DI pattern — read `getCtx()` usage first).
- [ ] **Step 2:** Wire: fire → `vibrate(10)`, hit taken → `vibrate(30)`, death → `vibrate([50,40,50])`.
- [ ] **Step 3:** Verify on real phone (user) + assert no errors in desktop Chrome (no `navigator.vibrate`) via journey script console-error check.

### Task 4: Dead UI bundle removal

**Files:**
- Delete: `static_client/touch-controls.js`, `static_client/ui-template.html`, `static_client/ui-main.css`, `ui-variables.css`, `ui-hud.css`, `ui-ingame.css`, `ui-menu.css`, `static_client/hud-controller.js`, `static_client/ui-manager.js` (confirm exact names first)

- [ ] **Step 1:** `grep -rn 'ui-template\|hud-controller\|ui-manager\|touch-controls' static_client/client.html static_client/client_logic/ static_client/index.html static_client/website/` → only self-references allowed.
- [ ] **Step 2:** Delete files; `curl` the public site after next deploy for 200s on `/`, `/client.html`.
- [ ] **Step 3:** Journey script stays 15/15.

### Task 5: Ship sprites + engine glow/trails

**Files:**
- Modify: `static_client/client_logic/SpriteManager.js:11-113` (ship body/gun texture generation), `:939-1003` (projectile pool), `RenderAssetManager.js`
- Modify: `client_logic/effects_audio_runtime.js` (trail emitter hooks)

- [ ] **Step 1:** Read ship texture generation; note canvas size, team coloring mechanism, health-bar composition.
- [ ] **Step 2:** Redesign ship texture: layered hull (dark base → mid gradient → bright top edge), cockpit stripe, team-colored engine glow disc behind hull. Pre-rendered once per team color at init (same lifecycle as today).
- [ ] **Step 3:** Engine trail: pooled sprites emitted while velocity > threshold, budget-gated via `PerformanceBudget.js` device class caps (read `:499-517` first); off on `low` profile.
- [ ] **Step 4:** Verify: journey 15/15 + screenshot read-back shows new ships; check `mid`/`low` profiles still hit FPS caps (use `?perf=` overrides already in client.html if available).

### Task 6: Wall panel visuals

**Files:**
- Modify: `static_client/client_logic/GameRenderer.js:137-310` (wallGraphics), `WorldRenderer.js` wall paths

- [ ] **Step 1:** Read current wall drawing (flat `PIXI.Graphics` rects).
- [ ] **Step 2:** Panel style: base fill + 1px top/left highlight + bottom/right shadow + faint inner grid; cracked overlay for damaged destructible walls (wall health is available in state — read the wall update path first). Cache per-wall sprites; only redraw on damage state change.
- [ ] **Step 3:** Verify screenshot: walls read as panels, not gray boxes; journey 15/15.

### Task 7: Map themes ("better worlds")

**Files:**
- Create: `static_client/client_logic/MapThemes.js` (theme table + applier)
- Modify: `client_logic/GameRenderer.js:312-396` (starfield/parallax), `:398` (vignette), `WorldRenderer.js:131-354` (fog/zone overlays)

- [ ] **Step 1:** Read how the client learns the current map (match info snapshot) — `window.__e2e.matchInfoSnapshot` exists; find its source field.
- [ ] **Step 2:** `MAP_THEMES = { default: {...}, <mapName>: {...} }` — bg gradient stops, star density/color, nebula tint sprite, wall palette override hook for Task 6, vignette tint. 3 concrete themes (e.g. "Void" purple-black, "Ember" deep red-orange, "Frost" blue-teal).
- [ ] **Step 3:** Apply theme on match start / map change; unknown map → `default`.
- [ ] **Step 4:** Verify screenshots per theme by forcing map (dev server config) or by reading theme application logs; journey 15/15.

### Task 8: Effect enablement on mobile profiles

**Files:**
- Modify: `static_client/client.html:1926-1938` (`MOBILE_RENDER_PROFILES`), `client_logic/effects_audio_runtime.js:288-317, 536-575` (quality profiles, `shouldEmitEffect` strides)

- [ ] **Step 1:** Read profile gates; list which effect kinds are suppressed on `high`/`mid`.
- [ ] **Step 2:** Key combat effects (muzzle, explosion, kill hitstop, dash) always emit on `high` and `mid` (lower spawn stride instead of suppressing); only `low` keeps suppression.
- [ ] **Step 3:** Verify with journey screenshot on emulated mid-tier profile + perf spec (`tests/ui_performance.spec.js`).

### Task 9: Music player restyle + playlist cleanup

**Files:**
- Modify: `static_client/css/game.css` (new dedicated `.music-player` block, ~client.html:460-505 markup), `static_client/client_logic/music_player.js:2-12` (playlist), `:213` (now-playing)
- Rename: `static_client/music/Untitled (N).mp3`, `cassete.mp3` → clean names

- [ ] **Step 1:** Read markup + current shim classes used; write self-contained CSS (neon-glass: dark translucent panel, lime accent, backdrop-filter, rounded, mobile-positioned) not depending on the Tailwind shim.
- [ ] **Step 2:** Rename files; update playlist entries `{ title, file }`; now-playing shows `title`.
- [ ] **Step 3:** Add a compact music toggle in the mobile menu (`#mobileControls` area, read `client.html:169-193` first).
- [ ] **Step 4:** Verify: Playwright screenshot of the player open (desktop + mobile viewport); no console errors.

### Task 10: SFX gap-fill

**Files:**
- Create: `scripts/media/gen_sfx.mjs` (PCM wav writer: 2 footsteps, soft/hard impact — short noise/sine bursts with envelopes)
- Create: 4 files under `static_client/sfx/`
- Modify: `client_logic/effects_audio_runtime.js:3536-3569` (sample map), mobile budgets `:3596-3686`

- [ ] **Step 1:** Read the sample registration + rate-limit structure.
- [ ] **Step 2:** Generate wavs (44.1kHz mono 16-bit, <0.4s each); register with rate limits matching existing entries.
- [ ] **Step 3:** Verify: sounds listed in AudioManager map load (200 on public URL after deploy); journey clean.

### Task 11: Performance guardrails + final verification

**Files:**
- Modify: `static_client/client_logic/PerformanceBudget.js:499-517` (device-class caps for new emitters)

- [ ] **Step 1:** Ensure every new emitter (trails, theme starfields) reads its cap from PerformanceBudget.
- [ ] **Step 2:** Full gate: `cargo test -p massive_game_server_core` (all), `cd scripts/e2e && npx playwright test tests/mobile_touch.spec.js tests/ui_performance.spec.js --workers=1`, `node user-journey.js` → 15/15.
- [ ] **Step 3:** 5-minute soak on local server: watch `window.__e2e.renderFrames` advance and no memory climb (performance.memory samples in console).
- [ ] **Step 4:** Rebuild release (`cargo build --release -p massive_game_server_core`), restart `massive-game-server.service`, public journey green, final screenshots reviewed.

---

## Self-review notes

- Spec coverage: A1→Task 1, A2→Task 2, A3→Task 3, A4→Task 4, B1→Task 5, B2→Task 6, B3→Task 7, B4→Task 8, C1→Task 9, C2/C3→Task 10, D→Task 11. All covered.
- Tasks marked "read first" are read-modify tasks on existing code; executors must match surrounding style.
- Client tasks have no unit-test harness for visuals — verification is e2e journey + Playwright screenshots, as established in the repo.
