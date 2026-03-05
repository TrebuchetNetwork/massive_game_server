import test from "node:test";
import assert from "node:assert/strict";

import { createAuthHelpers } from "../client_logic/auth_utils.js";

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

function makeElement() {
    return {
        textContent: "",
        style: {},
        disabled: false,
        value: "",
        classList: {
            toggle() {},
        },
    };
}

function makeAuthElements() {
    return {
        authSignedOutDiv: makeElement(),
        authSignedInDiv: makeElement(),
        authDisplayNameSpan: makeElement(),
        authPhoneMaskedSpan: makeElement(),
        authTotalScoreSpan: makeElement(),
        authBestScoreSpan: makeElement(),
        authLifetimeKdSpan: makeElement(),
        authTopStreakSpan: makeElement(),
        authTotalCapturesSpan: makeElement(),
        authFavoriteWeaponSpan: makeElement(),
        authStatusP: makeElement(),
        authRequestCodeButton: makeElement(),
        authVerifyCodeButton: makeElement(),
        authSignOutButton: makeElement(),
        authPhoneInput: makeElement(),
        authCodeInput: makeElement(),
    };
}

test.beforeEach(() => {
    globalThis.sessionStorage = makeStorage();
    globalThis.localStorage = makeStorage();
});

test.afterEach(() => {
    delete globalThis.sessionStorage;
    delete globalThis.localStorage;
});

function makeHelpers({ fetchImpl, initialToken = "" } = {}) {
    const authElements = makeAuthElements();
    let sessionToken = initialToken;
    let authProfile = null;
    const helpers = createAuthHelpers({
        authElements,
        authSessionTokenKey: "mgs_auth_token",
        getAuthSessionToken: () => sessionToken,
        setAuthSessionToken: (token) => {
            sessionToken = token;
        },
        setAuthProfile: (profile) => {
            authProfile = profile;
        },
        fetchImpl,
    });
    return { helpers, authElements, getSessionToken: () => sessionToken, getAuthProfile: () => authProfile };
}

test("verifyPhoneCode accepts cookie-mode response without token", async () => {
    const profile = {
        user_id: "u1",
        display_name: "Player1",
        phone_masked: "+1****1111",
    };
    const { helpers, authElements, getSessionToken, getAuthProfile } = makeHelpers({
        fetchImpl: async () => ({
            ok: true,
            json: async () => ({
                ok: true,
                data: {
                    token_expires_at: 123456,
                    profile,
                },
            }),
        }),
    });

    authElements.authPhoneInput.value = "+15551230111";
    authElements.authCodeInput.value = "123456";

    const ok = await helpers.verifyPhoneCode();
    assert.equal(ok, true);
    assert.equal(getSessionToken(), "");
    assert.equal(getAuthProfile()?.user_id, "u1");
});

test("refreshAuthProfile works without bearer token (cookie mode)", async () => {
    let called = 0;
    let receivedOptions = null;
    const { helpers, getAuthProfile } = makeHelpers({
        initialToken: "",
        fetchImpl: async (_url, options) => {
            called += 1;
            receivedOptions = options;
            return {
                ok: true,
                json: async () => ({
                    ok: true,
                    data: {
                        profile: {
                            user_id: "u_cookie",
                            display_name: "CookieUser",
                            phone_masked: "+1****2222",
                        },
                    },
                }),
            };
        },
    });

    const ok = await helpers.refreshAuthProfile({ silentIfUnauth: true });
    assert.equal(ok, true);
    assert.equal(called, 1);
    assert.equal(receivedOptions?.credentials, "same-origin");
    assert.deepEqual(receivedOptions?.headers || {}, {});
    assert.equal(getAuthProfile()?.user_id, "u_cookie");
});

test("signOutAuthSession posts logout without bearer token", async () => {
    let receivedOptions = null;
    const { helpers, getSessionToken } = makeHelpers({
        initialToken: "",
        fetchImpl: async (_url, options) => {
            receivedOptions = options;
            return {
                ok: true,
                json: async () => ({ ok: true }),
            };
        },
    });

    await helpers.signOutAuthSession();
    assert.equal(receivedOptions?.method, "POST");
    assert.equal(receivedOptions?.credentials, "same-origin");
    assert.deepEqual(receivedOptions?.headers || {}, {});
    assert.equal(getSessionToken(), "");
});
