/**
 * WorldRenderer.js - World-space rendering: walls, zones, flags, camera
 *
 * Extracted from client.html. Contains drawZones, updateZonePulse,
 * drawWalls, drawEnhancedWallCracks, drawSimplifiedWallCracks,
 * updateFlags, and updateCamera. Uses getCtx callback pattern.
 */

export function createWorldRenderer(getCtx) {

    let zonePulsePhase = 0;

    function drawZones() {
        const ctx = getCtx();
        const { zoneContainer, zones, PIXI, GP } = ctx;
        if (!zoneContainer) return;
        zoneContainer.removeChildren();

        for (const [zoneId, zone] of zones) {
            const g = new PIXI.Graphics();
            const ZT = GP.ZoneType;

            if (zone.zone_type === ZT.SlowZone) {
                g.beginFill(0x2D1B69, 0.25);
                g.drawRect(zone.x, zone.y, zone.width, zone.height);
                g.endFill();
                g.lineStyle(2, 0x6B3FA0, 0.6);
                g.drawRect(zone.x, zone.y, zone.width, zone.height);
            } else if (zone.zone_type === ZT.DamageZone) {
                g.beginFill(0x8B0000, 0.15);
                g.drawRect(zone.x, zone.y, zone.width, zone.height);
                g.endFill();
                g.lineStyle(3, 0xFF2200, 0.7);
                g.drawRect(zone.x, zone.y, zone.width, zone.height);
                g.lineStyle(1, 0xFF4444, 0.3);
                g.drawRect(zone.x + 4, zone.y + 4, zone.width - 8, zone.height - 8);
            } else if (zone.zone_type === ZT.BoostPad) {
                g.beginFill(0x00AAFF, 0.20);
                g.drawRect(zone.x, zone.y, zone.width, zone.height);
                g.endFill();
                g.lineStyle(2, 0x00DDFF, 0.8);
                g.drawRect(zone.x, zone.y, zone.width, zone.height);
                const cx = zone.x + zone.width / 2;
                const cy = zone.y + zone.height / 2;
                const dir = zone.direction || 0;
                const cos = Math.cos(dir), sin = Math.sin(dir);
                for (let i = -1; i <= 1; i++) {
                    const ox = cx + cos * i * 12;
                    const oy = cy + sin * i * 12;
                    g.lineStyle(2, 0x00FFFF, 0.6);
                    g.moveTo(ox - sin * 6 - cos * 6, oy + cos * 6 - sin * 6);
                    g.lineTo(ox, oy);
                    g.lineTo(ox + sin * 6 - cos * 6, oy - cos * 6 - sin * 6);
                }
            }

            zoneContainer.addChild(g);
        }
    }

    function updateZonePulse(dt) {
        const ctx = getCtx();
        const { zoneContainer, zones, GP } = ctx;
        if (!zoneContainer || zones.size === 0) return;
        zonePulsePhase += dt * 0.003;
        const pulse = 0.5 + 0.5 * Math.sin(zonePulsePhase * 3);
        let childIdx = 0;
        for (const [, zone] of zones) {
            if (childIdx < zoneContainer.children.length) {
                const g = zoneContainer.children[childIdx];
                if (zone.zone_type === GP.ZoneType.DamageZone) {
                    g.alpha = 0.6 + 0.4 * pulse;
                }
            }
            childIdx++;
        }
    }

    function drawEnhancedWallCracks(wallGraphics, wall, healthPercent, mixColors) {
        const numCracks = Math.floor((1 - healthPercent) * 12);
        const crackColor = mixColors(0x2E3440, 0x000000, 0.5);

        for (let i = 0; i < numCracks; i++) {
            wallGraphics.lineStyle(Math.max(1, 3 * (1 - healthPercent)), crackColor, 0.7);

            const startX = wall.x + Math.random() * wall.width;
            const startY = wall.y + Math.random() * wall.height;

            wallGraphics.moveTo(startX, startY);

            let currentX = startX;
            let currentY = startY;
            const crackLength = Math.min(wall.width, wall.height) * 0.4 * (1 - healthPercent);
            const segments = 3 + Math.floor(Math.random() * 3);

            for (let j = 0; j < segments; j++) {
                const angle = Math.random() * Math.PI * 2;
                const segmentLength = crackLength / segments;
                currentX += Math.cos(angle) * segmentLength;
                currentY += Math.sin(angle) * segmentLength;

                currentX = Math.max(wall.x, Math.min(wall.x + wall.width, currentX));
                currentY = Math.max(wall.y, Math.min(wall.y + wall.height, currentY));

                wallGraphics.lineTo(currentX, currentY);
            }
        }

        if (healthPercent < 0.5) {
            wallGraphics.beginFill(crackColor, 0.5);
            for (let i = 0; i < 5; i++) {
                const debrisX = wall.x + Math.random() * wall.width;
                const debrisY = wall.y + Math.random() * wall.height;
                const debrisSize = Math.random() * 3 + 1;
                wallGraphics.drawRect(debrisX, debrisY, debrisSize, debrisSize);
            }
            wallGraphics.endFill();
        }
    }

    function drawSimplifiedWallCracks(wallGraphics, wall, healthPercent) {
        const severity = 1 - healthPercent;
        const numCracks = Math.max(1, Math.floor(severity * 6));
        const crackColor = 0x1A1A2E;
        const lineWidth = Math.max(1, 2 * severity);

        wallGraphics.lineStyle(lineWidth, crackColor, 0.55 + severity * 0.25);

        for (let i = 0; i < numCracks; i++) {
            const startX = wall.x + ((i * 0.618) % 1) * wall.width;
            const startY = wall.y + ((i * 0.382) % 1) * wall.height;
            wallGraphics.moveTo(startX, startY);

            let cx = startX, cy = startY;
            const crackLen = Math.min(wall.width, wall.height) * 0.35 * severity;
            const segments = 2 + (i % 2);
            for (let j = 0; j < segments; j++) {
                const seed = (wall.x * 31 + wall.y * 17 + i * 7 + j * 3) & 0xFFFF;
                const angle = (seed / 0xFFFF) * Math.PI * 2;
                const segLen = crackLen / segments;
                cx = Math.max(wall.x, Math.min(wall.x + wall.width, cx + Math.cos(angle) * segLen));
                cy = Math.max(wall.y, Math.min(wall.y + wall.height, cy + Math.sin(angle) * segLen));
                wallGraphics.lineTo(cx, cy);
            }
        }
    }

    function interpolateColor(color1, color2, amount) {
        const r1 = (color1 >> 16) & 0xFF;
        const g1 = (color1 >> 8) & 0xFF;
        const b1 = color1 & 0xFF;
        const r2 = (color2 >> 16) & 0xFF;
        const g2 = (color2 >> 8) & 0xFF;
        const b2 = color2 & 0xFF;
        const r = Math.round(r1 + (r2 - r1) * amount);
        const g = Math.round(g1 + (g2 - g1) * amount);
        const b = Math.round(b1 + (b2 - b1) * amount);
        return (r << 16) | (g << 8) | b;
    }

    function drawWalls(force = false) {
        const ctx = getCtx();
        const {
            wallGraphics, walls, PIXI, mixColors, minimap,
            ultraPerformanceMode, STABLE_MODE_FORCED, smoothedFrameMs,
            WALL_REDRAW_MIN_INTERVAL_MS, WALL_REDRAW_MIN_INTERVAL_ULTRA_MS,
            GLOBAL_LIGHT_DIR, gameSettings,
        } = ctx;
        if (!wallGraphics) return;

        const now = Date.now();
        const minRedrawIntervalMs = ultraPerformanceMode
            ? WALL_REDRAW_MIN_INTERVAL_ULTRA_MS
            : WALL_REDRAW_MIN_INTERVAL_MS;
        if (!force && (now - _lastWallRedrawAt) < minRedrawIntervalMs) {
            _pendingWallRedraw = true;
            return;
        }
        _pendingWallRedraw = false;
        _lastWallRedrawAt = now;

        wallGraphics.clear();
        const debugChildren = wallGraphics.removeChildren();
        debugChildren.forEach((child) => child.destroy?.());

        const showDestroyedWallDebug = !!gameSettings.showDestroyedWallDebug;
        const simplifiedWallRender = !showDestroyedWallDebug && (
            ultraPerformanceMode ||
            STABLE_MODE_FORCED ||
            smoothedFrameMs >= 22 ||
            walls.size >= 72
        );

        if (showDestroyedWallDebug) {
            walls.forEach((wall) => {
                if (wall.is_destructible && wall.current_health <= 0) {
                    wallGraphics.lineStyle(2, 0xFF0000, 0.5);
                    wallGraphics.beginFill(0xFF0000, 0.1);
                    wallGraphics.drawRect(wall.x, wall.y, wall.width, wall.height);
                    wallGraphics.endFill();

                    const text = new PIXI.Text('DESTROYED', {
                        fontSize: 10,
                        fill: 0xFF0000,
                        stroke: 0x000000,
                        strokeThickness: 2
                    });
                    text.x = wall.x + wall.width / 2 - text.width / 2;
                    text.y = wall.y + wall.height / 2 - text.height / 2;
                    wallGraphics.addChild(text);
                }
            });
        }

        if (simplifiedWallRender) {
            walls.forEach((wall) => {
                if (wall.is_destructible && wall.current_health <= 0) return;

                let wallColor = 0x374151;
                let wallAlpha = 0.9;
                if (wall.is_destructible) {
                    const healthPercent = Math.max(0, Math.min(1, wall.current_health / Math.max(1, wall.max_health)));
                    wallAlpha = 0.62 + (healthPercent * 0.28);
                    wallColor = healthPercent > 0.5
                        ? interpolateColor(0x4B5563, 0x374151, (healthPercent - 0.5) * 2)
                        : interpolateColor(0xBF616A, 0x4B5563, healthPercent * 2);
                }
                wallGraphics.beginFill(wallColor, wallAlpha);
                wallGraphics.drawRect(wall.x, wall.y, wall.width, wall.height);
                wallGraphics.endFill();

                wallGraphics.lineStyle(1, mixColors(wallColor, 0x000000, 0.35), Math.min(1, wallAlpha + 0.1));
                wallGraphics.drawRect(wall.x, wall.y, wall.width, wall.height);

                if (wall.is_destructible) {
                    const hp = Math.max(0, Math.min(1, wall.current_health / Math.max(1, wall.max_health)));
                    if (hp <= 0.75 && hp > 0) {
                        drawSimplifiedWallCracks(wallGraphics, wall, hp);
                    }
                }
            });

            ctx.setMinimapWallsCacheDirty(true);
            if (minimap) minimap.wallsNeedUpdate = true;
            return;
        }

        walls.forEach((wall) => {
            if (wall.is_destructible && wall.current_health <= 0) return;

            const shadowX = GLOBAL_LIGHT_DIR.x;
            const shadowY = GLOBAL_LIGHT_DIR.y;
            wallGraphics.beginFill(0x000000, 0.22);
            wallGraphics.drawRect(
                wall.x + shadowX * 3,
                wall.y + shadowY * 3,
                wall.width,
                wall.height
            );
            wallGraphics.endFill();

            wallGraphics.beginFill(0x000000, 0.1);
            wallGraphics.drawRect(
                wall.x + shadowX * 6 - 0.5,
                wall.y + shadowY * 6 - 0.5,
                wall.width + 1,
                wall.height + 1
            );
            wallGraphics.endFill();
        });

        walls.forEach((wall) => {
            if (wall.is_destructible && wall.current_health <= 0 && !showDestroyedWallDebug) return;

            let wallColor = 0x374151;
            let wallAlpha = 1.0;

            if (wall.is_destructible) {
                const healthPercent = wall.current_health / Math.max(1, wall.max_health);
                wallAlpha = 0.6 + healthPercent * 0.4;
                if (healthPercent > 0.5) {
                    wallColor = interpolateColor(0x4B5563, 0x374151, (healthPercent - 0.5) * 2);
                } else {
                    wallColor = interpolateColor(0xBF616A, 0x4B5563, healthPercent * 2);
                }

                wallGraphics.beginFill(wallColor, wallAlpha * 0.9);
                wallGraphics.drawRect(wall.x, wall.y, wall.width, wall.height);
                wallGraphics.endFill();

                wallGraphics.lineStyle(1, mixColors(wallColor, 0x000000, 0.3), wallAlpha * 0.5);
                const lineSpacing = 10;
                for (let i = wall.x + lineSpacing; i < wall.x + wall.width; i += lineSpacing) {
                    wallGraphics.moveTo(i, wall.y);
                    wallGraphics.lineTo(i, wall.y + wall.height);
                }

                if (healthPercent <= 0.75) {
                    drawEnhancedWallCracks(wallGraphics, wall, healthPercent, mixColors);
                }

                if (healthPercent < 0.3) {
                    wallGraphics.lineStyle(2, 0xFF6B6B, (1 - healthPercent) * 0.5);
                    wallGraphics.drawRect(wall.x - 1, wall.y - 1, wall.width + 2, wall.height + 2);
                }
            } else {
                wallGraphics.beginFill(wallColor);
                wallGraphics.drawRect(wall.x, wall.y, wall.width, wall.height);
                wallGraphics.endFill();

                wallGraphics.lineStyle(1, mixColors(wallColor, 0xFFFFFF, 0.1), 0.5);
                wallGraphics.moveTo(wall.x, wall.y + wall.height);
                wallGraphics.lineTo(wall.x, wall.y);
                wallGraphics.lineTo(wall.x + wall.width, wall.y);

                wallGraphics.lineStyle(1, mixColors(wallColor, 0x000000, 0.3), 0.5);
                wallGraphics.moveTo(wall.x + wall.width, wall.y);
                wallGraphics.lineTo(wall.x + wall.width, wall.y + wall.height);
                wallGraphics.lineTo(wall.x, wall.y + wall.height);
            }

            wallGraphics.lineStyle(2, mixColors(wallColor, 0x000000, 0.4), wallAlpha);
            wallGraphics.drawRect(wall.x, wall.y, wall.width, wall.height);
        });

        ctx.setMinimapWallsCacheDirty(true);
        if (minimap) minimap.wallsNeedUpdate = true;
    }

    // Mutable state for wall throttle
    let _lastWallRedrawAt = 0;
    let _pendingWallRedraw = false;

    function hasPendingWallRedraw() {
        return _pendingWallRedraw;
    }

    function updateFlags(newFlagStates) {
        const ctx = getCtx();
        const { flagContainer, flagStates, GP, createFlagSprite, minimap } = ctx;
        if (!flagContainer || !Array.isArray(flagContainer.children)) {
            return;
        }

        newFlagStates.forEach(fs => flagStates.set(fs.team_id, fs));

        flagContainer.children.forEach(sprite => {
            const state = flagStates.get(sprite.flagTeamId);
            if (state) {
                sprite.position.set(state.position.x, state.position.y);
                sprite.visible = state.status !== GP.FlagStatus.Carried;

                if (sprite.timerText) {
                    if (state.status === GP.FlagStatus.Dropped && state.respawn_timer > 0) {
                        sprite.timerText.text = Math.ceil(state.respawn_timer) + 's';
                        sprite.timerText.visible = true;
                    } else {
                        sprite.timerText.visible = false;
                    }
                }
            } else {
                sprite.visible = false;
            }
        });

        flagStates.forEach(state => {
            if (!flagContainer.children.find(s => s.flagTeamId === state.team_id)) {
                const flagSprite = createFlagSprite(state);
                flagContainer.addChild(flagSprite);
            }
        });

        ctx.setMinimapFlagsCacheDirty(true);
        if (minimap) minimap.objectivesNeedUpdate = true;
    }

    function updateCamera() {
        const ctx = getCtx();
        const {
            app, gameScene, localPlayerState, overviewMode, overviewScale,
            dynamicsTuning, cameraCombatImpulse, setCameraCombatImpulse,
        } = ctx;
        if (!app || !gameScene) return;

        const deltaSec = Math.max(0.001, (app.ticker?.deltaMS || 16.67) / 1000);
        const nextImpulse = Math.max(0, cameraCombatImpulse - dynamicsTuning.cameraCombatDecayPerSec * deltaSec);
        setCameraCombatImpulse(nextImpulse);

        if (overviewMode) {
            const targetScale = overviewScale;
            const currentScale = gameScene.scale.x;
            const scaleSmoothing = 0.15;
            const newScale = currentScale + (targetScale - currentScale) * scaleSmoothing;
            gameScene.scale.set(newScale);

            const centerX = app.screen.width / 2;
            const centerY = app.screen.height / 2;
            const targetX = centerX;
            const targetY = centerY;

            const posSmoothing = 0.15;
            gameScene.position.x += (targetX - gameScene.position.x) * posSmoothing;
            gameScene.position.y += (targetY - gameScene.position.y) * posSmoothing;
        } else if (localPlayerState) {
            const playerX = localPlayerState.render_x !== undefined ? localPlayerState.render_x : localPlayerState.x;
            const playerY = localPlayerState.render_y !== undefined ? localPlayerState.render_y : localPlayerState.y;
            const velocityX = localPlayerState.velocity_x || 0;
            const velocityY = localPlayerState.velocity_y || 0;
            const speed = Math.hypot(velocityX, velocityY);

            const speedZoomOut = Math.min(
                dynamicsTuning.cameraMaxSpeedZoomOut,
                speed * dynamicsTuning.cameraSpeedZoomFactor
            );
            const combatZoomOut = Math.min(nextImpulse, dynamicsTuning.cameraMaxSpeedZoomOut * 0.8);
            const targetScale = Math.max(0.72, dynamicsTuning.cameraBaseScale - speedZoomOut - combatZoomOut);
            const currentScale = gameScene.scale.x;
            const scaleSmoothing = 0.15;
            const newScale = currentScale + (targetScale - currentScale) * scaleSmoothing;
            gameScene.scale.set(newScale);

            const lookAheadX = velocityX * dynamicsTuning.cameraLookAheadFactor;
            const lookAheadY = velocityY * dynamicsTuning.cameraLookAheadFactor;
            const targetX = app.screen.width / 2 - (playerX + lookAheadX) * newScale;
            const targetY = app.screen.height / 2 - (playerY + lookAheadY) * newScale;

            const smoothing = Math.min(
                0.28,
                dynamicsTuning.cameraSmoothingBase + (speed / 450) * dynamicsTuning.cameraSmoothingBySpeed
            );
            gameScene.position.x += (targetX - gameScene.position.x) * smoothing;
            gameScene.position.y += (targetY - gameScene.position.y) * smoothing;
        }
    }

    return {
        drawZones,
        updateZonePulse,
        drawWalls,
        drawEnhancedWallCracks,
        drawSimplifiedWallCracks,
        updateFlags,
        updateCamera,
        hasPendingWallRedraw,
    };
}
