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
        } else {
            if (authDisplayNameSpan) authDisplayNameSpan.textContent = "N/A";
            if (authPhoneMaskedSpan) authPhoneMaskedSpan.textContent = "N/A";
            if (authTotalScoreSpan) authTotalScoreSpan.textContent = "0";
            if (authBestScoreSpan) authBestScoreSpan.textContent = "0";
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

    function setAuthToken(token) {
        const normalized = String(token || "").trim();
        setAuthSessionToken(normalized);
        if (normalized) {
            localStorage.setItem(authSessionTokenKey, normalized);
        } else {
            localStorage.removeItem(authSessionTokenKey);
        }
    }

    function loadAuthToken() {
        const stored = localStorage.getItem(authSessionTokenKey);
        const normalized = String(stored || "").trim();
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
        const token = String(getAuthSessionToken() || "").trim();
        if (!token) return rawUrl;
        try {
            const parsed = new URL(rawUrl);
            parsed.searchParams.set("auth_token", token);
            return parsed.toString();
        } catch (_) {
            return rawUrl;
        }
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
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ phone_number: phone, code }),
            });
            const payload = await response.json().catch(() => null);
            if (!response.ok || !payload || payload.ok !== true) {
                setAuthStatus(parseApiError(payload, "Verification failed."), "error");
                return false;
            }
            const token = String(payload?.data?.token || "").trim();
            const profile = payload?.data?.profile || null;
            if (!token || !profile) {
                setAuthStatus("Verification response missing token/profile.", "error");
                return false;
            }
            setAuthToken(token);
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

    async function refreshAuthProfile() {
        const token = String(getAuthSessionToken() || "").trim();
        if (!token) {
            clearAuthSession();
            return false;
        }
        try {
            const response = await fetchImpl("/auth/me", {
                method: "GET",
                headers: {
                    Authorization: `Bearer ${token}`,
                },
            });
            const payload = await response.json().catch(() => null);
            if (!response.ok || !payload || payload.ok !== true) {
                clearAuthSession();
                setAuthStatus("Session expired. Verify phone again.", "error");
                return false;
            }
            updateAuthUi(payload.data?.profile || null);
            setAuthStatus("Signed in. Persistent score tracking enabled.", "success");
            return true;
        } catch (error) {
            setAuthStatus(`Failed to load profile: ${error?.message || error}`, "error");
            return false;
        }
    }

    async function signOutAuthSession() {
        const token = String(getAuthSessionToken() || "").trim();
        if (token) {
            try {
                await fetchImpl("/auth/logout", {
                    method: "POST",
                    headers: {
                        Authorization: `Bearer ${token}`,
                    },
                });
            } catch (_) {}
        }
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
