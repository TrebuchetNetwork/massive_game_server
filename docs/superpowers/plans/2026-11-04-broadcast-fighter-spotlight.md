# Arena TV Fighter Spotlight Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Identify the exact real fighter followed by Arena TV's automatic camera, expire stale commentary, and clear all broadcast presentation state on reconnect.

**Architecture:** Extend the existing director's camera frame with metadata copied from its selected candidate. Keep rendering inside `BroadcastOverlay`, with cached DOM writes and a shared reset helper called by both client reset paths. No server, protocol, replay, or simulation changes.

**Tech Stack:** Plain JavaScript ES modules, browser DOM, Node `node:test`, Playwright production probes.

**Design:** `docs/superpowers/specs/2026-11-04-broadcast-fighter-spotlight-design.md`

---

## File map

- `static_client/client_logic/BroadcastDirector.js`: authoritative target metadata and director reset.
- `static_client/client_logic/BroadcastOverlay.js`: focus card, commentary TTL, DOM caching, overlay reset.
- `static_client/tests/broadcast_director.test.js`: director contract tests.
- `static_client/tests/broadcast_overlay.test.js`: overlay formatting, TTL, cache, and reset tests.
- `static_client/client.html`: focus styling, camera wiring, shared broadcast reset, cache version.
- `static_client/client_logic/index.js`: versioned leaf exports.
- `scripts/e2e/arena-tv-client-check.cjs`: real target/spotlight agreement and reset probe.

### Task 1: Director target metadata and reset

**Files:**
- Modify: `static_client/client_logic/BroadcastDirector.js`
- Test: `static_client/tests/broadcast_director.test.js`

- [ ] **Step 1: Write failing director tests**

Append these tests:

```js
test('camera frame exposes metadata from the exact selected candidate', () => {
    const d = createBroadcastDirector();
    const frame = d.cameraFrame([
        P('a', { username: '#2 Astra', teamId: 2, hp: 72, maxHp: 100 }),
        P('leader', { username: '#1 Orbit', teamId: 1, hp: 90, maxHp: 120 }),
    ], 'leader', 1000);
    assert.equal(frame.targetId, 'leader');
    assert.equal(frame.targetName, '#1 Orbit');
    assert.equal(frame.targetTeamId, 1);
    assert.equal(frame.targetHp, 90);
    assert.equal(frame.targetMaxHp, 120);
});

test('reset clears target and event history', () => {
    const d = createBroadcastDirector();
    const players = [P('a'), P('b')];
    d.cameraFrame(players, 'a', 1000);
    d.recordDamage({ instigatorId: 'b', value: 500, atMs: 1100 });
    d.recordKill({ killerName: 'a', atMs: 1100 });
    d.reset();
    const frame = d.cameraFrame(players, null, 1200);
    assert.equal(frame.targetId, 'a');
    assert.equal(frame.zoom, 1);
    assert.equal(frame.targetName, 'a');
});
```

- [ ] **Step 2: Verify the tests fail**

Run: `cd static_client && node --test tests/broadcast_director.test.js`
Expected: FAIL because `targetName` is undefined and `reset` is not a function.

- [ ] **Step 3: Implement the minimal director contract**

Add this method inside `createBroadcastDirector`:

```js
function reset() {
    damage.clear();
    kills.clear();
    targetId = null;
    targetSince = 0;
    challengerId = null;
    challengerSince = 0;
    zoomUntil = 0;
    currentTargetName = null;
    lastSweepAt = null;
}
```

Replace the final return in `cameraFrame` and the exported object with:

```js
return {
    focusX: target.x,
    focusY: target.y,
    zoom: atMs < zoomUntil ? 1.15 : 1,
    targetId: id,
    targetName: target.username,
    targetTeamId: target.teamId,
    targetHp: target.hp,
    targetMaxHp: target.maxHp,
};
```

```js
return { recordDamage, recordKill, selectTarget, cameraFrame, reset };
```

- [ ] **Step 4: Verify director tests and full UI tests**

Run: `cd static_client && node --test tests/broadcast_director.test.js && npm run test:ui`
Expected: director tests PASS; complete UI suite PASS.

- [ ] **Step 5: Commit Task 1**

```bash
git add static_client/client_logic/BroadcastDirector.js static_client/tests/broadcast_director.test.js
git commit -m "feat(arena): expose followed fighter state"
```

### Task 2: Overlay focus, TTL, caching, and reset

**Files:**
- Modify: `static_client/client_logic/BroadcastOverlay.js`
- Test: `static_client/tests/broadcast_overlay.test.js`

- [ ] **Step 1: Add a lightweight fake DOM and failing overlay tests**

Append this helper and tests. The fake DOM supports only the static overlay markup and deliberately counts text writes.

```js
import { createBroadcastOverlay, COMMENTARY_TTL_MS } from '../client_logic/BroadcastOverlay.js';

function fakeElement() {
    let text = '';
    let html = '';
    const element = {
        className: '', hidden: false, children: [], textWrites: 0,
        set textContent(value) { text = String(value); this.textWrites += 1; },
        get textContent() { return text; },
        set innerHTML(value) {
            html = String(value);
            this.children = [...html.matchAll(/class="([^"]+)"/g)].map((match) => {
                const child = fakeElement();
                child.className = match[1];
                child.hidden = match[0].includes(' hidden');
                return child;
            });
        },
        get innerHTML() { return html; },
        appendChild(child) { this.children.push(child); return child; },
        querySelector(selector) {
            const className = selector.slice(1);
            return this.children.find((child) => child.className.split(' ').includes(className)) || null;
        },
    };
    return element;
}

function makeOverlay() {
    const parent = fakeElement();
    const doc = { createElement: () => fakeElement() };
    const overlay = createBroadcastOverlay({ document: doc, parent });
    return { overlay, root: overlay.element };
}

test('focus uses exact target metadata and caches unchanged DOM writes', () => {
    const { overlay, root } = makeOverlay();
    const frame = { targetId: 'p1', targetName: '#1 Orbit', targetTeamId: 2, targetHp: 72.1, targetMaxHp: 100 };
    overlay.setFocus(frame);
    const card = root.querySelector('.arena-tv-focus');
    const name = root.querySelector('.arena-tv-focus-name');
    const meta = root.querySelector('.arena-tv-focus-meta');
    assert.equal(card.hidden, false);
    assert.equal(name.textContent, '#1 Orbit');
    assert.equal(meta.textContent, 'TEAM 2 · HP 73/100');
    const writes = name.textWrites;
    overlay.setFocus({ ...frame, focusX: 999 });
    assert.equal(name.textWrites, writes);
    overlay.setFocus({ targetId: 'p2', targetHp: NaN });
    assert.equal(name.textContent, 'p2');
    assert.equal(meta.textContent, '');
    assert.equal(meta.hidden, true);
    overlay.setFocus(null);
    assert.equal(card.hidden, true);
});

test('commentary expires and reset clears all rendered broadcast state', () => {
    const { overlay, root } = makeOverlay();
    overlay.setCommentary('Real streak', 1000);
    overlay.tick(1000 + COMMENTARY_TTL_MS - 1);
    assert.equal(root.querySelector('.arena-tv-commentary').textContent, 'Real streak');
    overlay.tick(1000 + COMMENTARY_TTL_MS);
    assert.equal(root.querySelector('.arena-tv-commentary').textContent, '');
    overlay.setCommentary('Old session', 2000);
    overlay.setFocus({ targetId: 'p1', targetName: 'Old fighter', targetTeamId: 1 });
    overlay.pushKill({ killerName: 'A', victimName: 'B', atMs: 2000 });
    overlay.reset();
    assert.equal(root.querySelector('.arena-tv-commentary').textContent, '');
    assert.equal(root.querySelector('.arena-tv-focus').hidden, true);
    assert.equal(root.querySelector('.arena-tv-focus-name').textContent, '');
    assert.equal(root.querySelector('.arena-tv-killfeed').innerHTML, '');
    assert.equal(root.querySelector('.arena-tv-clock').textContent, '--:--');
});
```

- [ ] **Step 2: Verify overlay tests fail**

Run: `cd static_client && node --test tests/broadcast_overlay.test.js`
Expected: FAIL because `COMMENTARY_TTL_MS`, `setFocus`, and `reset` are absent.

- [ ] **Step 3: Implement the overlay contract**

Export the TTL beside the kill-feed constants:

```js
export const COMMENTARY_TTL_MS = 7000;
```

Add the focus markup between the kill feed and commentary:

```js
'<div class="arena-tv-focus" hidden>' +
'<span class="arena-tv-focus-kicker">FOLLOWING</span>' +
'<strong class="arena-tv-focus-name"></strong>' +
'<span class="arena-tv-focus-meta"></span>' +
'</div>' +
```

Cache those elements and state:

```js
const focusEl = element.querySelector('.arena-tv-focus');
const focusNameEl = element.querySelector('.arena-tv-focus-name');
const focusMetaEl = element.querySelector('.arena-tv-focus-meta');
let focusSignature = null;
let commentarySetAt = null;
```

Add these methods to the returned overlay object and call them from `tick`/`setCommentary` as shown:

```js
setFocus(frame) {
    if (!frame) {
        if (focusSignature !== null) {
            focusSignature = null;
            focusEl.hidden = true;
        }
        return;
    }
    const name = String(frame.targetName || frame.targetId || '').trim();
    if (!name) {
        focusSignature = null;
        focusEl.hidden = true;
        return;
    }
    const teamId = Number(frame.targetTeamId);
    const hp = Number(frame.targetHp);
    const maxHp = Number(frame.targetMaxHp);
    const team = Number.isFinite(teamId) && teamId !== 0 ? `TEAM ${teamId}` : '';
    const health = Number.isFinite(hp) && Number.isFinite(maxHp) && maxHp > 0
        ? `HP ${Math.ceil(Math.max(0, hp))}/${Math.ceil(maxHp)}`
        : '';
    const meta = [team, health].filter(Boolean).join(' · ');
    const signature = JSON.stringify([name, meta]);
    if (signature === focusSignature) return;
    focusSignature = signature;
    focusNameEl.textContent = name;
    focusMetaEl.textContent = meta;
    focusMetaEl.hidden = !meta;
    focusEl.hidden = false;
},
setCommentary(line, atMs = Date.now()) {
    const text = String(line || '');
    commentaryEl.textContent = text;
    commentarySetAt = text ? (Number.isFinite(atMs) ? atMs : Date.now()) : null;
},
reset() {
    feed = [];
    renderFeed();
    focusSignature = null;
    focusEl.hidden = true;
    focusNameEl.textContent = '';
    focusMetaEl.textContent = '';
    focusMetaEl.hidden = true;
    commentarySetAt = null;
    commentaryEl.textContent = '';
    scoresEl.textContent = '';
    clockEl.textContent = '--:--';
},
```

At the end of `tick(atMs)`, add:

```js
if (commentarySetAt !== null && atMs - commentarySetAt >= COMMENTARY_TTL_MS) {
    commentarySetAt = null;
    commentaryEl.textContent = '';
}
```

Keep only one `setCommentary` method: replace the existing one-liner with the timestamped implementation above.

- [ ] **Step 4: Verify overlay tests and full UI tests**

Run: `cd static_client && node --test tests/broadcast_overlay.test.js && npm run test:ui`
Expected: overlay tests PASS; complete UI suite PASS.

- [ ] **Step 5: Commit Task 2**

```bash
git add static_client/client_logic/BroadcastOverlay.js static_client/tests/broadcast_overlay.test.js
git commit -m "feat(arena): add live fighter spotlight"
```

### Task 3: Client wiring, lifecycle, cache versions, and production probe

**Files:**
- Modify: `static_client/client.html`
- Modify: `static_client/client_logic/index.js`
- Modify: `scripts/e2e/arena-tv-client-check.cjs`

- [ ] **Step 1: Extend the failing production probe before wiring**

In the director wrapper, replace each recorded call with enough real target data to compare against the DOM:

```js
const target = (args[0] || []).find((player) => player.id === frame?.targetId);
calls.push({
    playerCount: (args[0] || []).length,
    leaderId: args[1],
    target: target ? {
        id: target.id,
        username: target.username,
        teamId: target.teamId,
        hp: target.hp,
        maxHp: target.maxHp,
    } : null,
    frame,
});
```

Add to the page result:

```js
focus: {
    hidden: document.querySelector('.arena-tv-focus')?.hidden,
    name: document.querySelector('.arena-tv-focus-name')?.textContent || '',
    meta: document.querySelector('.arena-tv-focus-meta')?.textContent || '',
},
```

After the existing target assertion, add:

```js
const latest = [...result.calls].reverse().find((call) => call.frame && call.target);
assert.ok(latest, 'director never exposed its selected candidate');
assert.equal(result.focus.hidden, false, 'fighter spotlight is hidden during a live target');
assert.equal(result.focus.name, latest.target.username || latest.target.id);
assert.ok(result.focus.meta.includes(`TEAM ${latest.target.teamId}`));
if (Number.isFinite(latest.target.hp) && Number.isFinite(latest.target.maxHp) && latest.target.maxHp > 0) {
    const hp = Math.ceil(Math.max(0, latest.target.hp));
    assert.ok(result.focus.meta.includes(`HP ${hp}/${Math.ceil(latest.target.maxHp)}`));
}
await page.evaluate(() => window.__arenaTv.reset());
const resetState = await page.evaluate(() => ({
    hidden: document.querySelector('.arena-tv-focus').hidden,
    name: document.querySelector('.arena-tv-focus-name').textContent,
    commentary: document.querySelector('.arena-tv-commentary').textContent,
}));
assert.deepEqual(resetState, { hidden: true, name: '', commentary: '' });
```

Run against the unmodified client:

Run: `node scripts/e2e/arena-tv-client-check.cjs`
Expected: FAIL because focus metadata/elements and `window.__arenaTv.reset` are absent.

- [ ] **Step 2: Add focus styling and wire the camera frame**

Add this CSS after `.arena-tv-commentary:empty`:

```css
.arena-tv-focus {
    position: absolute;
    left: 16px;
    bottom: 68px;
    display: grid;
    gap: 3px;
    max-width: calc(100vw - 32px);
    padding: 9px 13px;
    border-left: 3px solid #22d3ee;
    border-radius: 3px;
    background: rgba(2, 6, 23, 0.82);
}
.arena-tv-focus[hidden] { display: none; }
.arena-tv-focus-kicker {
    color: #67e8f9;
    font-size: 10px;
    letter-spacing: 0.16em;
}
.arena-tv-focus-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 16px;
}
.arena-tv-focus-meta { color: #fbbf24; font-size: 12px; }
.arena-tv-focus-meta[hidden] { display: none; }
```

Replace the camera adapter return with:

```js
const frame = broadcastDirector.cameraFrame(directorPlayers, exhibitionLeaderId, Date.now());
broadcastOverlay?.setFocus(frame);
return frame;
```

Pass the event timestamp into commentary display:

```js
if (line && broadcastOverlay) broadcastOverlay.setCommentary(line, atMs);
```

- [ ] **Step 3: Add one shared reset helper and use both reset paths**

Define beside `emitBroadcastCommentary`:

```js
function resetBroadcastPresentation() {
    broadcastDirector.reset();
    broadcastOverlay?.reset();
}
```

Replace the existing debug export with:

```js
window.__arenaTv = { director: broadcastDirector, reset: resetBroadcastPresentation };
```

Call `resetBroadcastPresentation();` immediately after every `broadcastLastLeaderTeam = null;` in `clearGameStateForReconnect` and `resetConnectionUI`.

- [ ] **Step 4: Coordinate cache versions**

Use `20261104-spotlight1` in all three places:

```js
export * from './BroadcastDirector.js?v=20261104-spotlight1';
export * from './BroadcastOverlay.js?v=20261104-spotlight1';
```

```js
} from './client_logic/index.js?v=20261104-spotlight1';
```

- [ ] **Step 5: Run static and live verification**

Run: `cd static_client && npm run test:ui`
Expected: complete UI suite PASS.

Run: `cd .. && node --check scripts/e2e/arena-tv-client-check.cjs && node scripts/e2e/arena-tv-client-check.cjs`
Expected: real target metadata and spotlight agree; reset probe PASS; no browser/console errors; screenshot at `/tmp/arena-tv-client.png`.

Run: `ARENA_TV_REQUIRE_LIVE=1 node scripts/e2e/arena-tv-check.cjs && node scripts/e2e/viewer-to-player-check.cjs`
Expected: live/fallback/opt-out Arena TV PASS; mobile/desktop viewer-to-player PASS.

- [ ] **Step 6: Commit Task 3**

```bash
git add static_client/client.html static_client/client_logic/index.js scripts/e2e/arena-tv-client-check.cjs
git commit -m "feat(arena): connect spotlight to live camera"
```

## Final verification and rollout

- Inspect `/tmp/arena-tv-client.png` and `/tmp/arena-tv-landing.png` for overlap at desktop and landing-iframe sizes.
- Confirm `curl -fsS https://space.selfware.design/healthz` reports `"ok":true` and `"match_degraded":false`.
- Review the scoped diff; do not stage generated media/model-page churn or unrelated `tmp-*` probes.
- Push the reviewed commits to `main` and rerun the live Arena TV client probe against production.
