# Viewer-to-Player Flow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a visitor go from the live Arena TV hero into the same real fight within 15 seconds, with onboarding that completes only from authoritative gameplay signals.

**Architecture:** A live-only landing CTA opens the existing mobile-blitz client. A new pure `JoinOnboarding` state machine tracks Move → Aim → Fire → Done using server local-state snapshots, explicit aim gestures, and newly visible server projectiles owned by the player. A thin client.html presenter displays hints; no server/protocol changes.

**Tech Stack:** Vanilla JS ESM, `node:test`, Playwright, existing landing/client CSS.

**Spec:** `docs/superpowers/specs/2026-11-01-viewer-to-player-design.md`

---

### Task 1: JoinOnboarding pure state machine

**Files:**
- Create: `static_client/client_logic/JoinOnboarding.js`
- Create: `static_client/tests/join_onboarding.test.js`
- Modify: `static_client/client_logic/index.js` (add `export * from './JoinOnboarding.js';`)

- [ ] **Step 1: Write failing tests**

Create tests covering: initial Move hint; touch alone does nothing; movement below 12 units does not advance; ≥12 advances to Aim; aim gesture without rotation delta does not advance; explicit gesture + ≥0.08 rad advances to Fire; Fire completes only for a newly seen projectile whose `owner_id` matches; unrelated/old projectiles do not complete; Done persists across factory recreation; death does not reset; reconnect preserves step but rebases movement/rotation/projectiles; storage failures do not throw; `reset()` returns to Move.

Use this public API exactly:

```js
const ob = createJoinOnboarding({ storage });
ob.view(); // { step: 'move'|'aim'|'fire'|'done', visible: bool, hint: string }
ob.bind({ x, y, rotation }); // establishes initial baselines
ob.onLocalState({ x, y, rotation }); // authoritative local snapshot
ob.notifyAimInput(); // explicit mobile/desktop aim gesture
ob.onProjectiles(projectiles, myPlayerId); // iterable of { id, owner_id }
ob.onReconnect({ x, y, rotation }); // preserve step, rebase all session signals
ob.reset(); // clear persisted completion and return to Move
```

For Done persistence use `storageKey = 'mgs_join_onboarding_v1'`, value `'done'`; inject a Map-compatible `{getItem,setItem}` object. `reset()` removes via `removeItem` if available, otherwise sets `''`.

- [ ] **Step 2: Run tests RED**

Run: `cd static_client && node --test tests/join_onboarding.test.js`
Expected: module-not-found failure.

- [ ] **Step 3: Implement the module**

Implement `createJoinOnboarding({ storage, storageKey='mgs_join_onboarding_v1', minMoveDistance=12, minAimDelta=0.08 } = {})`.

Required behavior:
- `step` starts `'done'` only when storage contains `'done'`, otherwise `'move'`.
- `view().visible` is `step !== 'done'`; hints are exactly:
  - move: `Move: left thumb / WASD`
  - aim: `Aim: drag right side / mouse`
  - fire: `Fire: tap FIRE / click`
  - done: empty string
- `bind()` sets position/rotation baselines and primes `seenProjectileIds` on the first `onProjectiles` call.
- Move advances only when `Math.hypot(x-baseX,y-baseY) >= minMoveDistance`. On advancing, rebase rotation to the current snapshot and clear `aimGesture`.
- Aim requires `notifyAimInput()` after entering Aim AND `abs(rotation-baseRotation) >= minAimDelta`. On advancing, prime projectile observation so existing projectiles cannot complete Fire.
- Fire completes only on a projectile id not in `seenProjectileIds` whose `String(owner_id) === String(myPlayerId)`. Always record seen ids in a bounded set (keep newest 512) to avoid growth. On completion persist `'done'` safely.
- `onReconnect()` preserves `step`, clears aim gesture, rebases position/rotation, and clears/re-primes projectile ids. An unfinished Move cannot complete from a reconnect teleport.
- Storage reads/writes/removals are wrapped in try/catch. No DOM or globals.

- [ ] **Step 4: Run tests GREEN**

Run: `cd static_client && node --test tests/join_onboarding.test.js && npm run test:ui`
Expected: new tests and full suite pass.

- [ ] **Step 5: Commit**

```bash
git add static_client/client_logic/JoinOnboarding.js static_client/client_logic/index.js static_client/tests/join_onboarding.test.js
git commit -m "feat(client): authoritative first-join onboarding state machine"
```

---

### Task 2: Client hint card + real input/projectile wiring

**Files:**
- Modify: `static_client/client.html` (imports ~628-668; card near `#controlHintsOverlay` ~143; local-state setter ~5301-5330; aim handlers; projectile update path; debug publication ~4694-4886; reset paths)
- Modify: `static_client/css/game.css` (dedicated card, mobile-safe offset)
- Modify: `static_client/client_logic/InputManager.js` (expose explicit aim gesture notification through existing input context/callback or a small callback)
- Modify: `scripts/e2e/arena-tv-client-check.cjs` (add the new card to broadcast-hidden assertions)

- [ ] **Step 1: Add card and lifecycle rules**

Add `<div id="joinOnboarding" class="join-onboarding" role="status" aria-live="polite"></div>` near `#controlHintsOverlay`. CSS: pointer-events none, readable monospace, dark/acid theme matching `.tips-toast`, positioned above mobile controls without covering the joystick/FIRE buttons, hidden by default. Add `body.broadcast-mode #joinOnboarding { display:none !important; }` to the existing broadcast CSS.

Instantiate `joinOnboarding = createJoinOnboarding()` once. Start only when all are true: `!BROADCAST_MODE`, local player exists, `Number(localPlayerState.team_id) !== 0`, and join is a human (not a bot). Do not start on data-channel open. Spectators and Arena TV never create a visible card. `view()` drives text/visibility; when existing `#controlHintsOverlay` is visible, hide the onboarding card to avoid stacking.

- [ ] **Step 2: Wire authoritative signals**

- Feed `bind()` once on first valid local state and `onLocalState()` from the raw local-state setter before client-side prediction/reconciliation. If the existing setter cannot provide raw values, feed from the JSON and fast-delta paths immediately after raw local assignment and before replay.
- Aim: call `notifyAimInput()` only from real mobile right-side aim start/move and desktop mousemove handlers. Do not call it from aim assist or `setAimRotation` test hooks.
- Fire: after each projectile-map update, call `onProjectiles(projectiles.values(), myPlayerId)`. New server-owned local projectiles are the only completion signal.
- Reconnect: call `onReconnect(currentLocalState)` after a new joined local state arrives; preserve step but rebase. Death/respawn does not call reset. `reset()` is only for explicit test/replay use.

- [ ] **Step 3: Debug and regression checks**

Publish `window.__e2e.joinOnboarding = { step, visible }` and `localOwnedProjectileCount` in the existing debug block; initialize/reset these fields with the other e2e fields. Add the card to the Arena TV hidden-selector probe.

Run: `cd static_client && npm run test:ui`
Expected: full suite green. Run `node scripts/e2e/arena-tv-client-check.cjs`; live broadcast must still work with zero errors and the new card hidden.

- [ ] **Step 4: Commit**

```bash
git add static_client/client.html static_client/css/game.css static_client/client_logic/InputManager.js scripts/e2e/arena-tv-client-check.cjs
git commit -m "feat(client): live move/aim/fire onboarding for joined human players"
```

---

### Task 3: Live-only Join this fight landing CTA

**Files:**
- Modify: `static_client/index.html` (`.hero__actions` ~66-70)
- Modify: `static_client/website/js/main.js` (Arena TV live/fallback branches ~439-484)
- Modify: `static_client/website/css/styles.css` (live-only action rule)

- [ ] **Step 1: Implement CTA and visibility**

Add to `.hero__actions`:

```html
<a class="button button--primary hero__join-live" href="/client.html?match_type=mobile_blitz" data-arena-entry data-join-live hidden>Join this fight <span aria-hidden="true">↗</span></a>
```

Keep the existing gauntlet/champion actions. `main.js` queries `[data-join-live]` and sets `hidden = false` on Arena TV live, `hidden = true` on every fallback path (watchdog, client fallback, `?tv=0`, no field). Use a shared setter so live/fallback cannot leave it stale. Do not use `:has()`; browser support is unnecessary here. CSS only needs `.hero__join-live[hidden] { display:none; }`; existing button styles do the rest. No new inline script and no claim that highlights are joinable.

- [ ] **Step 2: Verify landing states**

Run `ARENA_TV_REQUIRE_LIVE=1 node scripts/e2e/arena-tv-check.cjs` — live path still green. Extend the probe or a small temporary check to assert Join is visible on live and hidden after fallback/`?tv=0`. Confirm href is exactly `/client.html?match_type=mobile_blitz`.

- [ ] **Step 3: Commit**

```bash
git add static_client/index.html static_client/website/js/main.js static_client/website/css/styles.css scripts/e2e/arena-tv-check.cjs
git commit -m "feat(site): join the live fight directly from Arena TV"
```

---

### Task 4: Real viewer-to-player journeys + final validation

**Files:**
- Create: `scripts/e2e/viewer-to-player-check.cjs`
- Modify: `MAINTENANCE.md` (deployment root; short note about CTA/onboarding and probe command)

- [ ] **Step 1: Production mobile journey**

Write a Playwright script using a 390×844 coarse-touch context and a fresh player name/onboarding key:
1. Open `https://space.selfware.design/index.html`, wait for live Arena TV, assert Join visible.
2. Activate Join, then the existing mobile Enter Arena button.
3. Use real `TouchEvent`s: hold/move the left joystick ≥60px until step becomes `aim`; drag on the right side until step becomes `fire`; hold `#mobileFire` until step becomes `done` and `localOwnedProjectileCount > 0`.
4. Assert no page errors, card hidden at Done, and screenshot `/tmp/viewer-to-player-mobile.png`.
Timeout each real step at 20s; if the player dies, continue after respawn (do not reset progress).

- [ ] **Step 2: Desktop journey and negative paths**

In the same script, use a desktop context: Join → WASD movement → real mouse movement → canvas click fire → Done. Also assert direct `client.html?spectator=1&broadcast=1&auto_connect=1` never shows onboarding and join failure never reports Done. Use actual outcomes in logs, no forced input hooks (`forcePrimaryFire`, `setAimRotation`) and no fabricated state.

- [ ] **Step 3: Full verification and push**

Run:

```bash
cd static_client && npm run test:ui
cargo test -p massive_game_server_core --lib
node scripts/e2e/arena-tv-client-check.cjs
ARENA_TV_REQUIRE_LIVE=1 node scripts/e2e/arena-tv-check.cjs
node scripts/e2e/viewer-to-player-check.cjs
```

All must pass. Commit `test(e2e): validate live viewer-to-player onboarding on mobile and desktop`, update MAINTENANCE.md with the feature/probe, then `git push origin main`.

---

## Self-review notes

- Spec coverage: CTA same world (T3), human-only onboarding (T1/T2), authoritative movement/aim/fire (T1/T2), lifecycle/reconnect (T1/T2), negative paths and real journeys (T4), no server changes.
- Fire uses newly visible server projectiles, avoiding the stale `lastProcessedInput` reconnect pitfall. Movement uses raw local state before prediction; Aim requires an explicit gesture plus rotation delta.
- No separate replay menu is added (optional in spec); `reset()` is available for tests/future settings use. Existing audio unlock remains the reliable join-gesture path; the landing click cannot itself unlock a newly loaded AudioContext.
