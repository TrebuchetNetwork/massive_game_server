import test from 'node:test';
import assert from 'node:assert/strict';

import { collectBroadcastPlayers, createBroadcastLifecycle } from '../client_logic/broadcast_lifecycle.js';

test('broadcast candidates exclude spectators, dead players, and invalid coordinates', () => {
    const players = new Map([
        ['spectator', { id: 'spectator', x: 1, y: 2, alive: true, team_id: 0, username: 'Viewer' }],
        ['dead', { id: 'dead', x: 1, y: 2, alive: false, team_id: 1, username: 'Dead' }],
        ['invalid', { id: 'invalid', x: NaN, y: 2, alive: true, team_id: 1, username: 'Invalid' }],
        ['leader', { id: 'leader', x: 10, y: 20, alive: true, team_id: 2, health: 75, max_health: 100, username: '#1 Champion' }],
    ]);

    const result = collectBroadcastPlayers(players);

    assert.equal(result.leaderId, 'leader');
    assert.deepEqual(result.players, [{
        id: 'leader', x: 10, y: 20, hp: 75, maxHp: 100, teamId: 2, username: '#1 Champion',
    }]);
});

test('empty snapshots never announce live and fall back as exhibition idle', () => {
    const statuses = [];
    const lifecycle = createBroadcastLifecycle({ onStatus: (status, reason) => statuses.push({ status, reason }) });

    lifecycle.onSnapshot(false, 100);
    assert.equal(lifecycle.hasSnapshots(), true);
    lifecycle.check(10099);
    assert.deepEqual(statuses, []);
    lifecycle.check(10100);
    assert.deepEqual(statuses, [{ status: 'fallback', reason: 'exhibition_idle' }]);
    lifecycle.check(20000);
    assert.equal(statuses.length, 1, 'idle fallback is emitted once per empty period');

    lifecycle.onSnapshot(true, 20001);
    assert.deepEqual(statuses.at(-1), { status: 'live', reason: null });
});

test('a live broadcast falls back after an idle roster window and recovers', () => {
    const statuses = [];
    const lifecycle = createBroadcastLifecycle({ onStatus: (status, reason) => statuses.push({ status, reason }) });

    lifecycle.onSnapshot(true, 0);
    lifecycle.onSnapshot(false, 1000);
    lifecycle.check(10999);
    assert.deepEqual(statuses, [{ status: 'live', reason: null }]);
    lifecycle.check(11000);
    assert.deepEqual(statuses.at(-1), { status: 'fallback', reason: 'exhibition_idle' });
    lifecycle.onSnapshot(true, 11001);
    assert.deepEqual(statuses.at(-1), { status: 'live', reason: null });
});

test('stale snapshots and external fallbacks both allow later live recovery', () => {
    const statuses = [];
    const lifecycle = createBroadcastLifecycle({ onStatus: (status, reason) => statuses.push({ status, reason }) });

    lifecycle.onSnapshot(true, 0);
    lifecycle.check(11999);
    assert.equal(statuses.length, 1);
    lifecycle.check(12000);
    assert.deepEqual(statuses.at(-1), { status: 'fallback', reason: 'stale' });
    lifecycle.check(24000);
    assert.equal(statuses.length, 2, 'stale fallback does not repeat while offline');

    lifecycle.onSnapshot(true, 24001);
    assert.deepEqual(statuses.at(-1), { status: 'live', reason: null });
    lifecycle.markFallback();
    lifecycle.onSnapshot(true, 24002);
    assert.deepEqual(statuses.at(-1), { status: 'live', reason: null });

    lifecycle.reset();
    assert.equal(lifecycle.hasSnapshots(), false);
});
