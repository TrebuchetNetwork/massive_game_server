/**
 * UpdateSprites.js - Entity sprite update loop
 *
 * Extracted from client.html. Contains the updateSprites function which handles
 * player, projectile, and pickup sprite visibility, LOD, WebGPU instance
 * batching, and sprite lifecycle.
 * Uses getCtx callback pattern.
 */

export function createUpdateSprites(getCtx) {

    // ── WebGPU scratch buffer helpers (moved from client.html module scope) ──

    let _webgpuProjectileInstanceScratch = new Float32Array(0);
    let _webgpuPlayerInstanceScratch = new Float32Array(0);

    function getWebGPUProjectileSize(weaponType) {
        const { WEBGPU_PROJECTILE_BASE_SIZE, GP } = getCtx();
        const size = WEBGPU_PROJECTILE_BASE_SIZE[weaponType];
        return Number.isFinite(size) ? size : WEBGPU_PROJECTILE_BASE_SIZE[GP.WeaponType.Pistol];
    }

    function ensureWebGPUProjectileScratchCapacity(instanceCount) {
        const { WEBGPU_PROJECTILE_INSTANCE_STRIDE } = getCtx();
        const requiredFloats = Math.max(0, instanceCount) * WEBGPU_PROJECTILE_INSTANCE_STRIDE;
        if (requiredFloats <= _webgpuProjectileInstanceScratch.length) {
            return _webgpuProjectileInstanceScratch;
        }
        let nextSize = Math.max(1024 * WEBGPU_PROJECTILE_INSTANCE_STRIDE, _webgpuProjectileInstanceScratch.length || 0);
        while (nextSize < requiredFloats) {
            nextSize *= 2;
        }
        _webgpuProjectileInstanceScratch = new Float32Array(nextSize);
        return _webgpuProjectileInstanceScratch;
    }

    function ensureWebGPUPlayerScratchCapacity(instanceCount) {
        const { WEBGPU_PLAYER_INSTANCE_STRIDE } = getCtx();
        const requiredFloats = Math.max(0, instanceCount) * WEBGPU_PLAYER_INSTANCE_STRIDE;
        if (requiredFloats <= _webgpuPlayerInstanceScratch.length) {
            return _webgpuPlayerInstanceScratch;
        }
        let nextSize = Math.max(512 * WEBGPU_PLAYER_INSTANCE_STRIDE, _webgpuPlayerInstanceScratch.length || 0);
        while (nextSize < requiredFloats) {
            nextSize *= 2;
        }
        _webgpuPlayerInstanceScratch = new Float32Array(nextSize);
        return _webgpuPlayerInstanceScratch;
    }

    function writeWebGPUPlayerInstance(buffer, offset, player, worldX, worldY) {
        const { teamColors, WEBGPU_PLAYER_BASE_SIZE, WEBGPU_PLAYER_INSTANCE_STRIDE } = getCtx();
        const renderRotation = Number(
            player.render_rotation !== undefined ? player.render_rotation : player.rotation
        ) || 0;
        const lodTier = player._renderLodTier || 'full';
        const alive = !!player.alive;
        const tint = alive
            ? (teamColors[player.team_id] || teamColors[0])
            : 0x6B7280;
        let alpha = alive ? 1 : 0.55;
        let size = WEBGPU_PLAYER_BASE_SIZE;
        if (lodTier === 'medium') {
            alpha = Math.min(alpha, alive ? 0.92 : 0.54);
            size *= 0.88;
        } else if (lodTier === 'low') {
            alpha = Math.min(alpha, alive ? 0.82 : 0.5);
            size *= 0.72;
        } else if (lodTier === 'dot') {
            alpha = Math.min(alpha, alive ? 0.68 : 0.44);
            size *= 0.5;
        }

        buffer[offset] = worldX;
        buffer[offset + 1] = worldY;
        buffer[offset + 2] = renderRotation;
        buffer[offset + 3] = Math.max(5.5, size);
        buffer[offset + 4] = ((tint >> 16) & 0xFF) / 255;
        buffer[offset + 5] = ((tint >> 8) & 0xFF) / 255;
        buffer[offset + 6] = (tint & 0xFF) / 255;
        buffer[offset + 7] = alpha;
        return offset + WEBGPU_PLAYER_INSTANCE_STRIDE;
    }

    function writeWebGPUProjectileInstance(buffer, offset, projectile, worldX, worldY) {
        const {
            GP, weaponColors, ultraPerformanceMode, STABLE_MODE_FORCED,
            players, HIGH_POPULATION_PLAYER_COUNT, smoothedFrameMs,
            STABLE_DENSE_FRAME_MS, frameNowMs, WEBGPU_PROJECTILE_INSTANCE_STRIDE,
        } = getCtx();
        const weaponType = projectile.weapon_type ?? GP.WeaponType.Pistol;
        const tint = weaponColors[weaponType] || 0xFFFFFF;
        const lodTier = projectile._renderLodTier || 'full';
        const denseVisualMode =
            ultraPerformanceMode ||
            STABLE_MODE_FORCED ||
            players.size > HIGH_POPULATION_PLAYER_COUNT ||
            smoothedFrameMs > STABLE_DENSE_FRAME_MS;

        let alpha = 0.9;
        if (denseVisualMode) {
            alpha = 0.88;
        } else if (weaponType === GP.WeaponType.Sniper || weaponType === GP.WeaponType.Rifle) {
            const projectileId = projectile.id;
            const idSeed = typeof projectileId === 'string' ? projectileId.length : (Number(projectileId) || 0);
            alpha = Math.sin(frameNowMs * 0.02 + idSeed) * 0.08 + 0.92;
        }
        if (lodTier === 'medium') {
            alpha = Math.min(alpha, 0.84);
        } else if (lodTier === 'low') {
            alpha = Math.min(alpha, 0.74);
        } else if (lodTier === 'dot') {
            alpha = Math.min(alpha, 0.62);
        }
        let projectileSize = getWebGPUProjectileSize(weaponType);
        if (lodTier === 'medium') {
            projectileSize *= 0.86;
        } else if (lodTier === 'low') {
            projectileSize *= 0.72;
        } else if (lodTier === 'dot') {
            projectileSize *= 0.54;
        }

        buffer[offset] = worldX;
        buffer[offset + 1] = worldY;
        buffer[offset + 2] = Math.max(2.4, projectileSize);
        buffer[offset + 3] = ((tint >> 16) & 0xFF) / 255;
        buffer[offset + 4] = ((tint >> 8) & 0xFF) / 255;
        buffer[offset + 5] = (tint & 0xFF) / 255;
        buffer[offset + 6] = alpha;
        return offset + WEBGPU_PROJECTILE_INSTANCE_STRIDE;
    }

    // ── Main sprite update loop ──────────────────────────────────────

    function updateSprites() {
        const ctx = getCtx();
        const {
            players, projectiles, pickups, localPlayerState, myPlayerId,
            ultraPerformanceMode, STABLE_MODE_FORCED, STABLE_DENSE_FRAME_MS,
            HIGH_POPULATION_PLAYER_COUNT, smoothedFrameMs, frameCounter, frameNowMs,
            PLAYER_CULL_MARGIN_WORLD, PROJECTILE_CULL_MARGIN_WORLD, PICKUP_CULL_MARGIN_WORLD,
            PLAYER_PRIORITY_DISTANCE_SQ, PROJECTILE_PRIORITY_DISTANCE_SQ,
            PLAYER_LOD_MEDIUM_DISTANCE_SQ, PLAYER_LOD_LOW_DISTANCE_SQ, PLAYER_LOD_DOT_DISTANCE_SQ,
            PROJECTILE_LOD_MEDIUM_DISTANCE_SQ, PROJECTILE_LOD_LOW_DISTANCE_SQ, PROJECTILE_LOD_DOT_DISTANCE_SQ,
            WEBGPU_PLAYER_INSTANCE_STRIDE, WEBGPU_PROJECTILE_INSTANCE_STRIDE,
            WEBGPU_PLAYER_ACTIVATION_MIN_COUNT, WEBGPU_PROJECTILE_ACTIVATION_MIN_COUNT,
            WEBGPU_FRAME_PRESSURE_MS, WEBGPU_FORCE_ACTIVE,
            PROJECTILE_WORKER_CULL_GRACE_MS, ALPHA_EPSILON,
            PICKUP_VISIBILITY_UPDATE_STRIDE,
            worldViewBounds, cullWorkerResult, projectileRawModeActive,
            playerSprites, playerContainer, projectileSprites, projectileContainer,
            pickupSprites, pickupContainer, projectileWorkerCullGraceUntil,
            webgpuPlayerLayer, webgpuProjectileLayer,
            webgpuPlayerRenderPathActive, webgpuProjectileRenderPathActive,
            localPlayerSprite, log,
            // Functions
            getPlayerRenderCap, getProjectileRenderCap,
            getRemotePlayerSpriteUpdateStride, getProjectileSpriteUpdateStride,
            computeRenderLodScale, createLodSummaryCounter, countLodTier,
            resolveRenderLodTier, getEntityCadenceBucket,
            getProjectileDotCullStride, getWorkerCullDispatchIntervalMs,
            hidePlayerSprite, createPlayerSprite, updatePlayerSprite,
            createProjectileSprite, releaseProjectileSprite, updateProjectileSprite,
            createPickupSprite,
            disableWebGPUPlayerLayer, disableWebGPUProjectileLayer,
            getAcceleratedLayerBackend, formatAcceleratedBackendLabel,
            // Setters
            setLastRemotePlayerUpdateStride, setLastProjectileSpriteUpdateStride,
            setLastRenderLodScale,
            setLastVisiblePlayers, setLastPlayerRenderCap, setLastPlayerLodSummary,
            setLastVisibleProjectiles, setLastProjectileRenderCap, setLastProjectileLodSummary,
            setWebgpuPlayerRenderPathActive, setWebgpuProjectileRenderPathActive,
            setLocalPlayerSprite,
        } = ctx;

        // Update player sprites
        const playerRenderCap = getPlayerRenderCap();
        const totalPlayerCount = players.size;
        const localAnchorX = localPlayerState
            ? (localPlayerState.render_x !== undefined ? localPlayerState.render_x : localPlayerState.x)
            : 0;
        const localAnchorY = localPlayerState
            ? (localPlayerState.render_y !== undefined ? localPlayerState.render_y : localPlayerState.y)
            : 0;
        const playerDenseVisualMode =
            ultraPerformanceMode ||
            totalPlayerCount > HIGH_POPULATION_PLAYER_COUNT ||
            smoothedFrameMs > 24 ||
            (STABLE_MODE_FORCED && (totalPlayerCount >= 24 || smoothedFrameMs > STABLE_DENSE_FRAME_MS));
        const remotePlayerUpdateStride = getRemotePlayerSpriteUpdateStride();
        const projectileSpriteUpdateStride = getProjectileSpriteUpdateStride();
        setLastRemotePlayerUpdateStride(remotePlayerUpdateStride);
        setLastProjectileSpriteUpdateStride(projectileSpriteUpdateStride);
        const playerUpdateContext = {
            totalPlayerCount,
            denseVisualMode: playerDenseVisualMode,
            detailTickDivisor: ultraPerformanceMode ? 4 : (STABLE_MODE_FORCED ? 3 : 2),
            playerLodTier: 'full'
        };
        const lodScale = computeRenderLodScale();
        setLastRenderLodScale(lodScale);
        const playerLodSummary = createLodSummaryCounter();
        const remoteRenderCap = Math.max(0, playerRenderCap - ((myPlayerId && players.has(myPlayerId)) ? 1 : 0));
        const remotePriorityOverflowCap = remoteRenderCap > 0
            ? Math.max(4, Math.floor(remoteRenderCap * 0.1))
            : 0;
        let renderedRemotePlayers = 0;
        let visiblePlayers = 0;
        const workerPlayerSet = cullWorkerResult?.playerSet || null;
        const workerProjectileSet = cullWorkerResult?.projectileSet || null;
        const playerCullLeft = worldViewBounds.left - PLAYER_CULL_MARGIN_WORLD;
        const playerCullRight = worldViewBounds.right + PLAYER_CULL_MARGIN_WORLD;
        const playerCullTop = worldViewBounds.top - PLAYER_CULL_MARGIN_WORLD;
        const playerCullBottom = worldViewBounds.bottom + PLAYER_CULL_MARGIN_WORLD;
        const webgpuPlayersReady = !!(webgpuPlayerLayer && webgpuPlayerLayer.ready);
        if (webgpuPlayersReady && !webgpuPlayerRenderPathActive) {
            const playerCountPressure = totalPlayerCount >= WEBGPU_PLAYER_ACTIVATION_MIN_COUNT;
            const framePressure =
                smoothedFrameMs >= WEBGPU_FRAME_PRESSURE_MS &&
                totalPlayerCount >= Math.max(8, Math.floor(WEBGPU_PLAYER_ACTIVATION_MIN_COUNT * 0.5));
            if (WEBGPU_FORCE_ACTIVE || playerCountPressure || framePressure) {
                setWebgpuPlayerRenderPathActive(true);
                const backendLabel = formatAcceleratedBackendLabel(getAcceleratedLayerBackend(webgpuPlayerLayer));
                log(
                    `${backendLabel} player shader path engaged (players=${totalPlayerCount}, frameMs=${smoothedFrameMs.toFixed(2)}).`,
                    'info'
                );
            }
        }
        const useWebGPUPlayers = webgpuPlayersReady && (WEBGPU_FORCE_ACTIVE || webgpuPlayerRenderPathActive);
        if (window.__e2e) {
            window.__e2e.webgpuPlayerLayerActive = useWebGPUPlayers;
            window.__e2e.webgpuPlayerLayerBackend = getAcceleratedLayerBackend(webgpuPlayerLayer);
            window.__e2e.acceleratedPlayerBackend = window.__e2e.webgpuPlayerLayerBackend;
        }
        let playerInstanceBuffer = null;
        let playerInstanceOffset = 0;
        if (useWebGPUPlayers) {
            const projectedCount = workerPlayerSet
                ? Math.min(
                    Math.max(0, players.size - ((myPlayerId && players.has(myPlayerId)) ? 1 : 0)),
                    Math.max(remoteRenderCap, workerPlayerSet.size)
                )
                : Math.max(0, remoteRenderCap + remotePriorityOverflowCap);
            playerInstanceBuffer = ensureWebGPUPlayerScratchCapacity(projectedCount);

            if (playerSprites.size > 0) {
                for (const [existingPlayerId, existingSprite] of playerSprites) {
                    if (existingPlayerId === myPlayerId) continue;
                    playerContainer.removeChild(existingSprite);
                    existingSprite.destroy({ children: true });
                    playerSprites.delete(existingPlayerId);
                }
            }
        }

        const processPlayerCandidate = (playerId, playerEntry) => {
            const player = (playerId === myPlayerId && localPlayerState) ? localPlayerState : playerEntry;
            const isLocal = playerId === myPlayerId;
            const px = player.render_x !== undefined ? player.render_x : player.x;
            const py = player.render_y !== undefined ? player.render_y : player.y;
            const dxToLocal = px - localAnchorX;
            const dyToLocal = py - localAnchorY;
            const distanceSqToLocal = (dxToLocal * dxToLocal) + (dyToLocal * dyToLocal);
            const isPriorityPlayer = !isLocal && (distanceSqToLocal <= PLAYER_PRIORITY_DISTANCE_SQ);
            const playerLodTier = resolveRenderLodTier(
                distanceSqToLocal,
                PLAYER_LOD_MEDIUM_DISTANCE_SQ,
                PLAYER_LOD_LOW_DISTANCE_SQ,
                PLAYER_LOD_DOT_DISTANCE_SQ,
                { isLocal, isPriority: isPriorityPlayer, lodScale }
            );
            let sprite = playerSprites.get(playerId);
            if (!isLocal) {
                if (workerPlayerSet) {
                    if (!workerPlayerSet.has(playerId)) {
                        if (sprite) hidePlayerSprite(sprite);
                        return;
                    }
                } else {
                    const inView =
                        px >= playerCullLeft &&
                        px <= playerCullRight &&
                        py >= playerCullTop &&
                        py <= playerCullBottom;
                    if (!inView) {
                        if (sprite) hidePlayerSprite(sprite);
                        return;
                    }

                    const allowedRemoteCount = isPriorityPlayer
                        ? (remoteRenderCap + remotePriorityOverflowCap)
                        : remoteRenderCap;
                    if (renderedRemotePlayers >= allowedRemoteCount) {
                        if (sprite) hidePlayerSprite(sprite);
                        return;
                    }
                    renderedRemotePlayers += 1;
                }
                if (workerPlayerSet && !player.alive && player.respawn_timer <= 0) {
                    if (sprite) hidePlayerSprite(sprite);
                    return;
                }

                const remoteVisibleState = player.alive || (player.respawn_timer !== undefined && player.respawn_timer > 0);
                if (!remoteVisibleState) {
                    if (sprite) hidePlayerSprite(sprite);
                    return;
                }

                if (!useWebGPUPlayers && remotePlayerUpdateStride > 1 && !isPriorityPlayer && sprite) {
                    const cadenceBucket = getEntityCadenceBucket(playerId);
                    const shouldUpdateRemoteSprite = ((frameCounter + cadenceBucket) % remotePlayerUpdateStride) === 0;
                    if (!shouldUpdateRemoteSprite) {
                        if (sprite.position.x !== px) sprite.position.x = px;
                        if (sprite.position.y !== py) sprite.position.y = py;
                        const effectiveRotation = (player.render_rotation !== undefined ? player.render_rotation : player.rotation) + (Math.PI / 2);
                        if (sprite.rotation !== effectiveRotation) sprite.rotation = effectiveRotation;
                        if (!sprite.visible) sprite.visible = true;
                        countLodTier(playerLodSummary, playerLodTier);
                        visiblePlayers += 1;
                        return;
                    }
                }

                if (useWebGPUPlayers) {
                    player._renderLodTier = playerLodTier;
                    countLodTier(playerLodSummary, playerLodTier);
                    playerInstanceOffset = writeWebGPUPlayerInstance(
                        playerInstanceBuffer,
                        playerInstanceOffset,
                        player,
                        px,
                        py
                    );
                    visiblePlayers += 1;
                    return;
                }
            }
            if (!sprite) {
                sprite = createPlayerSprite(player, isLocal);
                playerSprites.set(playerId, sprite);
                playerContainer.addChild(sprite);
                if (isLocal) {
                    setLocalPlayerSprite(sprite);
                }
            }

            playerUpdateContext.playerLodTier = playerLodTier;
            updatePlayerSprite(sprite, player, localAnchorX, localAnchorY, playerUpdateContext, true);
            if (sprite.visible) {
                countLodTier(playerLodSummary, playerLodTier);
                visiblePlayers += 1;
            }
        };

        const iteratePlayersWithWorkerSet = !!(workerPlayerSet && workerPlayerSet.size > 0);
        if (iteratePlayersWithWorkerSet) {
            if (myPlayerId && players.has(myPlayerId)) {
                processPlayerCandidate(myPlayerId, players.get(myPlayerId));
            }
            workerPlayerSet.forEach((playerId) => {
                if (playerId === myPlayerId) return;
                const playerEntry = players.get(playerId);
                if (!playerEntry) return;
                processPlayerCandidate(playerId, playerEntry);
            });
            for (const [playerId, sprite] of playerSprites) {
                if (playerId === myPlayerId) continue;
                if (!players.has(playerId)) continue;
                if (!workerPlayerSet.has(playerId)) {
                    hidePlayerSprite(sprite);
                }
            }
        } else {
            for (const [playerId, playerEntry] of players) {
                processPlayerCandidate(playerId, playerEntry);
            }
        }
        setLastVisiblePlayers(visiblePlayers);
        setLastPlayerRenderCap(playerRenderCap);
        setLastPlayerLodSummary(playerLodSummary);

        if (useWebGPUPlayers) {
            try {
                if (playerInstanceOffset > 0) {
                    const instances = playerInstanceOffset / WEBGPU_PLAYER_INSTANCE_STRIDE;
                    webgpuPlayerLayer.render(
                        worldViewBounds,
                        playerInstanceBuffer.subarray(0, playerInstanceOffset)
                    );
                    if (window.__e2e) {
                        window.__e2e.webgpuPlayerInstances = instances;
                    }
                } else {
                    webgpuPlayerLayer.clear(worldViewBounds);
                    if (window.__e2e) {
                        window.__e2e.webgpuPlayerInstances = 0;
                    }
                }
            } catch (error) {
                disableWebGPUPlayerLayer('player render failure', error);
            }
        } else {
            if (window.__e2e) {
                window.__e2e.webgpuPlayerInstances = 0;
            }
            if (webgpuPlayerLayer && webgpuPlayerLayer.ready) {
                try {
                    webgpuPlayerLayer.clear(worldViewBounds);
                } catch (error) {
                    disableWebGPUPlayerLayer('player clear failure', error);
                }
            }
        }

        for (const [playerId, sprite] of playerSprites) {
            if (players.has(playerId)) continue;
            if (localPlayerSprite === sprite) {
                setLocalPlayerSprite(null);
            }
            playerContainer.removeChild(sprite);
            sprite.destroy({ children: true });
            playerSprites.delete(playerId);
        }

        // Update projectile sprites
        const projectileRenderCap = getProjectileRenderCap();
        const rawProjectileMode = projectileRawModeActive;
        const webgpuProjectilesReady = !!(webgpuProjectileLayer && webgpuProjectileLayer.ready);
        if (webgpuProjectilesReady && !webgpuProjectileRenderPathActive) {
            const projectileCountPressure = projectiles.size >= WEBGPU_PROJECTILE_ACTIVATION_MIN_COUNT;
            const framePressure =
                smoothedFrameMs >= WEBGPU_FRAME_PRESSURE_MS &&
                projectiles.size >= Math.max(40, Math.floor(WEBGPU_PROJECTILE_ACTIVATION_MIN_COUNT * 0.5));
            if (WEBGPU_FORCE_ACTIVE || projectileCountPressure || framePressure) {
                setWebgpuProjectileRenderPathActive(true);
                const backendLabel = formatAcceleratedBackendLabel(getAcceleratedLayerBackend(webgpuProjectileLayer));
                log(
                    `${backendLabel} projectile shader path engaged (projectiles=${projectiles.size}, frameMs=${smoothedFrameMs.toFixed(2)}).`,
                    'info'
                );
            }
        }
        const useWebGPUProjectiles =
            webgpuProjectilesReady && (WEBGPU_FORCE_ACTIVE || webgpuProjectileRenderPathActive);
        if (window.__e2e) {
            window.__e2e.webgpuProjectileLayerActive = useWebGPUProjectiles;
            window.__e2e.webgpuProjectileLayerBackend = getAcceleratedLayerBackend(webgpuProjectileLayer);
            window.__e2e.acceleratedProjectileBackend = window.__e2e.webgpuProjectileLayerBackend;
        }
        const projectileDenseVisualMode =
            ultraPerformanceMode ||
            STABLE_MODE_FORCED ||
            totalPlayerCount > HIGH_POPULATION_PLAYER_COUNT ||
            smoothedFrameMs > STABLE_DENSE_FRAME_MS;
        const projectileCullLeft = worldViewBounds.left - PROJECTILE_CULL_MARGIN_WORLD;
        const projectileCullRight = worldViewBounds.right + PROJECTILE_CULL_MARGIN_WORLD;
        const projectileCullTop = worldViewBounds.top - PROJECTILE_CULL_MARGIN_WORLD;
        const projectileCullBottom = worldViewBounds.bottom + PROJECTILE_CULL_MARGIN_WORLD;
        let projectileInstanceBuffer = null;
        let projectileInstanceOffset = 0;
        const projectileLodSummary = createLodSummaryCounter();
        const projectileDotCullStride = getProjectileDotCullStride();
        const LOCAL_PROJECTILE_OVERFLOW_CAP = 24;
        let localProjectileOverflowCount = 0;
        const workerProjectileCullGraceMs = Math.max(
            PROJECTILE_WORKER_CULL_GRACE_MS,
            getWorkerCullDispatchIntervalMs() * 3
        );
        if (useWebGPUProjectiles) {
            const projectedCount = workerProjectileSet
                ? Math.min(projectiles.size, Math.max(projectileRenderCap, workerProjectileSet.size))
                : Math.min(projectiles.size, projectileRenderCap);
            projectileInstanceBuffer = ensureWebGPUProjectileScratchCapacity(projectedCount);

            if (projectileSprites.size > 0) {
                for (const [projectileId, sprite] of projectileSprites) {
                    projectileContainer.removeChild(sprite);
                    releaseProjectileSprite(sprite);
                    projectileSprites.delete(projectileId);
                }
            }
        }
        let visibleProjectiles = 0;
        const processProjectileCandidate = (projectileId, projectile) => {
            let sprite = projectileSprites.get(projectileId);
            const px =
                Number.isFinite(projectile.render_x) ? projectile.render_x : projectile.x;
            const py =
                Number.isFinite(projectile.render_y) ? projectile.render_y : projectile.y;
            const pdxToLocal = px - localAnchorX;
            const pdyToLocal = py - localAnchorY;
            const projectileDistanceSq = (pdxToLocal * pdxToLocal) + (pdyToLocal * pdyToLocal);
            const projectileOwnerId = projectile.owner_id ?? null;
            const isLocalOwnedProjectile = !!(
                myPlayerId &&
                projectileOwnerId &&
                String(projectileOwnerId) === String(myPlayerId)
            );
            const isPriorityProjectile =
                isLocalOwnedProjectile ||
                projectileDistanceSq <= PROJECTILE_PRIORITY_DISTANCE_SQ;
            const projectileLodTier = resolveRenderLodTier(
                projectileDistanceSq,
                PROJECTILE_LOD_MEDIUM_DISTANCE_SQ,
                PROJECTILE_LOD_LOW_DISTANCE_SQ,
                PROJECTILE_LOD_DOT_DISTANCE_SQ,
                { isPriority: isPriorityProjectile, lodScale }
            );
            const markProjectileVisible = () => {
                if (isLocalOwnedProjectile && visibleProjectiles >= projectileRenderCap) {
                    localProjectileOverflowCount += 1;
                }
                visibleProjectiles += 1;
            };
            const inView =
                px >= projectileCullLeft &&
                px <= projectileCullRight &&
                py >= projectileCullTop &&
                py <= projectileCullBottom;

            if (!inView) {
                if (sprite && !useWebGPUProjectiles) {
                    sprite.visible = false;
                }
                return;
            }

            if (workerProjectileSet) {
                const selectedByWorker = workerProjectileSet.has(projectileId);
                if (selectedByWorker) {
                    projectileWorkerCullGraceUntil.set(projectileId, frameNowMs + workerProjectileCullGraceMs);
                } else {
                    const graceUntilMs = projectileWorkerCullGraceUntil.get(projectileId) || 0;
                    const withinCullGrace = graceUntilMs > frameNowMs;
                    if (!withinCullGrace && !isPriorityProjectile) {
                        if (sprite && !useWebGPUProjectiles) {
                            sprite.visible = false;
                        }
                        return;
                    }
                }
            } else if (visibleProjectiles >= projectileRenderCap) {
                if (
                    isLocalOwnedProjectile &&
                    localProjectileOverflowCount < LOCAL_PROJECTILE_OVERFLOW_CAP
                ) {
                    // Keep locally-fired projectiles visible even when global cap is reached.
                } else {
                if (sprite && !useWebGPUProjectiles) {
                    sprite.visible = false;
                }
                return;
                }
            }
            if (
                projectileLodTier === 'dot' &&
                !isPriorityProjectile &&
                !useWebGPUProjectiles &&
                ((frameCounter + getEntityCadenceBucket(projectileId)) % projectileDotCullStride) !== 0
            ) {
                if (sprite) {
                    if (sprite.position.x !== px) sprite.position.x = px;
                    if (sprite.position.y !== py) sprite.position.y = py;
                    if (!sprite.visible) sprite.visible = true;
                    if (Math.abs(sprite.alpha - 0.58) > ALPHA_EPSILON) {
                        sprite.alpha = 0.58;
                    }
                    countLodTier(projectileLodSummary, projectileLodTier);
                    markProjectileVisible();
                }
                return;
            }
            projectile._renderLodTier = projectileLodTier;

            if (useWebGPUProjectiles) {
                countLodTier(projectileLodSummary, projectileLodTier);
                projectileInstanceOffset = writeWebGPUProjectileInstance(
                    projectileInstanceBuffer,
                    projectileInstanceOffset,
                    projectile,
                    px,
                    py
                );
                markProjectileVisible();
                return;
            }

            if (!sprite) {
                sprite = createProjectileSprite(projectile);
                projectileSprites.set(projectileId, sprite);
                projectileContainer.addChild(sprite);
            }
            if (!useWebGPUProjectiles && projectileSpriteUpdateStride > 1) {
                if (!isPriorityProjectile) {
                    const cadenceBucket = getEntityCadenceBucket(projectileId);
                    const shouldUpdateProjectileSprite = ((frameCounter + cadenceBucket) % projectileSpriteUpdateStride) === 0;
                    if (!shouldUpdateProjectileSprite) {
                        if (sprite.position.x !== px) sprite.position.x = px;
                        if (sprite.position.y !== py) sprite.position.y = py;
                        if (!sprite.visible) sprite.visible = true;
                        countLodTier(projectileLodSummary, projectileLodTier);
                        markProjectileVisible();
                        return;
                    }
                }
            }
            if (
                updateProjectileSprite(
                    sprite,
                    projectile,
                    px,
                    py,
                    true,
                    projectileDenseVisualMode,
                    projectileLodTier
                )
            ) {
                countLodTier(projectileLodSummary, projectileLodTier);
                markProjectileVisible();
            }
        };

        const iterateProjectilesWithWorkerSet = !!(workerProjectileSet && workerProjectileSet.size > 0);
        if (iterateProjectilesWithWorkerSet) {
            workerProjectileSet.forEach((projectileId) => {
                const projectile = projectiles.get(projectileId);
                if (!projectile) return;
                processProjectileCandidate(projectileId, projectile);
            });
            if (myPlayerId) {
                // Ensure newly-fired local projectiles are still considered even if worker culling
                // has not selected them yet in the current result batch.
                for (const [projectileId, projectile] of projectiles) {
                    if (!projectile) continue;
                    if (workerProjectileSet.has(projectileId)) continue;
                    if (projectileWorkerCullGraceUntil.has(projectileId)) continue;
                    const ownerId = projectile.owner_id ?? '';
                    if (!ownerId || String(ownerId) !== String(myPlayerId)) continue;
                    processProjectileCandidate(projectileId, projectile);
                }
            }
            projectileWorkerCullGraceUntil.forEach((graceUntilMs, projectileId) => {
                if (workerProjectileSet.has(projectileId)) return;
                if (graceUntilMs <= frameNowMs) {
                    projectileWorkerCullGraceUntil.delete(projectileId);
                    return;
                }
                const projectile = projectiles.get(projectileId);
                if (!projectile) {
                    projectileWorkerCullGraceUntil.delete(projectileId);
                    return;
                }
                processProjectileCandidate(projectileId, projectile);
            });
            if (!useWebGPUProjectiles) {
                for (const [projectileId, sprite] of projectileSprites) {
                    if (!projectiles.has(projectileId)) continue;
                    if (workerProjectileSet.has(projectileId)) continue;
                    const graceUntilMs = projectileWorkerCullGraceUntil.get(projectileId) || 0;
                    if (graceUntilMs > frameNowMs) continue;
                    if (sprite && !useWebGPUProjectiles) {
                        sprite.visible = false;
                    }
                }
            }
        } else {
            for (const [projectileId, projectile] of projectiles) {
                processProjectileCandidate(projectileId, projectile);
            }
        }
        setLastVisibleProjectiles(visibleProjectiles);
        setLastProjectileRenderCap(projectileRenderCap);
        setLastProjectileLodSummary(projectileLodSummary);

        if (useWebGPUProjectiles) {
            try {
                if (projectileInstanceOffset > 0) {
                    const instances = projectileInstanceOffset / WEBGPU_PROJECTILE_INSTANCE_STRIDE;
                    webgpuProjectileLayer.render(
                        worldViewBounds,
                        projectileInstanceBuffer.subarray(0, projectileInstanceOffset)
                    );
                    if (window.__e2e) {
                        window.__e2e.webgpuProjectileInstances = instances;
                    }
                } else {
                    webgpuProjectileLayer.clear(worldViewBounds);
                    if (window.__e2e) {
                        window.__e2e.webgpuProjectileInstances = 0;
                    }
                }
            } catch (error) {
                disableWebGPUProjectileLayer('projectile render failure', error);
            }
        } else {
            if (window.__e2e) {
                window.__e2e.webgpuProjectileInstances = 0;
            }
            if (webgpuProjectileLayer && webgpuProjectileLayer.ready) {
                try {
                    webgpuProjectileLayer.clear(worldViewBounds);
                } catch (error) {
                    disableWebGPUProjectileLayer('projectile clear failure', error);
                }
            }
        }

        for (const [projectileId, sprite] of projectileSprites) {
            if (projectiles.has(projectileId)) continue;
            projectileContainer.removeChild(sprite);
            releaseProjectileSprite(sprite);
            projectileSprites.delete(projectileId);
            projectileWorkerCullGraceUntil.delete(projectileId);
        }

        // Update pickup sprites
        const pickupCullLeft = worldViewBounds.left - PICKUP_CULL_MARGIN_WORLD;
        const pickupCullRight = worldViewBounds.right + PICKUP_CULL_MARGIN_WORLD;
        const pickupCullTop = worldViewBounds.top - PICKUP_CULL_MARGIN_WORLD;
        const pickupCullBottom = worldViewBounds.bottom + PICKUP_CULL_MARGIN_WORLD;
        const shouldRefreshPickupVisibility =
            !ultraPerformanceMode ||
            (frameCounter % PICKUP_VISIBILITY_UPDATE_STRIDE === 0);
        for (const [pickupId, pickup] of pickups) {
            let sprite = pickupSprites.get(pickupId);
            let isNewSprite = false;
            if (!sprite) {
                sprite = createPickupSprite(pickup);
                pickupSprites.set(pickupId, sprite);
                pickupContainer.addChild(sprite);
                isNewSprite = true;
            }

            if (isNewSprite || sprite._lastWorldX !== pickup.x || sprite._lastWorldY !== pickup.y) {
                sprite.position.set(pickup.x, pickup.y);
                sprite._lastWorldX = pickup.x;
                sprite._lastWorldY = pickup.y;
            }

            if (isNewSprite || shouldRefreshPickupVisibility) {
                sprite.visible =
                    !!pickup.is_active &&
                    pickup.x >= pickupCullLeft &&
                    pickup.x <= pickupCullRight &&
                    pickup.y >= pickupCullTop &&
                    pickup.y <= pickupCullBottom;
            }
        }

        for (const [pickupId, sprite] of pickupSprites) {
            if (pickups.has(pickupId)) continue;
            pickupContainer.removeChild(sprite);
            sprite.destroy({ children: true });
            pickupSprites.delete(pickupId);
        }
    }

    return { updateSprites };
}
