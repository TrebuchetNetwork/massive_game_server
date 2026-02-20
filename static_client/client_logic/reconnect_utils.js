export function createReconnectHelpers(options) {
    const {
        getAutoReconnectEnabled,
        getHasAttemptedConnection,
        getReconnectTimerId,
        setReconnectTimerId,
        getConnectAttemptInFlight,
        getDataChannel,
        getSignalingSocket,
        getReconnectAttemptCount,
        setReconnectAttemptCount,
        getAutoReconnectMaxAttempts,
        getAutoReconnectBaseDelayMs,
        getAutoReconnectMaxDelayMs,
        log,
        applyConnectionStatus,
        startConnectionAttempt,
    } = options;

    function clearReconnectTimer() {
        const timerId = getReconnectTimerId();
        if (timerId !== null) {
            clearTimeout(timerId);
            setReconnectTimerId(null);
        }
    }

    function resetReconnectState() {
        setReconnectAttemptCount(0);
        clearReconnectTimer();
    }

    function canStartConnectionAttempt() {
        if (getConnectAttemptInFlight()) return false;
        const dataChannel = getDataChannel();
        if (dataChannel && dataChannel.readyState === "open") return false;
        const signalingSocket = getSignalingSocket();
        if (
            signalingSocket &&
            (signalingSocket.readyState === WebSocket.CONNECTING ||
                signalingSocket.readyState === WebSocket.OPEN)
        ) {
            return false;
        }
        return true;
    }

    function computeReconnectDelayMs(attemptNumber) {
        const safeAttempt = Math.max(1, attemptNumber);
        const backoff = Math.pow(1.6, safeAttempt - 1);
        return Math.min(
            getAutoReconnectMaxDelayMs(),
            Math.floor(getAutoReconnectBaseDelayMs() * backoff)
        );
    }

    function scheduleAutoReconnect(reason = "") {
        if (!getAutoReconnectEnabled()) return false;
        if (!getHasAttemptedConnection()) return false;
        if (getReconnectTimerId() !== null) return false;
        if (!canStartConnectionAttempt()) return false;

        const maxAttempts = getAutoReconnectMaxAttempts();
        const currentAttempts = getReconnectAttemptCount();
        if (currentAttempts >= maxAttempts) {
            const detail = `Reconnect limit reached (${maxAttempts}). Tap Connect to retry.`;
            log(detail, "warn");
            applyConnectionStatus("error", detail);
            return false;
        }

        const attemptNumber = currentAttempts + 1;
        setReconnectAttemptCount(attemptNumber);
        const delayMs = computeReconnectDelayMs(attemptNumber);
        const delaySec = Math.max(1, Math.round(delayMs / 1000));
        const prefix = reason ? `${reason}. ` : "";
        applyConnectionStatus(
            "connecting",
            `${prefix}Retrying in ${delaySec}s (${attemptNumber}/${maxAttempts})`
        );

        const timerId = setTimeout(() => {
            setReconnectTimerId(null);
            startConnectionAttempt({ isRetry: true });
        }, delayMs);
        setReconnectTimerId(timerId);
        return true;
    }

    return {
        clearReconnectTimer,
        resetReconnectState,
        canStartConnectionAttempt,
        computeReconnectDelayMs,
        scheduleAutoReconnect,
    };
}
