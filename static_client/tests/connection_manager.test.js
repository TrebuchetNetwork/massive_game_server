import test from 'node:test';
import assert from 'node:assert/strict';

import { createConnectionManager } from '../client_logic/ConnectionManager.js';

function createClassList() {
    const replacements = [];
    return {
        replacements,
        replace(from, to) {
            replacements.push([from, to]);
        },
        add() {},
        remove() {},
    };
}

function replaceGlobal(t, name, value) {
    const hadOwnValue = Object.prototype.hasOwnProperty.call(globalThis, name);
    const previousValue = globalThis[name];
    globalThis[name] = value;
    t.after(() => {
        if (hadOwnValue) {
            globalThis[name] = previousValue;
        } else {
            delete globalThis[name];
        }
    });
}

function createFakeWebSocketClass() {
    return class FakeWebSocket {
        static CONNECTING = 0;
        static OPEN = 1;
        static CLOSED = 3;
        static instances = [];

        constructor(url) {
            this.url = url;
            this.readyState = this.constructor.CONNECTING;
            this.sent = [];
            this.constructor.instances.push(this);
        }

        send(payload) {
            this.sent.push(payload);
        }

        close() {
            this.readyState = this.constructor.CLOSED;
        }
    };
}

function createHarness(t, { rtcConstructor, isMobileDevice = false } = {}) {
    const FakeWebSocket = createFakeWebSocketClass();
    replaceGlobal(t, 'window', { __e2e: null });
    replaceGlobal(t, 'WebSocket', FakeWebSocket);
    if (rtcConstructor) {
        replaceGlobal(t, 'RTCPeerConnection', rtcConstructor);
    }

    const records = {
        logs: [],
        errors: [],
        statuses: [],
        aborts: [],
        resets: [],
        reconnects: [],
        timingStages: [],
        createOfferCount: 0,
        clearReconnectCount: 0,
        hasAttemptedConnection: false,
    };
    const connectButton = {
        disabled: false,
        textContent: 'Connect',
        classList: createClassList(),
    };
    const controlsDiv = { classList: createClassList() };
    const ctx = {
        uiModeParams: new URLSearchParams(),
        getSelectedMatchType: () => 'quick',
        getPreferredPlayerName: () => '',
        log(message, level = 'info') {
            records.logs.push({ message: String(message), level });
        },
        normalizeSignalingUrl: () => ({ ok: true, url: 'wss://arena.example.test/ws' }),
        summarizeSignalingError: () => 'Temporary signaling failure',
        wsUrlInput: { value: 'wss://arena.example.test/ws' },
        isMobileDevice,
        dataSaverMode: false,
        PIXI: {},
        canStartConnectionAttempt: () => true,
        initCullWorker() {},
        cullWorker: {},
        WORKER_CULL_ENABLED: false,
        startJoinTimingAttempt() {},
        markJoinTimingStage(stage) {
            records.timingStages.push(stage);
        },
        markJoinTimingAborted(detail) {
            records.aborts.push(String(detail));
        },
        withAuthTokenInUrl: (url) => url,
        setConnectionError(detail) {
            records.errors.push(String(detail));
        },
        clearConnectionOverride() {},
        clearReconnectTimer() {
            records.clearReconnectCount += 1;
        },
        scheduleAutoReconnect(reason) {
            records.reconnects.push(String(reason));
        },
        applyConnectionStatus(state, detail) {
            records.statuses.push({ state, detail: String(detail) });
        },
        connectButton,
        peerConnectionConfig: { iceServers: [] },
        peerConnection: null,
        signalingSocket: null,
        dataChannel: null,
        GameProtocol: {},
        GP: {},
        setActiveSignalingUrl() {},
        setConnectAttemptInFlight() {},
        setHasAttemptedConnection(value) {
            records.hasAttemptedConnection = value;
        },
        setSignalingSocket(value) {
            ctx.signalingSocket = value;
        },
        setPeerConnection(value) {
            ctx.peerConnection = value;
        },
        setDataChannel(value) {
            ctx.dataChannel = value;
        },
        createOffer() {
            records.createOfferCount += 1;
        },
        clearGameStateForReconnect() {},
        processServerUpdate() {},
        tryProcessDeltaMessageFast: () => false,
        parseFlatBufferMessage: () => null,
        unpackCoalescedPackets: () => null,
        trackNetworkProfilerMessage() {},
        trackFastDeltaPacket() {},
        trackFullParsePacket() {},
        handleParsedMessage() {},
        markJoinTimingComplete() {},
        controlsDiv,
        setupInputHandlers() {},
        ensureHudWidgets() {},
        loadSettings() {},
        initRenderAssetCache() {},
        resetReconnectState() {},
        resetConnectionUIImpl(options) {
            const snapshot = { ...options };
            records.resets.push(snapshot);
            connectButton.disabled = false;
            connectButton.textContent = 'Connect';
            if (snapshot.allowReconnect) {
                ctx.scheduleAutoReconnect(snapshot.reconnectReason || 'Connection lost');
            }
        },
    };

    return {
        ctx,
        records,
        connectButton,
        FakeWebSocket,
        manager: createConnectionManager(() => ctx),
    };
}

function createUrlManager({ params = '', matchType = '', preferredName = '' } = {}) {
    const ctx = {
        uiModeParams: new URLSearchParams(params),
        getSelectedMatchType: () => matchType,
        getPreferredPlayerName: () => preferredName,
        log() {},
    };
    return createConnectionManager(() => ctx);
}

test('mobile match selections do not misclassify a desktop as a mobile device', () => {
    for (const matchType of ['mobile_blitz', 'mobile_standard']) {
        const manager = createUrlManager({ matchType });
        const result = new URL(manager.withJoinSelectionInUrl('wss://arena.example.test/ws?token=abc'));

        assert.equal(result.searchParams.get('match_type'), matchType);
        assert.equal(result.searchParams.has('is_mobile'), false);
    }
});

test('explicit mobile URL modes add is_mobile=true without changing a desktop match selection', () => {
    for (const params of ['mobile=1', 'platform=mobile', 'platform=MOBILE']) {
        const manager = createUrlManager({ params, matchType: 'quick' });
        const result = new URL(manager.withJoinSelectionInUrl('wss://arena.example.test/ws'));

        assert.equal(result.searchParams.get('match_type'), 'quick');
        assert.equal(result.searchParams.get('is_mobile'), 'true');
    }

    const desktopManager = createUrlManager({ matchType: 'quick' });
    const desktopResult = new URL(desktopManager.withJoinSelectionInUrl('wss://arena.example.test/ws'));
    assert.equal(desktopResult.searchParams.has('is_mobile'), false);
});

test('device classification adds is_mobile=true to the signaling socket', (t) => {
    const { manager, FakeWebSocket } = createHarness(t, { isMobileDevice: true });
    t.after(() => manager.destroy());

    assert.equal(manager.startConnectionAttempt(), true);
    const socketUrl = new URL(FakeWebSocket.instances[0].url);
    assert.equal(socketUrl.searchParams.get('is_mobile'), 'true');
});

test('signaling open remains negotiating until the data channel opens', (t) => {
    const { manager, FakeWebSocket, connectButton, records } = createHarness(t);
    t.after(() => manager.destroy());

    assert.equal(manager.startConnectionAttempt(), true);
    const socket = FakeWebSocket.instances[0];
    socket.readyState = FakeWebSocket.OPEN;
    socket.onopen();

    assert.equal(connectButton.textContent, 'Negotiating...');
    assert.equal(records.statuses.at(-1)?.state, 'negotiating');
    assert.notEqual(connectButton.textContent, 'Connected');

    const dataChannel = { label: 'gameDataChannel', readyState: 'open' };
    manager.setupDataChannelEvents(dataChannel);
    dataChannel.onopen();

    assert.equal(connectButton.textContent, 'Connected');
    assert.equal(records.statuses.at(-1)?.state, 'waiting');
});

test('RTCPeerConnection constructor failure is stable, private, and non-retrying', async (t) => {
    class ThrowingPeerConnection {
        constructor() {
            throw new Error('RAW_BROWSER_INTERNAL_DEBUG');
        }
    }
    const { manager, FakeWebSocket, records } = createHarness(t, {
        rtcConstructor: ThrowingPeerConnection,
    });
    t.after(() => manager.destroy());

    assert.equal(manager.startConnectionAttempt(), true);
    const socket = FakeWebSocket.instances[0];
    socket.readyState = FakeWebSocket.OPEN;
    socket.onopen();

    await assert.doesNotReject(() => socket.onmessage({
        data: JSON.stringify({ event: 'ice_servers', ice_servers: [] }),
    }));

    assert.equal(records.createOfferCount, 0);
    assert.equal(records.statuses.at(-1)?.state, 'error');
    assert.match(records.statuses.at(-1)?.detail || '', /WebRTC is unavailable/);
    assert.equal(records.hasAttemptedConnection, false);
    assert.equal(records.resets.at(-1)?.allowReconnect, false);
    assert.deepEqual(records.reconnects, []);

    const exposedOutput = JSON.stringify({
        logs: records.logs,
        errors: records.errors,
        statuses: records.statuses,
        aborts: records.aborts,
    });
    assert.doesNotMatch(exposedOutput, /RAW_BROWSER_INTERNAL_DEBUG/);

    socket.onclose({ code: 1006, wasClean: false });
    assert.equal(records.resets.at(-1)?.allowReconnect, false);
    assert.deepEqual(records.reconnects, []);

    const socketCountBeforeRetry = FakeWebSocket.instances.length;
    assert.equal(manager.startConnectionAttempt({ isRetry: true }), false);
    assert.equal(FakeWebSocket.instances.length, socketCountBeforeRetry);
    assert.deepEqual(records.reconnects, []);
});

test('invalid ICE configuration retries once with browser defaults', async (t) => {
    class ConfigSensitivePeerConnection {
        static constructorArgs = [];

        constructor(config) {
            this.constructor.constructorArgs.push(config);
            if (config !== undefined) {
                throw new Error('CONFIG_SPECIFIC_BROWSER_DETAIL');
            }
            this.iceConnectionState = 'new';
        }

        createDataChannel(label) {
            return { label, readyState: 'connecting' };
        }
    }
    const { manager, FakeWebSocket, records } = createHarness(t, {
        rtcConstructor: ConfigSensitivePeerConnection,
    });
    t.after(() => manager.destroy());

    assert.equal(manager.startConnectionAttempt(), true);
    const socket = FakeWebSocket.instances[0];
    socket.readyState = FakeWebSocket.OPEN;
    socket.onopen();
    await socket.onmessage({
        data: JSON.stringify({ event: 'ice_servers', ice_servers: [] }),
    });

    assert.equal(ConfigSensitivePeerConnection.constructorArgs.length, 2);
    assert.deepEqual(ConfigSensitivePeerConnection.constructorArgs[0], { iceServers: [] });
    assert.equal(ConfigSensitivePeerConnection.constructorArgs[1], undefined);
    assert.equal(records.createOfferCount, 1);
    assert.notEqual(records.statuses.at(-1)?.state, 'error');
    assert.doesNotMatch(JSON.stringify(records), /CONFIG_SPECIFIC_BROWSER_DETAIL/);
});

test('server signaling rejection resets immediately and enables manual retry', async (t) => {
    const { manager, FakeWebSocket, records, connectButton } = createHarness(t);
    t.after(() => manager.destroy());

    assert.equal(manager.startConnectionAttempt(), true);
    const socket = FakeWebSocket.instances[0];
    socket.readyState = FakeWebSocket.OPEN;
    socket.onopen();
    connectButton.disabled = true;

    await socket.onmessage({
        data: JSON.stringify({ error: 'invalid_signaling_payload', detail: 'Offer rejected' }),
    });

    assert.equal(records.resets.length, 1);
    assert.equal(records.resets[0].allowReconnect, false);
    assert.equal(connectButton.disabled, false);
    assert.equal(connectButton.textContent, 'Connect');
    assert.equal(records.statuses.at(-1)?.state, 'error');
});

test('transient signaling failures still request automatic reconnect', (t) => {
    const { manager, FakeWebSocket, records } = createHarness(t);
    t.after(() => manager.destroy());

    assert.equal(manager.startConnectionAttempt(), true);
    const socket = FakeWebSocket.instances[0];
    socket.onerror({ type: 'error' });

    assert.equal(records.resets.at(-1)?.allowReconnect, true);
    assert.deepEqual(records.reconnects, ['Temporary signaling failure']);
});
