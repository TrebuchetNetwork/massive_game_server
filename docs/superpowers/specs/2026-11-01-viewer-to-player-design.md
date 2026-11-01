# Viewer-to-Player Flow Design

Date: 2026-11-01
Status: approved direction (user selected "Viewers into players" and approved the compact design)

## Goal

A visitor watching the real fight on `space.selfware.design` can enter that same world within 15 seconds, with a first-join onboarding sequence that responds only to real input and server acknowledgements. This connects Arena TV to actual play without adding a lobby, tutorial simulation, or fabricated game state.

Non-goals: new game modes, separate training worlds, bot difficulty retuning, team-selection UI, changes to league evaluation, or a long tutorial.

## 1. Landing entry

When Arena TV is live, show a primary **Join this fight** action beneath the hero, next to the existing champion-duel action. The action navigates to:

- coarse-pointer/mobile: `client.html?match_type=mobile_blitz`
- desktop: `client.html?match_type=mobile_blitz` as well, because this is the same world and current public entry; device controls adapt automatically

No separate match is created by this CTA: `mobile_blitz` joins the existing authoritative world. Team assignment remains automatic and server-side. The join gesture unlocks audio. Leaving the landing page disposes of its spectator connection; returning starts a fresh Arena TV connection.

When Arena TV is not live, preserve the existing entry actions and highlight fallback. Do not imply that a recorded highlight is joinable live gameplay.

## 2. First-join onboarding

Implement a small client-side state machine and a non-blocking hint card. It runs only for a human player after successful join, never for spectators or `broadcast=1`.

Steps:

1. **Move** — mobile joystick or desktop movement input. Complete after the server-authoritative local position has changed meaningfully from the join position (not merely a touch/click).
2. **Aim** — right-side drag/mouse aim. Complete after local rotation changes from real input.
3. **Fire** — complete only when the server acknowledges the player's processed input/shot signal (use an existing authoritative field such as `input_seq_processed`, or an actual projectile owned by the local player; do not complete on button press alone).

After completion, hide the card and leave the normal objective layer in charge (CTF return/recovery arrows, hot-zone and match-state hints). Do not force the player to replay onboarding on every respawn. Persist completion locally so later visits skip it, with a menu action to replay hints if an existing settings surface allows it without new navigation.

The card never blocks controls, never pauses play, and uses actual current control labels (thumb joystick vs WASD). No tutorial bots, fake projectiles, or scripted score events.

## 3. Failure and lifecycle behavior

- Join failure shows the existing honest connection error and retry flow; onboarding cannot start before a successful player join.
- Death/respawn preserves the current onboarding step.
- Disconnect/reconnect does not reset completed steps; an uncompleted step resumes.
- Team 0/spectator and broadcast mode never instantiate the state machine.
- Missing or malformed state leaves the current hint visible rather than advancing incorrectly.

## 4. Architecture

- `static_client/website/js/main.js`: add the live-hero Join action using existing CSS/action patterns. No inline script.
- `static_client/client_logic/JoinOnboarding.js`: pure reducer/state machine (`createJoinOnboarding`) plus a thin DOM presenter. Inputs are authoritative local snapshots and explicit aim-input events; outputs are `move | aim | fire | done` and the current hint. Export through `client_logic/index.js`.
- `static_client/client.html`: instantiate only for joined human players, feed snapshots/events, and manage the small hint card. No server or protocol changes.

## 5. Testing and success criteria

- Unit tests: step gating, no advancement from touches alone, meaningful position delta, real rotation delta, server-acknowledged fire, respawn/reconnect persistence, spectator/broadcast exclusion, replay/reset behavior.
- Playwright production journeys:
  - mobile landing live → Join this fight → real touch move/aim/fire → authoritative processed-input counter advances → onboarding completes → zero page errors;
  - desktop join → WASD/mouse/fire path;
  - Arena TV spectator/broadcast never shows the card;
  - join failure does not show a success/onboarding state.
- Existing suites stay green (`cargo test -p massive_game_server_core --lib`, `npm run test:ui`, Arena TV checks).

Success: a first-time visitor can watch live and enter real play within 15 seconds, understand the three basic controls, and receive correct objective guidance without any simulated progress.
