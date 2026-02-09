// Part 3 - Starfield, Walls, and Game Loop Functions

// Create procedural starfield background
function createStarfield(app) {
    const starfieldContainer = new PIXI.Container();
    const starLayers = [
        { count: 100, scrollFactor: 0.1, minRadius: 0.5, maxRadius: 1, color: 0xFFFFFF },
        { count: 50, scrollFactor: 0.3, minRadius: 1, maxRadius: 1.5, color: 0xAAAAFF },
        { count: 30, scrollFactor: 0.5, minRadius: 1.5, maxRadius: 2, color: 0xFFFFAA }
    ];

    // Generate nebula clouds
    const nebulaContainer = new PIXI.Container();
    for (let i = 0; i < 3; i++) {
        const nebula = new PIXI.Graphics();
        const size = 200 + Math.random() * 300;
        const x = Math.random() * app.screen.width;
        const y = Math.random() * app.screen.height;
        const color = [0x4B0082, 0x191970, 0x2F4F4F][i % 3];

        nebula.beginFill(color, 0.1);
        nebula.drawCircle(0, 0, size);
        nebula.endFill();
        nebula.position.set(x, y);
        nebula.filters = [new PIXI.BlurFilter(50)];
        nebulaContainer.addChild(nebula);
    }
    starfieldContainer.addChild(nebulaContainer);

    // Generate star layers
    starLayers.forEach((layerData, layerIndex) => {
        const layerContainer = new PIXI.Container();
        layerContainer.scrollFactor = layerData.scrollFactor;

        // Generate star texture once
        const starGraphics = new PIXI.Graphics();
        starGraphics.beginFill(0xFFFFFF);
        starGraphics.drawCircle(0, 0, 2);
        starGraphics.endFill();
        const starTexture = app.renderer.generateTexture(starGraphics);
        starGraphics.destroy();

        // Create stars using sprites for better performance
        for (let i = 0; i < layerData.count; i++) {
            const star = new PIXI.Sprite(starTexture);
            star.anchor.set(0.5);

            const radius = Math.random() * (layerData.maxRadius - layerData.minRadius) + layerData.minRadius;
            star.scale.set(radius / 2);
            star.tint = layerData.color;
            star.alpha = Math.random() * 0.5 + 0.5;

            star.x = Math.random() * app.screen.width * 2;
            star.y = Math.random() * app.screen.height * 2;

            // Store initial position for wrapping
            star.initialX = star.x;
            star.initialY = star.y;

            // Add twinkle effect to some stars
            if (Math.random() < 0.3) {
                star.twinkleSpeed = Math.random() * 0.002 + 0.001;
                star.twinkleOffset = Math.random() * Math.PI * 2;
            }

            layerContainer.addChild(star);
        }

        starfieldContainer.addChild(layerContainer);
    });

    return starfieldContainer;
}

// Update starfield position based on camera movement
function updateStarfield(starfieldContainer, cameraX, cameraY, delta) {
    starfieldContainer.children.forEach((layer, index) => {
        if (layer.scrollFactor !== undefined) {
            // Parallax scrolling
            layer.x = -cameraX * layer.scrollFactor;
            layer.y = -cameraY * layer.scrollFactor;

            // Update individual stars for twinkling
            layer.children.forEach(star => {
                if (star.twinkleSpeed) {
                    star.alpha = 0.5 + Math.sin(Date.now() * star.twinkleSpeed + star.twinkleOffset) * 0.5;
                }

                // Wrap stars around screen edges
                const screenBuffer = 100;
                if (star.x + layer.x < -screenBuffer) {
                    star.x += app.screen.width + screenBuffer * 2;
                } else if (star.x + layer.x > app.screen.width + screenBuffer) {
                    star.x -= app.screen.width + screenBuffer * 2;
                }

                if (star.y + layer.y < -screenBuffer) {
                    star.y += app.screen.height + screenBuffer * 2;
                } else if (star.y + layer.y > app.screen.height + screenBuffer) {
                    star.y -= app.screen.height + screenBuffer * 2;
                }
            });
        }
    });
}

function drawWalls() {
    wallGraphics.clear();

    // First pass: Draw wall shadows
    walls.forEach(wall => {
        // Draw shadows for all walls, including destroyed ones
        wallGraphics.beginFill(0x000000, 0.3);
        wallGraphics.drawRect(wall.x + 3, wall.y + 3, wall.width, wall.height);
        wallGraphics.endFill();
    });

    // Second pass: Draw walls
    walls.forEach(wall => {
        // Render destroyed walls as rubble instead of skipping them
        if (wall.is_destructible && wall.current_health <= 0) {
            // Draw destroyed wall as rubble/debris
            drawDestroyedWall(wall);
            return;
        }

        let wallColor = 0x374151;
        let wallAlpha = 1.0;

        if (wall.is_destructible) {
            const healthPercent = wall.current_health / wall.max_health;
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

            if (healthPercent < 0.8) {
                drawEnhancedWallCracks(wall, healthPercent);
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

    if (minimap) minimap.wallsNeedUpdate = true;
}

function applyScreenShake(container, duration, magnitude) {
    let shakeTime = duration;
    const initialX = container.x;
    const initialY = container.y;

    const shakeTicker = (delta) => {
        if (shakeTime > 0) {
            shakeTime -= delta;
            const offsetX = (Math.random() - 0.5) * 2 * magnitude * (shakeTime / duration);
            const offsetY = (Math.random() - 0.5) * 2 * magnitude * (shakeTime / duration);
            container.x = initialX + offsetX;
            container.y = initialY + offsetY;
        } else {
            container.x = initialX;
            container.y = initialY;
            app.ticker.remove(shakeTicker);
        }
    };
    app.ticker.add(shakeTicker);
}

// Screen effects for game feel
function createScreenFlash(app, color = 0xFFFFFF, duration = 15, maxAlpha = 0.7) {
    const flashOverlay = new PIXI.Graphics();
    flashOverlay.beginFill(color, 1);
    flashOverlay.drawRect(0, 0, app.screen.width, app.screen.height);
    flashOverlay.endFill();
    flashOverlay.alpha = maxAlpha;
    app.stage.addChild(flashOverlay);

    let framesPassed = 0;
    const flashTicker = (delta) => {
        framesPassed += delta;
        flashOverlay.alpha = maxAlpha * (1 - (framesPassed / duration));
        if (framesPassed >= duration) {
            app.ticker.remove(flashTicker);
            app.stage.removeChild(flashOverlay);
            flashOverlay.destroy();
        }
    };
    app.ticker.add(flashTicker);
}

function drawEnhancedWallCracks(wall, healthPercent) {
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

// Draw destroyed wall as rubble/debris
function drawDestroyedWall(wall) {
    const rubbleColor = 0x5B5B5B;
    const debrisColor = 0x7B7B7B;
    
    // Draw base rubble area with higher alpha for visibility
    wallGraphics.beginFill(rubbleColor, 0.6);
    wallGraphics.drawRect(wall.x, wall.y, wall.width, wall.height);
    wallGraphics.endFill();
    
    // Draw scattered debris pieces
    const numDebris = Math.floor(Math.min(wall.width, wall.height) / 8);
    for (let i = 0; i < numDebris; i++) {
        const debrisX = wall.x + Math.random() * wall.width;
        const debrisY = wall.y + Math.random() * wall.height;
        const debrisSize = Math.random() * 10 + 5;
        const debrisRotation = Math.random() * Math.PI;
        
        wallGraphics.beginFill(debrisColor, 0.7 + Math.random() * 0.3);
        
        // Draw rotated rectangular debris
        const halfSize = debrisSize / 2;
        const cos = Math.cos(debrisRotation);
        const sin = Math.sin(debrisRotation);
        
        wallGraphics.moveTo(
            debrisX + cos * halfSize - sin * halfSize,
            debrisY + sin * halfSize + cos * halfSize
        );
        wallGraphics.lineTo(
            debrisX - cos * halfSize - sin * halfSize,
            debrisY - sin * halfSize + cos * halfSize
        );
        wallGraphics.lineTo(
            debrisX - cos * halfSize + sin * halfSize,
            debrisY - sin * halfSize - cos * halfSize
        );
        wallGraphics.lineTo(
            debrisX + cos * halfSize + sin * halfSize,
            debrisY + sin * halfSize - cos * halfSize
        );
        wallGraphics.closePath();
        wallGraphics.endFill();
    }
    
    // Draw prominent outline to clearly show it's still an obstacle
    wallGraphics.lineStyle(2, 0xAAAAAA, 0.8);
    wallGraphics.drawRect(wall.x, wall.y, wall.width, wall.height);
    
    // Add warning stripes pattern with higher visibility
    wallGraphics.lineStyle(3, 0xFF8800, 0.5);
    const stripeSpacing = 15;
    for (let i = -wall.height; i < wall.width; i += stripeSpacing) {
        wallGraphics.moveTo(wall.x + i, wall.y);
        wallGraphics.lineTo(wall.x + i + wall.height, wall.y + wall.height);
    }
    
    // Add a red X pattern to make it even more visible
    wallGraphics.lineStyle(2, 0xFF4444, 0.6);
    wallGraphics.moveTo(wall.x, wall.y);
    wallGraphics.lineTo(wall.x + wall.width, wall.y + wall.height);
    wallGraphics.moveTo(wall.x + wall.width, wall.y);
    wallGraphics.lineTo(wall.x, wall.y + wall.height);
}

// Create flag sprite
function createFlagSprite(flagState) {
    const container = new PIXI.Container();
    container.flagTeamId = flagState.team_id;

    // Flag base/stand
    const base = new PIXI.Graphics();
    base.beginFill(0x4B4B4B);
    base.drawCircle(0, 5, 8);
    base.endFill();
    base.beginFill(0x2B2B2B);
    base.drawEllipse(0, 5, 10, 4);
    base.endFill();
    container.addChild(base);

    // Enhanced pole with metallic effect
    const pole = new PIXI.Graphics();
    pole.lineStyle(4, 0x8B4513);
    pole.moveTo(0, 5);
    pole.lineTo(0, -40);
    pole.lineStyle(2, 0xCD853F);
    pole.moveTo(-1, 5);
    pole.lineTo(-1, -40);
    container.addChild(pole);

    // Flag fabric container
    const flagContainer = new PIXI.Container();
    flagContainer.position.y = -40;

    // Flag shadow/depth
    const flagShadow = new PIXI.Graphics();
    const flagColor = teamColors[flagState.team_id] || 0xFFFFFF;
    flagShadow.beginFill(mixColors(flagColor, 0x000000, 0.5), 0.5);
    flagShadow.drawPolygon([2, 2, 32, 7, 32, 22, 2, 17]);
    flagContainer.addChild(flagShadow);

    // Main flag
    const flagGraphic = new PIXI.Graphics();
    flagGraphic.beginFill(flagColor);

    // More detailed flag shape with notch
    const flagPoints = [
        0, 0,    // Top left
        30, 5,   // Top right
        28, 7.5, // Notch top
        32, 10,  // Notch point
        28, 12.5,// Notch bottom
        30, 15,  // Bottom right
        0, 20    // Bottom left
    ];
    flagGraphic.drawPolygon(flagPoints);
    flagGraphic.endFill();

    // Flag emblem/pattern
    flagGraphic.beginFill(mixColors(flagColor, 0xFFFFFF, 0.3), 0.5);
    if (flagState.team_id === 1) {
        // Red team - star emblem
        drawStar(flagGraphic, 10, 10, 5, 6, 3);
    } else if (flagState.team_id === 2) {
        // Blue team - circle emblem
        flagGraphic.drawCircle(10, 10, 6);
    }
    flagGraphic.endFill();

    // Flag highlights
    flagGraphic.lineStyle(1, mixColors(flagColor, 0xFFFFFF, 0.2), 0.5);
    flagGraphic.moveTo(0, 5);
    flagGraphic.lineTo(25, 8);
    flagGraphic.moveTo(0, 15);
    flagGraphic.lineTo(25, 17);

    flagContainer.addChild(flagGraphic);
    container.addChild(flagContainer);

    container.flagGraphic = flagContainer;
    container.position.set(flagState.position.x, flagState.position.y);

    // Status effects
    if (flagState.status === GP.FlagStatus.Dropped) {
        // Add dropped indicator
        const droppedGlow = new PIXI.Graphics();
        droppedGlow.lineStyle(3, flagColor, 0.5);
        droppedGlow.drawCircle(0, 0, 20);
        container.addChildAt(droppedGlow, 0);

        // Pulsing effect
        container.droppedGlow = droppedGlow;

        // Timer text with background
        if (flagState.respawn_timer > 0) {
            const timerBg = new PIXI.Graphics();
            timerBg.beginFill(0x000000, 0.7);
            timerBg.drawRoundedRect(-15, -55, 30, 20, 5);
            timerBg.endFill();
            container.addChild(timerBg);

            const timerStyle = new PIXI.TextStyle({
                fontSize: 14,
                fill: 0xFFFFFF,
                fontWeight: 'bold'
            });
            const timerText = new PIXI.Text(Math.ceil(flagState.respawn_timer) + 's', timerStyle);
            timerText.anchor.set(0.5);
            timerText.position.y = -45;
            container.addChild(timerText);
            container.timerText = timerText;
        }
    }

    // Glow effect for base position
    const baseGlow = new PIXI.Graphics();
    baseGlow.beginFill(flagColor, 0.1);
    baseGlow.drawCircle(0, 5, 25);
    baseGlow.endFill();
    container.addChildAt(baseGlow, 0);

    return container;
}

function updateFlags(newFlagStates) {
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

    if (minimap) minimap.objectivesNeedUpdate = true;
}

// Update camera
function updateCamera() {
    if (localPlayerState && app && gameScene) {
        const targetX = app.screen.width / 2 - (localPlayerState.render_x !== undefined ? localPlayerState.render_x : localPlayerState.x);
        const targetY = app.screen.height / 2 - (localPlayerState.render_y !== undefined ? localPlayerState.render_y : localPlayerState.y);

        const smoothing = 0.1;
        gameScene.position.x += (targetX - gameScene.position.x) * smoothing;
        gameScene.position.y += (targetY - gameScene.position.y) * smoothing;
    }
}

// Calculate viewport bounds for culling
function getViewportBounds() {
    if (!app || !gameScene) return null;

    const screenBounds = app.renderer.screen;
    return new PIXI.Rectangle(
        -gameScene.x - VIEW_DISTANCE_BUFFER,
        -gameScene.y - VIEW_DISTANCE_BUFFER,
        screenBounds.width + VIEW_DISTANCE_BUFFER * 2,
        screenBounds.height + VIEW_DISTANCE_BUFFER * 2
    );
}

// Main game loop
function gameLoop(delta) {
    const currentTime = Date.now();
    renderTimestamp = currentTime - INTERPOLATION_DELAY;

    // Update FPS
    frameCount++;
    if (currentTime - lastFPSUpdate >= 1000) {
        fpsValueSpan.textContent = frameCount;
        frameCount = 0;
        lastFPSUpdate = currentTime;
    }

    if (localPlayerState && localPlayerState.alive) {
        updateLocalPlayerPrediction(app.ticker.deltaMS / 1000);
    }

    interpolateEntities();

    const clientDeltaTime = app.ticker.deltaMS / 1000;

    // Client-side projectile interpolation
    projectiles.forEach(proj => {
        if (proj.velocity_x !== undefined && proj.velocity_y !== undefined) {
            // Update projectile position based on velocity
            if (proj.render_x === undefined) {
                proj.render_x = proj.x;
            }
            if (proj.render_y === undefined) {
                proj.render_y = proj.y;
            }
            
            // Move projectile smoothly
            proj.render_x += proj.velocity_x * clientDeltaTime;
            proj.render_y += proj.velocity_y * clientDeltaTime;
        }
    });

    updateSprites();
    updateCamera();

    if (starfield) {
        updateStarfield(starfield, gameScene.position.x, gameScene.position.y, delta);
    }

    animatePickups(delta);
    animateFlags(delta);

    if (effectsManager) effectsManager.update(app.ticker.deltaMS);
    if (minimap && localPlayerState) {
        minimap.update(localPlayerState, players, Array.from(walls.values()), Array.from(flagStates.values()));
    }

    sendInputsToServer();
    updateGameStatsUI();
}

// Client-side prediction
function updateLocalPlayerPrediction(deltaTime) {
    if (!localPlayerState || !localPlayerState.alive) return;

    let moveXIntent = 0;
    let moveYIntent = 0;
    if (inputState.move_forward) moveYIntent -= 1;
    if (inputState.move_backward) moveYIntent += 1;
    if (inputState.move_left) moveXIntent -= 1;
    if (inputState.move_right) moveXIntent += 1;

    const effectiveSpeed = localPlayerState.speed_boost_remaining > 0 ? 225 : 150;

    if (moveXIntent !== 0 || moveYIntent !== 0) {
        const magnitude = Math.sqrt(moveXIntent * moveXIntent + moveYIntent * moveYIntent);
        localPlayerState.x += (moveXIntent / magnitude) * effectiveSpeed * deltaTime;
        localPlayerState.y += (moveYIntent / magnitude) * effectiveSpeed * deltaTime;
    }

    localPlayerState.rotation = inputState.rotation;
    localPlayerState.render_x = localPlayerState.x;
    localPlayerState.render_y = localPlayerState.y;
    localPlayerState.render_rotation = localPlayerState.rotation;
}

// Interpolate entities
function interpolateEntities() {
    const now = Date.now();
    const renderTime = now - INTERPOLATION_DELAY;

    serverUpdates = serverUpdates.filter(update => update.timestamp > renderTime - 500);

    if (serverUpdates.length < 2) return;

    let update1 = null, update2 = null;
    for (let i = serverUpdates.length - 1; i >= 1; i--) {
        if (serverUpdates[i].timestamp >= renderTime && serverUpdates[i - 1].timestamp <= renderTime) {
            update2 = serverUpdates[i];
            update1 = serverUpdates[i - 1];
            break;
        }
    }

    if (!update1 && serverUpdates[0].timestamp <= renderTime && serverUpdates.length > 0) {
        update1 = serverUpdates[0];
        update2 = serverUpdates[0];
    } else if (!update1 || !update2) {
        return;
    }

    const t = (update1.timestamp === update2.timestamp) ? 1 : (renderTime - update1.timestamp) / (update2.timestamp - update1.timestamp);
    const clampedT = Math.max(0, Math.min(1, t));

    // Interpolate players
    players.forEach((currentPlayerState, playerId) => {
        if (playerId === myPlayerId) return;

        const state1 = update1.players.get(playerId);
        const state2 = update2.players.get(playerId);

        if (state1 && state2) {
            currentPlayerState.render_x = state1.x + (state2.x - state1.x) * clampedT;
            currentPlayerState.render_y = state1.y + (state2.y - state1.y) * clampedT;

            let rotDiff = state2.rotation - state1.rotation;
            while (rotDiff > Math.PI) rotDiff -= 2 * Math.PI;
            while (rotDiff < -Math.PI) rotDiff += 2 * Math.PI;
            currentPlayerState.render_rotation = state1.rotation + rotDiff * clampedT;
        } else if (state2) {
            currentPlayerState.render_x = state2.x;
            currentPlayerState.render_y = state2.y;
            currentPlayerState.render_rotation = state2.rotation;
        }
    });

    // Interpolate projectiles
    projectiles.forEach((currentProjState, projId) => {
        const state1 = update1.projectiles.get(projId);
        const state2 = update2.projectiles.get(projId);

        if (state1 && state2) {
            currentProjState.render_x = state1.x + (state2.x - state1.x) * clampedT;
            currentProjState.render_y = state1.y + (state2.y - state1.y) * clampedT;
        } else if (state2) {
            currentProjState.render_x = state2.x;
            currentProjState.render_y = state2.y;
        }
    });
}
