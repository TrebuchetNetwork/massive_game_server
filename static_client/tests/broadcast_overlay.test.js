import test from 'node:test';
import assert from 'node:assert/strict';
import { pruneKillFeed, formatClock, formatTeamScores, pushKillEntry } from '../client_logic/BroadcastOverlay.js';

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
