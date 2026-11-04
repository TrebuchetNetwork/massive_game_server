import test from 'node:test';
import assert from 'node:assert/strict';
import { createBroadcastOverlay, pruneKillFeed, formatClock, formatTeamScores, pushKillEntry } from '../client_logic/BroadcastOverlay.js';
import * as BroadcastOverlay from '../client_logic/BroadcastOverlay.js';

test('pushKillEntry keeps newest 5, 8s TTL, newest first', () => {
    let feed = [];
    for (let i = 0; i < 7; i += 1) {
        feed = pushKillEntry(feed, { killerName: `k${i}`, victimName: `v${i}`, weapon: 'rifle', atMs: i * 100 });
    }
    assert.equal(feed.length, 5);
    assert.equal(feed[0].killerName, 'k6');
    feed = pruneKillFeed(feed, 50000);
    assert.equal(feed.length, 0);
});

test('pruneKillFeed drops only expired entries', () => {
    const feed = [
        { killerName: 'new', atMs: 7900 },
        { killerName: 'old', atMs: -100 },
    ];
    const out = pruneKillFeed(feed, 8000);
    assert.deepEqual(out.map((e) => e.killerName), ['new']);
});

test('formatClock renders m:ss and handles null', () => {
    assert.equal(formatClock(125), '2:05');
    assert.equal(formatClock(5), '0:05');
    assert.equal(formatClock(null), '--:--');
    assert.equal(formatClock(-3), '0:00');
});

test('formatTeamScores renders live scoreboard team/score objects', () => {
    assert.equal(formatTeamScores([{ team_id: 1, score: 12 }, { team_id: 2, score: 9 }]), 'T1 12 · T2 9');
    assert.equal(formatTeamScores([4, 7]), 'T1 4 · T2 7');
});

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
    overlay.setFocus({ targetId: 'p2', targetName: '  #2 Astra  ' });
    assert.equal(name.textContent, '  #2 Astra  ');
    overlay.setFocus({ targetId: 'p3', targetName: '   ' });
    assert.equal(card.hidden, false);
    assert.equal(name.textContent, 'p3');
    overlay.setFocus({ targetId: '  ', targetName: '\t' });
    assert.equal(card.hidden, true);
});

test('focus rejects non-number team and health metadata', () => {
    const { overlay, root } = makeOverlay();
    const meta = root.querySelector('.arena-tv-focus-meta');
    const invalidFrames = [
        { targetTeamId: true },
        { targetTeamId: '2' },
        { targetHp: null, targetMaxHp: 100 },
        { targetHp: '', targetMaxHp: 100 },
        { targetHp: false, targetMaxHp: 100 },
        { targetHp: 50, targetMaxHp: '100' },
        { targetHp: 50, targetMaxHp: true },
    ];
    invalidFrames.forEach((frame, index) => {
        overlay.setFocus({ ...frame, targetId: `p${index}` });
        assert.equal(meta.textContent, '');
        assert.equal(meta.hidden, true);
    });
});

test('commentary expires and reset clears all rendered broadcast state', () => {
    const { overlay, root } = makeOverlay();
    assert.equal(BroadcastOverlay.COMMENTARY_TTL_MS, 7000);
    overlay.setCommentary('Real streak', 1000);
    overlay.tick(1000 + 7000 - 1);
    assert.equal(root.querySelector('.arena-tv-commentary').textContent, 'Real streak');
    overlay.tick(1000 + 7000);
    assert.equal(root.querySelector('.arena-tv-commentary').textContent, '');
    const focus = { targetId: 'p1', targetName: 'Old fighter', targetTeamId: 1 };
    overlay.setCommentary('Old session', 2000);
    overlay.setFocus(focus);
    overlay.pushKill({ killerName: 'A', victimName: 'B', atMs: 2000 });
    overlay.setScoreboard({ teamScores: [{ team_id: 1, score: 12 }], timeRemaining: 65 });
    assert.equal(root.querySelector('.arena-tv-scores').textContent, 'T1 12');
    assert.equal(root.querySelector('.arena-tv-clock').textContent, '1:05');
    overlay.reset();
    assert.equal(root.querySelector('.arena-tv-commentary').textContent, '');
    assert.equal(root.querySelector('.arena-tv-focus').hidden, true);
    assert.equal(root.querySelector('.arena-tv-focus-name').textContent, '');
    assert.equal(root.querySelector('.arena-tv-focus-meta').textContent, '');
    assert.equal(root.querySelector('.arena-tv-killfeed').innerHTML, '');
    assert.equal(root.querySelector('.arena-tv-scores').textContent, '');
    assert.equal(root.querySelector('.arena-tv-clock').textContent, '--:--');
    const commentaryWrites = root.querySelector('.arena-tv-commentary').textWrites;
    overlay.tick(9000);
    assert.equal(root.querySelector('.arena-tv-commentary').textWrites, commentaryWrites);
    overlay.setFocus(focus);
    assert.equal(root.querySelector('.arena-tv-focus').hidden, false);
    assert.equal(root.querySelector('.arena-tv-focus-name').textContent, 'Old fighter');
});
