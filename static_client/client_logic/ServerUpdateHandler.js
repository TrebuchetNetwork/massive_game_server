/**
 * ServerUpdateHandler.js - Server state update processing
 *
 * Extracted from client.html. Contains processServerUpdate (JSON-parsed path),
 * processDeltaStateFast (FlatBuffer zero-copy path), and
 * tryProcessDeltaMessageFast (entry point for fast delta processing).
 * Uses getCtx callback pattern to access shared game state.
 */

export function createServerUpdateHandler(getCtx) {

    function processServerUpdate(messageData, isInitial = false) {
        const ctx = getCtx();
        const {
            log, GP, normalizeAngle, walls, players, projectiles, pickups, zones,
            localPlayerState: _lps, myPlayerId, drawWalls, drawZones,
            removePlayerClientState, removeProjectileClientState,
            normalizePlayerDeltaMask, assignPlayerStateFromObject,
            markProjectileServerUpdate, isWallDebugEnabled, logWallDebug,
            effectsManager, minimap, killFeed: _kf, matchInfo: _mi,
            updateKillFeed, refreshMatchInfoUi, updateFlags,
            RESPAWN_ANIMATION_LIGHTWEIGHT, pendingInputs: _pi,
            maybeRecordInterpolationSnapshot,
            minimapWallsCacheDirty: _mwcd,
            currentMapName: _cmn,
        } = ctx;

        if (!messageData) {
            log(`[processServerUpdate] Error: messageData is ${messageData}. isInitial: ${isInitial}. Stack: ${new Error().stack}`, 'error');
            console.error("[processServerUpdate] messageData:", messageData, "isInitial:", isInitial);
            return;
        }

        let localPlayerState = ctx.localPlayerState;
        let killFeed = ctx.killFeed;
        let matchInfo = ctx.matchInfo;
        let minimapWallsCacheDirty = ctx.minimapWallsCacheDirty;
        let currentMapName = ctx.currentMapName;
        let lastProcessedInput = ctx.lastProcessedInput;
        let pendingInputs = ctx.pendingInputs;

        const serverTime = Number(messageData.timestamp);
        let wallsChanged = false;
        const wallDebugEnabled = isWallDebugEnabled();

        if (isInitial) {
            walls.clear();
            if (messageData.walls) {
                const initialWalls = messageData.walls;
                for (let i = 0; i < initialWalls.length; i += 1) {
                    const wallData = initialWalls[i];
                    if (!wallData.is_destructible || wallData.current_health > 0) {
                        walls.set(wallData.id, wallData);
                        wallsChanged = true;
                    } else if (wallDebugEnabled) {
                        logWallDebug(`[INITIAL STATE] Filtering out destroyed wall ${wallData.id} with health ${wallData.current_health}`, 'warn');
                    }
                }
            }
            drawWalls();
            if (messageData.zones && messageData.zones.length > 0) {
                zones.clear();
                for (const zoneData of messageData.zones) {
                    zones.set(zoneData.id, zoneData);
                }
                drawZones();
            }
            if (messageData.map_name) {
                ctx.setCurrentMapName(messageData.map_name);
            }
            ctx.setMinimapWallsCacheDirty(true);
            if (minimap) minimap.wallsNeedUpdate = true;
        } else {
            if (messageData.destroyed_wall_ids && messageData.destroyed_wall_ids.length > 0) {
                if (wallDebugEnabled) {
                    logWallDebug(`[WALL DEBUG] Destroying ${messageData.destroyed_wall_ids.length} walls`, 'info');
                }
                for (let i = 0; i < messageData.destroyed_wall_ids.length; i += 1) {
                    const wallId = messageData.destroyed_wall_ids[i];
                    const wall = walls.get(wallId);
                    if (wall) {
                        wall.current_health = 0;
                        wallsChanged = true;
                        if (wallDebugEnabled) {
                            logWallDebug(`[WALL DESTROYED] Wall ${wallId} destroyed at (${wall.x}, ${wall.y})`, 'warn');
                        }
                    }
                }
            }

            if (messageData.updated_walls && messageData.updated_walls.length > 0) {
                if (wallDebugEnabled) {
                    logWallDebug(`[WALL DEBUG] Received ${messageData.updated_walls.length} updated walls in delta`, 'info');
                }
                const updatedWalls = messageData.updated_walls;
                for (let i = 0; i < updatedWalls.length; i += 1) {
                    const wallData = updatedWalls[i];
                    const prevWall = walls.get(wallData.id);
                    walls.set(wallData.id, wallData);
                    wallsChanged = true;

                    if (prevWall && prevWall.current_health === 0 && wallData.current_health > 0) {
                        if (wallDebugEnabled) {
                            logWallDebug(`[WALL RESPAWN] Wall ${wallData.id} respawned at (${wallData.x}, ${wallData.y}) with health ${wallData.current_health}/${wallData.max_health}`, 'success');
                        }
                        if (effectsManager && localPlayerState) {
                            const dx = wallData.x + wallData.width / 2 - localPlayerState.x;
                            const dy = wallData.y + wallData.height / 2 - localPlayerState.y;
                            const distance = Math.sqrt(dx * dx + dy * dy);
                            if (distance < 1000) {
                                effectsManager.createWallRespawnEffect({
                                    x: wallData.x + wallData.width / 2,
                                    y: wallData.y + wallData.height / 2
                                }, wallData);
                            }
                        }
                    } else if (!prevWall) {
                        if (wallDebugEnabled) {
                            logWallDebug(`[WALL NEW] New wall ${wallData.id} discovered at (${wallData.x}, ${wallData.y}) with health ${wallData.current_health}/${wallData.max_health}`, 'warn');
                        }
                        if (wallDebugEnabled && wallData.is_destructible && wallData.current_health === wallData.max_health) {
                            logWallDebug(`[WALL RESPAWN?] This new wall has full health - might be a respawn`, 'warn');
                        }
                        wallsChanged = true;
                    } else if (wallDebugEnabled && prevWall && prevWall.current_health !== wallData.current_health) {
                        logWallDebug(`[WALL HEALTH] Wall ${wallData.id} health changed from ${prevWall.current_health} to ${wallData.current_health}`, 'info');
                    }
                }
            } else {
                if (wallDebugEnabled && messageData.destroyed_wall_ids && messageData.destroyed_wall_ids.length > 0) {
                    logWallDebug(`[WALL DEBUG] ${messageData.destroyed_wall_ids.length} walls destroyed but no updated_walls in this delta`, 'warn');
                }
            }
        }

        if (messageData.walls && messageData.walls.length > 0 && !isInitial) {
            if (wallDebugEnabled) {
                logWallDebug(`Received ${messageData.walls.length} walls in delta update - possible AOI change`, 'info');
            }
            const incomingWalls = messageData.walls;
            const incomingWallIds = new Set();
            const previousWallIds = new Set(walls.keys());

            for (let i = 0; i < incomingWalls.length; i += 1) {
                const wallData = incomingWalls[i];
                walls.set(wallData.id, wallData);
                incomingWallIds.add(wallData.id);

                if (!previousWallIds.has(wallData.id)) {
                    wallsChanged = true;
                    if (wallDebugEnabled) {
                        logWallDebug(`New wall ${wallData.id} discovered in AOI at (${wallData.x}, ${wallData.y})`, 'info');
                    }
                    if (wallData.current_health > 0 && wallData.is_destructible && effectsManager && localPlayerState) {
                        const dx = wallData.x + wallData.width / 2 - localPlayerState.x;
                        const dy = wallData.y + wallData.height / 2 - localPlayerState.y;
                        const distance = Math.sqrt(dx * dx + dy * dy);
                        if (distance < 1000) {
                            effectsManager.createWallRespawnEffect({
                                x: wallData.x + wallData.width / 2,
                                y: wallData.y + wallData.height / 2
                            }, wallData);
                        }
                    }
                }
            }

            previousWallIds.forEach(wallId => {
                if (!incomingWallIds.has(wallId)) {
                    walls.delete(wallId);
                    wallsChanged = true;
                    if (wallDebugEnabled) {
                        logWallDebug(`Wall ${wallId} removed - left AOI`, 'info');
                    }
                }
            });
        }

        if (wallsChanged && !isInitial) {
            drawWalls();
        }

        if (messageData.players) {
            const incomingPlayers = messageData.players;
            for (let i = 0; i < incomingPlayers.length; i += 1) {
                const pData = incomingPlayers[i];
                if (!pData || !pData.id) continue;
                const existingPlayer = players.get(pData.id);
                const forceFullState = isInitial || !existingPlayer;
                const changedMask = normalizePlayerDeltaMask(pData.changed_fields, forceFullState);
                const resolvedUsername = pData.id === myPlayerId
                    ? (localPlayerState?.username || existingPlayer?.username || pData.username || '')
                    : (existingPlayer?.username || pData.username || '');

                if (pData.id === myPlayerId) {
                    if (!localPlayerState) {
                        localPlayerState = existingPlayer || {};
                        ctx.setLocalPlayerState(localPlayerState);
                    }
                    const previousPredictedX = Number.isFinite(localPlayerState.x) ? localPlayerState.x : (Number(pData.x) || 0);
                    const previousPredictedY = Number.isFinite(localPlayerState.y) ? localPlayerState.y : (Number(pData.y) || 0);
                    const previousPredictedRotation = Number.isFinite(localPlayerState.rotation)
                        ? localPlayerState.rotation
                        : (Number(pData.rotation) || 0);

                    assignPlayerStateFromObject(
                        localPlayerState,
                        pData,
                        resolvedUsername,
                        changedMask,
                        forceFullState
                    );

                    const serverX = Number.isFinite(localPlayerState.x) ? localPlayerState.x : previousPredictedX;
                    const serverY = Number.isFinite(localPlayerState.y) ? localPlayerState.y : previousPredictedY;
                    const serverRotation = Number.isFinite(localPlayerState.rotation)
                        ? localPlayerState.rotation
                        : previousPredictedRotation;
                    const errorX = serverX - previousPredictedX;
                    const errorY = serverY - previousPredictedY;
                    const errorSq = errorX * errorX + errorY * errorY;
                    if (!localPlayerState.alive || errorSq > (260 * 260)) {
                        localPlayerState.x = serverX;
                        localPlayerState.y = serverY;
                        localPlayerState.rotation = serverRotation;
                    } else {
                        localPlayerState.x = previousPredictedX + (errorX * 0.24);
                        localPlayerState.y = previousPredictedY + (errorY * 0.24);
                        const rotError = normalizeAngle(serverRotation - previousPredictedRotation);
                        localPlayerState.rotation = normalizeAngle(previousPredictedRotation + (rotError * 0.28));
                    }
                    if (messageData.last_processed_input_sequence !== undefined && messageData.last_processed_input_sequence !== null) {
                        lastProcessedInput = Number(messageData.last_processed_input_sequence);
                        ctx.setLastProcessedInput(lastProcessedInput);
                        if (!isNaN(lastProcessedInput)) {
                            pendingInputs = pendingInputs.filter(inp => inp.sequence > lastProcessedInput);
                            ctx.setPendingInputs(pendingInputs);
                        }
                    }
                    localPlayerState.render_x = localPlayerState.x;
                    localPlayerState.render_y = localPlayerState.y;
                    localPlayerState.render_rotation = localPlayerState.rotation;
                    players.set(pData.id, localPlayerState);
                } else {
                    const remoteState = existingPlayer || {};
                    assignPlayerStateFromObject(
                        remoteState,
                        pData,
                        resolvedUsername,
                        changedMask,
                        forceFullState
                    );
                    players.set(pData.id, remoteState);
                }
            }
        }

        if (messageData.removed_player_ids && messageData.removed_player_ids.length > 0) {
            const removedPlayers = messageData.removed_player_ids;
            for (let i = 0; i < removedPlayers.length; i += 1) {
                const removedId = removedPlayers[i];
                removePlayerClientState(removedId);
                log(`Player ${removedId} removed.`, 'info');
            }
        }

        if (messageData.projectiles) {
            const incomingProjectiles = messageData.projectiles;
            const projectileServerUpdateMs = performance.now();
            for (let i = 0; i < incomingProjectiles.length; i += 1) {
                const projectileData = incomingProjectiles[i];
                if (!projectileData) continue;
                markProjectileServerUpdate(projectileData, projectileServerUpdateMs);
                projectiles.set(projectileData.id, projectileData);
            }
        }
        if (messageData.removed_projectiles) {
            const removedProjectiles = messageData.removed_projectiles;
            for (let i = 0; i < removedProjectiles.length; i += 1) {
                removeProjectileClientState(removedProjectiles[i]);
            }
        }

        if (messageData.pickups) {
            const incomingPickups = messageData.pickups;
            for (let i = 0; i < incomingPickups.length; i += 1) {
                const pickupData = incomingPickups[i];
                pickups.set(pickupData.id, pickupData);
            }
        }
        if (messageData.deactivated_pickup_ids) {
            const deactivatedPickups = messageData.deactivated_pickup_ids;
            for (let i = 0; i < deactivatedPickups.length; i += 1) {
                const id = deactivatedPickups[i];
                const pickup = pickups.get(id);
                if (pickup) pickup.is_active = false;
            }
        }

        if (messageData.kill_feed) {
            ctx.setKillFeed(messageData.kill_feed);
            updateKillFeed();
        }

        if (messageData.match_info) {
            ctx.setMatchInfo(messageData.match_info);
            if (window.__e2e) {
                window.__e2e.matchInfoReady = true;
            }
            refreshMatchInfoUi(isInitial);
        } else if (isInitial) {
            log("Initial state received without match_info.", "warn");
        }

        if (messageData.flag_states) {
            updateFlags(messageData.flag_states);
        }

        if (messageData.game_events && effectsManager) {
            const suppressRespawnEventEffects =
                RESPAWN_ANIMATION_LIGHTWEIGHT &&
                !!localPlayerState &&
                !localPlayerState.alive;
            if (!suppressRespawnEventEffects) {
                const gameEvents = messageData.game_events;
                for (let i = 0; i < gameEvents.length; i += 1) {
                    effectsManager.processGameEvent(gameEvents[i]);
                }
            }
        }

        maybeRecordInterpolationSnapshot(serverTime);
    }


    function processDeltaStateFast(delta) {
        const ctx = getCtx();
        const {
            log, GP, normalizeAngle, walls, players, projectiles, pickups, zones,
            myPlayerId, drawWalls,
            removePlayerClientState, removeProjectileClientState,
            normalizePlayerDeltaMask, assignPlayerStateFromTable,
            assignWallStateFromTable, assignProjectileStateFromTable,
            assignPickupStateFromTable, parseMatchInfo, parseTeamScores,
            markProjectileServerUpdate,
            isWallDebugEnabled, logWallDebug,
            effectsManager, minimap,
            updateKillFeed, refreshMatchInfoUi, updateFlags,
            flatbufferParseScratch,
            DELTA_SUPPORTS_REMOVED_PLAYER_IDS,
            DELTA_SUPPORTS_CHANGED_PLAYER_FIELDS,
            DELTA_SUPPORTS_UPDATED_WALLS,
            DELTA_SUPPORTS_FULL_WALLS,
            PLAYER_DELTA_FULL_MASK,
            maybeRecordInterpolationSnapshot,
        } = ctx;

        let localPlayerState = ctx.localPlayerState;
        let lastProcessedInput = ctx.lastProcessedInput;
        let pendingInputs = ctx.pendingInputs;

        const serverTime = Number(delta.timestamp());
        let wallsChanged = false;
        const wallDebugEnabled = isWallDebugEnabled();

        const destroyedWallLength = delta.destroyedWallIdsLength();
        if (destroyedWallLength > 0) {
            if (wallDebugEnabled) {
                logWallDebug(`[WALL DEBUG] Destroying ${destroyedWallLength} walls`, 'info');
            }
            for (let i = 0; i < destroyedWallLength; i += 1) {
                const wallId = delta.destroyedWallIds(i);
                if (!wallId) continue;
                const wall = walls.get(wallId);
                if (wall) {
                    wall.current_health = 0;
                    wallsChanged = true;
                    if (wallDebugEnabled) {
                        logWallDebug(`[WALL DESTROYED] Wall ${wallId} destroyed at (${wall.x}, ${wall.y})`, 'warn');
                    }
                }
            }
        }

        if (DELTA_SUPPORTS_UPDATED_WALLS) {
            const updatedWallLength = delta.updatedWallsLength();
            if (updatedWallLength > 0) {
                if (wallDebugEnabled) {
                    logWallDebug(`[WALL DEBUG] Received ${updatedWallLength} updated walls in delta`, 'info');
                }
                const wallTable = flatbufferParseScratch.wall;
                for (let i = 0; i < updatedWallLength; i += 1) {
                    const wall = delta.updatedWalls(i, wallTable);
                    if (!wall) continue;
                    const wallId = wall.id();
                    const prevWall = walls.get(wallId);
                    const prevHealth = prevWall ? prevWall.current_health : null;
                    const nextWall = prevWall || {};
                    assignWallStateFromTable(nextWall, wall);
                    walls.set(wallId, nextWall);
                    wallsChanged = true;

                    if (prevWall && prevHealth === 0 && nextWall.current_health > 0) {
                        if (wallDebugEnabled) {
                            logWallDebug(`[WALL RESPAWN] Wall ${nextWall.id} respawned at (${nextWall.x}, ${nextWall.y}) with health ${nextWall.current_health}/${nextWall.max_health}`, 'success');
                        }
                        if (effectsManager && localPlayerState) {
                            const dx = nextWall.x + nextWall.width / 2 - localPlayerState.x;
                            const dy = nextWall.y + nextWall.height / 2 - localPlayerState.y;
                            const distance = Math.sqrt(dx * dx + dy * dy);
                            if (distance < 1000) {
                                effectsManager.createWallRespawnEffect({
                                    x: nextWall.x + nextWall.width / 2,
                                    y: nextWall.y + nextWall.height / 2
                                }, nextWall);
                            }
                        }
                    } else if (!prevWall) {
                        if (wallDebugEnabled) {
                            logWallDebug(`[WALL NEW] New wall ${nextWall.id} discovered at (${nextWall.x}, ${nextWall.y}) with health ${nextWall.current_health}/${nextWall.max_health}`, 'warn');
                        }
                        if (wallDebugEnabled && nextWall.is_destructible && nextWall.current_health === nextWall.max_health) {
                            logWallDebug(`[WALL RESPAWN?] This new wall has full health - might be a respawn`, 'warn');
                        }
                    } else if (wallDebugEnabled && prevHealth !== nextWall.current_health) {
                        logWallDebug(`[WALL HEALTH] Wall ${nextWall.id} health changed from ${prevHealth} to ${nextWall.current_health}`, 'info');
                    }
                }
            } else if (wallDebugEnabled && destroyedWallLength > 0) {
                logWallDebug(`[WALL DEBUG] ${destroyedWallLength} walls destroyed but no updated_walls in this delta`, 'warn');
            }
        }

        if (DELTA_SUPPORTS_FULL_WALLS) {
            const wallLength = delta.wallsLength();
            if (wallLength > 0) {
                if (wallDebugEnabled) {
                    logWallDebug(`Received ${wallLength} walls in delta update - possible AOI change`, 'info');
                }
                const wallTable = flatbufferParseScratch.wall;
                const incomingWallIds = new Set();
                const previousWallIds = new Set(walls.keys());
                for (let i = 0; i < wallLength; i += 1) {
                    const wall = delta.walls(i, wallTable);
                    if (!wall) continue;
                    const wallId = wall.id();
                    incomingWallIds.add(wallId);
                    const previousWall = walls.get(wallId);
                    const nextWall = previousWall || {};
                    assignWallStateFromTable(nextWall, wall);
                    walls.set(wallId, nextWall);
                    if (!previousWall) {
                        wallsChanged = true;
                        if (wallDebugEnabled) {
                            logWallDebug(`New wall ${wallId} discovered in AOI at (${nextWall.x}, ${nextWall.y})`, 'info');
                        }
                        if (nextWall.current_health > 0 && nextWall.is_destructible && effectsManager && localPlayerState) {
                            const dx = nextWall.x + nextWall.width / 2 - localPlayerState.x;
                            const dy = nextWall.y + nextWall.height / 2 - localPlayerState.y;
                            const distance = Math.sqrt(dx * dx + dy * dy);
                            if (distance < 1000) {
                                effectsManager.createWallRespawnEffect({
                                    x: nextWall.x + nextWall.width / 2,
                                    y: nextWall.y + nextWall.height / 2
                                }, nextWall);
                            }
                        }
                    }
                }
                previousWallIds.forEach((wallId) => {
                    if (incomingWallIds.has(wallId)) return;
                    walls.delete(wallId);
                    wallsChanged = true;
                    if (wallDebugEnabled) {
                        logWallDebug(`Wall ${wallId} removed - left AOI`, 'info');
                    }
                });
            }
        }

        if (wallsChanged) {
            drawWalls();
        }

        const playerTable = flatbufferParseScratch.playerState;
        const playerLength = delta.playersLength();
        const changedPlayerFieldLength = DELTA_SUPPORTS_CHANGED_PLAYER_FIELDS
            ? delta.changedPlayerFieldsLength()
            : 0;
        for (let i = 0; i < playerLength; i += 1) {
            const player = delta.players(i, playerTable);
            if (!player) continue;
            const playerId = player.id();
            const existingPlayer = players.get(playerId);
            const changedMask = i < changedPlayerFieldLength
                ? delta.changedPlayerFields(i)
                : PLAYER_DELTA_FULL_MASK;
            let resolvedUsername = playerId === myPlayerId
                ? (localPlayerState?.username || existingPlayer?.username || '')
                : (existingPlayer?.username || '');
            if (!resolvedUsername) {
                resolvedUsername = player.username() || '';
            }

            if (playerId === myPlayerId) {
                if (!localPlayerState) {
                    localPlayerState = existingPlayer || {};
                    ctx.setLocalPlayerState(localPlayerState);
                }
                const previousPredictedX = Number.isFinite(localPlayerState.x) ? localPlayerState.x : player.x();
                const previousPredictedY = Number.isFinite(localPlayerState.y) ? localPlayerState.y : player.y();
                const previousPredictedRotation = Number.isFinite(localPlayerState.rotation)
                    ? localPlayerState.rotation
                    : player.rotation();

                assignPlayerStateFromTable(
                    localPlayerState,
                    player,
                    resolvedUsername,
                    changedMask,
                    !existingPlayer
                );

                const serverX = localPlayerState.x;
                const serverY = localPlayerState.y;
                const serverRotation = localPlayerState.rotation;
                const errorX = serverX - previousPredictedX;
                const errorY = serverY - previousPredictedY;
                const errorSq = errorX * errorX + errorY * errorY;
                if (!localPlayerState.alive || errorSq > (260 * 260)) {
                    localPlayerState.x = serverX;
                    localPlayerState.y = serverY;
                    localPlayerState.rotation = serverRotation;
                } else {
                    localPlayerState.x = previousPredictedX + (errorX * 0.24);
                    localPlayerState.y = previousPredictedY + (errorY * 0.24);
                    const rotError = normalizeAngle(serverRotation - previousPredictedRotation);
                    localPlayerState.rotation = normalizeAngle(previousPredictedRotation + (rotError * 0.28));
                }
                localPlayerState.render_x = localPlayerState.x;
                localPlayerState.render_y = localPlayerState.y;
                localPlayerState.render_rotation = localPlayerState.rotation;
                players.set(playerId, localPlayerState);
            } else {
                const remoteState = existingPlayer || {};
                assignPlayerStateFromTable(
                    remoteState,
                    player,
                    resolvedUsername,
                    changedMask,
                    !existingPlayer
                );
                players.set(playerId, remoteState);
            }
        }

        const lastProcessedInputSequence = delta.lastProcessedInputSequence();
        if (lastProcessedInputSequence !== undefined && lastProcessedInputSequence !== null) {
            lastProcessedInput = Number(lastProcessedInputSequence);
            ctx.setLastProcessedInput(lastProcessedInput);
            if (!isNaN(lastProcessedInput)) {
                pendingInputs = pendingInputs.filter(inp => inp.sequence > lastProcessedInput);
                ctx.setPendingInputs(pendingInputs);
            }
        }

        if (DELTA_SUPPORTS_REMOVED_PLAYER_IDS) {
            const removedPlayerLength = delta.removedPlayerIdsLength();
            if (removedPlayerLength > 0) {
                for (let i = 0; i < removedPlayerLength; i += 1) {
                    const removedId = delta.removedPlayerIds(i);
                    if (!removedId) continue;
                    removePlayerClientState(removedId);
                    log(`Player ${removedId} removed.`, 'info');
                }
            }
        }

        const projectileTable = flatbufferParseScratch.projectileState;
        const projectileLength = delta.projectilesLength();
        const projectileServerUpdateMs = performance.now();
        for (let i = 0; i < projectileLength; i += 1) {
            const projectile = delta.projectiles(i, projectileTable);
            if (!projectile) continue;
            const projectileId = projectile.id();
            const projectileState = projectiles.get(projectileId) || {};
            assignProjectileStateFromTable(projectileState, projectile, projectileServerUpdateMs);
            projectiles.set(projectileId, projectileState);
        }

        const removedProjectileLength = delta.removedProjectilesLength();
        if (removedProjectileLength > 0) {
            for (let i = 0; i < removedProjectileLength; i += 1) {
                const projectileId = delta.removedProjectiles(i);
                if (!projectileId) continue;
                removeProjectileClientState(projectileId);
            }
        }

        const pickupTable = flatbufferParseScratch.pickup;
        const pickupLength = delta.pickupsLength();
        for (let i = 0; i < pickupLength; i += 1) {
            const pickup = delta.pickups(i, pickupTable);
            if (!pickup) continue;
            const pickupId = pickup.id();
            const pickupState = pickups.get(pickupId) || {};
            assignPickupStateFromTable(pickupState, pickup);
            pickups.set(pickupId, pickupState);
        }

        const deactivatedPickupLength = delta.deactivatedPickupIdsLength();
        if (deactivatedPickupLength > 0) {
            for (let i = 0; i < deactivatedPickupLength; i += 1) {
                const pickupId = delta.deactivatedPickupIds(i);
                if (!pickupId) continue;
                const pickup = pickups.get(pickupId);
                if (pickup) pickup.is_active = false;
            }
        }

        const vecTable = flatbufferParseScratch.vec2;
        const killFeedEntry = flatbufferParseScratch.killFeedEntry;
        const killFeedLength = delta.killFeedLength();
        if (killFeedLength > 0) {
            const rows = new Array(killFeedLength);
            let writeIdx = 0;
            for (let i = 0; i < killFeedLength; i += 1) {
                const killFeedRow = delta.killFeed(i, killFeedEntry);
                if (!killFeedRow) continue;
                const killerPosition = killFeedRow.killerPosition(vecTable);
                const victimPosition = killFeedRow.victimPosition(vecTable);
                rows[writeIdx++] = {
                    killer_id: typeof killFeedRow.killerId === 'function' ? killFeedRow.killerId() : null,
                    victim_id: typeof killFeedRow.victimId === 'function' ? killFeedRow.victimId() : null,
                    killer_name: killFeedRow.killerName(),
                    victim_name: killFeedRow.victimName(),
                    weapon: killFeedRow.weapon(),
                    timestamp: killFeedRow.timestamp(),
                    killer_position: killerPosition ? { x: killerPosition.x(), y: killerPosition.y() } : null,
                    victim_position: victimPosition ? { x: victimPosition.x(), y: victimPosition.y() } : null,
                    is_headshot: killFeedRow.isHeadshot()
                };
            }
            if (writeIdx > 0) {
                rows.length = writeIdx;
                ctx.setKillFeed(rows);
                updateKillFeed();
            }
        }

        const parsedMatchInfo = parseMatchInfo(delta.matchInfo(flatbufferParseScratch.matchInfo));
        if (parsedMatchInfo) {
            ctx.setMatchInfo(parsedMatchInfo);
            if (window.__e2e) {
                window.__e2e.matchInfoReady = true;
            }
            refreshMatchInfoUi(false);
        }

        const flagTable = flatbufferParseScratch.flagState;
        const flagLength = delta.flagStatesLength();
        if (flagLength > 0) {
            const rows = new Array(flagLength);
            let writeIdx = 0;
            for (let i = 0; i < flagLength; i += 1) {
                const flagState = delta.flagStates(i, flagTable);
                if (!flagState) continue;
                const position = flagState.position(vecTable);
                rows[writeIdx++] = {
                    team_id: flagState.teamId(),
                    status: flagState.status(),
                    position: position ? { x: position.x(), y: position.y() } : { x: 0, y: 0 },
                    carrier_id: flagState.carrierId(),
                    respawn_timer: flagState.respawnTimer()
                };
            }
            if (writeIdx > 0) {
                rows.length = writeIdx;
                updateFlags(rows);
            }
        }

        if (effectsManager) {
            const gameEventTable = flatbufferParseScratch.gameEvent;
            const gameEventLength = delta.gameEventsLength();
            for (let i = 0; i < gameEventLength; i += 1) {
                const gameEvent = delta.gameEvents(i, gameEventTable);
                if (!gameEvent) continue;
                const position = gameEvent.position(vecTable);
                effectsManager.processGameEvent({
                    event_type: gameEvent.eventType(),
                    position: position ? { x: position.x(), y: position.y() } : { x: 0, y: 0 },
                    instigator_id: gameEvent.instigatorId(),
                    target_id: gameEvent.targetId(),
                    weapon_type: gameEvent.weaponType(),
                    value: gameEvent.value()
                });
            }
        }

        maybeRecordInterpolationSnapshot(serverTime);
    }


    function tryProcessDeltaMessageFast(data, e2eRef = null) {
        const ctx = getCtx();
        const { GameProtocol, GP, bindFlatBufferData, flatbufferParseScratch,
                FAST_DELTA_APPLY_ENABLED, log } = ctx;

        if (!FAST_DELTA_APPLY_ENABLED) {
            return false;
        }
        const buf = bindFlatBufferData(data);
        if (!buf) {
            return false;
        }
        try {
            const gameMsg = GameProtocol.GameMessage.getRootAsGameMessage(buf, flatbufferParseScratch.gameMessage);
            if (gameMsg.msgType() !== GP.MessageType.DeltaState) {
                return false;
            }
            const delta = gameMsg.actualMessage(flatbufferParseScratch.deltaStateMessage);
            if (!delta) {
                return false;
            }
            processDeltaStateFast(delta);
            if (e2eRef) {
                e2eRef.lastStateUpdate = performance.now();
            }
            return true;
        } catch (error) {
            ctx.incrementFastDeltaPathErrorCount();
            const errorCount = ctx.fastDeltaPathErrorCount;
            if (errorCount <= 3 || (errorCount % 50) === 0) {
                console.error('Fast delta apply failed; falling back to generic parser.', error);
                log(`Fast delta apply failed (${errorCount}): ${error?.message || error}`, 'warn');
            }
            return false;
        }
    }

    return {
        processServerUpdate,
        processDeltaStateFast,
        tryProcessDeltaMessageFast,
    };
}
