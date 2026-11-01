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

function setupInput(t, { mobile = false, notifyAimInput }) {
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
    const mobileControlsDiv = mobile ? {
        classList: makeClassList(),
        style: { setProperty() {} },
    } : null;
    const inputState = { rotation: 0 };
    const manager = createInputManager({
        PIXI: { Point: class { constructor(x, y) { this.x = x; this.y = y; } } },
        GP: {},
        clamp: (value, min, max) => Math.max(min, Math.min(max, value)),
        normalizeAngle: (value) => value,
        localStorage,
        getApp: () => ({ view }),
        getGameScene: () => ({ toLocal: (point) => ({ x: point.x, y: point.y }) }),
        getLocalPlayerState: () => ({ x: 0, y: 0, render_x: 0, render_y: 0 }),
        getLocalPlayerSprite: () => ({}),
        getInputState: () => inputState,
        getGameSettings: () => ({ mobileAutoFireAim: false }),
        getMouseWorldPos: () => ({}),
        mobileControlsDiv,
        mobileAimArea,
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
    return { view, mobileAimArea };
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
