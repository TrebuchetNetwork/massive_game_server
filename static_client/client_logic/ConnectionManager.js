import { CLIENT_PROTOCOL_VERSION } from './protocol_version.js';

/**
 * ConnectionManager.js - WebRTC connection lifecycle management
 *
 * Extracted from client.html. Contains startConnectionAttempt,
 * resetConnectionUI, initializePeerConnection, setupDataChannelEvents,
 * and withJoinSelectionInUrl. Uses getCtx callback pattern.
 */

/** Default timeout for WebRTC connection setup (ms). */
const CONNECTION_TIMEOUT_MS = 15000;

/** Maximum ICE restart attempts before falling back to full reconnect. */
const MAX_ICE_RESTART_ATTEMPTS = 2;

/** Maximum time to wait for server-provided ICE config before proceeding with defaults (ms). */
const ICE_CONFIG_WAIT_MS = 2000;

export function createConnectionManager(getCtx) {

    /** Timer id for the connection setup timeout. */
    let connectionTimeoutId = null;

    /** Counter for ICE restart attempts on the current peer connection. */
    let iceRestartAttempts = 0;

    /** Timer id for waiting on server-provided ICE config. */
    let iceConfigTimeoutId = null;

    /** Whether the peer connection has been initialized for the current attempt. */
    let peerConnectionInitialized = false;

    function logSuppressedError(context, error, level = 'warn') {
        const ctx = getCtx();
        if (typeof ctx.log !== 'function') return;
        const detail = error?.message || error || 'unknown error';
        ctx.log(`${context}: ${detail}`, level);
    }

    function clearConnectionTimeout() {
        if (connectionTimeoutId !== null) {
            clearTimeout(connectionTimeoutId);
            connectionTimeoutId = null;
        }
        if (iceConfigTimeoutId !== null) {
            clearTimeout(iceConfigTimeoutId);
            iceConfigTimeoutId = null;
        }
    }

    function sendSignalingPayload(payload) {
        const currentSocket = getCtx().signalingSocket;
        if (!currentSocket || currentSocket.readyState !== WebSocket.OPEN) {
            return false;
        }
        currentSocket.send(JSON.stringify({
            ...payload,
            protocol_version: CLIENT_PROTOCOL_VERSION,
        }));
        return true;
    }

    /**
     * Merge server-provided ICE servers into the current peerConnectionConfig.
     * Server-provided TURN servers with credentials are added alongside the
     * client-configured servers, with deduplication by URL.
     */
    function mergeServerIceServers(serverIceServers) {
        const ctx = getCtx();
        const config = ctx.peerConnectionConfig;
        if (!config || !config.iceServers) return;

        const existingKeys = new Set();
        config.iceServers.forEach(s => {
            const urls = Array.isArray(s.urls) ? s.urls : [s.urls];
            urls.forEach(u => existingKeys.add(String(u)));
        });

        for (const server of serverIceServers) {
            const urls = Array.isArray(server.urls) ? server.urls : [server.urls];
            const newUrls = urls.filter(u => !existingKeys.has(String(u)));
            if (newUrls.length === 0) {
                // URLs already present — but update credentials if the server
                // provided them (e.g. HMAC time-limited credentials).
                if (server.username || server.credential) {
                    const existing = config.iceServers.find(s => {
                        const eu = Array.isArray(s.urls) ? s.urls : [s.urls];
                        return urls.some(u => eu.includes(u));
                    });
                    if (existing) {
                        if (server.username) existing.username = server.username;
                        if (server.credential) existing.credential = server.credential;
                    }
                }
                continue;
            }
            const entry = { urls: newUrls.length === 1 ? newUrls[0] : newUrls };
            if (server.username) entry.username = server.username;
            if (server.credential) entry.credential = server.credential;
            config.iceServers.push(entry);
        }
    }

    /**
     * Begin WebRTC peer negotiation. Called either when the server ICE config
     * is received, or after a timeout if no config arrives.
     */
    function beginPeerNegotiation() {
        if (peerConnectionInitialized) return;
        peerConnectionInitialized = true;
        const ctx = getCtx();
        const { log, applyConnectionStatus } = ctx;
        applyConnectionStatus('negotiating', 'Establishing peer connection...');
        initializePeerConnection();
        ctx.createOffer();
    }

    function withJoinSelectionInUrl(rawUrl) {
        const ctx = getCtx();
        const { uiModeParams } = ctx;
        const joinTeam = uiModeParams.get('team') || '';
        const spectatorRequested = uiModeParams.get('spectator') === '1';
        const selectedMatchTypeRaw = typeof ctx.getSelectedMatchType === 'function'
            ? ctx.getSelectedMatchType()
            : (uiModeParams.get('match_type') || '');
        const selectedMatchType = (() => {
            const normalized = String(selectedMatchTypeRaw || '').trim().toLowerCase();
            if (!normalized) return '';
            if (normalized === 'full') return 'full';
            if (normalized === 'quick') return 'quick';
            if (normalized === 'mobile_blitz' || normalized === 'blitz') return 'mobile_blitz';
            if (normalized === 'mobile_standard' || normalized === 'mobile') return 'mobile_standard';
            return '';
        })();
        const preferredNameRaw = typeof ctx.getPreferredPlayerName === 'function'
            ? ctx.getPreferredPlayerName()
            : '';
        const preferredName = String(preferredNameRaw || '').trim();
        if (!joinTeam && !spectatorRequested) {
            if (!preferredName) {
                if (!selectedMatchType) return rawUrl;
                try {
                    const urlObj = new URL(rawUrl);
                    urlObj.searchParams.set('match_type', selectedMatchType);
                    return urlObj.toString();
                } catch (error) {
                    logSuppressedError('Failed to apply match_type join param', error);
                    return rawUrl;
                }
            }
            try {
                const urlObj = new URL(rawUrl);
                urlObj.searchParams.set('username', preferredName);
                if (selectedMatchType) {
                    urlObj.searchParams.set('match_type', selectedMatchType);
                }
                return urlObj.toString();
            } catch (error) {
                logSuppressedError('Failed to apply username join param', error);
                return rawUrl;
            }
        }
        try {
            const urlObj = new URL(rawUrl);
            if (spectatorRequested && !joinTeam) {
                urlObj.searchParams.set('team_id', '0');
            } else if (joinTeam) {
                const normalized = String(joinTeam).trim().toLowerCase();
                if (normalized === 'spectator' || normalized === 'spec') {
                    urlObj.searchParams.set('team_id', '0');
                } else if (normalized === '1' || normalized === '2') {
                    urlObj.searchParams.set('team_id', normalized);
                } else {
                    urlObj.searchParams.set('team', joinTeam);
                }
            }
            if (preferredName) {
                urlObj.searchParams.set('username', preferredName);
            }
            if (selectedMatchType) {
                urlObj.searchParams.set('match_type', selectedMatchType);
            }
            return urlObj.toString();
        } catch (error) {
            logSuppressedError('Failed to apply team/spectator join params', error);
            return rawUrl;
        }
    }

    function startConnectionAttempt(options = {}) {
        const ctx = getCtx();
        const {
            log, normalizeSignalingUrl, summarizeSignalingError,
            wsUrlInput, isMobileDevice, dataSaverMode, PIXI,
            canStartConnectionAttempt, initCullWorker, cullWorker, WORKER_CULL_ENABLED,
            startJoinTimingAttempt, markJoinTimingStage, markJoinTimingAborted,
            withAuthTokenInUrl, setConnectionError, clearConnectionOverride,
            clearReconnectTimer, scheduleAutoReconnect, applyConnectionStatus,
            connectButton, peerConnectionConfig, GameProtocol, GP,
        } = ctx;

        const isRetry = !!options.isRetry;
        if (!canStartConnectionAttempt()) {
            return false;
        }

        // (#30) On retry, clear any residual game state from the previous
        // session so ghost entities do not persist across reconnections.
        if (isRetry && typeof ctx.clearGameStateForReconnect === 'function') {
            ctx.clearGameStateForReconnect();
        }

        // Reset ICE restart counter for the new connection attempt.
        iceRestartAttempts = 0;

        if (!cullWorker && WORKER_CULL_ENABLED) {
            initCullWorker();
        }
        startJoinTimingAttempt(isRetry ? 'retry' : 'manual');

        const normalized = normalizeSignalingUrl(wsUrlInput.value);
        if (!normalized.ok) {
            log(normalized.error, 'error');
            setConnectionError(normalized.error);
            markJoinTimingAborted(normalized.error);
            if (isRetry) {
                scheduleAutoReconnect('Invalid signaling URL');
            }
            return false;
        }

        const url = normalized.url;
        let wsConnectUrl = withJoinSelectionInUrl(withAuthTokenInUrl(url));
        if (isMobileDevice || dataSaverMode) {
            try {
                const mobileUrl = new URL(wsConnectUrl);
                mobileUrl.searchParams.set('is_mobile', 'true');
                wsConnectUrl = mobileUrl.toString();
            } catch (error) {
                logSuppressedError('Failed to append mobile signaling URL flag', error);
            }
        }
        ctx.setActiveSignalingUrl(url);
        wsUrlInput.value = url;
        if (window.__e2e) {
            window.__e2e.activeSignalingUrl = url;
        }
        ctx.setConnectAttemptInFlight(true);
        ctx.setHasAttemptedConnection(true);
        clearConnectionOverride();
        clearReconnectTimer();
        applyConnectionStatus(
            'connecting',
            isRetry ? 'Retrying signaling connection...' : 'Contacting signaling server...'
        );
        log(`${isRetry ? 'Reconnecting' : 'Connecting'} to signaling server: ${url}`);
        const signalingSocket = new WebSocket(wsConnectUrl);
        ctx.setSignalingSocket(signalingSocket);

        // (#52) Start a connection timeout. If the data channel is not open
        // within CONNECTION_TIMEOUT_MS, abort and schedule a reconnect.
        clearConnectionTimeout();
        connectionTimeoutId = setTimeout(() => {
            connectionTimeoutId = null;
            const currentCtx = getCtx();
            const dc = currentCtx.dataChannel;
            if (dc && dc.readyState === 'open') {
                return; // Connection succeeded before timeout fired.
            }
            const detail = `Connection timed out after ${CONNECTION_TIMEOUT_MS / 1000}s`;
            log(detail, 'error');
            currentCtx.setConnectionError(detail);
            markJoinTimingAborted(detail);
            resetConnectionUI({ allowReconnect: true, reconnectReason: detail });
        }, CONNECTION_TIMEOUT_MS);

        signalingSocket.onopen = () => {
            ctx.setConnectAttemptInFlight(false);
            markJoinTimingStage('signalingOpenAtMs');
            log('Connected to signaling server.', 'success');
            applyConnectionStatus('negotiating', 'Waiting for server ICE config...');
            connectButton.disabled = true;
            connectButton.textContent = 'Connected';
            connectButton.classList.replace('bg-indigo-600', 'bg-gray-500');
            connectButton.classList.replace('hover:bg-indigo-700', 'cursor-not-allowed');

            // Wait briefly for the server to send ICE server configuration
            // (including TURN credentials). If none arrives, proceed with
            // the client-side defaults.
            peerConnectionInitialized = false;
            iceConfigTimeoutId = setTimeout(() => {
                iceConfigTimeoutId = null;
                if (!peerConnectionInitialized) {
                    log('No server ICE config received; using client defaults.', 'info');
                    beginPeerNegotiation();
                }
            }, ICE_CONFIG_WAIT_MS);
        };

        signalingSocket.onmessage = async (event) => {
            const ctx2 = getCtx();
            const peerConnection = ctx2.peerConnection;
            let msg;
            try {
                msg = JSON.parse(event.data);
            } catch (error) {
                log(`Ignoring malformed signaling message: ${error?.message || error}`, 'warn');
                return;
            }
            if (msg.event === 'ice_servers') {
                // Server-provided ICE configuration (may include TURN credentials).
                const serverIceServers = msg.ice_servers;
                if (Array.isArray(serverIceServers) && serverIceServers.length > 0) {
                    const turnCount = serverIceServers.filter(
                        s => Array.isArray(s.urls)
                            ? s.urls.some(u => String(u).startsWith('turn:'))
                            : String(s.urls || '').startsWith('turn:')
                    ).length;
                    log(`Received ${serverIceServers.length} ICE server(s) from server (${turnCount} TURN).`, 'info');
                    if (typeof window !== 'undefined' && window.__e2e) {
                        window.__e2e.serverIceServerCount = serverIceServers.length;
                        window.__e2e.serverTurnCount = turnCount;
                    }
                    // Merge server-provided ICE servers into the peer connection config.
                    mergeServerIceServers(serverIceServers);
                }
                // Now create the peer connection with the updated config.
                if (!peerConnectionInitialized) {
                    if (iceConfigTimeoutId !== null) {
                        clearTimeout(iceConfigTimeoutId);
                        iceConfigTimeoutId = null;
                    }
                    beginPeerNegotiation();
                }
                return;
            }
            if (msg.event === 'sdp_offer_queue') {
                const queueHint = Number(msg.queue_position_hint);
                const queueText = Number.isFinite(queueHint) && queueHint > 0
                    ? `Queued for SDP processing (position ~${queueHint})...`
                    : 'Queued for SDP processing...';
                applyConnectionStatus('negotiating', queueText);
                log(queueText, 'info');
                return;
            }
            if (msg.error) {
                const detail = String(msg.detail || msg.error || 'Server rejected join request');
                log(detail, 'warn');
                setConnectionError(detail);
                markJoinTimingAborted(detail);
                applyConnectionStatus('error', detail);
                if (msg.error === 'protocol_version_mismatch') {
                    resetConnectionUI({ allowReconnect: false });
                }
                return;
            }

            if (msg.sdp) {
                try {
                    if (msg.sdp.type === 'answer') {
                        markJoinTimingStage('answerReceivedAtMs');
                    }
                    await peerConnection.setRemoteDescription(new RTCSessionDescription(msg.sdp));
                    markJoinTimingStage('remoteDescriptionAtMs');
                    if (msg.sdp.type === 'offer') {
                        log('Server sent offer, creating answer...', 'info');
                        const answer = await peerConnection.createAnswer();
                        await peerConnection.setLocalDescription(answer);
                        markJoinTimingStage('localDescriptionAtMs');
                        sendSignalingPayload({ sdp: peerConnection.localDescription });
                    }
                } catch (e) {
                    log(`Error setting remote desc: ${e}`, 'error');
                }
            } else if (msg.ice) {
                markJoinTimingStage('firstIceCandidateAtMs');
                try {
                    await peerConnection.addIceCandidate(new RTCIceCandidate(msg.ice));
                } catch (e) {
                    // Benign errors often happen with ICE candidates
                }
            }
        };

        signalingSocket.onerror = (e) => {
            ctx.setConnectAttemptInFlight(false);
            clearConnectionTimeout();
            const detail = summarizeSignalingError(e, signalingSocket, url);
            log(detail, 'error');
            setConnectionError(detail);
            markJoinTimingAborted(detail);
            resetConnectionUI({ allowReconnect: true, reconnectReason: detail });
        };

        signalingSocket.onclose = (event) => {
            ctx.setConnectAttemptInFlight(false);
            const currentCtx = getCtx();
            if (currentCtx.dataChannel && currentCtx.dataChannel.readyState === 'open') {
                log('Signaling channel closed after negotiation; data channel remains open.', 'warn');
                return;
            }
            clearConnectionTimeout();
            const closeCode = typeof event?.code === 'number' ? event.code : 'unknown';
            const closeReason = event?.reason ? ` reason="${event.reason}"` : '';
            const clean = event?.wasClean ? 'clean' : 'unclean';
            const detail = `Signaling closed (${clean}, code=${closeCode}${closeReason})`;
            log(detail, 'error');
            setConnectionError(detail);
            markJoinTimingAborted(detail);
            resetConnectionUI({ allowReconnect: true, reconnectReason: detail });
        };

        return true;
    }

    function initializePeerConnection() {
        const ctx = getCtx();
        const { log, peerConnectionConfig, setupDataChannelEvents: _sdc } = ctx;

        // (#55) Close any existing peer connection before creating a new one
        // to prevent resource leaks (media streams, ICE agents, etc.).
        const existingPc = ctx.peerConnection;
        if (existingPc) {
            log('Closing previous RTCPeerConnection before creating a new one.', 'info');
            try {
                existingPc.onicecandidate = null;
                existingPc.oniceconnectionstatechange = null;
                existingPc.ondatachannel = null;
                existingPc.close();
            } catch (e) {
                log(`Error closing previous peer connection: ${e?.message || e}`, 'warn');
            }
            ctx.setPeerConnection(null);
        }

        log(`Initializing RTCPeerConnection (${peerConnectionConfig.iceServers.length} ICE server(s))...`);
        const peerConnection = new RTCPeerConnection(peerConnectionConfig);
        ctx.setPeerConnection(peerConnection);

        peerConnection.onicecandidate = (event) => {
            if (event.candidate) {
                sendSignalingPayload({ ice: event.candidate });
            }
        };
        try {
            // Create the outbound game data channel before creating an SDP offer.
            // Without this, the generated offer can omit data-channel transport sections.
            const outboundDataChannel = peerConnection.createDataChannel('gameDataChannel', {
                ordered: false,
                maxRetransmits: 0,
            });
            ctx.setDataChannel(outboundDataChannel);
            log(`Data channel "${outboundDataChannel.label}" created by client.`, 'info');
            setupDataChannelEvents(outboundDataChannel);
        } catch (error) {
            log(`Failed to create local data channel: ${error?.message || error}`, 'error');
        }
        peerConnection.oniceconnectionstatechange = () => {
            const currentCtx = getCtx();
            log(`ICE state: ${peerConnection.iceConnectionState}`, 'info');

            // (#32) On ICE failure or disconnection, attempt an ICE restart
            // before falling back to a full reconnect. This avoids tearing
            // down the entire session for transient network blips.
            if (peerConnection.iceConnectionState === 'failed' || peerConnection.iceConnectionState === 'disconnected') {
                if (iceRestartAttempts < MAX_ICE_RESTART_ATTEMPTS) {
                    iceRestartAttempts += 1;
                    const attempt = iceRestartAttempts;
                    log(`Attempting ICE restart (${attempt}/${MAX_ICE_RESTART_ATTEMPTS})...`, 'warn');
                    currentCtx.applyConnectionStatus(
                        'negotiating',
                        `ICE ${peerConnection.iceConnectionState} - restarting (${attempt}/${MAX_ICE_RESTART_ATTEMPTS})...`
                    );
                    attemptIceRestart(peerConnection);
                    return;
                }
                const detail = `ICE connection ${peerConnection.iceConnectionState} (after ${MAX_ICE_RESTART_ATTEMPTS} restart attempts)`;
                log(detail, 'error');
                currentCtx.setConnectionError(detail);
                resetConnectionUI({ allowReconnect: true, reconnectReason: detail });
            }

            // Reset ICE restart counter on successful reconnection.
            if (peerConnection.iceConnectionState === 'connected' || peerConnection.iceConnectionState === 'completed') {
                if (iceRestartAttempts > 0) {
                    log(`ICE restart succeeded after ${iceRestartAttempts} attempt(s).`, 'success');
                }
                iceRestartAttempts = 0;

                // Log the active ICE candidate pair type to detect TURN relay usage.
                logSelectedCandidatePairType(peerConnection, log);
            }
        };
        peerConnection.ondatachannel = (event) => {
            log('Data channel received from server.', 'success');
            ctx.setDataChannel(event.channel);
            setupDataChannelEvents(event.channel);
        };
    }

    /**
     * (#32) Attempt an ICE restart by creating a new offer with
     * { iceRestart: true } and sending it over the signaling socket.
     */
    async function attemptIceRestart(peerConnection) {
        const ctx = getCtx();
        const { log } = ctx;
        try {
            const offer = await peerConnection.createOffer({ iceRestart: true });
            await peerConnection.setLocalDescription(offer);
            if (sendSignalingPayload({ sdp: peerConnection.localDescription })) {
                log('ICE restart offer sent.', 'info');
            } else {
                log('Cannot send ICE restart offer: signaling socket not open. Falling back to full reconnect.', 'warn');
                const detail = 'ICE restart failed (signaling socket closed)';
                ctx.setConnectionError(detail);
                resetConnectionUI({ allowReconnect: true, reconnectReason: detail });
            }
        } catch (e) {
            log(`ICE restart failed: ${e?.message || e}. Falling back to full reconnect.`, 'error');
            const detail = `ICE restart error: ${e?.message || e}`;
            ctx.setConnectionError(detail);
            resetConnectionUI({ allowReconnect: true, reconnectReason: detail });
        }
    }

    function setupDataChannelEvents(dcInstance) {
        const ctx = getCtx();
        const {
            log, GP, GameProtocol, PIXI, processServerUpdate,
            tryProcessDeltaMessageFast, parseFlatBufferMessage, unpackCoalescedPackets,
            markJoinTimingStage, markJoinTimingComplete,
            setConnectionError, applyConnectionStatus,
            controlsDiv, setupInputHandlers, ensureHudWidgets,
            loadSettings, initRenderAssetCache,
        } = ctx;

        dcInstance.binaryType = 'arraybuffer';
        dcInstance.onopen = () => {
            log('Data channel open!', 'success');
            // Connection is established; cancel any pending timeout.
            clearConnectionTimeout();
            markJoinTimingStage('dataChannelOpenAtMs');
            applyConnectionStatus('waiting', 'Waiting for initial state...');
            controlsDiv.classList.remove('hidden');
            setupInputHandlers();
            ensureHudWidgets();
            loadSettings();
            initRenderAssetCache();

            // (#30) Reset reconnect state on successful connection so the
            // next disconnect starts with a clean attempt counter.
            const currentCtx = getCtx();
            if (typeof currentCtx.resetReconnectState === 'function') {
                currentCtx.resetReconnectState();
            }

            if (window.__e2e) {
                window.__e2e.dataChannelOpen = true;
                window.__e2e.dataChannelLabel = dcInstance.label || '';
            }
        };

        dcInstance.onmessage = (event) => {
            const currentCtx = getCtx();
            const data = event.data;
            const byteLength = data instanceof ArrayBuffer
                ? data.byteLength
                : (typeof data === 'string' ? data.length : 0);

            currentCtx.trackNetworkProfilerMessage(byteLength);

            if (data instanceof ArrayBuffer) {
                const byteView = new Uint8Array(data);
                const packets = (typeof unpackCoalescedPackets === 'function'
                    ? unpackCoalescedPackets(byteView)
                    : null) || [byteView];
                for (let i = 0; i < packets.length; i += 1) {
                    const packet = packets[i];
                    const fastApplied = tryProcessDeltaMessageFast(packet, window.__e2e || null);
                    if (fastApplied) {
                        currentCtx.trackFastDeltaPacket();
                        continue;
                    }
                    currentCtx.trackFullParsePacket();
                    const parsed = parseFlatBufferMessage(packet);
                    if (!parsed) continue;
                    if (parsed.type === 'protocol_error') {
                        const detail = parsed.detail || 'Protocol mismatch';
                        currentCtx.setConnectionError(detail);
                        applyConnectionStatus('error', detail);
                        resetConnectionUI({ allowReconnect: false });
                        return;
                    }
                    currentCtx.handleParsedMessage(parsed);
                }
                return;
            } else {
                let parsed;
                try {
                    parsed = JSON.parse(data);
                } catch (e) {
                    log('Failed to parse server message: ' + e.message, 'error');
                    return;
                }
                if (!parsed) return;
                currentCtx.handleParsedMessage(parsed);
            }
        };

        dcInstance.onerror = (e) => {
            log(`Data channel error: ${e?.error?.message || e}`, 'error');
        };

        dcInstance.onclose = () => {
            log('Data channel closed.', 'warn');
            clearConnectionTimeout();
            const detail = 'Data channel closed unexpectedly';
            const currentCtx = getCtx();
            currentCtx.setConnectionError(detail);
            resetConnectionUI({ allowReconnect: true, reconnectReason: detail });
            if (window.__e2e) {
                window.__e2e.dataChannelOpen = false;
            }
        };
    }

    /**
     * Log the selected ICE candidate pair type after connection is established.
     * Detects TURN relay fallback and logs a warning so operators know.
     */
    function logSelectedCandidatePairType(peerConnection, logFn) {
        try {
            if (typeof peerConnection.getStats !== 'function') return;
            peerConnection.getStats().then(stats => {
                let selectedPairId = null;
                // Find the active transport's selected candidate pair.
                stats.forEach(report => {
                    if (report.type === 'transport' && report.selectedCandidatePairId) {
                        selectedPairId = report.selectedCandidatePairId;
                    }
                });
                if (!selectedPairId) {
                    // Fallback: look for a succeeded pair directly.
                    stats.forEach(report => {
                        if (report.type === 'candidate-pair' && report.state === 'succeeded' && report.nominated) {
                            selectedPairId = report.id;
                        }
                    });
                }
                if (!selectedPairId) return;

                const pair = stats.get(selectedPairId);
                if (!pair) return;

                const localCandidate = stats.get(pair.localCandidateId);
                const remoteCandidate = stats.get(pair.remoteCandidateId);
                const localType = localCandidate?.candidateType || 'unknown';
                const remoteType = remoteCandidate?.candidateType || 'unknown';

                if (localType === 'relay' || remoteType === 'relay') {
                    logFn(
                        `ICE connection using TURN relay (local=${localType}, remote=${remoteType}). ` +
                        'Direct/STUN connectivity was not possible.',
                        'warn'
                    );
                } else {
                    logFn(
                        `ICE candidate pair: local=${localType}, remote=${remoteType}`,
                        'info'
                    );
                }
            }).catch((error) => {
                logFn(`ICE candidate pair stats unavailable: ${error?.message || error}`, 'warn');
            });
        } catch (error) {
            logFn(`ICE candidate pair detection failed: ${error?.message || error}`, 'warn');
        }
    }

    function onConnectionReset() {
        clearConnectionTimeout();
        iceRestartAttempts = 0;
        peerConnectionInitialized = false;
    }

    function destroy() {
        onConnectionReset();
    }

    function resetConnectionUI(options = {}) {
        clearConnectionTimeout();
        iceRestartAttempts = 0;
        peerConnectionInitialized = false;
        const ctx = getCtx();
        ctx.resetConnectionUIImpl(options);
    }

    return {
        withJoinSelectionInUrl,
        startConnectionAttempt,
        initializePeerConnection,
        setupDataChannelEvents,
        resetConnectionUI,
        onConnectionReset,
        destroy,
    };
}
