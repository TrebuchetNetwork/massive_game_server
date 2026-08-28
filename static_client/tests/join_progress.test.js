import test from "node:test";
import assert from "node:assert/strict";

import {
    JOIN_PROGRESS_STAGES,
    CONTROL_HINTS_STORAGE_KEY,
    createJoinProgressTracker,
    applyJoinProgressUi,
    getControlHintItems,
    shouldShowControlHints,
    markControlHintsSeen,
} from "../client_logic/join_progress.js";

function createClassList(initial = []) {
    const values = new Set(initial);
    return {
        add(...classes) { classes.forEach((c) => values.add(c)); },
        remove(...classes) { classes.forEach((c) => values.delete(c)); },
        toggle(className, force) {
            const shouldAdd = force === undefined ? !values.has(className) : !!force;
            if (shouldAdd) values.add(className); else values.delete(className);
        },
        contains(className) { return values.has(className); },
    };
}

function createElement(initialClasses = []) {
    return {
        classList: createClassList(initialClasses),
        textContent: "",
        attributes: {},
        setAttribute(name, value) { this.attributes[name] = String(value); },
    };
}

function createUi() {
    return {
        overlay: createElement(["join-progress", "hidden"]),
        detailElement: createElement(),
        retryButton: createElement(["hidden"]),
        stageElements: {
            connecting: createElement(),
            negotiating: createElement(),
            spawning: createElement(),
        },
    };
}

test("tracker walks connecting -> negotiating -> spawning -> done", () => {
    const tracker = createJoinProgressTracker();

    let snap = tracker.handleStatus("connecting", "Contacting signaling server...");
    assert.equal(snap.phase, "joining");
    assert.equal(snap.currentStage, "connecting");

    snap = tracker.handleStatus("negotiating", "Establishing peer connection...");
    assert.equal(snap.phase, "joining");
    assert.equal(snap.currentStage, "negotiating");

    snap = tracker.handleStatus("waiting", "Waiting for initial state...");
    assert.equal(snap.phase, "joining");
    assert.equal(snap.currentStage, "spawning");

    snap = tracker.handleStatus("playing", "");
    assert.equal(snap.phase, "done");
    assert.equal(snap.currentStage, "spawning");
});

test("tracker completes on respawn status too (player state known)", () => {
    const tracker = createJoinProgressTracker();
    tracker.handleStatus("connecting", "");
    tracker.handleStatus("negotiating", "");
    const snap = tracker.handleStatus("respawn", "Respawning...");
    assert.equal(snap.phase, "done");
});

test("tracker enters error phase only while a join is in flight", () => {
    const tracker = createJoinProgressTracker();
    // Error before any join attempt is ignored.
    let snap = tracker.handleStatus("error", "boom");
    assert.equal(snap.phase, "idle");

    tracker.handleStatus("connecting", "");
    snap = tracker.handleStatus("error", "Connection timed out after 15s");
    assert.equal(snap.phase, "error");
    assert.equal(snap.detailText, "Connection timed out after 15s");

    // A retry re-arms the tracker.
    snap = tracker.handleStatus("connecting", "Retrying signaling connection...");
    assert.equal(snap.phase, "joining");
    assert.equal(snap.currentStage, "connecting");
});

test("tracker ignores post-match waiting/playing once done or idle", () => {
    const tracker = createJoinProgressTracker();
    // Post-match 'waiting' (queued for next round) must not arm the overlay.
    let snap = tracker.handleStatus("waiting", "Queued for next round...");
    assert.equal(snap.phase, "idle");

    tracker.handleStatus("connecting", "");
    tracker.handleStatus("playing", "");
    // Further playing/waiting updates keep the done phase without re-arming.
    snap = tracker.handleStatus("waiting", "Queued for next round...");
    assert.equal(snap.phase, "done");
});

test("tracker falls back to per-stage detail when none is provided", () => {
    const tracker = createJoinProgressTracker();
    let snap = tracker.handleStatus("connecting");
    assert.equal(snap.detailText, "Contacting signaling server...");
    snap = tracker.handleStatus("negotiating");
    assert.equal(snap.detailText, "Establishing peer connection...");
    snap = tracker.handleStatus("waiting");
    assert.equal(snap.detailText, "Waiting for match state...");
});

test("applyJoinProgressUi shows overlay with current step during join", () => {
    const tracker = createJoinProgressTracker();
    const ui = createUi();

    tracker.handleStatus("connecting", "Contacting signaling server...");
    tracker.handleStatus("negotiating", "Establishing peer connection...");
    applyJoinProgressUi({ snapshot: tracker.snapshot(), ...ui });

    assert.equal(ui.overlay.classList.contains("hidden"), false);
    assert.equal(ui.overlay.classList.contains("join-progress--error"), false);
    assert.equal(ui.overlay.attributes["aria-hidden"], "false");
    assert.equal(ui.stageElements.connecting.classList.contains("join-progress__step--done"), true);
    assert.equal(ui.stageElements.negotiating.classList.contains("join-progress__step--current"), true);
    assert.equal(ui.stageElements.spawning.classList.contains("join-progress__step--done"), false);
    assert.equal(ui.detailElement.textContent, "Establishing peer connection...");
    assert.equal(ui.retryButton.classList.contains("hidden"), true);
});

test("applyJoinProgressUi shows retry button and error styling on failure", () => {
    const tracker = createJoinProgressTracker();
    const ui = createUi();

    tracker.handleStatus("connecting", "");
    tracker.handleStatus("error", "Signaling closed (unclean, code=1006)");
    applyJoinProgressUi({ snapshot: tracker.snapshot(), ...ui });

    assert.equal(ui.overlay.classList.contains("hidden"), false);
    assert.equal(ui.overlay.classList.contains("join-progress--error"), true);
    assert.equal(ui.stageElements.connecting.classList.contains("join-progress__step--error"), true);
    assert.equal(ui.retryButton.classList.contains("hidden"), false);
    assert.equal(ui.detailElement.textContent, "Signaling closed (unclean, code=1006)");
});

test("applyJoinProgressUi hides overlay when idle or done", () => {
    const tracker = createJoinProgressTracker();
    const ui = createUi();

    applyJoinProgressUi({ snapshot: tracker.snapshot(), ...ui });
    assert.equal(ui.overlay.classList.contains("hidden"), true);
    assert.equal(ui.overlay.attributes["aria-hidden"], "true");

    tracker.handleStatus("connecting", "");
    tracker.handleStatus("playing", "");
    applyJoinProgressUi({ snapshot: tracker.snapshot(), ...ui });
    assert.equal(ui.overlay.classList.contains("hidden"), true);
});

test("applyJoinProgressUi tolerates missing elements", () => {
    const tracker = createJoinProgressTracker();
    tracker.handleStatus("connecting", "");
    assert.doesNotThrow(() => applyJoinProgressUi({ snapshot: tracker.snapshot() }));
    assert.doesNotThrow(() => applyJoinProgressUi({ snapshot: tracker.snapshot(), overlay: null }));
});

test("getControlHintItems reflects desktop bindings from InputManager", () => {
    const items = getControlHintItems(false);
    const keys = items.map((i) => i.key).join("|");
    assert.match(keys, /W A S D/);
    assert.match(keys, /1 \/ 2/);
    assert.match(keys, /Q \/ E/);
    assert.ok(items.length >= 5);
});

test("getControlHintItems reflects mobile touch layout", () => {
    const items = getControlHintItems(true);
    const text = items.map((i) => `${i.key} ${i.action}`).join("|");
    assert.match(text, /stick/i);
    assert.match(text, /aim/i);
    assert.match(text, /FIRE/);
});

test("control hints storage flag gates first-visit display", () => {
    const store = new Map();
    const storageLike = {
        getItem: (k) => (store.has(k) ? store.get(k) : null),
        setItem: (k, v) => store.set(k, v),
    };

    assert.equal(shouldShowControlHints(storageLike), true);
    markControlHintsSeen(storageLike);
    assert.equal(store.get(CONTROL_HINTS_STORAGE_KEY), "1");
    assert.equal(shouldShowControlHints(storageLike), false);
});

test("control hints helpers survive throwing storage (private mode)", () => {
    const throwingStorage = {
        getItem() { throw new Error("denied"); },
        setItem() { throw new Error("denied"); },
    };
    assert.equal(shouldShowControlHints(throwingStorage), true);
    assert.doesNotThrow(() => markControlHintsSeen(throwingStorage));
    assert.equal(shouldShowControlHints(null), true);
    assert.doesNotThrow(() => markControlHintsSeen(null));
});

test("JOIN_PROGRESS_STAGES order is connect, negotiate, spawn", () => {
    assert.deepEqual(JOIN_PROGRESS_STAGES.map((s) => s.key), ["connecting", "negotiating", "spawning"]);
});
