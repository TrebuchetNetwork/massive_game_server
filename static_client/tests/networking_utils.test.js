import test from "node:test";
import assert from "node:assert/strict";

import {
    buildPeerConnectionConfig,
    getDefaultWsUrl,
    normalizeSignalingUrl,
    parseIceServerSpec,
    resolveTurnCredentials,
    splitNonEmptyValues,
    summarizeSignalingError,
} from "../client_logic/networking_utils.js";

function makeStorage(initial = {}) {
    const store = new Map(Object.entries(initial));
    return {
        getItem(key) {
            return store.has(key) ? store.get(key) : null;
        },
        setItem(key, value) {
            store.set(key, String(value));
        },
        removeItem(key) {
            store.delete(key);
        },
    };
}

function installBrowserEnv({
    pageUrl = "https://example.com/client.html",
    online = true,
    turnConfig,
    sessionValues = {},
    localValues = {},
} = {}) {
    const location = new URL(pageUrl);
    globalThis.window = {
        location,
        __MGS_TURN_CONFIG: turnConfig,
    };
    Object.defineProperty(globalThis, "navigator", {
        value: { onLine: online },
        configurable: true,
    });
    globalThis.sessionStorage = makeStorage(sessionValues);
    globalThis.localStorage = makeStorage(localValues);
}

test.afterEach(() => {
    delete globalThis.window;
    delete globalThis.sessionStorage;
    delete globalThis.localStorage;
    delete globalThis.navigator;
});

test("getDefaultWsUrl derives secure websocket from current location", () => {
    installBrowserEnv({ pageUrl: "https://arena.example.com/play" });
    assert.equal(getDefaultWsUrl(), "wss://arena.example.com/ws");
});

test("normalizeSignalingUrl supports host-only and relative values", () => {
    installBrowserEnv({ pageUrl: "http://localhost:8080/client.html" });

    const hostOnly = normalizeSignalingUrl("localhost:9001");
    assert.equal(hostOnly.ok, true);
    assert.equal(hostOnly.url, "ws://localhost:9001/ws");

    const relative = normalizeSignalingUrl("/custom-ws");
    assert.equal(relative.ok, true);
    assert.equal(relative.url, "ws://localhost:8080/custom-ws");
});

test("normalizeSignalingUrl rejects unsupported protocols", () => {
    installBrowserEnv();
    const result = normalizeSignalingUrl("ftp://example.com/socket");
    assert.equal(result.ok, false);
    assert.match(result.error, /Unsupported protocol/i);
});

test("summarizeSignalingError includes connectivity hints", () => {
    installBrowserEnv({ pageUrl: "https://example.com/client.html", online: false });
    const summary = summarizeSignalingError(
        { type: "error" },
        { readyState: 3 },
        "ws://example.com/ws"
    );
    assert.match(summary, /readyState=3 \(CLOSED\)/);
    assert.match(summary, /network=offline/);
    assert.match(summary, /Mixed-content blocked/);
});

test("splitNonEmptyValues and parseIceServerSpec parse flexible entries", () => {
    assert.deepEqual(splitNonEmptyValues(" a, ,b ,, c "), ["a", "b", "c"]);
    assert.equal(parseIceServerSpec(""), null);

    const parsed = parseIceServerSpec("turn:turn.example.com|alice|secret", {
        includeCredentials: true,
    });
    assert.deepEqual(parsed, {
        urls: "turn:turn.example.com",
        username: "alice",
        credential: "secret",
    });
});

test("resolveTurnCredentials prefers runtime config and uses session storage only", () => {
    installBrowserEnv({
        turnConfig: { username: "runtime-user", credential: "runtime-secret" },
        sessionValues: {
            mgs_turn_username: "session-user",
            mgs_turn_credential: "session-secret",
        },
    });
    assert.deepEqual(resolveTurnCredentials(), {
        username: "runtime-user",
        credential: "runtime-secret",
    });

    installBrowserEnv({
        turnConfig: null,
        sessionValues: {
            mgs_turn_username: "session-user",
            mgs_turn_credential: "session-secret",
        },
    });
    assert.deepEqual(resolveTurnCredentials(), {
        username: "session-user",
        credential: "session-secret",
    });

    installBrowserEnv({
        turnConfig: null,
        sessionValues: {},
        localValues: {
            mgs_turn_username: "legacy-local-user",
            mgs_turn_credential: "legacy-local-secret",
        },
    });
    assert.deepEqual(resolveTurnCredentials(), {
        username: "",
        credential: "",
    });
});

test("buildPeerConnectionConfig applies toggles, dedupes servers, and strips URL credentials", () => {
    installBrowserEnv({
        pageUrl: "https://example.com/client.html",
        sessionValues: {
            mgs_turn_username: "stored-user",
            mgs_turn_credential: "stored-secret",
        },
    });

    const warnings = [];
    const params = new URLSearchParams(
        "disable_stun=1" +
            "&ice=stun:stun.example.com|inline-user|inline-secret;stun:stun.example.com" +
            "&turn=turn:turn1.example.com,turn:turn2.example.com" +
            "&turn_user=legacy-user"
    );
    const config = buildPeerConnectionConfig(params, (msg, level) =>
        warnings.push({ msg, level })
    );

    assert.ok(Array.isArray(config.iceServers));
    assert.equal(config.iceServers.length, 2);

    const stun = config.iceServers.find((s) => String(s.urls).includes("stun:stun.example.com"));
    assert.ok(stun);
    assert.equal(stun.username, undefined);
    assert.equal(stun.credential, undefined);

    const turn = config.iceServers.find((s) =>
        Array.isArray(s.urls)
            ? s.urls.includes("turn:turn1.example.com")
            : String(s.urls).includes("turn:turn1.example.com")
    );
    assert.ok(turn);
    assert.equal(turn.username, "stored-user");
    assert.equal(turn.credential, "stored-secret");

    assert.ok(warnings.some((w) => /Ignoring ICE credentials in URL query params/i.test(w.msg)));
    assert.ok(warnings.some((w) => /TURN credentials in URL query params are disabled/i.test(w.msg)));
});
