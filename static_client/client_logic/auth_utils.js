// SECURITY NOTE: For production deployments, the ideal token storage is an
// HttpOnly, Secure, SameSite=Strict cookie set by the server. This removes
// tokens from JS-accessible storage entirely and eliminates XSS token theft.
// To enable this, set MGS_AUTH_USE_COOKIES=true on the server. The server will
// then set the session token as a cookie on the verify-code response, and the
// WebSocket upgrade path will read the cookie automatically.
//
// The current client-side approach uses sessionStorage (not localStorage) to
// limit the exposure window: tokens are cleared when the tab/browser closes,
// reducing the risk from persistent XSS or shared-device scenarios.

// Key used to store the token expiry timestamp alongside the token itself.
const TOKEN_EXPIRY_SUFFIX = "_expires_at";

export function createAuthHelpers(options) {
    const {
        authElements,
        authSessionTokenKey,
        getAuthSessionToken,
        setAuthSessionToken,
        setAuthProfile,
        fetchImpl = fetch,
    } = options;

    const {
        authSignedOutDiv,
        authSignedInDiv,
        authDisplayNameSpan,
        authPhoneMaskedSpan,
        authTotalScoreSpan,
        authBestScoreSpan,
        authLifetimeKdSpan,
        authTopStreakSpan,
        authTotalCapturesSpan,
        authFavoriteWeaponSpan,
        authStatusP,
        authRequestCodeButton,
        authVerifyCodeButton,
        authSignOutButton,
        authPhoneInput,
        authCodeInput,
    } = authElements;

    function updateAuthUi(profile = null) {
        setAuthProfile(profile);
        const signedIn = !!(profile && profile.user_id);
        if (authSignedOutDiv) authSignedOutDiv.classList.toggle("hidden", signedIn);
        if (authSignedInDiv) authSignedInDiv.classList.toggle("hidden", !signedIn);

        if (signedIn) {
            if (authDisplayNameSpan) authDisplayNameSpan.textContent = profile.display_name || "Player";
            if (authPhoneMaskedSpan) authPhoneMaskedSpan.textContent = profile.phone_masked || "N/A";
            if (authTotalScoreSpan) authTotalScoreSpan.textContent = Number(profile.cumulative_score || 0).toString();
            if (authBestScoreSpan) authBestScoreSpan.textContent = Number(profile.best_score || 0).toString();
            if (authLifetimeKdSpan) authLifetimeKdSpan.textContent = Number(profile.lifetime_kd || 0).toFixed(2);
            if (authTopStreakSpan) authTopStreakSpan.textContent = Number(profile.top_streak || 0).toString();
            if (authTotalCapturesSpan) authTotalCapturesSpan.textContent = Number(profile.total_flag_captures || 0).toString();
            if (authFavoriteWeaponSpan) authFavoriteWeaponSpan.textContent = String(profile.favorite_weapon || "None");
        } else {
            if (authDisplayNameSpan) authDisplayNameSpan.textContent = "N/A";
            if (authPhoneMaskedSpan) authPhoneMaskedSpan.textContent = "N/A";
            if (authTotalScoreSpan) authTotalScoreSpan.textContent = "0";
            if (authBestScoreSpan) authBestScoreSpan.textContent = "0";
            if (authLifetimeKdSpan) authLifetimeKdSpan.textContent = "0.00";
            if (authTopStreakSpan) authTopStreakSpan.textContent = "0";
            if (authTotalCapturesSpan) authTotalCapturesSpan.textContent = "0";
            if (authFavoriteWeaponSpan) authFavoriteWeaponSpan.textContent = "None";
        }
    }

    function setAuthStatus(message, kind = "info") {
        if (!authStatusP) return;
        authStatusP.textContent = message || "";
        if (kind === "error") {
            authStatusP.style.color = "#FCA5A5";
        } else if (kind === "success") {
            authStatusP.style.color = "#86EFAC";
        } else {
            authStatusP.style.color = "#9CA3AF";
        }
    }

    function setAuthControlsBusy(isBusy) {
        const disabled = !!isBusy;
        if (authRequestCodeButton) authRequestCodeButton.disabled = disabled;
        if (authVerifyCodeButton) authVerifyCodeButton.disabled = disabled;
        if (authSignOutButton) authSignOutButton.disabled = disabled;
        if (authPhoneInput) authPhoneInput.disabled = disabled;
        if (authCodeInput) authCodeInput.disabled = disabled;
    }

    function setAuthToken(token, expiresAt) {
        const normalized = String(token || "").trim();
        setAuthSessionToken(normalized);
        if (normalized) {
            sessionStorage.setItem(authSessionTokenKey, normalized);
            // Store expiry so we can reject stale tokens on load.
            if (expiresAt) {
                sessionStorage.setItem(
                    authSessionTokenKey + TOKEN_EXPIRY_SUFFIX,
                    String(expiresAt),
                );
            }
        } else {
            sessionStorage.removeItem(authSessionTokenKey);
            sessionStorage.removeItem(authSessionTokenKey + TOKEN_EXPIRY_SUFFIX);
        }
        // Clean up any legacy localStorage token from before this migration.
        try {
            localStorage.removeItem(authSessionTokenKey);
            localStorage.removeItem(authSessionTokenKey + TOKEN_EXPIRY_SUFFIX);
        } catch (_) { /* storage may be unavailable */ }
    }

    function loadAuthToken() {
        // Migrate: if a token exists in localStorage but not sessionStorage,
        // move it over and remove the old copy.
        let stored = sessionStorage.getItem(authSessionTokenKey);
        if (!stored) {
            const legacy = localStorage.getItem(authSessionTokenKey);
            if (legacy) {
                stored = legacy;
                sessionStorage.setItem(authSessionTokenKey, legacy);
                try { localStorage.removeItem(authSessionTokenKey); } catch (_) {}
            }
        }
        const normalized = String(stored || "").trim();
        // Enforce client-side TTL: reject tokens past their expiry.
        if (normalized) {
            const expiryRaw = sessionStorage.getItem(authSessionTokenKey + TOKEN_EXPIRY_SUFFIX);
            if (expiryRaw) {
                const expiresAt = Number(expiryRaw);
                const nowSeconds = Math.floor(Date.now() / 1000);
                if (expiresAt > 0 && nowSeconds >= expiresAt) {
                    // Token has expired client-side; clear it.
                    setAuthToken("");
                    return "";
                }
            }
        }
        setAuthSessionToken(normalized);
        return normalized;
    }

    function clearAuthSession() {
        setAuthToken("");
        updateAuthUi(null);
    }

    function parseApiError(payload, fallbackMessage = "Request failed") {
        if (payload && payload.error && payload.error.message) {
            return payload.error.message;
        }
        if (payload && typeof payload.message === "string" && payload.message.trim()) {
            return payload.message;
        }
        return fallbackMessage;
    }

    function withAuthTokenInUrl(rawUrl) {
        // Query-string token transport is intentionally disabled.
        // Server-side signaling ignores auth_token query params to avoid
        // token leakage through logs, browser history, and referrers.
        return rawUrl;
    }

    async function requestPhoneCode() {
        if (!authPhoneInput) return false;
        const phone = String(authPhoneInput.value || "").trim();
        if (!phone) {
            setAuthStatus("Enter phone number first.", "error");
            return false;
        }
        setAuthControlsBusy(true);
        setAuthStatus("Sending SMS code...");
        try {
            const response = await fetchImpl("/auth/phone/request-code", {
                method: "POST",
                credentials: "same-origin",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ phone_number: phone }),
            });
            const payload = await response.json().catch(() => null);
            if (!response.ok || !payload || payload.ok !== true) {
                setAuthStatus(parseApiError(payload, "Failed to send verification code."), "error");
                return false;
            }
            const devCode = payload?.data?.dev_code;
            if (devCode && authCodeInput && !authCodeInput.value) {
                authCodeInput.value = String(devCode);
            }
            setAuthStatus("Code sent. Enter the 6-digit code to verify.", "success");
            return true;
        } catch (error) {
            setAuthStatus(`Failed to send verification code: ${error?.message || error}`, "error");
            return false;
        } finally {
            setAuthControlsBusy(false);
        }
    }

    async function verifyPhoneCode() {
        if (!authPhoneInput || !authCodeInput) return false;
        const phone = String(authPhoneInput.value || "").trim();
        const code = String(authCodeInput.value || "").trim();
        if (!phone || !code) {
            setAuthStatus("Phone and code are required.", "error");
            return false;
        }
        setAuthControlsBusy(true);
        setAuthStatus("Verifying code...");
        try {
            const response = await fetchImpl("/auth/phone/verify-code", {
                method: "POST",
                credentials: "same-origin",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ phone_number: phone, code }),
            });
            const payload = await response.json().catch(() => null);
            if (!response.ok || !payload || payload.ok !== true) {
                setAuthStatus(parseApiError(payload, "Verification failed."), "error");
                return false;
            }
            const tokenRaw = payload?.data?.token;
            const token = typeof tokenRaw === "string" ? tokenRaw.trim() : "";
            const tokenExpiresAt = payload?.data?.token_expires_at || null;
            const profile = payload?.data?.profile || null;
            if (!profile) {
                setAuthStatus("Verification response missing profile.", "error");
                return false;
            }
            // In cookie-auth mode the server omits token from JSON and sets
            // HttpOnly mgs_session instead.
            setAuthToken(token, tokenExpiresAt);
            updateAuthUi(profile);
            setAuthStatus("Phone verified. Your score will persist to this account.", "success");
            return true;
        } catch (error) {
            setAuthStatus(`Verification failed: ${error?.message || error}`, "error");
            return false;
        } finally {
            setAuthControlsBusy(false);
        }
    }

    async function refreshAuthProfile(options = {}) {
        const { silentIfUnauth = false } = options || {};
        const token = String(getAuthSessionToken() || "").trim();
        const headers = {};
        if (token) {
            headers.Authorization = `Bearer ${token}`;
        }
        try {
            const response = await fetchImpl("/auth/me", {
                method: "GET",
                credentials: "same-origin",
                headers,
            });
            const payload = await response.json().catch(() => null);
            if (!response.ok || !payload || payload.ok !== true) {
                clearAuthSession();
                if (!silentIfUnauth) {
                    setAuthStatus("Session expired. Verify phone again.", "error");
                }
                return false;
            }
            updateAuthUi(payload.data?.profile || null);
            setAuthStatus("Signed in. Persistent score tracking enabled.", "success");
            return true;
        } catch (error) {
            if (!silentIfUnauth) {
                setAuthStatus(`Failed to load profile: ${error?.message || error}`, "error");
            }
            return false;
        }
    }

    async function signOutAuthSession() {
        const token = String(getAuthSessionToken() || "").trim();
        const headers = {};
        if (token) {
            headers.Authorization = `Bearer ${token}`;
        }
        try {
            await fetchImpl("/auth/logout", {
                method: "POST",
                credentials: "same-origin",
                headers,
            });
        } catch (_) {}
        clearAuthSession();
        setAuthStatus("Signed out.", "info");
    }

    return {
        updateAuthUi,
        setAuthStatus,
        setAuthControlsBusy,
        setAuthToken,
        loadAuthToken,
        clearAuthSession,
        parseApiError,
        withAuthTokenInUrl,
        requestPhoneCode,
        verifyPhoneCode,
        refreshAuthProfile,
        signOutAuthSession,
    };
}
