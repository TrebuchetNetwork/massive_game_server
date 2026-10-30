import test from 'node:test';
import assert from 'node:assert/strict';
import { createCommentary } from '../client_logic/commentary_templates.js';

test('streak event produces a line containing name and count', () => {
    const c = createCommentary();
    const line = c.onEvent({ type: 'streak', name: '#1 Opus 5', count: 3 }, 1000);
    assert.ok(line.includes('#1 Opus 5'));
    assert.ok(line.includes('3'));
});

test('per-type cooldown suppresses repeats, other types still flow', () => {
    const c = createCommentary();
    assert.ok(c.onEvent({ type: 'streak', name: 'A', count: 3 }, 1000));
    assert.equal(c.onEvent({ type: 'streak', name: 'B', count: 5 }, 2000), null);
    assert.ok(c.onEvent({ type: 'multi_kill', name: 'B', count: 2 }, 2000));
    assert.ok(c.onEvent({ type: 'streak', name: 'B', count: 5 }, 22000));
});

test('identical lines dedup within 60s even after cooldown', () => {
    const c = createCommentary();
    const first = c.onEvent({ type: 'streak', name: 'A', count: 3 }, 1000);
    assert.ok(first);
    const again = c.onEvent({ type: 'streak', name: 'A', count: 3 }, 30000);
    assert.equal(again, null);
});

test('missing fields never produce undefined/NaN in lines', () => {
    const c = createCommentary();
    assert.equal(c.onEvent({ type: 'streak', count: 3 }, 1000), null);
    assert.equal(c.onEvent({ type: 'streak', name: 'A' }, 1000), null);
    assert.equal(c.onEvent({ type: 'unknown_type', name: 'A' }, 1000), null);
});

test('non-finite numeric fields are rejected', () => {
    const c = createCommentary();
    assert.equal(c.onEvent({ type: 'streak', name: 'A', count: NaN }, 1000), null);
    assert.equal(c.onEvent({ type: 'streak', name: 'A', count: Infinity }, 1000), null);
});

test('pool rotation is deterministic', () => {
    const run = () => {
        const c = createCommentary();
        // Distinct names avoid dedup so all three pool entries are exercised.
        return ['Challenger', 'Rival', 'Contender'].map((name, i) => c.onEvent({ type: 'duel_start', name }, i * 30000));
    };
    const lines = run();
    assert.ok(lines[0]);
    assert.ok(lines[0].includes('Challenger'));
    assert.deepEqual(lines, [
        'Challenger steps up to challenge the champion',
        'A wild challenger appears: Rival',
        'Contender wants the crown',
    ]);
    assert.deepEqual(run(), lines);
});
