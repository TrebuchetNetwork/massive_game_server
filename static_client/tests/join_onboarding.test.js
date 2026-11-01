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

test('old, unrelated, and other-owner projectiles do not complete Fire', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterFire(ob);
    const existing = [
        { id: 'old-owned', owner_id: 'me' },
        { id: 'old-other', owner_id: 'other' },
    ];

    ob.onProjectiles(existing, 'me');
    ob.onProjectiles(existing, 'me');
    ob.onProjectiles([{ id: 'new-other', owner_id: 'other' }], 'me');

    assert.equal(ob.view().step, 'fire');
});

test('bind re-primes projectiles before another owned shot can complete Fire', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterFire(ob);
    ob.onProjectiles([], 'me');

    ob.bind({ x: 12, y: 0, rotation: 0.08 });
    ob.onProjectiles([{ id: 'existing', owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'fire');

    ob.onProjectiles([{ id: 'new-shot', owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'done');
});

test('a new owned projectile completes Done and persists with string-safe owner matching', () => {
    const storage = createStorage();
    const ob = createJoinOnboarding({ storage });
    enterFire(ob);
    ob.onProjectiles([], 42);

    ob.onProjectiles([{ id: 'new-shot', owner_id: '42' }], 42);

    assert.deepEqual(ob.view(), { step: 'done', visible: false, hint: '' });
    assert.equal(storage.getItem(STORAGE_KEY), 'done');
});

test('Done survives factory recreation with the same storage', () => {
    const storage = createStorage();
    const first = createJoinOnboarding({ storage });
    enterFire(first);
    first.onProjectiles([], 'me');
    first.onProjectiles([{ id: 'new-shot', owner_id: 'me' }], 'me');

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

    ob.onReconnect({ x: 50, y: 60, rotation: 4 });
    ob.onProjectiles([{ id: 'existing', owner_id: 'me' }], 'me');
    ob.onProjectiles([{ id: 'existing', owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'fire');

    ob.onProjectiles([{ id: 'new-shot', owner_id: 'me' }], 'me');
    assert.equal(ob.view().step, 'done');
});

test('projectile history retains only the newest 512 ids', () => {
    const ob = createJoinOnboarding({ storage: createStorage() });
    enterFire(ob);
    ob.onProjectiles([], 'me');
    ob.onProjectiles([{ id: 'evicted', owner_id: 'other' }], 'me');
    const fillers = Array.from({ length: 512 }, (_, index) => ({
        id: `filler-${index}`,
        owner_id: 'other',
    }));
    ob.onProjectiles(fillers, 'me');

    ob.onProjectiles([{ id: 'evicted', owner_id: 'me' }], 'me');

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
        ob.onProjectiles([{ id: 'new-shot', owner_id: 'me' }], 'me');
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
    ob.onProjectiles([{ id: 'new-shot', owner_id: 'me' }], 'me');

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
    ob.onProjectiles([{ id: 'new-shot', owner_id: 'me' }], 'me');

    assert.equal(storage.getItem('custom-onboarding'), 'done');
    assert.equal(storage.getItem(STORAGE_KEY), null);
    assert.equal(createJoinOnboarding({
        storage,
        storageKey: 'custom-onboarding',
    }).view().step, 'done');
});
