/**
 * GameState.js - Entity state management extracted from client.html
 *
 * Contains connection quality tracking, network type change detection,
 * connection status evaluation, adaptive interpolation delay,
 * render target smoothing, and entity lifecycle helpers.
 */

export function createGameState({
    log,
    clamp,
    lerp,
    normalizeAngle,
    applyConnectionStatusUi,
    normalizeConnectionErrorDetail,
    getDefaultWsUrl,
    normalizeSignalingUrl,
    GP,
    // Config constants
    INTERPOLATION_DELAY,
    MIN_INTERPOLATION_DELAY_MS,
    MAX_INTERPOLATION_DELAY_MS,
    INTERPOLATION_SNAPSHOT_INTERVAL_MS,
    NETWORK_TIMING_EMA_ALPHA,
    // Touch/mobile
    isTouchDevice,
}) {
    // ── Connection quality ─────────────────────────────────────────
    let connectionQuality = 'good'; // good, fair, poor

    function updateConnectionQuality(rtt, jitter) {
        if (jitter > 50 || rtt > 200) connectionQuality = 'poor';
        else if (jitter > 20 || rtt > 100) connectionQuality = 'fair';
        else connectionQuality = 'good';
    }

    function getConnectionQuality() {
        return connectionQuality;
    }

    // ── Network type change detection (WiFi <-> cellular) ─────────
    let lastNetworkType = null;
    let networkConnectionRef = null;
    let networkChangeHandler = null;

    function initNetworkChangeDetection(getAdaptiveDelayRef) {
        teardownNetworkChangeDetection();
        if (!navigator.connection) return;
        networkConnectionRef = navigator.connection;
        lastNetworkType = networkConnectionRef.type || networkConnectionRef.effectiveType;
        networkChangeHandler = () => {
            if (!networkConnectionRef) return;
            const newType = networkConnectionRef.type || networkConnectionRef.effectiveType;
            if (lastNetworkType && newType !== lastNetworkType) {
                log(`Network changed: ${lastNetworkType} -> ${newType}`, 'warn');
                if (getAdaptiveDelayRef) {
                    getAdaptiveDelayRef().value = MAX_INTERPOLATION_DELAY_MS;
                }
            }
            lastNetworkType = newType;
        };
        networkConnectionRef.addEventListener('change', networkChangeHandler);
    }

    function teardownNetworkChangeDetection() {
        if (networkConnectionRef && networkChangeHandler) {
            networkConnectionRef.removeEventListener('change', networkChangeHandler);
        }
        networkConnectionRef = null;
        networkChangeHandler = null;
    }

    // ── Connection status titles and constants ────────────────────
    const connectionStatusTitles = {
        idle: 'Not connected',
        connecting: 'Connecting',
        negotiating: 'Negotiating',
        waiting: 'Connected',
        respawn: 'Respawning',
        error: 'Disconnected',
        playing: ''
    };
    const CONNECTION_ERROR_FALLBACK = 'Connection lost. Click Connect to retry.';

    function createConnectionStatusManager({
        connectionStatusDiv,
        connectionStatusTitle,
        connectionStatusDetail,
    }) {
        let lastConnectionStatusKey = '';
        let lastConnectionDetail = '';
        let lastConnectionStatusUpdate = 0;
        let connectionStateOverride = null;
        let connectionErrorDetail = CONNECTION_ERROR_FALLBACK;

        function applyConnectionStatus(statusKey, detailText = '') {
            const nextStatus = applyConnectionStatusUi({
                statusKey,
                detailText,
                connectionStatusDiv,
                connectionStatusTitle,
                connectionStatusDetail,
                connectionStatusTitles,
                lastConnectionStatusKey,
                lastConnectionDetail,
                onStatusChange: (nextKey, nextDetail) => {
                    if (window.__e2e) {
                        window.__e2e.connectionStatus = { statusKey: nextKey, detailText: nextDetail };
                    }
                },
            });
            lastConnectionStatusKey = nextStatus.lastConnectionStatusKey;
            lastConnectionDetail = nextStatus.lastConnectionDetail;
        }

        function setConnectionError(detailText) {
            connectionStateOverride = 'error';
            connectionErrorDetail = normalizeConnectionErrorDetail(detailText, CONNECTION_ERROR_FALLBACK);
            applyConnectionStatus('error', connectionErrorDetail);
        }

        function clearConnectionOverride() {
            connectionStateOverride = null;
            connectionErrorDetail = CONNECTION_ERROR_FALLBACK;
        }

        function evaluateConnectionStatus({
            dataChannel,
            signalingSocket,
            localPlayerState,
            hasAttemptedConnection,
        }) {
            const now = Date.now();
            if (now - lastConnectionStatusUpdate < 200) return;
            lastConnectionStatusUpdate = now;

            if (connectionStateOverride === 'error') {
                applyConnectionStatus('error', connectionErrorDetail);
                return;
            }

            if (dataChannel && dataChannel.readyState === 'open') {
                if (!localPlayerState) {
                    applyConnectionStatus('waiting', 'Waiting for match state...');
                    return;
                }
                if (!localPlayerState.alive) {
                    const respawnTimer = Number(localPlayerState.respawn_timer);
                    if (Number.isFinite(respawnTimer) && respawnTimer > 0) {
                        const respawnLabel = respawnTimer >= 1
                            ? `${Math.ceil(respawnTimer)}s`
                            : `${Math.max(0, respawnTimer).toFixed(1)}s`;
                        applyConnectionStatus('respawn', `Respawning in ${respawnLabel}...`);
                    } else {
                        applyConnectionStatus('respawn', 'Respawning...');
                    }
                    return;
                }
                applyConnectionStatus('playing');
                return;
            }

            if (signalingSocket && signalingSocket.readyState === WebSocket.CONNECTING) {
                applyConnectionStatus('connecting', 'Contacting signaling server...');
                return;
            }

            if (signalingSocket && signalingSocket.readyState === WebSocket.OPEN) {
                applyConnectionStatus('negotiating', 'Establishing peer connection...');
                return;
            }

            if (hasAttemptedConnection) {
                applyConnectionStatus('error', 'Disconnected. Click Connect to retry.');
                return;
            }

            applyConnectionStatus('idle', 'Click Connect to join a match');
        }

        function hydrateDefaultWsUrl(wsUrlInput, uiModeParams) {
            if (!wsUrlInput) return;
            const urlParam = uiModeParams.get('ws');
            if (!urlParam) {
                wsUrlInput.value = getDefaultWsUrl();
                return;
            }
            const normalized = normalizeSignalingUrl(urlParam);
            if (normalized.ok) {
                wsUrlInput.value = normalized.url;
                return;
            }
            wsUrlInput.value = getDefaultWsUrl();
        }

        return {
            applyConnectionStatus,
            setConnectionError,
            clearConnectionOverride,
            evaluateConnectionStatus,
            hydrateDefaultWsUrl,
        };
    }

    // ── Adaptive interpolation ────────────────────────────────────
    let lastSnapshotArrivalTime = 0;
    let snapshotIntervalEma = INTERPOLATION_SNAPSHOT_INTERVAL_MS;
    let snapshotJitterEma = 0;

    function updateAdaptiveInterpolationDelay(snapshotTime, adaptiveDelayRef) {
        if (!Number.isFinite(snapshotTime) || snapshotTime <= 0) return;
        if (lastSnapshotArrivalTime > 0) {
            const interval = snapshotTime - lastSnapshotArrivalTime;
            if (interval > 0 && interval < 1000) {
                const previousInterval = snapshotIntervalEma;
                snapshotIntervalEma = lerp(snapshotIntervalEma, interval, NETWORK_TIMING_EMA_ALPHA);
                snapshotJitterEma = lerp(snapshotJitterEma, Math.abs(interval - previousInterval), NETWORK_TIMING_EMA_ALPHA);
                let targetDelay;
                if (snapshotJitterEma < 20) {
                    targetDelay = snapshotIntervalEma * 1.2 + snapshotJitterEma * 1.5;
                } else if (snapshotJitterEma < 50) {
                    targetDelay = snapshotIntervalEma * 1.4 + snapshotJitterEma * 2.2;
                } else {
                    targetDelay = snapshotIntervalEma * 1.8 + snapshotJitterEma * 3.0;
                }
                adaptiveDelayRef.value = clamp(
                    targetDelay,
                    MIN_INTERPOLATION_DELAY_MS,
                    MAX_INTERPOLATION_DELAY_MS
                );
            }
        }
        lastSnapshotArrivalTime = snapshotTime;
    }

    function getSnapshotTimingEma() {
        return { interval: snapshotIntervalEma, jitter: snapshotJitterEma };
    }

    // ── Render target smoothing ──────────────────────────────────
    function applyRenderTarget(entity, targetX, targetY, targetRotation, positionGain, rotationGain, snapDistanceSq) {
        if (!entity) return;
        const gain = positionGain;

        if (!Number.isFinite(entity.render_x) || !Number.isFinite(entity.render_y)) {
            entity.render_x = targetX;
            entity.render_y = targetY;
        } else {
            const dx = targetX - entity.render_x;
            const dy = targetY - entity.render_y;
            const distSq = dx * dx + dy * dy;
            if (distSq <= 0.04 || distSq >= snapDistanceSq) {
                entity.render_x = targetX;
                entity.render_y = targetY;
            } else {
                entity.render_x += dx * gain;
                entity.render_y += dy * gain;
            }
        }

        if (Number.isFinite(targetRotation)) {
            if (!Number.isFinite(entity.render_rotation)) {
                entity.render_rotation = targetRotation;
            } else {
                const rotDiff = normalizeAngle(targetRotation - entity.render_rotation);
                if (Math.abs(rotDiff) > 2.4) {
                    entity.render_rotation = targetRotation;
                } else {
                    entity.render_rotation = normalizeAngle(entity.render_rotation + rotDiff * rotationGain);
                }
            }
        }
    }

    // ── Preset settings ──────────────────────────────────────────
    function normalizeCombatUiQuality(value) {
        const normalized = String(value || '').trim().toLowerCase();
        if (normalized === 'low' || normalized === 'high') return normalized;
        return 'auto';
    }

    function applyMobilePresetSettings(gameSettings, forceMobileClient, setControlsPanelHidden) {
        if (!forceMobileClient) return;
        gameSettings.graphicsQuality = 'low';
        gameSettings.combatUiQuality = 'low';
        gameSettings.particleEffects = false;
        gameSettings.screenShake = false;
        gameSettings.showFPS = false;
        gameSettings.showNetworkProfiler = false;
        gameSettings.sensitivity = Math.min(gameSettings.sensitivity || 1, 0.9);
        gameSettings.mobileAutoFireAim = gameSettings.mobileAutoFireAim !== false;
        gameSettings.mobileHaptics = gameSettings.mobileHaptics !== false;
        setControlsPanelHidden(true);
        document.body.classList.add('mobile-mode');
    }

    function applyBenchPresetSettings(gameSettings, BENCH_MODE, setControlsPanelHidden, setActiveEffectsProfile) {
        if (!BENCH_MODE) return;
        gameSettings.soundEnabled = false;
        gameSettings.soundVolume = 0;
        gameSettings.musicEnabled = false;
        gameSettings.musicVolume = 0;
        gameSettings.graphicsQuality = 'low';
        gameSettings.combatUiQuality = 'low';
        gameSettings.particleEffects = false;
        gameSettings.screenShake = false;
        gameSettings.showFPS = false;
        gameSettings.showNetworkProfiler = false;
        gameSettings.mobileHaptics = false;
        setActiveEffectsProfile('ultra');
        setControlsPanelHidden(true);
    }

    function applyStablePresetSettings(gameSettings, STABLE_MODE_FORCED, setFocusMode, setActiveEffectsProfile) {
        if (!STABLE_MODE_FORCED) return;
        gameSettings.graphicsQuality = 'low';
        gameSettings.combatUiQuality = 'low';
        gameSettings.particleEffects = false;
        gameSettings.screenShake = false;
        gameSettings.showFPS = false;
        gameSettings.showNetworkProfiler = false;
        setActiveEffectsProfile('ultra');
        setFocusMode(false);
    }

    function applyTournamentPresetSettings(gameSettings, TOURNAMENT_MODE_FORCED, setActiveEffectsProfile) {
        if (!TOURNAMENT_MODE_FORCED) return;
        gameSettings.musicEnabled = false;
        gameSettings.musicVolume = 0;
        gameSettings.graphicsQuality = 'medium';
        gameSettings.combatUiQuality = 'auto';
        gameSettings.particleEffects = true;
        gameSettings.screenShake = false;
        gameSettings.showFPS = true;
        gameSettings.showNetworkProfiler = true;
        setActiveEffectsProfile('dense');
    }

    return {
        updateConnectionQuality,
        getConnectionQuality,
        initNetworkChangeDetection,
        teardownNetworkChangeDetection,
        createConnectionStatusManager,
        connectionStatusTitles,
        CONNECTION_ERROR_FALLBACK,
        updateAdaptiveInterpolationDelay,
        getSnapshotTimingEma,
        applyRenderTarget,
        normalizeCombatUiQuality,
        applyMobilePresetSettings,
        applyBenchPresetSettings,
        applyStablePresetSettings,
        applyTournamentPresetSettings,
    };
}
