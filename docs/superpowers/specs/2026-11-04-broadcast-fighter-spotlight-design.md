# Arena TV — authoritative fighter spotlight

Date: 2026-11-04
Status: approved direction (user delegated product choices and waived interviews)

## Goal

Make the existing automatic broadcast camera understandable and give viewers a fighter to attach to. When the director follows a ship, Arena TV will show a compact lower-third identifying that exact ship and its current real state. Commentary will expire instead of remaining on screen after its event is stale.

Non-goals: manual fighter pinning, canonical model identity in the live protocol, a new replay buffer, video streaming, LLM commentary, or game-simulation changes. Existing rendered highlights remain honestly labeled as offline highlights, not instant replays.

## Approaches considered

1. **Authoritative auto-follow spotlight (selected).** Extend the current director frame with the selected candidate's display name, team, and health, then render and expire overlay state. This is small, uses data already visible to the spectator, and directly explains the existing camera.
2. **Manual model picker.** More control, but current broadcast candidates have session player IDs rather than stable canonical model IDs. It also needs explicit respawn, disconnect, and reconnect behavior. Defer until the spotlight proves useful.
3. **Live instant replay.** The current interpolation buffer is too short and existing highlight videos are rendered offline. Calling those clips instant or live would be misleading. Defer rather than fabricate a replay path.

## Director contract

`BroadcastDirector.cameraFrame()` will retain its existing fields (`focusX`, `focusY`, `zoom`, `targetId`) and add metadata copied from the same selected candidate:

- `targetName`: the current server-provided display username, including any real rank prefix.
- `targetTeamId`: the numeric team already used by director scoring.
- `targetHp` and `targetMaxHp`: the candidate's current numeric health fields, which may be non-finite when unavailable.

A null frame remains the only no-target signal. The director will not infer rank, canonical model ID, health, or any other statistic.

## Overlay and visual hierarchy

`BroadcastOverlay` will own a new `setFocus(frame)` method:

- A non-null frame shows a compact lower-third: `FOLLOWING`, the full target name, team, and HP when those values are valid.
- A null frame hides the lower-third immediately. Dead, invalid, or disappeared fighters therefore cannot leave a stale card behind.
- The overlay caches the last rendered focus signature and only writes DOM when displayed values change. Camera positions do not appear in the signature, so normal camera motion adds no DOM churn.
- Text uses `textContent`; no player-provided name is inserted as HTML.
- The card sits above the bottom commentary line, including in the small landing-page iframe, and uses the existing Arena TV visual language rather than player HUD chrome.

Commentary gets a seven-second TTL. `setCommentary(line, atMs)` records the event-display time and `tick(atMs)` clears an expired line. Empty lines clear immediately. Existing kill-feed TTL behavior remains unchanged.

## Lifecycle and performance

The camera adapter in `client.html` will pass each computed frame to `broadcastOverlay.setFocus()` and return that same frame to `WorldRenderer`. This preserves one authoritative target-selection call per camera frame.

Both reconnect/reset paths will clear director state, overlay focus, feed, and commentary. A new `reset()` on the director clears target, event history, dwell, challenger, zoom, and sweep bookkeeping; overlay `reset()` clears rendered state. A reconnect therefore cannot show the previous session's fighter or story.

No server or protocol change is required. Changed leaf-module URLs and the client barrel URL will share a new cache version so stale re-exports cannot mix module contracts.

## Error handling

- Missing or non-finite HP hides only the HP segment; name and valid team remain visible.
- Missing names fall back to the session target ID, which is real state and avoids an empty card.
- Null frames and reset hide the focus card and clear commentary.
- Overlay hooks remain broadcast-only and do not affect human gameplay, spectator onboarding exclusions, or the simulation.

## Testing

- Unit tests first: director metadata matches the selected candidate; overlay focus formatting, null hiding, DOM-write caching, commentary expiry, and reset.
- Extend the real production Arena TV browser probe to map `frame.targetId` to the same candidate and verify that the visible spotlight name/team/HP agrees with that candidate. Capture desktop and landing-iframe screenshots.
- Run the complete `static_client` UI suite, Arena TV client check, live/fallback/opt-out landing check, and viewer-to-player check.
- Verify reconnect clearing through the browser probe rather than relying only on unit coverage.

## Rollout

Ship as static-client-only changes with coordinated cache versions. No Rust rebuild or game-server restart is needed. Commit the director/overlay change, wiring, tests, and probe updates separately from generated daily media churn, then validate `space.selfware.design` live before pushing the final result.
