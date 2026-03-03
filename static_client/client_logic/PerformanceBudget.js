/**
 * PerformanceBudget.js - Performance LOD, profiler, and WebGPU layer management
 *
 * Extracted from client.html. Contains performance interval tuning,
 * background throttling, world-view culling, cull worker management,
 * render LOD/cap functions, adaptive effects profiling, render resolution
 * budgeting, ultra-performance mode, lightweight profiler, synthetic
 * projectile management, and accelerated (WebGPU/WebGL2) layer init/disable.
 * Uses getCtx callback pattern to access shared game state.
 */

export function createPerformanceBudget(getCtx) {

    // ── Performance interval tuning ─────────────────────────────────

    function refreshRuntimePerfIntervals() {
        const ctx = getCtx();
        if (ctx.backgroundThrottleActive) {
            ctx.setUI_UPDATE_INTERVAL_MS(400);
            ctx.setMINIMAP_UPDATE_INTERVAL_MS(1600);
            ctx.setSTARFIELD_UPDATE_INTERVAL_MS(600);
            ctx.setFOG_UPDATE_INTERVAL_MS(900);
            ctx.setPICKUP_ANIM_INTERVAL_MS(220);
            ctx.setFLAG_ANIM_INTERVAL_MS(280);
            ctx.setEFFECTS_UPDATE_INTERVAL_MS(500);
            return;
        }
        if (ctx.BENCH_MODE) {
            ctx.setUI_UPDATE_INTERVAL_MS(250);
            ctx.setMINIMAP_UPDATE_INTERVAL_MS(700);
            ctx.setSTARFIELD_UPDATE_INTERVAL_MS(180);
            ctx.setFOG_UPDATE_INTERVAL_MS(180);
            ctx.setPICKUP_ANIM_INTERVAL_MS(90);
            ctx.setFLAG_ANIM_INTERVAL_MS(100);
            ctx.setEFFECTS_UPDATE_INTERVAL_MS(180);
            return;
        }
        if (ctx.STABLE_MODE_FORCED) {
            ctx.setUI_UPDATE_INTERVAL_MS(140);
            ctx.setMINIMAP_UPDATE_INTERVAL_MS(900);
            ctx.setSTARFIELD_UPDATE_INTERVAL_MS(180);
            ctx.setFOG_UPDATE_INTERVAL_MS(220);
            ctx.setPICKUP_ANIM_INTERVAL_MS(90);
            ctx.setFLAG_ANIM_INTERVAL_MS(120);
            ctx.setEFFECTS_UPDATE_INTERVAL_MS(180);
            return;
        }
        if (ctx.TOURNAMENT_MODE_FORCED) {
            ctx.setUI_UPDATE_INTERVAL_MS(110);
            ctx.setMINIMAP_UPDATE_INTERVAL_MS(300);
            ctx.setSTARFIELD_UPDATE_INTERVAL_MS(66);
            ctx.setFOG_UPDATE_INTERVAL_MS(90);
            ctx.setPICKUP_ANIM_INTERVAL_MS(66);
            ctx.setFLAG_ANIM_INTERVAL_MS(66);
            ctx.setEFFECTS_UPDATE_INTERVAL_MS(66);
            return;
        }
        if (ctx.ultraPerformanceMode) {
            ctx.setUI_UPDATE_INTERVAL_MS(180);
            ctx.setMINIMAP_UPDATE_INTERVAL_MS(1800);
            ctx.setSTARFIELD_UPDATE_INTERVAL_MS(200);
            ctx.setFOG_UPDATE_INTERVAL_MS(250);
            ctx.setPICKUP_ANIM_INTERVAL_MS(110);
            ctx.setFLAG_ANIM_INTERVAL_MS(140);
            ctx.setEFFECTS_UPDATE_INTERVAL_MS(240);
            return;
        }
        ctx.setUI_UPDATE_INTERVAL_MS(100);
        ctx.setMINIMAP_UPDATE_INTERVAL_MS(200);
        ctx.setSTARFIELD_UPDATE_INTERVAL_MS(33);
        ctx.setFOG_UPDATE_INTERVAL_MS(33);
        ctx.setPICKUP_ANIM_INTERVAL_MS(33);
        ctx.setFLAG_ANIM_INTERVAL_MS(33);
        ctx.setEFFECTS_UPDATE_INTERVAL_MS(33);
    }

    function getForegroundTickerMaxFps() {
        const ctx = getCtx();
        if (ctx.BENCH_MODE) return ctx.BENCH_MAX_FPS;
        // Mobile/battery-aware FPS cap
        if (typeof ctx.getEffectiveFPSCap === 'function') return ctx.getEffectiveFPSCap();
        return 60;
    }

    function applyBackgroundThrottling(forceHidden = document.hidden) {
        const ctx = getCtx();
        const shouldThrottle = ctx.TAB_THROTTLE_ENABLED && !!forceHidden;
        if (ctx.backgroundThrottleActive === shouldThrottle) {
            return;
        }

        ctx.setBackgroundThrottleActive(shouldThrottle);
        refreshRuntimePerfIntervals();

        if (ctx.app?.ticker) {
            const targetMaxFps = shouldThrottle
                ? Math.min(getForegroundTickerMaxFps(), ctx.BACKGROUND_TAB_MAX_FPS)
                : getForegroundTickerMaxFps();
            ctx.app.ticker.maxFPS = targetMaxFps;
        }

        if (shouldThrottle && typeof ctx.closePingWheel === 'function') {
            ctx.closePingWheel();
        }

        if (window.__e2e) {
            window.__e2e.backgroundThrottleActive = shouldThrottle;
        }
        ctx.log(`Background tab throttling ${shouldThrottle ? 'enabled' : 'disabled'}.`, 'info');
    }

    // ── World-view bounds & visibility ──────────────────────────────

    function updateWorldViewBounds() {
        const ctx = getCtx();
        if (!ctx.app || !ctx.gameScene) {
            ctx.worldViewBounds.left = -Infinity;
            ctx.worldViewBounds.right = Infinity;
            ctx.worldViewBounds.top = -Infinity;
            ctx.worldViewBounds.bottom = Infinity;
            return;
        }

        const scale = Math.max(0.01, ctx.gameScene.scale?.x || 1);
        const worldWidth = ctx.app.screen.width / scale;
        const worldHeight = ctx.app.screen.height / scale;
        const worldLeft = (-ctx.gameScene.position.x) / scale;
        const worldTop = (-ctx.gameScene.position.y) / scale;

        ctx.worldViewBounds.left = worldLeft;
        ctx.worldViewBounds.right = worldLeft + worldWidth;
        ctx.worldViewBounds.top = worldTop;
        ctx.worldViewBounds.bottom = worldTop + worldHeight;
    }

    function isWorldPointVisible(x, y, margin = 0) {
        const ctx = getCtx();
        return (
            x >= ctx.worldViewBounds.left - margin &&
            x <= ctx.worldViewBounds.right + margin &&
            y >= ctx.worldViewBounds.top - margin &&
            y <= ctx.worldViewBounds.bottom + margin
        );
    }

    // ── Cull worker management ──────────────────────────────────────

    function initCullWorker() {
        const ctx = getCtx();
        if (!ctx.WORKER_CULL_ENABLED || typeof Worker === 'undefined') {
            return;
        }
        if (ctx.cullWorker) {
            return;
        }
        let worker = null;
        try {
            worker = new Worker('./workers/entity_cull_worker.js', { type: 'module' });
            ctx.setCullWorker(worker);
        } catch (error) {
            ctx.log(`Cull worker unavailable: ${error?.message || error}`, 'warn');
            ctx.setCullWorker(null);
            return;
        }

        worker.onmessage = (event) => {
            const message = event?.data || {};
            if (message.type === 'ready') {
                ctx.setCullWorkerReady(true);
                ctx.cullWorkerStats.wasmKernelActive = !!message.wasmKernelActive;
                ctx.cullWorkerStats.wasmKernelLabel = String(message.kernel || 'js');
                if (window.__e2e) {
                    window.__e2e.workerCullEnabled = true;
                    window.__e2e.workerCullReady = true;
                    window.__e2e.workerCullKernel = ctx.cullWorkerStats.wasmKernelLabel;
                }
                ctx.log(`Cull worker ready (${ctx.cullWorkerStats.wasmKernelLabel}).`, 'info');
                return;
            }

            if (message.type === 'result') {
                ctx.setCullWorkerBusy(false);
                const seq = Number(message.seq) || 0;
                if (seq < ctx.cullWorkerResultSeq) {
                    return;
                }
                ctx.setCullWorkerResultSeq(seq);
                const playerIds = Array.isArray(message.playerIds) ? message.playerIds : [];
                const projectileIds = Array.isArray(message.projectileIds) ? message.projectileIds : [];
                const computeMs = Number(message.computeMs) || 0;
                const roundTripMs = Number(message.roundTripMs) || 0;
                const workerCullMode = String(message.cullMode || 'linear');
                ctx.setCullWorkerResult({
                    seq,
                    playerSet: new Set(playerIds),
                    projectileSet: new Set(projectileIds),
                    computeMs,
                    roundTripMs,
                    cullMode: workerCullMode,
                    generatedAtMs: Number(message.generatedAtMs) || 0
                });
                ctx.cullWorkerStats.responses += 1;
                ctx.cullWorkerStats.lastComputeMs = computeMs;
                ctx.cullWorkerStats.avgComputeMs = ctx.cullWorkerStats.avgComputeMs * 0.85 + computeMs * 0.15;
                if (window.__e2e) {
                    window.__e2e.workerCullMode = workerCullMode;
                }
                return;
            }

            if (message.type === 'error') {
                ctx.setCullWorkerBusy(false);
                ctx.log(`Cull worker error: ${message.error || 'unknown'}`, 'warn');
            }
        };

        worker.onerror = (event) => {
            ctx.setCullWorkerBusy(false);
            ctx.setCullWorkerReady(false);
            ctx.log(`Cull worker crashed: ${event?.message || 'unknown error'}`, 'warn');
        };

        const requestedWasmUrl = ctx.WORKER_CULL_WASM_URL || ctx.DEFAULT_WORKER_CULL_WASM_URL;
        let resolvedWasmUrl = '';
        try {
            resolvedWasmUrl = new URL(requestedWasmUrl, window.location.href).toString();
        } catch (_) {
            resolvedWasmUrl = '';
        }
        worker.postMessage({
            type: 'init',
            wasmUrl: resolvedWasmUrl
        });
        if (window.__e2e) {
            window.__e2e.workerCullEnabled = true;
            window.__e2e.workerCullReady = false;
            window.__e2e.workerCullKernel = 'js';
        }
    }

    function terminateCullWorker() {
        const ctx = getCtx();
        if (!ctx.cullWorker) {
            ctx.setCullWorkerReady(false);
            ctx.setCullWorkerBusy(false);
            ctx.setCullWorkerResult(null);
            return;
        }
        try {
            ctx.cullWorker.onmessage = null;
            ctx.cullWorker.onerror = null;
            ctx.cullWorker.terminate();
        } catch (_) {}
        ctx.setCullWorker(null);
        ctx.setCullWorkerReady(false);
        ctx.setCullWorkerBusy(false);
        ctx.setCullWorkerSeq(0);
        ctx.setCullWorkerResultSeq(0);
        ctx.setLastCullWorkerDispatchTime(0);
        ctx.setCullWorkerResult(null);
        ctx.cullWorkerStats.dispatches = 0;
        ctx.cullWorkerStats.responses = 0;
        ctx.cullWorkerStats.dropped = 0;
        ctx.cullWorkerStats.avgComputeMs = 0;
        ctx.cullWorkerStats.lastComputeMs = 0;
        ctx.cullWorkerStats.wasmKernelActive = false;
        ctx.cullWorkerStats.wasmKernelLabel = 'js';
        if (window.__e2e) {
            window.__e2e.workerCullReady = false;
            window.__e2e.workerCullBusy = false;
            window.__e2e.workerCullKernel = 'js';
        }
    }

    function isCullWorkerUsable() {
        const ctx = getCtx();
        return !!(ctx.WORKER_CULL_ENABLED && ctx.cullWorker && ctx.cullWorkerReady);
    }

    function getWorkerCullDispatchIntervalMs() {
        const ctx = getCtx();
        let intervalMs = ctx.WORKER_CULL_INTERVAL_MS;
        if (!ctx.ultraPerformanceMode && !ctx.STABLE_MODE_FORCED && !ctx.TOURNAMENT_MODE_FORCED) {
            return intervalMs;
        }
        if (ctx.projectiles.size >= 2400 || ctx.players.size >= 140 || ctx.smoothedFrameMs >= 28) {
            return Math.max(intervalMs, 140);
        }
        if (ctx.projectiles.size >= 1300 || ctx.players.size >= 90 || ctx.smoothedFrameMs >= 24) {
            return Math.max(intervalMs, 100);
        }
        if (ctx.projectiles.size >= 700 || ctx.players.size >= 56 || ctx.smoothedFrameMs >= 21) {
            return Math.max(intervalMs, 72);
        }
        return intervalMs;
    }

    function dispatchCullWorkerIfNeeded(currentTimeMs) {
        const ctx = getCtx();
        if (!isCullWorkerUsable()) return;
        const cullDispatchIntervalMs = getWorkerCullDispatchIntervalMs();
        if ((currentTimeMs - ctx.lastCullWorkerDispatchTime) < cullDispatchIntervalMs) return;

        const totalObjects = ctx.players.size + ctx.projectiles.size;
        if (!ctx.MASS_MODE_FORCED && totalObjects < 30) {
            return;
        }

        if (ctx.cullWorkerBusy) {
            ctx.cullWorkerStats.dropped += 1;
            return;
        }

        const playerRenderCap = getPlayerRenderCap();
        const localAnchorX = ctx.localPlayerState
            ? (ctx.localPlayerState.render_x !== undefined ? ctx.localPlayerState.render_x : ctx.localPlayerState.x)
            : 0;
        const localAnchorY = ctx.localPlayerState
            ? (ctx.localPlayerState.render_y !== undefined ? ctx.localPlayerState.render_y : ctx.localPlayerState.y)
            : 0;
        const remoteRenderCap = Math.max(0, playerRenderCap - ((ctx.myPlayerId && ctx.players.has(ctx.myPlayerId)) ? 1 : 0));
        const remotePriorityOverflowCap = remoteRenderCap > 0
            ? Math.max(4, Math.floor(remoteRenderCap * 0.1))
            : 0;
        const projectileRenderCap = getProjectileRenderCap();
        const rawProjectileMode = ctx.projectileRawModeActive;

        const playerRows = [];
        ctx.players.forEach((playerEntry, playerId) => {
            const player = (playerId === ctx.myPlayerId && ctx.localPlayerState) ? ctx.localPlayerState : playerEntry;
            if (!player) return;
            const px = player.render_x !== undefined ? player.render_x : player.x;
            const py = player.render_y !== undefined ? player.render_y : player.y;
            playerRows.push([
                playerId,
                Number(px) || 0,
                Number(py) || 0,
                playerId === ctx.myPlayerId ? 1 : 0,
                player.alive ? 1 : 0
            ]);
        });

        const projectileRows = [];
        ctx.projectiles.forEach((projectile, projectileId) => {
            const px = rawProjectileMode
                ? projectile.x
                : (projectile.render_x !== undefined ? projectile.render_x : projectile.x);
            const py = rawProjectileMode
                ? projectile.y
                : (projectile.render_y !== undefined ? projectile.render_y : projectile.y);
            projectileRows.push([
                projectileId,
                Number(px) || 0,
                Number(py) || 0
            ]);
        });

        const requestSeq = ctx.cullWorkerSeq + 1;
        ctx.setCullWorkerSeq(requestSeq);
        ctx.setCullWorkerBusy(true);
        ctx.setLastCullWorkerDispatchTime(currentTimeMs);
        ctx.cullWorkerStats.dispatches += 1;
        const requestedAtMs = performance.now();

        ctx.cullWorker.postMessage({
            type: 'compute',
            seq: requestSeq,
            requestedAtMs,
            config: {
                playerCullMargin: ctx.PLAYER_CULL_MARGIN_WORLD,
                projectileCullMargin: ctx.PROJECTILE_CULL_MARGIN_WORLD,
                playerPriorityDistanceSq: ctx.PLAYER_PRIORITY_DISTANCE_SQ,
                remoteRenderCap,
                remotePriorityOverflowCap,
                projectileRenderCap,
                localAnchorX,
                localAnchorY,
                cullMode: ctx.WORKER_CULL_MODE,
                quadtreeThreshold: ctx.WORKER_CULL_QUADTREE_THRESHOLD
            },
            viewBounds: {
                left: ctx.worldViewBounds.left,
                right: ctx.worldViewBounds.right,
                top: ctx.worldViewBounds.top,
                bottom: ctx.worldViewBounds.bottom
            },
            players: playerRows,
            projectiles: projectileRows
        });
    }

    // ── Raw projectile mode & interpolation set ─────────────────────

    function shouldUseRawProjectilePositions() {
        const ctx = getCtx();
        if (ctx.projectiles.size <= 140 && ctx.smoothedFrameMs <= (ctx.TARGET_FRAME_MS_60FPS + 1.0)) {
            return false;
        }
        if (ctx.projectiles.size >= ctx.PROJECTILE_RAW_MODE_HARD_COUNT) return true;
        if (ctx.smoothedFrameMs >= ctx.PROJECTILE_RAW_MODE_FRAME_MS) return true;
        if ((ctx.STABLE_MODE_FORCED || ctx.ultraPerformanceMode) && ctx.projectiles.size >= ctx.PROJECTILE_RAW_MODE_SOFT_COUNT) {
            return true;
        }
        return false;
    }

    function getProjectileInterpolationSet() {
        const ctx = getCtx();
        if (!isCullWorkerUsable()) return null;
        if (!ctx.cullWorkerResult?.projectileSet) return null;
        if (ctx.projectiles.size < ctx.PROJECTILE_RAW_MODE_SOFT_COUNT) return null;
        return ctx.cullWorkerResult.projectileSet;
    }

    function forEachInterpolatedProjectile(projectileSet, callback) {
        const ctx = getCtx();
        if (projectileSet && projectileSet.size > 0) {
            projectileSet.forEach((projectileId) => {
                const projectile = ctx.projectiles.get(projectileId);
                if (projectile) {
                    callback(projectile, projectileId);
                }
            });
            return;
        }
        ctx.projectiles.forEach(callback);
    }

    // ── Dynamic effects cap ─────────────────────────────────────────

    function applyDynamicEffectsCap() {
        const ctx = getCtx();
        if (!ctx.effectsManager || typeof ctx.effectsManager.dropOverflowEffects !== 'function') return;
        const profileMax = Math.max(
            0,
            Math.floor(Number(ctx.effectsManager.performanceProfile?.maxActiveEffects || ctx.effectsManager.maxActiveEffects || 0))
        );
        if (profileMax <= 0) return;

        let targetMax = profileMax;
        if (ctx.projectiles.size >= 760 || ctx.smoothedFrameMs >= 27) {
            targetMax = Math.min(targetMax, 36);
        } else if (ctx.projectiles.size >= 620 || ctx.smoothedFrameMs >= 24) {
            targetMax = Math.min(targetMax, 56);
        } else if (ctx.projectiles.size >= 480 || ctx.smoothedFrameMs >= 21.5) {
            targetMax = Math.min(targetMax, 96);
        }

        if (ctx.effectsManager.maxActiveEffects !== targetMax) {
            ctx.effectsManager.maxActiveEffects = targetMax;
            ctx.effectsManager.dropOverflowEffects(0);
        }
    }

    // ── Render caps ─────────────────────────────────────────────────

    function getProjectileRenderCap() {
        const ctx = getCtx();
        const severePressure = ctx.projectiles.size >= 760 || ctx.smoothedFrameMs >= 26;
        const recoveryPressure = ctx.projectiles.size >= 620 || ctx.smoothedFrameMs >= 23;
        const softPressure = ctx.projectiles.size >= 480 || ctx.smoothedFrameMs >= 21.5;

        if (ctx.ultraPerformanceMode) {
            if (severePressure) return ctx.PROJECTILE_RENDER_CAP_ULTRA_EMERGENCY;
            if (recoveryPressure) return ctx.PROJECTILE_RENDER_CAP_ULTRA_RECOVERY;
            if (softPressure) return ctx.PROJECTILE_RENDER_CAP_ULTRA_SOFT;
            return ctx.PROJECTILE_RENDER_CAP_ULTRA;
        }
        if (ctx.STABLE_MODE_FORCED) {
            if (severePressure) return ctx.PROJECTILE_RENDER_CAP_ULTRA_EMERGENCY;
            if (recoveryPressure) return ctx.PROJECTILE_RENDER_CAP_ULTRA_RECOVERY;
            if (softPressure) return ctx.PROJECTILE_RENDER_CAP_ULTRA_SOFT;
            if (ctx.players.size >= 32 || ctx.smoothedFrameMs > ctx.STABLE_ULTRA_FRAME_MS) return ctx.PROJECTILE_RENDER_CAP_ULTRA;
            if (ctx.players.size >= 22 || ctx.smoothedFrameMs > ctx.STABLE_DENSE_FRAME_MS) return ctx.PROJECTILE_RENDER_CAP_DENSE;
            return ctx.PROJECTILE_RENDER_CAP_STABLE;
        }
        if (ctx.players.size > ctx.HIGH_POPULATION_PLAYER_COUNT) return ctx.PROJECTILE_RENDER_CAP_DENSE;
        // Mobile-tier projectile caps
        if (ctx.deviceClassification === 'low') return Math.min(ctx.PROJECTILE_RENDER_CAP_ULTRA, 80);
        if (ctx.deviceClassification === 'mid') return Math.min(ctx.PROJECTILE_RENDER_CAP_DENSE, 160);
        if (ctx.deviceClassification === 'high') return Math.min(ctx.PROJECTILE_RENDER_CAP_DEFAULT, 320);
        return ctx.PROJECTILE_RENDER_CAP_DEFAULT;
    }

    function getPlayerRenderCap() {
        const ctx = getCtx();
        if (ctx.ultraPerformanceMode) return ctx.PLAYER_RENDER_CAP_ULTRA;
        if (ctx.STABLE_MODE_FORCED) {
            if (ctx.players.size >= 38 || ctx.smoothedFrameMs > ctx.STABLE_ULTRA_FRAME_MS) return ctx.PLAYER_RENDER_CAP_ULTRA;
            if (ctx.players.size >= 24 || ctx.smoothedFrameMs > ctx.STABLE_DENSE_FRAME_MS) return ctx.PLAYER_RENDER_CAP_DENSE;
            return ctx.PLAYER_RENDER_CAP_STABLE;
        }
        if (ctx.players.size > ctx.HIGH_POPULATION_PLAYER_COUNT || ctx.smoothedFrameMs > 24) return ctx.PLAYER_RENDER_CAP_DENSE;
        // Mobile-tier caps: reduce render load on constrained devices
        if (ctx.deviceClassification === 'low') return Math.min(ctx.PLAYER_RENDER_CAP_ULTRA, 32);
        if (ctx.deviceClassification === 'mid') return Math.min(ctx.PLAYER_RENDER_CAP_DENSE, 64);
        if (ctx.deviceClassification === 'high') return Math.min(ctx.PLAYER_RENDER_CAP_DEFAULT, 96);
        return ctx.PLAYER_RENDER_CAP_DEFAULT;
    }

    // ── LOD helpers ─────────────────────────────────────────────────

    function createLodSummaryCounter() {
        return {
            full: 0,
            medium: 0,
            low: 0,
            dot: 0
        };
    }

    function countLodTier(counter, tier) {
        if (!counter) return;
        if (tier === 'medium' || tier === 'low' || tier === 'dot') {
            counter[tier] += 1;
            return;
        }
        counter.full += 1;
    }

    function computeRenderLodScale() {
        const ctx = getCtx();
        let scale = 1;
        if (ctx.ultraPerformanceMode) {
            scale *= 0.58;
        } else if (ctx.STABLE_MODE_FORCED) {
            scale *= 0.74;
        }
        if (ctx.smoothedFrameMs >= 28) {
            scale *= 0.72;
        } else if (ctx.smoothedFrameMs >= 24) {
            scale *= 0.84;
        }
        if (ctx.players.size > ctx.HIGH_POPULATION_PLAYER_COUNT) {
            scale *= 0.8;
        } else if (ctx.players.size >= 56) {
            scale *= 0.9;
        }
        return ctx.clamp(scale, 0.44, 1);
    }

    function resolveRenderLodTier(distanceSq, mediumDistanceSq, lowDistanceSq, dotDistanceSq, options = {}) {
        if (options.isLocal || options.isPriority) return 'full';
        const lodScale = Number.isFinite(options.lodScale) ? options.lodScale : computeRenderLodScale();
        const scaleSq = lodScale * lodScale;
        if (distanceSq <= mediumDistanceSq * scaleSq) return 'full';
        if (distanceSq <= lowDistanceSq * scaleSq) return 'medium';
        if (distanceSq <= dotDistanceSq * scaleSq) return 'low';
        return 'dot';
    }

    // ── Sprite cadence / stride helpers ─────────────────────────────

    function getProjectileDotCullStride() {
        const ctx = getCtx();
        if (ctx.ultraPerformanceMode) return 4;
        if (ctx.STABLE_MODE_FORCED || ctx.smoothedFrameMs >= 24) return 3;
        return 2;
    }

    function getRemotePlayerSpriteUpdateStride() {
        const ctx = getCtx();
        if (!ctx.SPRITE_CADENCE_ENABLED) return 1;
        const mobileCadence = ctx.mobileDynamicsEnabled || ctx.forceMobileClient;
        const hardStride = mobileCadence
            ? ctx.REMOTE_PLAYER_UPDATE_STRIDE_HARD_MOBILE
            : ctx.REMOTE_PLAYER_UPDATE_STRIDE_HARD_DESKTOP;
        const softStride = mobileCadence
            ? ctx.REMOTE_PLAYER_UPDATE_STRIDE_SOFT_MOBILE
            : ctx.REMOTE_PLAYER_UPDATE_STRIDE_SOFT_DESKTOP;

        const severePressure =
            ctx.ultraPerformanceMode ||
            ctx.smoothedFrameMs >= (mobileCadence ? 20.5 : 25) ||
            ctx.players.size >= (mobileCadence ? 20 : 48) ||
            ctx.projectiles.size >= (mobileCadence ? 120 : 320);
        if (severePressure) return hardStride;
        const softPressure =
            ctx.STABLE_MODE_FORCED ||
            ctx.smoothedFrameMs >= (mobileCadence ? 18.5 : 22) ||
            ctx.players.size >= (mobileCadence ? 12 : 30) ||
            ctx.projectiles.size >= (mobileCadence ? 70 : 180);
        if (softPressure) return softStride;
        return 1;
    }

    function getProjectileSpriteUpdateStride() {
        const ctx = getCtx();
        if (!ctx.SPRITE_CADENCE_ENABLED) return 1;
        const mobileCadence = ctx.mobileDynamicsEnabled || ctx.forceMobileClient;
        const hardStride = mobileCadence
            ? ctx.PROJECTILE_SPRITE_UPDATE_STRIDE_HARD_MOBILE
            : ctx.PROJECTILE_SPRITE_UPDATE_STRIDE_HARD_DESKTOP;
        const softStride = mobileCadence
            ? ctx.PROJECTILE_SPRITE_UPDATE_STRIDE_SOFT_MOBILE
            : ctx.PROJECTILE_SPRITE_UPDATE_STRIDE_SOFT_DESKTOP;

        const severePressure =
            ctx.ultraPerformanceMode ||
            ctx.smoothedFrameMs >= (mobileCadence ? 20.5 : 25) ||
            ctx.projectiles.size >= (mobileCadence ? 220 : 560);
        if (severePressure) return hardStride;
        const softPressure =
            ctx.STABLE_MODE_FORCED ||
            ctx.smoothedFrameMs >= (mobileCadence ? 18.5 : 22) ||
            ctx.projectiles.size >= (mobileCadence ? 110 : 260);
        if (softPressure) return softStride;
        return 1;
    }

    function getEntityCadenceBucket(entityId) {
        if (typeof entityId === 'number') {
            return Math.abs(entityId) % 31;
        }
        if (typeof entityId === 'string') {
            let hash = 0;
            const maxLen = Math.min(entityId.length, 12);
            for (let i = 0; i < maxLen; i += 1) {
                hash = ((hash << 5) - hash + entityId.charCodeAt(i)) | 0;
            }
            return Math.abs(hash) % 31;
        }
        return 0;
    }

    // ── Frame performance signals ───────────────────────────────────

    function updateFramePerformanceSignals(deltaMs) {
        const ctx = getCtx();
        const clampedDeltaMs = Math.max(4, Math.min(250, Number(deltaMs) || 16.67));
        ctx.setSmoothedFrameMs(ctx.smoothedFrameMs * 0.92 + clampedDeltaMs * 0.08);
        const lowFpsThreshold = ctx.STABLE_MODE_FORCED ? ctx.STABLE_ULTRA_FRAME_MS : 28;
        const recoveryThreshold = ctx.STABLE_MODE_FORCED ? ctx.STABLE_DENSE_FRAME_MS : 20;

        if (ctx.smoothedFrameMs > lowFpsThreshold) {
            ctx.setLowFpsFrameStreak(Math.min(ctx.lowFpsFrameStreak + 1, 600));
            ctx.setLowFpsDurationMs(Math.min(ctx.lowFpsDurationMs + clampedDeltaMs, 20000));
        } else {
            ctx.setLowFpsFrameStreak(Math.max(ctx.lowFpsFrameStreak - 2, 0));
            ctx.setLowFpsDurationMs(Math.max(0, ctx.lowFpsDurationMs - (clampedDeltaMs * 2)));
        }

        if (ctx.smoothedFrameMs < recoveryThreshold) {
            ctx.setRecoveryFrameStreak(Math.min(ctx.recoveryFrameStreak + 1, 600));
            ctx.setRecoveryDurationMs(Math.min(ctx.recoveryDurationMs + clampedDeltaMs, 30000));
        } else {
            ctx.setRecoveryFrameStreak(Math.max(ctx.recoveryFrameStreak - 2, 0));
            ctx.setRecoveryDurationMs(Math.max(0, ctx.recoveryDurationMs - (clampedDeltaMs * 2)));
        }
    }

    function syncParticlesBudget() {
        const ctx = getCtx();
        if (!ctx.effectsManager) return;
        const disableByProfile = ctx.activeEffectsProfileName === 'ultra';
        const disableByLoad = ctx.players.size >= (ctx.HIGH_POPULATION_PLAYER_COUNT + 30) || ctx.smoothedFrameMs > 30;
        const disableByRespawn =
            ctx.RESPAWN_ANIMATION_LIGHTWEIGHT &&
            !!ctx.localPlayerState &&
            !ctx.localPlayerState.alive;
        ctx.effectsManager.setParticlesEnabled(ctx.gameSettings.particleEffects && !disableByProfile && !disableByLoad && !disableByRespawn);
    }

    // ── Adaptive effects profiling ──────────────────────────────────

    function getTargetEffectsProfileName() {
        const ctx = getCtx();
        const playerCount = ctx.players.size;
        const activeEffectCount = ctx.effectsManager?.activeEffects?.length || 0;
        if (ctx.BENCH_MODE || ctx.ultraPerformanceMode) return 'ultra';
        if (ctx.STABLE_MODE_FORCED) {
            if (
                ctx.smoothedFrameMs > ctx.TARGET_FRAME_MS_60FPS ||
                playerCount >= 24 ||
                activeEffectCount >= 140
            ) {
                return 'ultra';
            }
            return 'dense';
        }

        if (ctx.TOURNAMENT_MODE_FORCED) {
            if (ctx.smoothedFrameMs > 26 || playerCount >= 150 || activeEffectCount >= 1200) return 'ultra';
            if (ctx.smoothedFrameMs > 22 || playerCount >= 100 || activeEffectCount >= 850) return 'dense';
            if (ctx.smoothedFrameMs > 18 || playerCount >= 70 || activeEffectCount >= 500) return 'medium';
            return 'high';
        }

        if (ctx.smoothedFrameMs > 30 || playerCount >= 180 || activeEffectCount >= 1800) return 'ultra';
        if (ctx.smoothedFrameMs > 24 || playerCount >= 120 || activeEffectCount >= 1300) return 'dense';
        if (ctx.smoothedFrameMs > 20 || playerCount >= 75 || activeEffectCount >= 900) return 'medium';
        return 'high';
    }

    function evaluateAdaptiveEffectsProfile(currentTime) {
        const ctx = getCtx();
        if (!ctx.effectsManager) return;
        if ((currentTime - ctx.lastAdaptiveEffectsEvalTime) < ctx.EFFECTS_ADAPTIVE_EVAL_INTERVAL_MS) return;
        ctx.setLastAdaptiveEffectsEvalTime(currentTime);

        const targetProfile = getTargetEffectsProfileName();
        if (targetProfile === ctx.activeEffectsProfileName) return;

        const currentPriority = ctx.EFFECTS_PROFILE_PRIORITY[ctx.activeEffectsProfileName] ?? 1;
        const targetPriority = ctx.EFFECTS_PROFILE_PRIORITY[targetProfile] ?? 1;
        const isUpshift = targetPriority > currentPriority;

        if (
            isUpshift &&
            !ctx.TOURNAMENT_MODE_FORCED &&
            (
                ctx.recoveryFrameStreak < 90 ||
                ctx.recoveryDurationMs < ctx.ULTRA_AUTO_RECOVERY_TRIGGER_MS ||
                ctx.smoothedFrameMs > ctx.ULTRA_DOWNSHIFT_MAX_FRAME_MS
            )
        ) {
            return;
        }

        ctx.setActiveEffectsProfileName(ctx.effectsManager.setPerformanceProfile(targetProfile));
        syncParticlesBudget();

        if (window.__e2e) {
            window.__e2e.effectsProfile = ctx.activeEffectsProfileName;
        }
    }

    // ── Render resolution budgeting ─────────────────────────────────

    function updateRenderResolutionBudget(reason = '') {
        const ctx = getCtx();
        let targetResolution = ctx.BASE_RENDER_RESOLUTION;
        if (ctx.STABLE_MODE_FORCED) {
            targetResolution = ctx.STABLE_BASE_RENDER_RESOLUTION;
            if (ctx.ultraPerformanceMode || ctx.players.size >= 36 || ctx.smoothedFrameMs > ctx.STABLE_ULTRA_FRAME_MS) {
                targetResolution = ctx.ULTRA_RENDER_RESOLUTION;
            } else if (ctx.players.size >= 22 || ctx.smoothedFrameMs > ctx.STABLE_DENSE_FRAME_MS) {
                targetResolution = ctx.DENSE_RENDER_RESOLUTION;
            }
        } else if (ctx.ultraPerformanceMode) {
            targetResolution = ctx.ULTRA_RENDER_RESOLUTION;
        } else if (ctx.smoothedFrameMs > 34) {
            targetResolution = ctx.ULTRA_RENDER_RESOLUTION;
        } else if (ctx.players.size >= ctx.HIGH_POPULATION_PLAYER_COUNT || ctx.smoothedFrameMs > 24) {
            targetResolution = ctx.DENSE_RENDER_RESOLUTION;
        }
        if (ctx.TOURNAMENT_MODE_FORCED) {
            targetResolution = Math.min(targetResolution, Math.max(0.78, ctx.DENSE_RENDER_RESOLUTION));
        }

        targetResolution = Math.max(0.5, Math.min(ctx.BASE_RENDER_RESOLUTION, targetResolution));
        const delta = targetResolution - ctx.currentRenderResolution;
        if (Math.abs(delta) < 0.01) {
            ctx.setPendingRenderResolutionTarget(targetResolution);
            ctx.setPendingRenderResolutionSince(0);
            return;
        }
        const now = Date.now();
        const isUpshift = delta > 0;
        const bypassCooldown =
            reason === 'init' ||
            reason === 'perf mode' ||
            reason === 'forced';
        if (!bypassCooldown) {
            if (Math.abs(targetResolution - ctx.pendingRenderResolutionTarget) >= 0.01) {
                ctx.setPendingRenderResolutionTarget(targetResolution);
                ctx.setPendingRenderResolutionSince(now);
                return;
            }
            const holdMs = isUpshift
                ? ctx.RENDER_RESOLUTION_UPSHIFT_HOLD_MS
                : ctx.RENDER_RESOLUTION_DOWNSHIFT_HOLD_MS;
            if (
                ctx.pendingRenderResolutionSince > 0 &&
                (now - ctx.pendingRenderResolutionSince) < holdMs
            ) {
                return;
            }
            if (isUpshift && ctx.recoveryFrameStreak < ctx.RENDER_RESOLUTION_UPSHIFT_RECOVERY_FRAMES) {
                return;
            }
            const cooldownMs = isUpshift
                ? ctx.RENDER_RESOLUTION_UPSHIFT_COOLDOWN_MS
                : ctx.RENDER_RESOLUTION_DOWNSHIFT_COOLDOWN_MS;
            if ((now - ctx.lastRenderResolutionChangeAt) < cooldownMs) {
                return;
            }
        }

        ctx.setCurrentRenderResolution(targetResolution);
        ctx.setLastRenderResolutionChangeAt(now);
        ctx.setPendingRenderResolutionTarget(targetResolution);
        ctx.setPendingRenderResolutionSince(0);

        if (ctx.app?.renderer) {
            ctx.resizePixiApp();
        }

        if (window.__e2e) {
            window.__e2e.renderResolution = ctx.currentRenderResolution;
        }

        ctx.log(`Render scale ${ctx.currentRenderResolution.toFixed(2)}${reason ? ` (${reason})` : ''}`, 'info');
    }

    // ── Ultra performance mode ──────────────────────────────────────

    function setUltraPerformanceMode(enabled, reason = '') {
        const ctx = getCtx();
        const forced = ctx.ULTRA_MODE_FORCED;
        const next = forced ? true : !!enabled;
        if (ctx.ultraPerformanceMode === next) return;

        ctx._setUltraPerformanceMode(next);
        if (ctx.ultraPerformanceMode) {
            ctx.setUltraModeEnteredAt(Date.now());
        }
        refreshRuntimePerfIntervals();

        if (ctx.starfield) ctx.starfield.visible = !ctx.ultraPerformanceMode;
        if (ctx.fogOfWarContainer) ctx.fogOfWarContainer.visible = !ctx.ultraPerformanceMode;
        if (ctx.healthVignette) ctx.healthVignette.visible = !ctx.ultraPerformanceMode;
        if (ctx.combatOverlayDiv && ctx.EXCITEMENT_UI_ENABLED) {
            ctx.combatOverlayDiv.style.opacity = ctx.ultraPerformanceMode ? '0' : '1';
        }
        if (ctx.networkIndicator?.app?.view) {
            ctx.networkIndicator.app.view.style.opacity = ctx.ultraPerformanceMode ? '0.85' : '1';
        }

        if (ctx.effectsManager && typeof ctx.effectsManager.setPerformanceProfile === 'function') {
            const targetProfile = ctx.ultraPerformanceMode ? 'ultra' : getTargetEffectsProfileName();
            ctx.setActiveEffectsProfileName(ctx.effectsManager.setPerformanceProfile(targetProfile));
        }
        syncParticlesBudget();
        updateRenderResolutionBudget('perf mode');
        ctx.drawWalls(true);
        if (ctx.minimap && typeof ctx.minimap.setPerformanceMode === 'function') {
            if (ctx.ultraPerformanceMode) {
                ctx.minimap.setPerformanceMode('ultra');
            } else if (ctx.players.size > ctx.HIGH_POPULATION_PLAYER_COUNT) {
                ctx.minimap.setPerformanceMode('dense');
            } else {
                ctx.minimap.setPerformanceMode('normal');
            }
        }

        if (window.__e2e) {
            window.__e2e.ultraPerformanceMode = ctx.ultraPerformanceMode;
            window.__e2e.effectsProfile = ctx.activeEffectsProfileName;
        }

        const detail = reason ? ` (${reason})` : '';
        ctx.log(`Ultra performance ${ctx.ultraPerformanceMode ? 'enabled' : 'disabled'}${detail}`, 'info');
    }

    function evaluateUltraPerformanceMode(currentTime) {
        const ctx = getCtx();
        if (ctx.BENCH_MODE || ctx.ULTRA_MODE_FORCED) {
            if (ctx.ULTRA_MODE_FORCED && !ctx.ultraPerformanceMode) {
                setUltraPerformanceMode(true, 'forced');
            }
            if ((currentTime - ctx.lastRenderResolutionEvalTime) >= 1000) {
                ctx.setLastRenderResolutionEvalTime(currentTime);
                updateRenderResolutionBudget('forced');
            }
            return;
        }

        const playerCount = ctx.players.size;
        const hasActiveMatchLoad = playerCount >= 4 || !!ctx.localPlayerState;
        if (!ctx.ultraPerformanceMode && hasActiveMatchLoad && ctx.smoothedFrameMs >= ctx.ULTRA_EMERGENCY_FRAME_MS) {
            setUltraPerformanceMode(true, `emergency ${ctx.smoothedFrameMs.toFixed(1)}ms`);
            return;
        }

        if (currentTime - ctx.lastPerfModeEvalTime < 1000) return;
        ctx.setLastPerfModeEvalTime(currentTime);
        syncParticlesBudget();

        if (
            !ctx.ultraPerformanceMode &&
            hasActiveMatchLoad &&
            (ctx.lowFpsDurationMs >= ctx.ULTRA_AUTO_LOW_FPS_TRIGGER_MS || ctx.lowFpsFrameStreak >= 24)
        ) {
            setUltraPerformanceMode(true, `frame ${ctx.smoothedFrameMs.toFixed(1)}ms`);
            return;
        }
        if (!ctx.ultraPerformanceMode && playerCount >= ctx.ULTRA_AUTO_UPSHIFT_PLAYERS) {
            setUltraPerformanceMode(true, `${playerCount} players`);
            return;
        }
        if (
            ctx.ultraPerformanceMode &&
            ctx.ultraModeEnteredAt > 0 &&
            (currentTime - ctx.ultraModeEnteredAt) >= ctx.ULTRA_MIN_HOLD_MS &&
            playerCount <= ctx.ULTRA_AUTO_DOWNSHIFT_PLAYERS &&
            ctx.projectiles.size < ctx.PROJECTILE_RAW_MODE_SOFT_COUNT &&
            ctx.recoveryFrameStreak >= 90 &&
            ctx.recoveryDurationMs >= ctx.ULTRA_AUTO_RECOVERY_TRIGGER_MS &&
            ctx.smoothedFrameMs <= ctx.ULTRA_DOWNSHIFT_MAX_FRAME_MS &&
            ctx.lowFpsDurationMs <= ctx.ULTRA_DOWNSHIFT_MAX_LOW_FPS_MS
        ) {
            setUltraPerformanceMode(false, `${playerCount} players / ${ctx.smoothedFrameMs.toFixed(1)}ms`);
        }
        ctx.setLastRenderResolutionEvalTime(currentTime);
        updateRenderResolutionBudget('adaptive');
    }

    // ── UI cache & setTextIfChanged ─────────────────────────────────

    function setTextIfChanged(el, value, cacheKey) {
        const ctx = getCtx();
        if (!el) return;
        if (ctx.uiCache[cacheKey] === value) return;
        ctx.uiCache[cacheKey] = value;
        el.textContent = value;
    }

    // ── Lightweight profiler ────────────────────────────────────────

    function perfStart() {
        return performance.now();
    }

    function perfEnd(name, startTime) {
        const ctx = getCtx();
        const duration = performance.now() - startTime;
        const stat = ctx.perfStats[name] || { total: 0, count: 0, max: 0 };
        stat.total += duration;
        stat.count += 1;
        if (duration > stat.max) stat.max = duration;
        ctx.perfStats[name] = stat;
    }

    function clearPerfStats() {
        const ctx = getCtx();
        Object.keys(ctx.perfStats).forEach((key) => delete ctx.perfStats[key]);
    }

    function resetPerfStats() {
        const ctx = getCtx();
        clearPerfStats();
        ctx.setPerfSessionStartTime(performance.now());
        ctx.setPerfLastReport(0);
        if (window.__e2e) {
            window.__e2e.perfReport = null;
            window.__e2e.perfReportGeneratedAt = 0;
        }
    }

    function buildPerfReport(now = performance.now()) {
        const ctx = getCtx();
        const elapsedMs = Math.max(1, now - ctx.perfSessionStartTime);
        const elapsedSec = elapsedMs / 1000;
        const phases = {};
        let totalPhaseMs = 0;

        Object.keys(ctx.perfStats).forEach((key) => {
            const stat = ctx.perfStats[key];
            totalPhaseMs += stat.total;
        });

        Object.keys(ctx.perfStats).forEach((key) => {
            const stat = ctx.perfStats[key];
            const avgMs = stat.count > 0 ? stat.total / stat.count : 0;
            phases[key] = {
                avgMs: Number(avgMs.toFixed(3)),
                maxMs: Number(stat.max.toFixed(3)),
                totalMs: Number(stat.total.toFixed(3)),
                calls: stat.count,
                callsPerSec: Number((stat.count / elapsedSec).toFixed(3)),
                msPerSec: Number((stat.total / elapsedSec).toFixed(3)),
                dutyCyclePct: Number(((stat.total / elapsedMs) * 100).toFixed(2)),
                sharePct: Number((totalPhaseMs > 0 ? (stat.total / totalPhaseMs) * 100 : 0).toFixed(2)),
            };
        });

        const rankedPhases = Object.entries(phases)
            .sort((a, b) => b[1].totalMs - a[1].totalMs)
            .map(([name, data]) => ({ name, ...data }));

        return {
            enabled: ctx.perfEnabled,
            elapsedMs: Number(elapsedMs.toFixed(1)),
            elapsedSec: Number(elapsedSec.toFixed(2)),
            totalPhaseMs: Number(totalPhaseMs.toFixed(3)),
            instrumentedDutyCyclePct: Number(((totalPhaseMs / elapsedMs) * 100).toFixed(2)),
            phaseCount: rankedPhases.length,
            rankedPhases,
            phases,
        };
    }

    function setPerfProfilingEnabled(enabled) {
        const ctx = getCtx();
        ctx.setPerfEnabled(!!enabled);
        resetPerfStats();
        if (window.__e2e) {
            window.__e2e.perfProfilingEnabled = ctx.perfEnabled;
        }
        return ctx.perfEnabled;
    }

    // ── Synthetic projectiles ───────────────────────────────────────

    function clearSyntheticProjectiles() {
        const ctx = getCtx();
        ctx.removeSyntheticProjectiles(ctx.projectiles, ctx.SYNTHETIC_PROJECTILE_PREFIX);
        ctx._setSyntheticProjectileCount(0);
        if (window.__e2e) {
            window.__e2e.syntheticProjectileCount = 0;
        }
    }

    function setSyntheticProjectileCount(rawCount) {
        const ctx = getCtx();
        clearSyntheticProjectiles();
        ctx._setSyntheticProjectileCount(ctx.populateSyntheticProjectiles({
            rawCount,
            projectiles: ctx.projectiles,
            projectileIdPrefix: ctx.SYNTHETIC_PROJECTILE_PREFIX,
            weaponTypes: [
                ctx.GP.WeaponType.Pistol,
                ctx.GP.WeaponType.Shotgun,
                ctx.GP.WeaponType.Rifle,
                ctx.GP.WeaponType.Sniper
            ],
        }));
        if (window.__e2e) {
            window.__e2e.syntheticProjectileCount = ctx.syntheticProjectileCount;
        }
        return ctx.syntheticProjectileCount;
    }

    // ── WebGPU / WebGL2 accelerated layers ──────────────────────────

    async function initWebGPUProjectileLayer(hostElement, width, height) {
        const ctx = getCtx();
        if (!ctx.WEBGPU_PROJECTILE_LAYER_ENABLED || !hostElement) return;
        if (ctx.webgpuProjectileLayerInitStarted || ctx.webgpuProjectileLayer) return;
        ctx.setWebgpuProjectileLayerInitStarted(true);
        let layer = null;
        try {
            layer = new ctx.WebGPUProjectileLayer(hostElement);
            await layer.init(width, height);
            ctx.setWebgpuProjectileLayer(layer);
            ctx.log('WebGPU projectile instance layer enabled.', 'info');
        } catch (error) {
            try {
                layer?.destroy?.();
            } catch (_) {}
            ctx.setWebgpuProjectileLayer(null);
            ctx.log(`WebGPU projectile layer unavailable: ${error?.message || error}`, 'warn');
            if (ctx.WEBGL2_FALLBACK_ENABLED) {
                if (!ctx.WEBGL2_SUPPORTED) {
                    ctx.log('WebGL2 projectile fallback unavailable: WebGL2 unsupported in this runtime.', 'warn');
                } else {
                    let fallbackLayer = null;
                    try {
                        fallbackLayer = new ctx.WebGL2ProjectileLayer(hostElement);
                        fallbackLayer.init(width, height);
                        ctx.setWebgpuProjectileLayer(fallbackLayer);
                        ctx.log('WebGL2 projectile fallback layer enabled.', 'info');
                    } catch (fallbackError) {
                        try {
                            fallbackLayer?.destroy?.();
                        } catch (_) {}
                        ctx.setWebgpuProjectileLayer(null);
                        ctx.log(`WebGL2 projectile fallback unavailable: ${fallbackError?.message || fallbackError}`, 'warn');
                    }
                }
            }
        }
        if (window.__e2e) {
            const backend = ctx.getAcceleratedLayerBackend(ctx.webgpuProjectileLayer);
            window.__e2e.webgpuProjectileLayerEnabled = !!ctx.webgpuProjectileLayer;
            window.__e2e.webgpuProjectileLayerReady = !!(ctx.webgpuProjectileLayer && ctx.webgpuProjectileLayer.ready);
            window.__e2e.webgpuProjectileLayerBackend = backend;
            window.__e2e.acceleratedProjectileBackend = backend;
        }
    }

    function disableWebGPUProjectileLayer(reason, error = null) {
        const ctx = getCtx();
        if (!ctx.webgpuProjectileLayer) return;
        const backend = ctx.getAcceleratedLayerBackend(ctx.webgpuProjectileLayer);
        try {
            ctx.webgpuProjectileLayer.destroy();
        } catch (_) {}
        ctx.setWebgpuProjectileLayer(null);
        ctx.setWebgpuProjectileRenderPathActive(false);
        const detail = error?.message ? `${reason}: ${error.message}` : reason;
        ctx.log(`${ctx.formatAcceleratedBackendLabel(backend)} projectile layer disabled: ${detail}`, 'warn');
        if (window.__e2e) {
            window.__e2e.webgpuProjectileLayerEnabled = false;
            window.__e2e.webgpuProjectileLayerReady = false;
            window.__e2e.webgpuProjectileLayerActive = false;
            window.__e2e.webgpuProjectileLayerBackend = 'none';
            window.__e2e.acceleratedProjectileBackend = 'none';
            window.__e2e.webgpuProjectileInstances = 0;
        }
    }

    async function initWebGPUPlayerLayer(hostElement, width, height) {
        const ctx = getCtx();
        if (!ctx.WEBGPU_PLAYER_LAYER_ENABLED || !hostElement) return;
        if (ctx.webgpuPlayerLayerInitStarted || ctx.webgpuPlayerLayer) return;
        ctx.setWebgpuPlayerLayerInitStarted(true);
        let layer = null;
        try {
            layer = new ctx.WebGPUPlayerLayer(hostElement);
            await layer.init(width, height);
            ctx.setWebgpuPlayerLayer(layer);
            ctx.log('WebGPU player instance layer enabled.', 'info');
        } catch (error) {
            try {
                layer?.destroy?.();
            } catch (_) {}
            ctx.setWebgpuPlayerLayer(null);
            ctx.log(`WebGPU player layer unavailable: ${error?.message || error}`, 'warn');
            if (ctx.WEBGL2_FALLBACK_ENABLED) {
                if (!ctx.WEBGL2_SUPPORTED) {
                    ctx.log('WebGL2 player fallback unavailable: WebGL2 unsupported in this runtime.', 'warn');
                } else {
                    let fallbackLayer = null;
                    try {
                        fallbackLayer = new ctx.WebGL2PlayerLayer(hostElement);
                        fallbackLayer.init(width, height);
                        ctx.setWebgpuPlayerLayer(fallbackLayer);
                        ctx.log('WebGL2 player fallback layer enabled.', 'info');
                    } catch (fallbackError) {
                        try {
                            fallbackLayer?.destroy?.();
                        } catch (_) {}
                        ctx.setWebgpuPlayerLayer(null);
                        ctx.log(`WebGL2 player fallback unavailable: ${fallbackError?.message || fallbackError}`, 'warn');
                    }
                }
            }
        }
        if (window.__e2e) {
            const backend = ctx.getAcceleratedLayerBackend(ctx.webgpuPlayerLayer);
            window.__e2e.webgpuPlayerLayerEnabled = !!ctx.webgpuPlayerLayer;
            window.__e2e.webgpuPlayerLayerReady = !!(ctx.webgpuPlayerLayer && ctx.webgpuPlayerLayer.ready);
            window.__e2e.webgpuPlayerLayerBackend = backend;
            window.__e2e.acceleratedPlayerBackend = backend;
        }
    }

    function disableWebGPUPlayerLayer(reason, error = null) {
        const ctx = getCtx();
        if (!ctx.webgpuPlayerLayer) return;
        const backend = ctx.getAcceleratedLayerBackend(ctx.webgpuPlayerLayer);
        try {
            ctx.webgpuPlayerLayer.destroy();
        } catch (_) {}
        ctx.setWebgpuPlayerLayer(null);
        ctx.setWebgpuPlayerRenderPathActive(false);
        const detail = error?.message ? `${reason}: ${error.message}` : reason;
        ctx.log(`${ctx.formatAcceleratedBackendLabel(backend)} player layer disabled: ${detail}`, 'warn');
        if (window.__e2e) {
            window.__e2e.webgpuPlayerLayerEnabled = false;
            window.__e2e.webgpuPlayerLayerReady = false;
            window.__e2e.webgpuPlayerLayerActive = false;
            window.__e2e.webgpuPlayerLayerBackend = 'none';
            window.__e2e.acceleratedPlayerBackend = 'none';
            window.__e2e.webgpuPlayerInstances = 0;
        }
    }

    // ── Public API ──────────────────────────────────────────────────

    return {
        refreshRuntimePerfIntervals, getForegroundTickerMaxFps, applyBackgroundThrottling,
        updateWorldViewBounds, isWorldPointVisible,
        initCullWorker, terminateCullWorker, isCullWorkerUsable,
        getWorkerCullDispatchIntervalMs, dispatchCullWorkerIfNeeded,
        shouldUseRawProjectilePositions, getProjectileInterpolationSet, forEachInterpolatedProjectile,
        applyDynamicEffectsCap, getProjectileRenderCap, getPlayerRenderCap,
        createLodSummaryCounter, countLodTier, computeRenderLodScale, resolveRenderLodTier,
        getProjectileDotCullStride, getRemotePlayerSpriteUpdateStride, getProjectileSpriteUpdateStride,
        getEntityCadenceBucket, updateFramePerformanceSignals, syncParticlesBudget,
        getTargetEffectsProfileName, evaluateAdaptiveEffectsProfile,
        updateRenderResolutionBudget, setUltraPerformanceMode, evaluateUltraPerformanceMode,
        setTextIfChanged,
        perfStart, perfEnd, clearPerfStats, resetPerfStats, buildPerfReport, setPerfProfilingEnabled,
        clearSyntheticProjectiles, setSyntheticProjectileCount,
        initWebGPUProjectileLayer, disableWebGPUProjectileLayer,
        initWebGPUPlayerLayer, disableWebGPUPlayerLayer,
    };
}
