import test from 'node:test';
import assert from 'node:assert/strict';
import { createBroadcastDirector } from '../client_logic/BroadcastDirector.js';

const P = (id, over = {}) => ({ id, x: 0, y: 0, hp: 100, maxHp: 100, teamId: 1, username: id, ...over });

test('quiet arena follows the leader', () => {
    const d = createBroadcastDirector();
    const frame = d.cameraFrame([P('a'), P('leader', { x: 50 })], 'leader', 1000);
    assert.equal(frame.targetId, 'leader');
    assert.equal(frame.focusX, 50);
});

test('recent damage outweighs the leader bonus', () => {
    const d = createBroadcastDirector();
    d.recordDamage({ instigatorId: 'a', targetId: 'b', value: 40, atMs: 900 });
    const frame = d.cameraFrame([P('a', { x: 10 }), P('leader', { x: 50 })], 'leader', 1000);
    assert.equal(frame.targetId, 'a');
});

test('dwell: no switch inside 4s even with a better challenger', () => {
    const d = createBroadcastDirector();
    const players = [P('a'), P('b', { x: 99 })];
    assert.equal(d.cameraFrame(players, 'a', 1000).targetId, 'a');
    d.recordDamage({ instigatorId: 'b', targetId: 'a', value: 500, atMs: 1500 });
    assert.equal(d.cameraFrame(players, 'a', 2000).targetId, 'a');
});

test('switch after dwell + sustained margin', () => {
    const d = createBroadcastDirector();
    const players = [P('a'), P('b', { x: 99 })];
    d.cameraFrame(players, 'a', 1000);
    d.recordDamage({ instigatorId: 'b', targetId: 'a', value: 500, atMs: 5100 });
    assert.equal(d.cameraFrame(players, 'a', 5100).targetId, 'a');
    assert.equal(d.cameraFrame(players, 'a', 6700).targetId, 'b');
});

test('kill on the current target triggers a 1.2s zoom', () => {
    const d = createBroadcastDirector();
    const players = [P('a'), P('b')];
    d.cameraFrame(players, 'a', 1000);
    d.recordKill({ killerName: 'a', victimName: 'b', atMs: 5000 });
    assert.equal(d.cameraFrame(players, 'a', 5200).zoom, 1.15);
    assert.equal(d.cameraFrame(players, 'a', 6300).zoom, 1);
});

test('empty player list yields null frame', () => {
    const d = createBroadcastDirector();
    assert.equal(d.cameraFrame([], null, 1000), null);
});
