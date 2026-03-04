import test from "node:test";
import assert from "node:assert/strict";

import { createReconnectHelpers } from "../client_logic/reconnect_utils.js";

function makeState() {
    return {
        autoReconnectEnabled: true,
        hasAttemptedConnection: true,
        reconnectTimerId: null,
        connectAttemptInFlight: false,
        dataChannel: null,
        signalingSocket: null,
        reconnectAttemptCount: 0,
        autoReconnectMaxAttempts: 3,
        autoReconnectBaseDelayMs: 1000,
        autoReconnectMaxDelayMs: 8000,
        logs: [],
        statuses: [],
        reconnectStarts: [],
    };
}

function makeHelpers(state) {
    return createReconnectHelpers({
        getAutoReconnectEnabled: () => state.autoReconnectEnabled,
        getHasAttemptedConnection: () => state.hasAttemptedConnection,
        getReconnectTimerId: () => state.reconnectTimerId,
        setReconnectTimerId: (value) => {
            state.reconnectTimerId = value;
        },
        getConnectAttemptInFlight: () => state.connectAttemptInFlight,
        getDataChannel: () => state.dataChannel,
        getSignalingSocket: () => state.signalingSocket,
        getReconnectAttemptCount: () => state.reconnectAttemptCount,
        setReconnectAttemptCount: (value) => {
            state.reconnectAttemptCount = value;
        },
        getAutoReconnectMaxAttempts: () => state.autoReconnectMaxAttempts,
        getAutoReconnectBaseDelayMs: () => state.autoReconnectBaseDelayMs,
        getAutoReconnectMaxDelayMs: () => state.autoReconnectMaxDelayMs,
        log: (message, level) => state.logs.push({ message, level }),
        applyConnectionStatus: (statusKey, detail) =>
            state.statuses.push({ statusKey, detail }),
        startConnectionAttempt: (payload) => state.reconnectStarts.push(payload),
        clearGameState: () => {
            state.cleared = true;
        },
    });
}

test.beforeEach(() => {
    Object.defineProperty(globalThis, "WebSocket", {
        value: { CONNECTING: 0, OPEN: 1 },
        configurable: true,
    });
});

test.afterEach(() => {
    delete globalThis.WebSocket;
});

test("canStartConnectionAttempt blocks when current transports are active", () => {
    const state = makeState();
    const helpers = makeHelpers(state);

    assert.equal(helpers.canStartConnectionAttempt(), true);

    state.connectAttemptInFlight = true;
    assert.equal(helpers.canStartConnectionAttempt(), false);
    state.connectAttemptInFlight = false;

    state.dataChannel = { readyState: "open" };
    assert.equal(helpers.canStartConnectionAttempt(), false);
    state.dataChannel = null;

    state.signalingSocket = { readyState: globalThis.WebSocket.CONNECTING };
    assert.equal(helpers.canStartConnectionAttempt(), false);
    state.signalingSocket = { readyState: globalThis.WebSocket.OPEN };
    assert.equal(helpers.canStartConnectionAttempt(), false);
});

test("computeReconnectDelayMs grows with attempt count and applies jitter bounds", () => {
    const state = makeState();
    const helpers = makeHelpers(state);

    const first = helpers.computeReconnectDelayMs(1);
    const third = helpers.computeReconnectDelayMs(3);

    assert.ok(first >= 1000 && first <= 1250);
    assert.ok(third >= 2560 && third <= 3200);
    assert.ok(third >= first);
});

test("scheduleAutoReconnect queues retry and starts connection attempt on timer", () => {
    const state = makeState();
    const helpers = makeHelpers(state);

    const originalSetTimeout = globalThis.setTimeout;
    const originalClearTimeout = globalThis.clearTimeout;
    const timers = new Map();
    let nextTimerId = 1;
    globalThis.setTimeout = (fn, ms) => {
        const id = nextTimerId++;
        timers.set(id, { fn, ms });
        return id;
    };
    globalThis.clearTimeout = (id) => {
        timers.delete(id);
    };

    try {
        const scheduled = helpers.scheduleAutoReconnect("Socket closed");
        assert.equal(scheduled, true);
        assert.equal(state.reconnectAttemptCount, 1);
        assert.ok(state.reconnectTimerId !== null);
        assert.equal(state.statuses.length, 1);
        assert.equal(state.statuses[0].statusKey, "connecting");
        assert.match(state.statuses[0].detail, /Retrying in/);

        const timerEntry = timers.get(state.reconnectTimerId);
        assert.ok(timerEntry);
        assert.ok(timerEntry.ms >= 1000 && timerEntry.ms <= 1250);
        timerEntry.fn();

        assert.equal(state.reconnectTimerId, null);
        assert.deepEqual(state.reconnectStarts, [{ isRetry: true }]);
    } finally {
        globalThis.setTimeout = originalSetTimeout;
        globalThis.clearTimeout = originalClearTimeout;
    }
});

test("scheduleAutoReconnect enforces max attempts and surfaces status", () => {
    const state = makeState();
    state.reconnectAttemptCount = 3;
    state.autoReconnectMaxAttempts = 3;
    const helpers = makeHelpers(state);

    const scheduled = helpers.scheduleAutoReconnect("network");
    assert.equal(scheduled, false);
    assert.equal(state.statuses.length, 1);
    assert.equal(state.statuses[0].statusKey, "error");
    assert.match(state.statuses[0].detail, /Reconnect limit reached/);
    assert.equal(state.logs.length, 1);
});

test("resetReconnectState clears timer bookkeeping and stale game state", () => {
    const state = makeState();
    const helpers = makeHelpers(state);

    state.reconnectAttemptCount = 2;
    state.reconnectTimerId = 77;

    const originalClearTimeout = globalThis.clearTimeout;
    const cleared = [];
    globalThis.clearTimeout = (id) => {
        cleared.push(id);
    };

    try {
        helpers.resetReconnectState();
    } finally {
        globalThis.clearTimeout = originalClearTimeout;
    }

    assert.equal(state.reconnectAttemptCount, 0);
    assert.equal(state.reconnectTimerId, null);
    assert.deepEqual(cleared, [77]);
    assert.equal(state.cleared, true);
});
