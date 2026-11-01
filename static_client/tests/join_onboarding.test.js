import test from 'node:test';
import assert from 'node:assert/strict';

import { createJoinOnboarding } from '../client_logic/JoinOnboarding.js';

const STORAGE_KEY = 'mgs_join_onboarding_v1';

function createStorage(initialValues = {}) {
    const values = new Map(Object.entries(initialValues));
    return {
        values,
        getItem(key) {
            return values.has(key) ? values.get(key) : null;
        },
        setItem(key, value) {
            values.set(key, String(value));
        },
        removeItem(key) {
            values.delete(key);
        },
    };
}

function enterAim(ob) {
    ob.bind({ x: 0, y: 0, rotation: 0 });
    ob.onLocalState({ x: 12, y: 0, rotation: 0 });
}

function enterFire(ob) {
    enterAim(ob);
    ob.notifyAimInput();
    ob.onLocalState({ x: 12, y: 0, rotation: 0.08 });
}

test('initial view is Move with the exact hint', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });

    assert.deepEqual(ob.view(), {
        step: 'move',
        visible: true,
        hint: 'Move: left thumb / WASD',
    });
});

test('only stored done starts in the hidden Done state', () => {
    const done = createJoinOnboarding({
        storage: createStorage({ [STORAGE_KEY]: 'done' }),
    });
    const otherValue = createJoinOnboarding({
        storage: createStorage({ [STORAGE_KEY]: 'yes' }),
    });

    assert.deepEqual(done.view(), { step: 'done', visible: false, hint: '' });
    assert.equal(otherValue.view().step, 'move');
});

test('input and projectiles alone do not advance Move without server movement', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    ob.bind({ x: 10, y: 20, rotation: 1 });

    ob.notifyAimInput();
    ob.onLocalState({ x: 10, y: 20, rotation: 1.5 });
    ob.onProjectiles([{ id: 'shot', owner_id: 'me' }], 'me');

    assert.equal(ob.view().step, 'move');
});

test('movement below 12 units does not advance', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    ob.bind({ x: 0, y: 0, rotation: 0 });

    ob.onLocalState({ x: 8, y: 8, rotation: 0 });

    assert.equal(ob.view().step, 'move');
});

test('movement at 12 units advances to Aim with the exact hint', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    ob.bind({ x: 0, y: 0, rotation: 0 });

    ob.onLocalState({ x: 12, y: 0, rotation: 0 });

    assert.deepEqual(ob.view(), {
        step: 'aim',
        visible: true,
        hint: 'Aim: drag right side / mouse',
    });
});

test('custom movement and aim thresholds are honored', () => {
    const ob = createJoinOnboarding({
        storage: createStorage(),
        minMoveDistance: 20,
        minAimDelta: 0.2,
    });
    ob.bind({ x: 0, y: 0, rotation: 0 });

    ob.onLocalState({ x: 12, y: 0, rotation: 0 });
    assert.equal(ob.view().step, 'move');
    ob.onLocalState({ x: 20, y: 0, rotation: 0 });
    assert.equal(ob.view().step, 'aim');

    ob.notifyAimInput();
    ob.onLocalState({ x: 20, y: 0, rotation: 0.08 });
    assert.equal(ob.view().step, 'aim');
    ob.onLocalState({ x: 20, y: 0, rotation: 0.2 });
    assert.equal(ob.view().step, 'fire');
});

test('aim gesture without rotation delta does not advance', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    ob.bind({ x: 0, y: 0, rotation: 1 });
    ob.onLocalState({ x: 12, y: 0, rotation: 1.5 });

    ob.notifyAimInput();
    ob.onLocalState({ x: 12, y: 0, rotation: 1.5 });

    assert.equal(ob.view().step, 'aim');
});

test('rotation delta without a new aim gesture does not advance', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterAim(ob);

    ob.onLocalState({ x: 12, y: 0, rotation: 0.5 });

    assert.equal(ob.view().step, 'aim');
});

test('aim gesture plus rotation delta advances to Fire with the exact hint', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    ob.bind({ x: 0, y: 0, rotation: 1 });
    ob.onLocalState({ x: 12, y: 0, rotation: 1.5 });

    ob.notifyAimInput();
    ob.onLocalState({ x: 12, y: 0, rotation: 1.58 });

    assert.deepEqual(ob.view(), {
        step: 'fire',
        visible: true,
        hint: 'Fire: tap FIRE / click',
    });
});

test('non-finite current coordinates never advance Move', () => {
    for (const value of [NaN, Infinity, -Infinity]) {
        const ob = createJoinOnboarding({ storage: createStorage() });
        ob.bind({ x: 0, y: 0, rotation: 0 });

        ob.onLocalState({ x: value, y: 0, rotation: 0 });
        assert.equal(ob.view().step, 'move', `x=${value}`);
        ob.onLocalState({ x: 0, y: value, rotation: 0 });
        assert.equal(ob.view().step, 'move', `y=${value}`);
    }
});

test('non-finite movement baselines never advance Move', () => {
    for (const value of [NaN, Infinity, -Infinity]) {
        const ob = createJoinOnboarding({ storage: createStorage() });
        ob.bind({ x: value, y: value, rotation: 0 });

        ob.onLocalState({ x: 20, y: 20, rotation: 0 });

        assert.equal(ob.view().step, 'move', `baseline=${value}`);
    }
});

test('non-finite current rotations never advance Aim', () => {
    for (const value of [NaN, Infinity, -Infinity]) {
        const ob = createJoinOnboarding({ storage: createStorage() });
        enterAim(ob);
        ob.notifyAimInput();

        ob.onLocalState({ x: 12, y: 0, rotation: value });

        assert.equal(ob.view().step, 'aim', `rotation=${value}`);
    }
});

test('non-finite rotation baselines never advance Aim', () => {
    for (const value of [NaN, Infinity, -Infinity]) {
        const ob = createJoinOnboarding({ storage: createStorage() });
        enterAim(ob);
        ob.bind({ x: 12, y: 0, rotation: value });
        ob.notifyAimInput();

        ob.onLocalState({ x: 12, y: 0, rotation: 0.5 });

        assert.equal(ob.view().step, 'aim', `baseline=${value}`);
    }
});

test('aim uses shortest angular distance across the PI boundary', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    const baseRotation = Math.PI - 0.02;
    ob.bind({ x: 0, y: 0, rotation: baseRotation });
    ob.onLocalState({ x: 12, y: 0, rotation: baseRotation });
    ob.notifyAimInput();

    ob.onLocalState({ x: 12, y: 0, rotation: -Math.PI + 0.02 });

    assert.equal(ob.view().step, 'aim');
});

test('a sufficient wrapped aim delta advances to Fire', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    const baseRotation = Math.PI - 0.02;
    ob.bind({ x: 0, y: 0, rotation: baseRotation });
    ob.onLocalState({ x: 12, y: 0, rotation: baseRotation });
    ob.notifyAimInput();

    ob.onLocalState({ x: 12, y: 0, rotation: -Math.PI + 0.1 });

    assert.equal(ob.view().step, 'fire');
});

test('old, unrelated, and other-owner projectiles do not complete Fire', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterFire(ob);
    const existing = [
        { id: 100, owner_id: 'me' },
        { id: 101, owner_id: 'other' },
    ];

    ob.onProjectiles(existing, 'me');
    ob.onProjectiles(existing, 'me');
    ob.onProjectiles([{ id: 102, owner_id: 'other' }], 'me');

    assert.equal(ob.view().step, 'fire');
});

test('bind re-primes projectiles before another owned shot can complete Fire', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterFire(ob);
    ob.onProjectiles([], 'me');

    ob.bind({ x: 12, y: 0, rotation: 0.08 });
    ob.onProjectiles([{ id: 100, owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'fire');

    ob.onProjectiles([{ id: 101, owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'done');
});

test('repeated bind clears a pending aim gesture', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterAim(ob);
    ob.notifyAimInput();

    ob.bind({ x: 12, y: 0, rotation: 0 });
    ob.onLocalState({ x: 12, y: 0, rotation: 0.5 });
    assert.equal(ob.view().step, 'aim');

    ob.notifyAimInput();
    ob.onLocalState({ x: 12, y: 0, rotation: 0.58 });
    assert.equal(ob.view().step, 'fire');
});

test('non-finite and non-numeric projectile ids cannot complete Fire', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterFire(ob);
    ob.onProjectiles([], 'me');

    const invalidIds = [
        NaN, Infinity, -Infinity, 'not-a-number',
        null, '', '   ', false, true, [], [1],
    ];
    for (const id of invalidIds) {
        ob.onProjectiles([{ id, owner_id: 'me' }], 'me');
        assert.equal(ob.view().step, 'fire', `id=${id}`);
    }

    ob.onProjectiles([{ id: 1, owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'done');
});

test('a new owned projectile completes Done and persists with string-safe owner matching', () => {
    const storage = createStorage();
    const ob = createJoinOnboarding({ storage });
    enterFire(ob);
    ob.onProjectiles([], 42);

    ob.onProjectiles([{ id: '101', owner_id: '42' }], 42);

    assert.deepEqual(ob.view(), { step: 'done', visible: false, hint: '' });
    assert.equal(storage.getItem(STORAGE_KEY), 'done');
});

test('Done survives factory recreation with the same storage', () => {
    const storage = createStorage();
    const first = createJoinOnboarding({ storage });
    enterFire(first);
    first.onProjectiles([], 'me');
    first.onProjectiles([{ id: 101, owner_id: 'me' }], 'me');

    const recreated = createJoinOnboarding({ storage });

    assert.deepEqual(recreated.view(), { step: 'done', visible: false, hint: '' });
});

test('a death-like snapshot does not reset onboarding progress', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterFire(ob);

    ob.onLocalState({ x: -500, y: 900, rotation: 20 });

    assert.equal(ob.view().step, 'fire');
});

test('reconnect preserves Move and rebases the teleport position', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    ob.bind({ x: 0, y: 0, rotation: 0 });

    ob.onReconnect({ x: 100, y: 200, rotation: 3 });
    ob.onLocalState({ x: 100, y: 200, rotation: 3 });
    assert.equal(ob.view().step, 'move');

    ob.onLocalState({ x: 111.9, y: 200, rotation: 3 });
    assert.equal(ob.view().step, 'move');
    ob.onLocalState({ x: 112, y: 200, rotation: 3 });
    assert.equal(ob.view().step, 'aim');
});

test('reconnect preserves Aim, clears its gesture, and rebases rotation', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterAim(ob);
    ob.notifyAimInput();

    ob.onReconnect({ x: 50, y: 60, rotation: 4 });
    ob.onLocalState({ x: 50, y: 60, rotation: 4.5 });
    assert.equal(ob.view().step, 'aim');

    ob.notifyAimInput();
    ob.onLocalState({ x: 50, y: 60, rotation: 4.07 });
    assert.equal(ob.view().step, 'aim');
    ob.onLocalState({ x: 50, y: 60, rotation: 4.08 });
    assert.equal(ob.view().step, 'fire');
});

test('reconnect preserves Fire and re-primes existing projectiles', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterFire(ob);

    ob.onReconnect({ x: 50, y: 60, rotation: 4 }, 'me');
    ob.onProjectiles([{ id: 100, owner_id: 'me' }], 'me');
    ob.onProjectiles([{ id: 100, owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'fire');

    ob.onProjectiles([{ id: 101, owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'done');
});

test('reconnect preserves the projectile threshold across empty and stale snapshots', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterFire(ob);
    ob.onProjectiles([{ id: 100, owner_id: 'me' }], 'me');

    ob.onReconnect({ x: 50, y: 60, rotation: 4 }, 'me');
    ob.onProjectiles([], 'me');
    ob.onProjectiles([{ id: 100, owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'fire');
    ob.onProjectiles([{ id: 99, owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'fire');

    ob.onProjectiles([{ id: 101, owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'done');
});

test('partial reconnect priming cannot lower the projectile threshold', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterFire(ob);
    ob.onProjectiles([{ id: 100, owner_id: 'other' }], 'me');

    ob.onReconnect({ x: 50, y: 60, rotation: 4 }, 'me');
    ob.onProjectiles([{ id: 50, owner_id: 'other' }], 'me');
    ob.onProjectiles([{ id: 75, owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'fire');

    ob.onProjectiles([{ id: 101, owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'done');
});

test('new join identity resets a stale high projectile threshold and accepts newer low ids', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterFire(ob);
    ob.onProjectiles([{ id: 1000, owner_id: 'old-player' }], 'old-player');

    ob.onReconnect({ x: 50, y: 60, rotation: 4 }, 'new-player');
    ob.onProjectiles([{ id: 2, owner_id: 'new-player' }], 'new-player');
    assert.equal(ob.view().step, 'fire');

    ob.onProjectiles([
        { id: 2, owner_id: 'new-player' },
        { id: 3, owner_id: 'new-player' },
    ], 'new-player');
    assert.equal(ob.view().step, 'done');
});

test('same join identity preserves the threshold and rejects old projectile ids', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterFire(ob);
    ob.onProjectiles([{ id: 1000, owner_id: 'me' }], 'me');

    ob.onReconnect({ x: 50, y: 60, rotation: 4 }, 'me');
    ob.onProjectiles([{ id: 1000, owner_id: 'me' }], 'me');
    ob.onProjectiles([
        { id: 999, owner_id: 'me' },
        { id: 1000, owner_id: 'me' },
    ], 'me');

    assert.equal(ob.view().step, 'fire');
});

test('evicted old projectile ids stay ineligible and a newer owned id completes', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterFire(ob);
    ob.onProjectiles([{ id: 1, owner_id: 'me' }], 'me');
    const fillers = Array.from({ length: 512 }, (_, index) => ({
        id: index + 2,
        owner_id: 'other',
    }));
    ob.onProjectiles(fillers, 'me');

    ob.onProjectiles([{ id: 1, owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'fire');

    ob.onProjectiles([{ id: 514, owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'done');
});

test('throwing storage is safe for reads, writes, and reset', () => {
    const storage = {
        getItem() { throw new Error('denied'); },
        setItem() { throw new Error('denied'); },
        removeItem() { throw new Error('denied'); },
    };
    let ob;
    assert.doesNotThrow(() => {
        ob = createJoinOnboarding({ storage });
    });
    enterFire(ob);
    ob.onProjectiles([], 'me');

    assert.doesNotThrow(() => {
        ob.onProjectiles([{ id: 101, owner_id: 'me' }], 'me');
    });
    assert.equal(ob.view().step, 'done');
    assert.doesNotThrow(() => ob.reset());
    assert.equal(ob.view().step, 'move');
});

test('reset removes persisted Done and clears session baselines', () => {
    const storage = createStorage();
    const ob = createJoinOnboarding({ storage });
    enterFire(ob);
    ob.onProjectiles([], 'me');
    ob.onProjectiles([{ id: 101, owner_id: 'me' }], 'me');

    ob.reset();

    assert.deepEqual(ob.view(), {
        step: 'move',
        visible: true,
        hint: 'Move: left thumb / WASD',
    });
    assert.equal(storage.getItem(STORAGE_KEY), null);
    ob.onLocalState({ x: 1000, y: 1000, rotation: 10 });
    assert.equal(ob.view().step, 'move');
    assert.equal(createJoinOnboarding({ storage }).view().step, 'move');
});

test('reset falls back to setting an empty value without removeItem', () => {
    const values = new Map([[STORAGE_KEY, 'done']]);
    const storage = {
        getItem(key) { return values.has(key) ? values.get(key) : null; },
        setItem(key, value) { values.set(key, String(value)); },
    };
    const ob = createJoinOnboarding({ storage });
    assert.equal(ob.view().step, 'done');

    ob.reset();

    assert.equal(values.get(STORAGE_KEY), '');
    assert.equal(ob.view().step, 'move');
});

test('a custom storage key is used for persistence', () => {
    const storage = createStorage();
    const ob = createJoinOnboarding({ storage, storageKey: 'custom-onboarding' });
    enterFire(ob);
    ob.onProjectiles([], 'me');
    ob.onProjectiles([{ id: 101, owner_id: 'me' }], 'me');

    assert.equal(storage.getItem('custom-onboarding'), 'done');
    assert.equal(storage.getItem(STORAGE_KEY), null);
    assert.equal(createJoinOnboarding({
        storage,
        storageKey: 'custom-onboarding',
    }).view().step, 'done');
});

test('death suppresses Move and the next alive snapshot rebases before movement', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    ob.bind({ x: 0, y: 0, rotation: 0, alive: true });

    ob.onLocalState({ x: 500, y: 500, rotation: 2, alive: false });
    assert.equal(ob.view().step, 'move');

    ob.onLocalState({ x: 500, y: 500, rotation: 2, alive: true });
    assert.equal(ob.view().step, 'move');
    ob.onLocalState({ x: 511.9, y: 500, rotation: 2, alive: true });
    assert.equal(ob.view().step, 'move');
    ob.onLocalState({ x: 512, y: 500, rotation: 2, alive: true });
    assert.equal(ob.view().step, 'aim');
});

test('death suppresses Aim and respawn rebases rotation and clears the gesture', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterAim(ob);
    ob.notifyAimInput();

    ob.onLocalState({ x: 50, y: 60, rotation: 4, alive: false });
    ob.onLocalState({ x: 50, y: 60, rotation: 4.5, alive: true });
    assert.equal(ob.view().step, 'aim');

    ob.notifyAimInput();
    ob.onLocalState({ x: 50, y: 60, rotation: 4.57, alive: true });
    assert.equal(ob.view().step, 'aim');
    ob.onLocalState({ x: 50, y: 60, rotation: 4.58, alive: true });
    assert.equal(ob.view().step, 'fire');
});

test('binding while dead rebases before the first alive movement snapshot', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    ob.bind({ x: 0, y: 0, rotation: 0, alive: false });

    ob.onLocalState({ x: 500, y: 500, rotation: 2, alive: true });
    assert.equal(ob.view().step, 'move');
    ob.onLocalState({ x: 511.9, y: 500, rotation: 2, alive: true });
    assert.equal(ob.view().step, 'move');
    ob.onLocalState({ x: 512, y: 500, rotation: 2, alive: true });
    assert.equal(ob.view().step, 'aim');
});

test('respawn re-primes existing projectiles without resetting Fire', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterFire(ob);
    ob.onProjectiles([], 'me');

    ob.onLocalState({ x: 50, y: 60, rotation: 4, alive: false });
    ob.onLocalState({ x: 500, y: 600, rotation: 2, alive: true });
    ob.onProjectiles([{ id: 100, owner_id: 'me' }], 'me');
    ob.onProjectiles([{ id: 100, owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'fire');

    ob.onProjectiles([{ id: 101, owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'done');
});
