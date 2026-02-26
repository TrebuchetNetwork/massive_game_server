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

export function createConnectionManager(getCtx) {

    /** Timer id for the connection setup timeout. */
    let connectionTimeoutId = null;

    /** Counter for ICE restart attempts on the current peer connection. */
    let iceRestartAttempts = 0;

    function clearConnectionTimeout() {
        if (connectionTimeoutId !== null) {
            clearTimeout(connectionTimeoutId);
            connectionTimeoutId = null;
        }
    }

    function withJoinSelectionInUrl(rawUrl) {
        const ctx = getCtx();
        const { uiModeParams } = ctx;
        const joinTeam = uiModeParams.get('team') || '';
        const spectatorRequested = uiModeParams.get('spectator') === '1';
        if (!joinTeam && !spectatorRequested) {
            return rawUrl;
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
            return urlObj.toString();
        } catch (_) {
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
            } catch (_) { /* ignore malformed URL */ }
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
            applyConnectionStatus('negotiating', 'Establishing peer connection...');
            connectButton.disabled = true;
            connectButton.textContent = 'Connected';
            connectButton.classList.replace('bg-indigo-600', 'bg-gray-500');
            connectButton.classList.replace('hover:bg-indigo-700', 'cursor-not-allowed');
            initializePeerConnection();
            ctx.createOffer();
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
                        const currentSocket = getCtx().signalingSocket;
                        if (currentSocket && currentSocket.readyState === WebSocket.OPEN) {
                            currentSocket.send(JSON.stringify({ 'sdp': peerConnection.localDescription }));
                        }
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
                const currentSocket = getCtx().signalingSocket;
                if (currentSocket && currentSocket.readyState === WebSocket.OPEN) {
                    currentSocket.send(JSON.stringify({ 'ice': event.candidate }));
                }
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
            const currentSocket = getCtx().signalingSocket;
            if (currentSocket && currentSocket.readyState === WebSocket.OPEN) {
                currentSocket.send(JSON.stringify({ 'sdp': peerConnection.localDescription }));
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

    function resetConnectionUI(options = {}) {
        clearConnectionTimeout();
        iceRestartAttempts = 0;
        const ctx = getCtx();
        ctx.resetConnectionUIImpl(options);
    }

    return {
        withJoinSelectionInUrl,
        startConnectionAttempt,
        initializePeerConnection,
        setupDataChannelEvents,
        resetConnectionUI,
    };
}
