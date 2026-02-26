/**
 * ConnectionManager.js - WebRTC connection lifecycle management
 *
 * Extracted from client.html. Contains startConnectionAttempt,
 * resetConnectionUI, initializePeerConnection, setupDataChannelEvents,
 * and withJoinSelectionInUrl. Uses getCtx callback pattern.
 */

export function createConnectionManager(getCtx) {

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
            if (peerConnection.iceConnectionState === 'failed' || peerConnection.iceConnectionState === 'disconnected') {
                const detail = `ICE connection ${peerConnection.iceConnectionState}`;
                log(detail, 'error');
                currentCtx.setConnectionError(detail);
                resetConnectionUI({ allowReconnect: true, reconnectReason: detail });
            }
        };
        peerConnection.ondatachannel = (event) => {
            log('Data channel received from server.', 'success');
            ctx.setDataChannel(event.channel);
            setupDataChannelEvents(event.channel);
        };
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
            markJoinTimingStage('dataChannelOpenAtMs');
            applyConnectionStatus('waiting', 'Waiting for initial state...');
            controlsDiv.classList.remove('hidden');
            setupInputHandlers();
            ensureHudWidgets();
            loadSettings();
            initRenderAssetCache();
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
