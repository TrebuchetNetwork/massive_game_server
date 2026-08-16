# Game-Feel Overhaul — Design

**Date:** 2026-08-15
**Status:** Approved direction (all phases A→D)
**Scope:** `static_client/` (PixiJS client) + two server physics behaviors. No protocol changes.

## Goal

Make the live game (space.selfware.design mobile blitz) feel modern:
responsive movement, visible combat juice, themed worlds, working audio/haptics,
without regressing mobile performance.

## Current state (verified by code inventory + live screenshot)

- Effects system exists (`client_logic/effects_audio_runtime.js`): muzzle flash,
  explosions, trails, damage numbers, shake, parallax starfield — but the base
  art is flat (shape walls, plain ship sprites) and mobile profiles gate effects.
- Audio: WebAudio synth + 33 wav samples (`static_client/sfx/`); music player
  (`client_logic/music_player.js`) styled by a broken hand-written Tailwind shim
  (`css/game.css:1-90`) — the "win97 look". 11 tracks named `Untitled (N).mp3`.
- Collisions server-side: projectile tunneling and wall-LoS already fixed;
  player-player is soft-push only (acceptable); wall hits zero velocity +
  position revert (`player_physics.rs:369-372`) — sticky wall feel.

## Phase A — Game-feel fixes

1. **Wall sliding** (`server/src/server/instance/player_physics.rs`): on wall
   contact, project velocity onto the wall tangent and resolve penetration along
   the normal, instead of zeroing velocity and reverting position. Keep the
   `wall_slam` event for high-speed head-on impacts (same thresholds as today).
   Tests: new unit test (slide preserves tangential speed, kills normal speed);
   existing wall collision tests must pass.
2. **Screen-shake fix** (`client_logic/GameRenderer.js:489-506`): replace
   capture/restore with a single shake state `{ trauma }`; offset computed each
   frame in the main ticker from trauma with decay; no private rAF chain.
3. **Haptics**: `navigator.vibrate` on fire (10ms), hit taken (30ms), death
   ([50,40,50]) — gated by the existing `mobileHaptics` setting, no-ops
   elsewhere. Wired in `InputManager.js` / `CombatFeedback.js`.
4. **Dead code removal**: delete `static_client/touch-controls.js`,
   `ui-template.html`, `ui-main/ui-variables/ui-hud/ui-ingame/ui-menu.css`,
   `hud-controller.js`, `ui-manager.js` after a reference sweep confirms only
   `ui-template.html` references them.

## Phase B — Visual overhaul

1. **Ships** (`SpriteManager.js` / `RenderAssetManager`): new pre-rendered
   textures — sleeker hull with gradient bevel, cockpit stripe, team-colored
   engine glow sprite; pooled engine-trail emitter (budget-gated per device
   class via `PerformanceBudget.js`).
2. **Walls** (`GameRenderer.js` / `WorldRenderer.js`): panel look — base fill,
   1px top/left highlight + bottom/right shadow, subtle inner grid lines;
   cracked variant for destructible walls (damage states already exist
   server-side).
3. **Map themes** ("better worlds"): client-side `MAP_THEMES` table keyed by
   map name/id from match info — background gradient stops, starfield density,
   nebula sprite tint, wall palette, ambient vignette tint. 3 themes to start.
4. **Effects enablement**: verify mobile `high` profile enables the full effect
   set; tune `shouldEmitEffect` strides so combat moments (spawn, dash, kill,
   explosion) always fire their key effect even on `mid`.

## Phase C — Audio

1. **Music player restyle**: dedicated CSS in `game.css` for the player panel
   (no shim dependency), neon-glass look matching the arena theme; clean track
   titles (rename files, update `music_player.js` playlist); now-playing line;
   accessible from the mobile menu.
2. **SFX gap-fill**: generate 4 short wavs (footstep ×2, soft impact, hard
   impact) with a small Node PCM script under `scripts/media/`, register in
   `AudioManager.sounds` map with rate limits.
3. Volume/limiter sanity pass on mobile budgets (existing mobile caps).

## Phase D — Performance guardrails

- Particle/trail budgets per device class in `PerformanceBudget.js`; new
  emitters respect them from day one.
- After each phase: `ui_performance.spec.js` + the headless mobile journey
  (`scripts/e2e/user-journey.js`, must stay 15/15) + before/after screenshots
  reviewed by reading the PNGs.
- Target: 60fps on mobile-high, 30fps floor on mid, no permanent camera offset,
  no memory growth over a 5-minute soak (existing perf hooks).

## Error handling / fallbacks

- WebGL context loss: existing PIXI recovery path; new textures are
  pre-rendered at init (same lifecycle as current sprites).
- AudioContext locked until first gesture (mobile autoplay policy) — existing
  unlock path; new sounds join the same preload/critical list.
- If a map has no theme entry → default theme (never blank background).

## Testing

- Server: new wall-slide unit test + existing `walls_integration`,
  `player_wall_collision` e2e, full `cargo test -p massive_game_server_core`.
- Client: e2e journey green; `mobile_touch.spec.js`; perf spec; visual
  screenshot review per phase.
- Rollout: verify on the local debug server first, then restart production.

## Out of scope

- No WebGPU renderer rewrite, no engine swap, no new game modes.
- Model profile pages / highlight videos: separate spec
  (`2026-08-15-model-profiles-highlight-media-design.md`).
