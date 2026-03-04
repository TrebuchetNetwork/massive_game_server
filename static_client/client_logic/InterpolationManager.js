/**
 * InterpolationManager.js - Entity interpolation, prediction, snapshots
 *
 * Extracted from client.html. Contains updateLocalPlayerPrediction,
 * interpolateEntities, and maybeRecordInterpolationSnapshot.
 * Uses getCtx callback pattern.
 */

import { clamp, lerp, normalizeAngle, smoothFollowGain } from './math_utils.js';

export function createInterpolationManager(getCtx) {

    function resolveAdaptiveSnapDistanceSq(isMobileDevice, app, fallbackDistanceSq, multiplier = 1) {
        if (!isMobileDevice) return fallbackDistanceSq;
        const width = Number(app?.screen?.width) ||
            (typeof window !== 'undefined' ? Number(window.innerWidth) : 0);
        const height = Number(app?.screen?.height) ||
            (typeof window !== 'undefined' ? Number(window.innerHeight) : 0);
        const diagonal = Math.hypot(width, height);
        if (!Number.isFinite(diagonal) || diagonal <= 0) return fallbackDistanceSq;

        const m = Number.isFinite(multiplier) ? Math.max(0.5, multiplier) : 1;
        const minDistance = 56 * m;
        const maxDistance = 170 * m;
        const distancePx = clamp(diagonal * 0.15 * m, minDistance, maxDistance);
        return distancePx * distancePx;
    }

    function resolveAdaptiveRotationGain(currentRotation, targetRotation, baseGain, speed = 0, isProjectile = false) {
        const normalizedBaseGain = clamp(Number(baseGain) || 0.3, 0.05, 1);
        if (!Number.isFinite(currentRotation) || !Number.isFinite(targetRotation)) {
            return normalizedBaseGain;
        }

        const rotDiffAbs = Math.abs(normalizeAngle(targetRotation - currentRotation));
        // Snap very fast turns to avoid floaty 180-degree corrections.
        if (rotDiffAbs >= Math.PI * 0.8) return 1;
        if (rotDiffAbs >= Math.PI * 0.55) return Math.max(normalizedBaseGain, 0.74);

        const speedNorm = clamp((Number(speed) || 0) / (isProjectile ? 1250 : 320), 0, 1);
        const diffNorm = clamp(rotDiffAbs / (Math.PI * 0.55), 0, 1);
        const blend = Math.max(speedNorm, diffNorm);
        const minGain = isProjectile ? 0.34 : 0.3;
        const maxGain = isProjectile ? 0.86 : 0.62;
        return clamp(minGain + (maxGain - minGain) * blend, minGain, 1);
    }

    function updateLocalPlayerPrediction(deltaTime) {
        const ctx = getCtx();
        const { localPlayerState, inputState } = ctx;
        if (!localPlayerState || !localPlayerState.alive) return;

        let forwardIntent = 0;
        let strafeIntent = 0;
        if (inputState.move_forward) forwardIntent += 1;
        if (inputState.move_backward) forwardIntent -= 1;
        if (inputState.move_left) strafeIntent -= 1;
        if (inputState.move_right) strafeIntent += 1;

        const inputRotation = Number.isFinite(inputState.rotation)
            ? inputState.rotation
            : (Number.isFinite(localPlayerState.rotation) ? localPlayerState.rotation : 0);
        localPlayerState.rotation = inputRotation;

        const isSpectator = !!localPlayerState.is_spectator;
        let effectiveSpeed = localPlayerState.speed_boost_remaining > 0 ? 225 : 150;
        if (isSpectator) {
            effectiveSpeed = 150 * 1.35;
        } else {
            if ((localPlayerState.dash_remaining || 0) > 0) {
                effectiveSpeed *= 2.0;
            }
            if ((localPlayerState.dodge_roll_remaining || 0) > 0) {
                effectiveSpeed *= 1.6;
            }
            if (forwardIntent === 0 && strafeIntent === 0 &&
                (((localPlayerState.dash_remaining || 0) > 0) || ((localPlayerState.dodge_roll_remaining || 0) > 0))) {
                forwardIntent = 1;
            }
        }

        let predictedVelocityX = 0;
        let predictedVelocityY = 0;

        if (forwardIntent !== 0 || strafeIntent !== 0) {
            const magnitude = Math.sqrt(forwardIntent * forwardIntent + strafeIntent * strafeIntent);
            forwardIntent /= magnitude;
            strafeIntent /= magnitude;

            const cosRot = Math.cos(inputRotation);
            const sinRot = Math.sin(inputRotation);
            const forwardX = cosRot * forwardIntent;
            const forwardY = sinRot * forwardIntent;
            const strafeX = -sinRot * strafeIntent;
            const strafeY = cosRot * strafeIntent;

            predictedVelocityX = (forwardX + strafeX) * effectiveSpeed;
            predictedVelocityY = (forwardY + strafeY) * effectiveSpeed;
            localPlayerState.x += predictedVelocityX * deltaTime;
            localPlayerState.y += predictedVelocityY * deltaTime;
        }

        localPlayerState.velocity_x = predictedVelocityX;
        localPlayerState.velocity_y = predictedVelocityY;
        localPlayerState.render_x = localPlayerState.x;
        localPlayerState.render_y = localPlayerState.y;
        localPlayerState.render_rotation = localPlayerState.rotation;
    }

    function interpolateEntities(deltaSeconds) {
        const ctx = getCtx();
        const {
            players, projectiles, myPlayerId, localPlayerState,
            app,
            adaptiveInterpolationDelayMs, projectileRawModeActive,
            serverUpdates, isMobileDevice, applyRenderTarget,
            getProjectileInterpolationSet, forEachInterpolatedProjectile,
            INTERPOLATION_RETENTION_MS, POSITION_SNAP_DISTANCE_SQ,
            PROJECTILE_SNAP_DISTANCE_SQ, PLAYER_EXTRAPOLATION_LIMIT_MS,
            PROJECTILE_EXTRAPOLATION_LIMIT_MS,
        } = ctx;

        const now = Date.now();
        const renderTime = now - adaptiveInterpolationDelayMs;
        const projectileInterpolationSet = projectileRawModeActive ? null : getProjectileInterpolationSet();
        // On mobile, use smoother (lower) gains for gentler snap-back corrections
        const mobileSmooth = isMobileDevice ? 0.7 : 1.0;
        const playerPositionGain = smoothFollowGain(0.3 * mobileSmooth, deltaSeconds);
        const projectilePositionGain = smoothFollowGain(0.52 * mobileSmooth, deltaSeconds);
        const baseRotationGain = smoothFollowGain(0.34 * mobileSmooth, deltaSeconds);
        const playerSnapDistanceSq = resolveAdaptiveSnapDistanceSq(
            isMobileDevice,
            app,
            POSITION_SNAP_DISTANCE_SQ,
            1
        );
        const projectileSnapDistanceSq = resolveAdaptiveSnapDistanceSq(
            isMobileDevice,
            app,
            PROJECTILE_SNAP_DISTANCE_SQ,
            1.35
        );

        const staleBefore = renderTime - INTERPOLATION_RETENTION_MS;
        let firstFreshIndex = 0;
        while (
            firstFreshIndex < serverUpdates.length &&
            serverUpdates[firstFreshIndex].timestamp <= staleBefore
        ) {
            firstFreshIndex += 1;
        }
        // Avoid mutating the shared updates buffer while interpolation logic is resolving indices.
        const renderUpdates = firstFreshIndex > 0
            ? serverUpdates.slice(firstFreshIndex)
            : serverUpdates;

        if (renderUpdates.length === 0) {
            return;
        }

        let update1 = null;
        let update2 = null;
        for (let i = renderUpdates.length - 1; i >= 1; i--) {
            if (renderUpdates[i].timestamp >= renderTime && renderUpdates[i - 1].timestamp <= renderTime) {
                update2 = renderUpdates[i];
                update1 = renderUpdates[i - 1];
                break;
            }
        }

        const latestUpdate = renderUpdates[renderUpdates.length - 1];
        if (!update1 || !update2) {
            if (renderTime >= latestUpdate.timestamp) {
                const playerExtraSec = clamp(
                    renderTime - latestUpdate.timestamp,
                    0,
                    PLAYER_EXTRAPOLATION_LIMIT_MS
                ) / 1000;
                const projectileExtraSec = clamp(
                    renderTime - latestUpdate.timestamp,
                    0,
                    PROJECTILE_EXTRAPOLATION_LIMIT_MS
                ) / 1000;

                players.forEach((currentPlayerState, playerId) => {
                    if (playerId === myPlayerId) return;
                    const latestState = latestUpdate.players.get(playerId);
                    if (!latestState) return;

                    const velocityX = latestState.velocity_x || 0;
                    const velocityY = latestState.velocity_y || 0;
                    const targetX = latestState.x + velocityX * playerExtraSec;
                    const targetY = latestState.y + velocityY * playerExtraSec;
                    const currentRotation = Number.isFinite(currentPlayerState.render_rotation)
                        ? currentPlayerState.render_rotation
                        : (Number(currentPlayerState.rotation) || 0);
                    const speed = Math.hypot(velocityX, velocityY);
                    const rotationGain = resolveAdaptiveRotationGain(
                        currentRotation,
                        latestState.rotation,
                        baseRotationGain,
                        speed,
                        false
                    );
                    applyRenderTarget(
                        currentPlayerState,
                        targetX,
                        targetY,
                        latestState.rotation,
                        playerPositionGain,
                        rotationGain,
                        playerSnapDistanceSq
                    );
                    currentPlayerState.velocity_x = velocityX;
                    currentPlayerState.velocity_y = velocityY;
                });

                if (!projectileRawModeActive) {
                    forEachInterpolatedProjectile(projectileInterpolationSet, (currentProjState, projId) => {
                        const latestState = latestUpdate.projectiles.get(projId);
                        if (!latestState) return;
                        const vx = latestState.velocity_x || 0;
                        const vy = latestState.velocity_y || 0;
                        const targetX = latestState.x + vx * projectileExtraSec;
                        const targetY = latestState.y + vy * projectileExtraSec;
                        const targetRotation = (vx !== 0 || vy !== 0) ? Math.atan2(vy, vx) : currentProjState.render_rotation;
                        const currentRotation = Number.isFinite(currentProjState.render_rotation)
                            ? currentProjState.render_rotation
                            : 0;
                        const speed = Math.hypot(vx, vy);
                        const rotationGain = resolveAdaptiveRotationGain(
                            currentRotation,
                            targetRotation,
                            baseRotationGain,
                            speed,
                            true
                        );
                        applyRenderTarget(
                            currentProjState,
                            targetX,
                            targetY,
                            targetRotation,
                            projectilePositionGain,
                            rotationGain,
                            projectileSnapDistanceSq
                        );
                        currentProjState.velocity_x = vx;
                        currentProjState.velocity_y = vy;
                    });
                }
            } else {
                const oldestUpdate = renderUpdates[0];
                players.forEach((currentPlayerState, playerId) => {
                    if (playerId === myPlayerId) return;
                    const state = oldestUpdate.players.get(playerId);
                    if (!state) return;
                    const currentRotation = Number.isFinite(currentPlayerState.render_rotation)
                        ? currentPlayerState.render_rotation
                        : (Number(currentPlayerState.rotation) || 0);
                    const speed = Math.hypot(Number(state.velocity_x) || 0, Number(state.velocity_y) || 0);
                    const rotationGain = resolveAdaptiveRotationGain(
                        currentRotation,
                        state.rotation,
                        baseRotationGain,
                        speed,
                        false
                    );
                    applyRenderTarget(
                        currentPlayerState,
                        state.x,
                        state.y,
                        state.rotation,
                        playerPositionGain,
                        rotationGain,
                        playerSnapDistanceSq
                    );
                });
                if (!projectileRawModeActive) {
                    forEachInterpolatedProjectile(projectileInterpolationSet, (currentProjState, projId) => {
                        const state = oldestUpdate.projectiles.get(projId);
                        if (!state) return;
                        const vx = state.velocity_x || 0;
                        const vy = state.velocity_y || 0;
                        const targetRotation = (vx !== 0 || vy !== 0) ? Math.atan2(vy, vx) : currentProjState.render_rotation;
                        const currentRotation = Number.isFinite(currentProjState.render_rotation)
                            ? currentProjState.render_rotation
                            : 0;
                        const speed = Math.hypot(vx, vy);
                        const rotationGain = resolveAdaptiveRotationGain(
                            currentRotation,
                            targetRotation,
                            baseRotationGain,
                            speed,
                            true
                        );
                        applyRenderTarget(
                            currentProjState,
                            state.x,
                            state.y,
                            targetRotation,
                            projectilePositionGain,
                            rotationGain,
                            projectileSnapDistanceSq
                        );
                    });
                }
            }
            return;
        }

        const t = (update1.timestamp === update2.timestamp)
            ? 1
            : (renderTime - update1.timestamp) / (update2.timestamp - update1.timestamp);
        const clampedT = Math.max(0, Math.min(1, t));

        players.forEach((currentPlayerState, playerId) => {
            if (playerId === myPlayerId) return;

            const state1 = update1.players.get(playerId);
            const state2 = update2.players.get(playerId);

            if (state1 && state2) {
                const targetX = lerp(state1.x, state2.x, clampedT);
                const targetY = lerp(state1.y, state2.y, clampedT);
                const rot1 = state1.rotation;
                const rot2 = state2.rotation;
                const rotDiff = normalizeAngle(rot2 - rot1);
                const targetRotation = rot1 + rotDiff * clampedT;
                const velocityX = lerp(state1.velocity_x || 0, state2.velocity_x || 0, clampedT);
                const velocityY = lerp(state1.velocity_y || 0, state2.velocity_y || 0, clampedT);
                const currentRotation = Number.isFinite(currentPlayerState.render_rotation)
                    ? currentPlayerState.render_rotation
                    : (Number(currentPlayerState.rotation) || 0);
                const speed = Math.hypot(velocityX, velocityY);
                const rotationGain = resolveAdaptiveRotationGain(
                    currentRotation,
                    targetRotation,
                    baseRotationGain,
                    speed,
                    false
                );
                applyRenderTarget(
                    currentPlayerState,
                    targetX,
                    targetY,
                    targetRotation,
                    playerPositionGain,
                    rotationGain,
                    playerSnapDistanceSq
                );
                currentPlayerState.velocity_x = velocityX;
                currentPlayerState.velocity_y = velocityY;
            } else if (state2 || state1) {
                const fallback = state2 || state1;
                const currentRotation = Number.isFinite(currentPlayerState.render_rotation)
                    ? currentPlayerState.render_rotation
                    : (Number(currentPlayerState.rotation) || 0);
                const speed = Math.hypot(Number(fallback.velocity_x) || 0, Number(fallback.velocity_y) || 0);
                const rotationGain = resolveAdaptiveRotationGain(
                    currentRotation,
                    fallback.rotation,
                    baseRotationGain,
                    speed,
                    false
                );
                applyRenderTarget(
                    currentPlayerState,
                    fallback.x,
                    fallback.y,
                    fallback.rotation,
                    playerPositionGain,
                    rotationGain,
                    playerSnapDistanceSq
                );
                currentPlayerState.velocity_x = fallback.velocity_x || 0;
                currentPlayerState.velocity_y = fallback.velocity_y || 0;
            }
        });

        if (!projectileRawModeActive) {
            forEachInterpolatedProjectile(projectileInterpolationSet, (currentProjState, projId) => {
                const state1 = update1.projectiles.get(projId);
                const state2 = update2.projectiles.get(projId);

                if (state1 && state2) {
                    const targetX = lerp(state1.x, state2.x, clampedT);
                    const targetY = lerp(state1.y, state2.y, clampedT);
                    const vx = lerp(state1.velocity_x || 0, state2.velocity_x || 0, clampedT);
                    const vy = lerp(state1.velocity_y || 0, state2.velocity_y || 0, clampedT);
                    const targetRotation = (vx !== 0 || vy !== 0) ? Math.atan2(vy, vx) : currentProjState.render_rotation;
                    const currentRotation = Number.isFinite(currentProjState.render_rotation)
                        ? currentProjState.render_rotation
                        : 0;
                    const speed = Math.hypot(vx, vy);
                    const rotationGain = resolveAdaptiveRotationGain(
                        currentRotation,
                        targetRotation,
                        baseRotationGain,
                        speed,
                        true
                    );
                    applyRenderTarget(
                        currentProjState,
                        targetX,
                        targetY,
                        targetRotation,
                        projectilePositionGain,
                        rotationGain,
                        projectileSnapDistanceSq
                    );
                    currentProjState.velocity_x = vx;
                    currentProjState.velocity_y = vy;
                } else if (state2 || state1) {
                    const fallback = state2 || state1;
                    const vx = fallback.velocity_x || 0;
                    const vy = fallback.velocity_y || 0;
                    const targetRotation = (vx !== 0 || vy !== 0) ? Math.atan2(vy, vx) : currentProjState.render_rotation;
                    const currentRotation = Number.isFinite(currentProjState.render_rotation)
                        ? currentProjState.render_rotation
                        : 0;
                    const speed = Math.hypot(vx, vy);
                    const rotationGain = resolveAdaptiveRotationGain(
                        currentRotation,
                        targetRotation,
                        baseRotationGain,
                        speed,
                        true
                    );
                    applyRenderTarget(
                        currentProjState,
                        fallback.x,
                        fallback.y,
                        targetRotation,
                        projectilePositionGain,
                        rotationGain,
                        projectileSnapDistanceSq
                    );
                    currentProjState.velocity_x = vx;
                    currentProjState.velocity_y = vy;
                }
            });
        }
    }

    function maybeRecordInterpolationSnapshot(serverTime) {
        const ctx = getCtx();
        const {
            players, projectiles, myPlayerId, serverUpdates,
            INTERPOLATION_PLAYER_LIMIT, INTERPOLATION_PROJECTILE_LIMIT,
            INTERPOLATION_SNAPSHOT_INTERVAL_MS, MAX_INTERPOLATION_SNAPSHOTS,
            updateAdaptiveInterpolationDelay,
        } = ctx;

        if (players.size > INTERPOLATION_PLAYER_LIMIT || projectiles.size > INTERPOLATION_PROJECTILE_LIMIT) {
            serverUpdates.length = 0;
            return;
        }

        const nowTs = Date.now();
        const baseTs = Number.isFinite(serverTime) && serverTime > 0 ? serverTime : nowTs;
        if (baseTs - _lastInterpolationSnapshotAt < INTERPOLATION_SNAPSHOT_INTERVAL_MS) {
            return;
        }

        const lastSnapshot = serverUpdates.length > 0 ? serverUpdates[serverUpdates.length - 1] : null;
        const snapshotTimestamp = lastSnapshot ? Math.max(baseTs, lastSnapshot.timestamp + 1) : baseTs;
        _lastInterpolationSnapshotAt = snapshotTimestamp;
        updateAdaptiveInterpolationDelay(snapshotTimestamp);

        const playersSnapshot = new Map();
        players.forEach((pState, playerId) => {
            if (playerId === myPlayerId) return;
            playersSnapshot.set(playerId, {
                x: pState.x,
                y: pState.y,
                rotation: pState.rotation,
                velocity_x: pState.velocity_x || 0,
                velocity_y: pState.velocity_y || 0
            });
        });

        const projectilesSnapshot = new Map();
        projectiles.forEach((pState, projectileId) => {
            projectilesSnapshot.set(projectileId, {
                x: pState.x,
                y: pState.y,
                velocity_x: pState.velocity_x || 0,
                velocity_y: pState.velocity_y || 0
            });
        });

        serverUpdates.push({
            timestamp: snapshotTimestamp,
            players: playersSnapshot,
            projectiles: projectilesSnapshot
        });

        while (serverUpdates.length > MAX_INTERPOLATION_SNAPSHOTS) {
            serverUpdates.shift();
        }
    }

    // Internal mutable state
    let _lastInterpolationSnapshotAt = 0;

    function resetSnapshotState() {
        _lastInterpolationSnapshotAt = 0;
    }

    return {
        updateLocalPlayerPrediction,
        interpolateEntities,
        maybeRecordInterpolationSnapshot,
        resetSnapshotState,
    };
}
