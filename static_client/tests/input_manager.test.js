import test from "node:test";
import assert from "node:assert/strict";

import { createInputManager } from "../client_logic/InputManager.js";

function makeEventTarget() {
    const listeners = new Map();
    return {
        addEventListener(type, handler) {
            if (!listeners.has(type)) listeners.set(type, new Set());
            listeners.get(type).add(handler);
        },
        removeEventListener(type, handler) {
            listeners.get(type)?.delete(handler);
        },
        dispatch(type, event = {}) {
            for (const handler of [...(listeners.get(type) || [])]) {
                handler({ preventDefault() {}, ...event });
            }
        },
    };
}

function makeClassList() {
    const values = new Set();
    return {
        add: (...names) => names.forEach((name) => values.add(name)),
        remove: (...names) => names.forEach((name) => values.delete(name)),
        contains: (name) => values.has(name),
        toggle(name, force) {
            const enabled = force === undefined ? !values.has(name) : !!force;
            if (enabled) values.add(name); else values.delete(name);
            return enabled;
        },
    };
}

function setupInput(t, { mobile = false, mobileAutoFireAim = false, mobileStickyFire = false, notifyAimInput } = {}) {
    const previous = {
        document: globalThis.document,
        window: globalThis.window,
        localStorage: globalThis.localStorage,
    };
    const documentTarget = makeEventTarget();
    documentTarget.body = { classList: makeClassList() };
    documentTarget.activeElement = null;
    documentTarget.getElementById = () => null;
    const windowTarget = makeEventTarget();
    windowTarget.innerWidth = mobile ? 390 : 1440;
    windowTarget.innerHeight = mobile ? 844 : 900;
    globalThis.document = documentTarget;
    globalThis.window = windowTarget;
    globalThis.localStorage = { getItem: () => null };

    const view = makeEventTarget();
    view.getBoundingClientRect = () => ({ left: 0, top: 0 });
    const mobileAimArea = mobile ? makeEventTarget() : null;
    const mobileFireButton = mobile ? makeEventTarget() : null;
    if (mobileFireButton) mobileFireButton.classList = makeClassList();
    const mobileControlsDiv = mobile ? {
        classList: makeClassList(),
        style: { setProperty() {} },
    } : null;
    const inputState = { rotation: 0 };
    const manager = createInputManager({
        PIXI: { Point: class { constructor(x, y) { this.x = x; this.y = y; } } },
        GP: { WeaponType: { Melee: 0 } },
        clamp: (value, min, max) => Math.max(min, Math.min(max, value)),
        normalizeAngle: (value) => value,
        localStorage,
        getApp: () => ({ view }),
        getGameScene: () => ({ toLocal: (point) => ({ x: point.x, y: point.y }) }),
        getAudioManager: () => null,
        getLocalPlayerState: () => ({ x: 0, y: 0, render_x: 0, render_y: 0, weapon: 1, ammo: 10 }),
        getLocalPlayerSprite: () => ({}),
        getInputState: () => inputState,
        getGameSettings: () => ({ mobileAutoFireAim, mobileStickyFire }),
        getMouseWorldPos: () => ({}),
        mobileControlsDiv,
        mobileAimArea,
        mobileFireButton,
        triggerHapticFn: () => {},
        settingsMenuDiv: { classList: makeClassList() },
        isTouchDevice: mobile,
        isMobileDevice: mobile,
        forceMobileClient: mobile,
        notifyAimInput,
    });
    manager.setupInputHandlers();
    t.after(() => {
        manager.teardownInputHandlers();
        for (const [key, value] of Object.entries(previous)) {
            if (value === undefined) delete globalThis[key]; else globalThis[key] = value;
        }
    });
    return { view, mobileAimArea, mobileFireButton, inputState };
}

test("desktop mousemove emits an explicit aim gesture", (t) => {
    let notifications = 0;
    const { view } = setupInput(t, { notifyAimInput: () => { notifications += 1; } });

    view.dispatch("mousemove", { clientX: 100, clientY: 50 });

    assert.equal(notifications, 1);
});

test("mobile right-side aim start and move emit explicit aim gestures", (t) => {
    let notifications = 0;
    const { mobileAimArea } = setupInput(t, {
        mobile: true,
        notifyAimInput: () => { notifications += 1; },
    });
    const touch = { identifier: 1, clientX: 300, clientY: 400 };

    mobileAimArea.dispatch("touchstart", { changedTouches: [touch] });
    mobileAimArea.dispatch("touchmove", { changedTouches: [{ ...touch, clientX: 320 }] });

    assert.equal(notifications, 2);
});

test("desktop releasing the mouse outside the canvas clears firing", (t) => {
    const { view, inputState } = setupInput(t);
    view.dispatch("mousedown", { button: 0 });
    assert.equal(inputState.shooting, true);

    document.dispatch("mouseup", { button: 0 });

    assert.equal(inputState.shooting, false);
});

test("desktop losing window focus clears firing", (t) => {
    const { view, inputState } = setupInput(t);
    view.dispatch("mousedown", { button: 0 });
    assert.equal(inputState.shooting, true);

    window.dispatch("blur");

    assert.equal(inputState.shooting, false);
});

test("mobile releasing manual Fire clears firing while aim remains active", (t) => {
    const { mobileAimArea, mobileFireButton, inputState } = setupInput(t, { mobile: true });
    mobileAimArea.dispatch("touchstart", { changedTouches: [{ identifier: 1, clientX: 300, clientY: 400 }] });
    mobileFireButton.dispatch("touchstart", { changedTouches: [{ identifier: 2, clientX: 340, clientY: 700 }] });
    assert.equal(inputState.shooting, true);

    mobileFireButton.dispatch("touchend", { changedTouches: [{ identifier: 2, clientX: 340, clientY: 700 }] });

    assert.equal(inputState.shooting, false);
});

test("mobile auto-fire continues until aim is released", (t) => {
    const { mobileAimArea, mobileFireButton, inputState } = setupInput(t, { mobile: true, mobileAutoFireAim: true });
    mobileAimArea.dispatch("touchstart", { changedTouches: [{ identifier: 1, clientX: 300, clientY: 400 }] });
    mobileFireButton.dispatch("touchstart", { changedTouches: [{ identifier: 2, clientX: 340, clientY: 700 }] });
    mobileFireButton.dispatch("touchend", { changedTouches: [{ identifier: 2, clientX: 340, clientY: 700 }] });
    assert.equal(inputState.shooting, true);

    mobileAimArea.dispatch("touchend", { changedTouches: [{ identifier: 1, clientX: 300, clientY: 400 }] });

    assert.equal(inputState.shooting, false);
});

test("mobile sticky Fire stays latched through a compatibility mouseup", (t) => {
    const { mobileFireButton, inputState } = setupInput(t, { mobile: true, mobileStickyFire: true });
    const touch = { identifier: 2, clientX: 340, clientY: 700 };
    mobileFireButton.dispatch("touchstart", { changedTouches: [touch] });
    mobileFireButton.dispatch("touchend", { changedTouches: [touch] });
    mobileFireButton.dispatch("touchstart", { changedTouches: [touch] });
    mobileFireButton.dispatch("touchend", { changedTouches: [touch] });
    assert.equal(mobileFireButton.textContent, "Fire (Lock)");
    assert.equal(inputState.shooting, true);

    document.dispatch("mouseup", { button: 0 });

    assert.equal(mobileFireButton.textContent, "Fire (Lock)");
    assert.equal(inputState.shooting, true);
});

test("mobile auto-fire remains active through window blur until aim ends", (t) => {
    const { mobileAimArea, inputState } = setupInput(t, { mobile: true, mobileAutoFireAim: true });
    mobileAimArea.dispatch("touchstart", { changedTouches: [{ identifier: 1, clientX: 300, clientY: 400 }] });
    assert.equal(inputState.shooting, true);

    window.dispatch("blur");

    assert.equal(inputState.shooting, true);
});
