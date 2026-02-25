/**
 * DiagnosticsManager.js - FX stress testing, join timing, e2e test hooks
 *
 * Extracted from client.html. Contains the FX stress system, join timing
 * instrumentation, and e2e diagnostics helpers. Uses getCtx callback pattern.
 */

export function createDiagnosticsManager(getCtx) {

    // ── FX helpers ──────────────────────────────────────────────────

    let syntheticFxSeedState = null;

    function createSeededFxRandom(seed) {
        let state = (Math.floor(Number(seed) || 0) >>> 0);
        if (state === 0) state = 1;
        return () => {
            state = (state * 1664525 + 1013904223) >>> 0;
            return state / 4294967296;
        };
    }

    function configureSyntheticFxSeed(seedValue) {
        if (!Number.isFinite(seedValue)) {
            syntheticFxSeedState = null;
            return null;
        }
        const normalizedSeed = (Math.floor(Number(seedValue)) >>> 0) || 1;
        syntheticFxSeedState = {
            seed: normalizedSeed,
            random: createSeededFxRandom(normalizedSeed)
        };
        return normalizedSeed;
    }

    function runWithSyntheticFxRandom(callback) {
        if (!syntheticFxSeedState || typeof callback !== 'function') {
            return typeof callback === 'function' ? callback() : 0;
        }
        const originalRandom = Math.random;
        Math.random = syntheticFxSeedState.random;
        try {
            return callback();
        } finally {
            Math.random = originalRandom;
        }
    }

    function applyFullFxMode() {
        const ctx = getCtx();
        const { gameSettings, effectsManager, fpsCounterDiv } = ctx;
        if (typeof gameSettings !== 'object' || !gameSettings) {
            return false;
        }
        gameSettings.graphicsQuality = 'high';
        gameSettings.particleEffects = true;
        gameSettings.screenShake = true;
        gameSettings.showFPS = true;
        ctx.setActiveEffectsProfileName('high');
        ctx.setLastAdaptiveEffectsEvalTime(0);
        if (effectsManager && typeof effectsManager.setParticlesEnabled === 'function') {
            effectsManager.setParticlesEnabled(true);
        }
        if (effectsManager && typeof effectsManager.setPerformanceProfile === 'function') {
            effectsManager.setPerformanceProfile('high');
        }
        if (fpsCounterDiv) {
            fpsCounterDiv.classList.remove('hidden');
        }
        if (window.__e2e) {
            window.__e2e.fullFxMode = true;
            window.__e2e.effectsProfile = 'high';
        }
        return true;
    }

    function getFxAnchorPosition() {
        const ctx = getCtx();
        const { localPlayerState, players } = ctx;
        if (localPlayerState && Number.isFinite(localPlayerState.x) && Number.isFinite(localPlayerState.y)) {
            return { x: localPlayerState.x, y: localPlayerState.y };
        }
        const firstPlayer = players.values().next().value;
        if (firstPlayer && Number.isFinite(firstPlayer.x) && Number.isFinite(firstPlayer.y)) {
            return { x: firstPlayer.x, y: firstPlayer.y };
        }
        return { x: 0, y: 0 };
    }

    function getFxInstigatorId() {
        const ctx = getCtx();
        const { myPlayerId, playerSprites, players } = ctx;
        if (myPlayerId && playerSprites.has(myPlayerId)) {
            return myPlayerId;
        }
        for (const [playerId] of playerSprites) {
            return playerId;
        }
        for (const [playerId] of players) {
            return playerId;
        }
        return null;
    }

    function getFxTargetId(instigatorId) {
        const ctx = getCtx();
        const { players } = ctx;
        if (!instigatorId) return null;
        for (const [playerId] of players) {
            if (playerId !== instigatorId) return playerId;
        }
        return instigatorId;
    }

    function emitSyntheticBattleFx(rawIntensity = 1, options = {}) {
        const ctx = getCtx();
        const {
            effectsManager, GP, app, gameScene, gameSettings,
            applyScreenShake, createScreenFlash,
        } = ctx;
        if (!effectsManager) return 0;

        const inlineSeed = Number(options.seed);
        if (Number.isFinite(inlineSeed)) {
            configureSyntheticFxSeed(inlineSeed);
        }

        return runWithSyntheticFxRandom(() => {
            applyFullFxMode();
            const intensity = Math.max(1, Math.min(40, Math.floor(Number(rawIntensity) || 1)));
            const includeScreenFx = options.includeScreenFx !== false;
            const instigatorId = getFxInstigatorId();
            const targetId = getFxTargetId(instigatorId);
            const base = getFxAnchorPosition();
            const weaponTypes = [
                GP.WeaponType.Pistol,
                GP.WeaponType.Shotgun,
                GP.WeaponType.Rifle,
                GP.WeaponType.Sniper
            ];

            let emittedEvents = 0;

            for (let i = 0; i < intensity; i += 1) {
                const angle = (i / Math.max(1, intensity)) * Math.PI * 2;
                const radius = 28 + (i % 8) * 18;
                const pos = {
                    x: base.x + Math.cos(angle) * radius,
                    y: base.y + Math.sin(angle) * radius
                };
                const weaponType = weaponTypes[i % weaponTypes.length];
                const damageValue = 18 + (i % 6) * 8;
                const damageType = (i % 2 === 0) ? 'enemyReceived' : 'enemyDealt';

                effectsManager.createEnhancedBulletImpact(pos, weaponType);
                effectsManager.createEnhancedDamageNumbers(pos, damageValue, damageType);
                emittedEvents += 2;

                if (i % 3 === 0) {
                    effectsManager.createEnhancedExplosion(pos, 28 + (i % 5) * 8);
                    emittedEvents += 1;
                }

                if (instigatorId) {
                    effectsManager.processGameEvent({
                        event_type: GP.GameEventType.WeaponFire,
                        position: pos,
                        weapon_type: weaponType,
                        value: 0,
                        instigator_id: instigatorId,
                        target_id: targetId
                    });
                    effectsManager.processGameEvent({
                        event_type: GP.GameEventType.PlayerDamageEffect,
                        position: pos,
                        weapon_type: weaponType,
                        value: damageValue,
                        instigator_id: instigatorId,
                        target_id: targetId
                    });
                    emittedEvents += 2;
                }
            }

            effectsManager.createEnhancedPowerupCollectEffect({
                x: base.x + 60,
                y: base.y - 36
            });
            effectsManager.createEnhancedFlagCaptureEffect({
                x: base.x - 72,
                y: base.y + 48
            });
            emittedEvents += 2;

            if (includeScreenFx && app) {
                if (gameSettings && gameSettings.screenShake && gameScene) {
                    applyScreenShake(gameScene, 8, Math.min(12, 3 + intensity * 0.25));
                }
                createScreenFlash(app, 0xFFE9AA, 8, 0.18);
                emittedEvents += 1;
            }

            if (window.__e2e) {
                window.__e2e.syntheticFxBursts = (window.__e2e.syntheticFxBursts || 0) + 1;
                window.__e2e.syntheticFxEvents = (window.__e2e.syntheticFxEvents || 0) + emittedEvents;
                window.__e2e.lastSyntheticFxBurstAt = performance.now();
            }
            return emittedEvents;
        });
    }

    // ── FX Stress ───────────────────────────────────────────────────

    let fxStressIntervalId = null;
    let fxStressConfig = null;

    function stopFxStress(clearProjectiles = false) {
        const ctx = getCtx();
        if (fxStressIntervalId) {
            clearInterval(fxStressIntervalId);
            fxStressIntervalId = null;
        }
        fxStressConfig = null;
        syntheticFxSeedState = null;
        if (clearProjectiles) {
            ctx.clearSyntheticProjectiles();
        }
        if (window.__e2e) {
            window.__e2e.fxStressActive = false;
            window.__e2e.syntheticFxSeed = null;
        }
        return true;
    }

    function startFxStress(options = {}) {
        const ctx = getCtx();
        const intervalMs = Math.max(25, Math.min(1000, Math.floor(Number(options.intervalMs) || 120)));
        const intensity = Math.max(1, Math.min(40, Math.floor(Number(options.intensity) || 6)));
        const syntheticProjectiles = Math.max(
            0,
            Math.min(5000, Math.floor(Number(options.syntheticProjectiles) || 0))
        );
        const includeScreenFx = options.includeScreenFx !== false;
        const requestedSeed = Number(options.seed);
        const normalizedSeed = Number.isFinite(requestedSeed)
            ? configureSyntheticFxSeed(requestedSeed)
            : null;

        applyFullFxMode();
        stopFxStress(false);

        if (normalizedSeed !== null) {
            configureSyntheticFxSeed(normalizedSeed);
        }

        if (syntheticProjectiles > 0) {
            ctx.setSyntheticProjectileCount(syntheticProjectiles);
        }

        fxStressConfig = {
            intervalMs,
            intensity,
            syntheticProjectiles,
            includeScreenFx,
            seed: normalizedSeed
        };

        const emitBurst = () => emitSyntheticBattleFx(intensity, { includeScreenFx });
        emitBurst();
        fxStressIntervalId = setInterval(emitBurst, intervalMs);

        if (window.__e2e) {
            window.__e2e.fxStressActive = true;
            window.__e2e.fxStressConfig = { ...fxStressConfig };
            window.__e2e.syntheticFxSeed = normalizedSeed;
        }
        return true;
    }

    function isFxStressActive() {
        return fxStressIntervalId !== null;
    }

    function getFxStressConfig() {
        return fxStressConfig;
    }

    // ── Join Timing ─────────────────────────────────────────────────

    let joinTimingAttemptSeq = 0;
    let joinTimingState = null;

    function createJoinTimingState(source = 'manual') {
        joinTimingAttemptSeq += 1;
        return {
            attemptId: joinTimingAttemptSeq,
            source: String(source || 'manual'),
            attemptStartAtMs: 0,
            signalingOpenAtMs: 0,
            offerCreatedAtMs: 0,
            localDescriptionAtMs: 0,
            answerReceivedAtMs: 0,
            remoteDescriptionAtMs: 0,
            firstIceCandidateAtMs: 0,
            dataChannelOpenAtMs: 0,
            firstPacketAtMs: 0,
            firstStateAtMs: 0,
            firstRenderAtMs: 0,
            completed: false,
            abortedAtMs: 0,
            abortedReason: ''
        };
    }

    function toRelativeJoinMs(targetAtMs, startAtMs) {
        if (!Number.isFinite(targetAtMs) || !Number.isFinite(startAtMs) || startAtMs <= 0) {
            return null;
        }
        return Number(Math.max(0, targetAtMs - startAtMs).toFixed(2));
    }

    function summarizeJoinTiming(timing) {
        if (!timing || !Number.isFinite(timing.attemptStartAtMs) || timing.attemptStartAtMs <= 0) {
            return null;
        }

        const startAtMs = timing.attemptStartAtMs;
        const completedAtMs = Number.isFinite(timing.firstRenderAtMs) && timing.firstRenderAtMs > 0
            ? timing.firstRenderAtMs
            : (Number.isFinite(timing.firstStateAtMs) && timing.firstStateAtMs > 0
                ? timing.firstStateAtMs
                : (Number.isFinite(timing.firstPacketAtMs) && timing.firstPacketAtMs > 0
                    ? timing.firstPacketAtMs
                    : null));

        return {
            signalingOpenMs: toRelativeJoinMs(timing.signalingOpenAtMs, startAtMs),
            offerCreatedMs: toRelativeJoinMs(timing.offerCreatedAtMs, startAtMs),
            localDescriptionMs: toRelativeJoinMs(timing.localDescriptionAtMs, startAtMs),
            answerReceivedMs: toRelativeJoinMs(timing.answerReceivedAtMs, startAtMs),
            remoteDescriptionMs: toRelativeJoinMs(timing.remoteDescriptionAtMs, startAtMs),
            firstIceCandidateMs: toRelativeJoinMs(timing.firstIceCandidateAtMs, startAtMs),
            dataChannelOpenMs: toRelativeJoinMs(timing.dataChannelOpenAtMs, startAtMs),
            firstPacketMs: toRelativeJoinMs(timing.firstPacketAtMs, startAtMs),
            firstStateMs: toRelativeJoinMs(timing.firstStateAtMs, startAtMs),
            firstRenderMs: toRelativeJoinMs(timing.firstRenderAtMs, startAtMs),
            totalMs: toRelativeJoinMs(completedAtMs, startAtMs)
        };
    }

    function buildJoinTimingSnapshot(timing) {
        const currentTiming = timing || createJoinTimingState('unknown');
        return {
            attemptId: currentTiming.attemptId,
            source: currentTiming.source,
            attemptStartAtMs: currentTiming.attemptStartAtMs,
            signalingOpenAtMs: currentTiming.signalingOpenAtMs,
            offerCreatedAtMs: currentTiming.offerCreatedAtMs,
            localDescriptionAtMs: currentTiming.localDescriptionAtMs,
            answerReceivedAtMs: currentTiming.answerReceivedAtMs,
            remoteDescriptionAtMs: currentTiming.remoteDescriptionAtMs,
            firstIceCandidateAtMs: currentTiming.firstIceCandidateAtMs,
            dataChannelOpenAtMs: currentTiming.dataChannelOpenAtMs,
            firstPacketAtMs: currentTiming.firstPacketAtMs,
            firstStateAtMs: currentTiming.firstStateAtMs,
            firstRenderAtMs: currentTiming.firstRenderAtMs,
            completed: !!currentTiming.completed,
            abortedAtMs: currentTiming.abortedAtMs,
            abortedReason: currentTiming.abortedReason || '',
            summary: summarizeJoinTiming(currentTiming)
        };
    }

    function publishJoinTiming() {
        if (!window.__e2e) return;
        window.__e2e.joinTiming = buildJoinTimingSnapshot(joinTimingState);
    }

    function startJoinTimingAttempt(source = 'manual') {
        if (window.__e2e?.joinTiming) {
            window.__e2e.lastJoinTiming = window.__e2e.joinTiming;
        }
        joinTimingState = createJoinTimingState(source);
        joinTimingState.attemptStartAtMs = performance.now();
        publishJoinTiming();
    }

    function markJoinTimingStage(stageKey, stageNowMs = performance.now()) {
        if (!joinTimingState) {
            joinTimingState = createJoinTimingState('implicit');
        }
        if (!Object.prototype.hasOwnProperty.call(joinTimingState, stageKey)) return;
        if (Number.isFinite(joinTimingState[stageKey]) && joinTimingState[stageKey] > 0) return;
        joinTimingState[stageKey] = Number(stageNowMs) || performance.now();
        if (stageKey === 'firstRenderAtMs' || stageKey === 'firstStateAtMs') {
            joinTimingState.completed = true;
        }
        publishJoinTiming();
    }

    function markJoinTimingAborted(reason = '') {
        if (!joinTimingState || joinTimingState.completed) {
            return;
        }
        if (!Number.isFinite(joinTimingState.abortedAtMs) || joinTimingState.abortedAtMs <= 0) {
            joinTimingState.abortedAtMs = performance.now();
        }
        if (reason) {
            joinTimingState.abortedReason = String(reason);
        }
        publishJoinTiming();
    }

    function getJoinTimingState() {
        return joinTimingState;
    }

    // Initialize
    joinTimingState = createJoinTimingState('idle');
    publishJoinTiming();

    return {
        // FX
        applyFullFxMode,
        emitSyntheticBattleFx,
        startFxStress,
        stopFxStress,
        isFxStressActive,
        getFxStressConfig,
        configureSyntheticFxSeed,
        // Join timing
        startJoinTimingAttempt,
        markJoinTimingStage,
        markJoinTimingAborted,
        getJoinTimingState,
        publishJoinTiming,
    };
}
