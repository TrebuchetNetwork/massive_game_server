// Initialize PIXI Application
        function initPixi() {
            const pixiContainer = document.getElementById('pixiContainer');
            if (!pixiContainer) {
                log('CRITICAL ERROR: pixiContainer DOM element not found!', 'error');
                return;
            }

            const containerRect = pixiContainer.getBoundingClientRect();
            app = new PIXI.Application({
                width: containerRect.width,
                height: containerRect.height,
                backgroundColor: 0x1a202c,
                antialias: true,
                resolution: window.devicePixelRatio || 1,
                autoDensity: true,
            });
            pixiContainer.appendChild(app.view);

            app.ticker.maxFPS = 60;

            if (!app || !app.stage) {
                log('CRITICAL ERROR: PIXI Application failed to initialize!', 'error');
                return;
            }

            // Main scene container (moves with camera)
            gameScene = new PIXI.Container();
            app.stage.addChild(gameScene);

            // World container (children are in world coordinates)
            worldContainer = new PIXI.Container();
            const initializedManagers = initializeEnhancedGraphics(app, worldContainer);

            audioManager = initializedManagers.audioManager;
            effectsManager = initializedManagers.effectsManager;
            starfield = initializedManagers.starfield;
            healthVignette = initializedManagers.healthVignette;

            window.audioManager = audioManager;
            window.effectsManager = effectsManager;
            window.starfield = starfield;
            window.healthVignette = healthVignette;

            // Add this to resume audio context on first user interaction
            const resumeAudio = () => {
                if (audioManager && audioManager.audioContext && audioManager.audioContext.state === 'suspended') {
                    audioManager.audioContext.resume().then(() => {
                        console.log('Audio context resumed');
                    });
                }
                document.removeEventListener('click', resumeAudio);
                document.removeEventListener('keydown', resumeAudio);
            };
            document.addEventListener('click', resumeAudio);
            document.addEventListener('keydown', resumeAudio);

            gameScene.addChild(worldContainer);

            wallGraphics = new PIXI.Graphics();
            pickupContainer = new PIXI.Container();
            projectileContainer = new PIXI.Container();
            playerContainer = new PIXI.Container();
            flagContainer = new PIXI.Container();

            worldContainer.addChild(wallGraphics);
            worldContainer.addChild(pickupContainer);
            worldContainer.addChild(projectileContainer);
            worldContainer.addChild(playerContainer);
            worldContainer.addChild(flagContainer);

            // HUD container (fixed on screen)
            hudContainer = new PIXI.Container();
            app.stage.addChild(hudContainer);

            // Initialize Managers
            minimap = new Minimap(150, 150, 0.05);
            networkIndicator = new NetworkIndicator();

            // Add minimap to the minimap container
            const minimapContainerElement = document.getElementById('minimapContainer');
            minimapContainerElement.appendChild(minimap.app.view);

            // Add network indicator
            networkQualityIndicatorDiv.appendChild(networkIndicator.app.view);

            // Resize listener
            window.addEventListener('resize', resizePixiApp);
            resizePixiApp();

            app.ticker.add(gameLoop);
            log('PIXI scene graph initialized and ticker started.', 'info');
        }

        function resizePixiApp() {
            const pixiContainer = document.getElementById('pixiContainer');
            if (!app || !pixiContainer) return;
            const containerRect = pixiContainer.getBoundingClientRect();
            app.renderer.resize(containerRect.width, containerRect.height);
            updateCamera();
        }

        // OPTIMIZED: Create player sprite with one-time graphics creation
        function createPlayerSprite(player, isLocal = false) {
            const container = new PIXI.Container();
            container.playerId = player.id;

            // Shadow
            const shadow = new PIXI.Graphics();
            shadow.beginFill(0x000000, 0.3);
            shadow.drawEllipse(0, 8, PLAYER_RADIUS * 1.1, PLAYER_RADIUS * 0.6);
            shadow.endFill();
            shadow.filters = [new PIXI.BlurFilter(2)];
            container.addChild(shadow);

            // Body - Create once with neutral shape
            const body = new PIXI.Graphics();
            const playerTeamColor = teamColors[player.team_id] || teamColors[0];
            const mainBodyColor = player.alive ? playerTeamColor : 0x6B7280;

            // Draw the shape once
            body.lineStyle(2, 0x000000); // Use a neutral line color
            body.beginFill(0xFFFFFF); // Use white so we can tint later
            const shipPoints = [0, -PLAYER_RADIUS * 1.2, PLAYER_RADIUS * 0.8, PLAYER_RADIUS * 0.8,
                PLAYER_RADIUS * 0.3, PLAYER_RADIUS * 0.4, 0, PLAYER_RADIUS * 0.6,
                -PLAYER_RADIUS * 0.3, PLAYER_RADIUS * 0.4, -PLAYER_RADIUS * 0.8, PLAYER_RADIUS * 0.8];
            body.drawPolygon(shipPoints);
            body.endFill();
            body.tint = mainBodyColor; // Set initial tint
            container.addChild(body);
            container.body = body;

            // Engine Glow
            const engineGlow = new PIXI.Graphics();
            engineGlow.beginFill(0x00FFFF, 0.6);
            engineGlow.drawCircle(0, PLAYER_RADIUS * 0.8, PLAYER_RADIUS * 0.3);
            engineGlow.endFill();
            engineGlow.filters = [new PIXI.BlurFilter(4)];
            body.addChildAt(engineGlow, 0);
            container.engineGlow = engineGlow;

            // Local Player Indicator
            if (isLocal) {
                const localIndicator = new PIXI.Graphics();
                localIndicator.lineStyle(2, 0xFFD700, 0.7);
                localIndicator.drawCircle(0, 0, PLAYER_RADIUS + 4);
                container.addChild(localIndicator);
                container.localIndicator = localIndicator;
            }

            // Gun - Create base structure once
            const gun = new PIXI.Container(); // Use container to hold gun graphics
            gun.rotation = -Math.PI / 2;
            gun.cachedWeapons = new Map(); // Store cached weapon graphics
            gun.currentWeapon = null; // Track current weapon
            container.addChild(gun);
            container.gun = gun;

            // Health Bar Container
            const healthBarContainer = new PIXI.Container();
            healthBarContainer.position.set(0, -PLAYER_RADIUS - 15);

            // Health bar background - create once
            const healthBg = new PIXI.Graphics();
            healthBg.beginFill(0x1F2937, 0.9);
            healthBg.drawRoundedRect(-PLAYER_RADIUS - 2, -2, PLAYER_RADIUS * 2 + 4, 10, 5);
            healthBarContainer.addChild(healthBg);

            // Health bar border - create once
            const healthBorder = new PIXI.Graphics();
            healthBorder.lineStyle(1, 0x4B5563, 0.8);
            healthBorder.drawRoundedRect(-PLAYER_RADIUS - 2, -2, PLAYER_RADIUS * 2 + 4, 10, 5);
            healthBarContainer.addChild(healthBorder);

            // Health fill - create as full bar, we'll scale it
            const healthFg = new PIXI.Graphics();
            healthFg.beginFill(0x22C55E);
            healthFg.drawRoundedRect(-PLAYER_RADIUS, 0, PLAYER_RADIUS * 2, 6, 3);
            healthFg.endFill();
            healthBarContainer.addChild(healthFg);

            container.addChild(healthBarContainer);
            container.healthBarContainer = healthBarContainer;
            container.healthFg = healthFg;

            // Shield Visual
            const shieldVisual = new PIXI.Graphics();
            container.addChildAt(shieldVisual, 1);
            container.shieldVisual = shieldVisual;

            // Username Text
            const usernameStyle = new PIXI.TextStyle({
                fontFamily: 'Arial', fontSize: 12, fill: [0xFFFFFF, 0xE5E7EB],
                stroke: 0x111827, strokeThickness: 3,
                dropShadow: true, dropShadowColor: 0x000000, dropShadowBlur: 3, dropShadowDistance: 1,
                align: 'center'
            });
            const usernameText = new PIXI.Text(player.username || 'Player', usernameStyle);
            usernameText.anchor.set(0.5);
            usernameText.position.y = -PLAYER_RADIUS - 28;
            container.addChild(usernameText);
            container.usernameText = usernameText;

            // Placeholders for effects
            container.speedBoostEffect = null;
            container.damageBoostEffect = null;

            // Initial updates
            updatePlayerGun(container, player);
            updatePlayerHealthBar(container, player);
            updateShieldVisual(container, player.shield_current || 0, player.shield_max || 0);

            return container;
        }

        // OPTIMIZED: Update player sprite without redrawing graphics
        function updatePlayerSprite(sprite, player) {
            sprite.position.x = player.render_x !== undefined ? player.render_x : player.x;
            sprite.position.y = player.render_y !== undefined ? player.render_y : player.y;

            let effectiveRotation = (player.render_rotation !== undefined ? player.render_rotation : player.rotation) + (Math.PI / 2);
            sprite.rotation = effectiveRotation;

            // Update body tint instead of redrawing
            const playerTeamColor = teamColors[player.team_id] || teamColors[0];
            const mainBodyColor = player.alive ? playerTeamColor : 0x6B7280;
            sprite.body.tint = mainBodyColor;

            if (sprite.localIndicator) {
                sprite.localIndicator.visible = (sprite.playerId === myPlayerId && player.alive);
            }

            if (sprite.engineGlow) {
                if (player.alive && (player.velocity_x !== 0 || player.velocity_y !== 0)) {
                    sprite.engineGlow.visible = true;
                    const speed = Math.sqrt(player.velocity_x * player.velocity_x + player.velocity_y * player.velocity_y);
                    const intensity = Math.min(1, speed / 150);
                    sprite.engineGlow.alpha = 0.4 + intensity * 0.4;
                    sprite.engineGlow.scale.set(0.8 + intensity * 0.4);
                } else {
                    sprite.engineGlow.visible = false;
                }
            }

            sprite.visible = player.alive || (player.respawn_timer !== undefined && player.respawn_timer > 0);
            sprite.alpha = player.alive ? 1 : 0.5;

            updatePlayerGun(sprite, player);
            updatePlayerHealthBar(sprite, player);
            updateShieldVisual(sprite, player.shield_current || 0, player.shield_max || 0);

            if (sprite.usernameText.text !== (player.username || 'Player')) {
                sprite.usernameText.text = player.username || 'Player';
            }

            // Respawn Timer Text
            if (!player.alive && player.respawn_timer > 0) {
                if (!sprite.respawnText) {
                    const respawnStyle = new PIXI.TextStyle({ fontSize: 14, fill: 0xFFFFFF, stroke: 0x000000, strokeThickness: 3, fontWeight: 'bold' });
                    sprite.respawnText = new PIXI.Text('', respawnStyle);
                    sprite.respawnText.anchor.set(0.5);
                    sprite.respawnText.position.y = PLAYER_RADIUS + 10;
                    sprite.addChild(sprite.respawnText);
                }
                sprite.respawnText.text = Math.ceil(player.respawn_timer) + 's';
                sprite.respawnText.visible = true;
            } else if (sprite.respawnText) {
                sprite.respawnText.visible = false;
            }

            // Speed Boost Effect
            if (player.speed_boost_remaining > 0 && player.alive) {
                if (!sprite.speedBoostEffect) {
                    sprite.speedBoostEffect = createSpeedBoostEffect();
                    sprite.addChildAt(sprite.speedBoostEffect, 0);
                }
                sprite.speedBoostEffect.visible = true;
            } else if (sprite.speedBoostEffect) {
                sprite.speedBoostEffect.visible = false;
            }

            // Carried Flag Visual
            if (player.is_carrying_flag_team_id > 0 && player.alive) {
                if (!sprite.carriedFlagSprite) {
                    sprite.carriedFlagSprite = new PIXI.Container();
                    sprite.addChild(sprite.carriedFlagSprite);

                    // Create flag graphic once
                    const flagGraphics = new PIXI.Graphics();
                    flagGraphics.beginFill(0xFFFFFF); // White, will be tinted
                    flagGraphics.drawRect(PLAYER_RADIUS * 0.6, -PLAYER_RADIUS * 1.5, 3, PLAYER_RADIUS * 1.5);
                    flagGraphics.drawRect(PLAYER_RADIUS * 0.6 + 3, -PLAYER_RADIUS * 1.5, 15, 10);
                    flagGraphics.endFill();
                    sprite.carriedFlagSprite.addChild(flagGraphics);
                    sprite.carriedFlagGraphics = flagGraphics;
                }
                sprite.carriedFlagSprite.visible = true;
                // Update flag color via tint
                const flagColor = teamColors[player.is_carrying_flag_team_id] || 0xFFFFFF;
                sprite.carriedFlagGraphics.tint = flagColor;
            } else if (sprite.carriedFlagSprite) {
                sprite.carriedFlagSprite.visible = false;
            }
        }

        // OPTIMIZED: Update gun without redrawing
        function updatePlayerGun(sprite, player) {
            const gunContainer = sprite.gun;

            if (!player.alive) {
                gunContainer.visible = false;
                return;
            }
            gunContainer.visible = true;

            const weaponConfigs = {
                [GP.WeaponType.Pistol]: {
                    barrelLength: PLAYER_RADIUS + 12,
                    barrelWidth: 4,
                    color: 0xFFBF00,
                    muzzleSize: 6
                },
                [GP.WeaponType.Shotgun]: {
                    barrelLength: PLAYER_RADIUS + 14,
                    barrelWidth: 8,
                    color: 0xFF4444,
                    muzzleSize: 10,
                    barrelCount: 2
                },
                [GP.WeaponType.Rifle]: {
                    barrelLength: PLAYER_RADIUS + 18,
                    barrelWidth: 5,
                    color: 0x4444FF,
                    muzzleSize: 7
                },
                [GP.WeaponType.Sniper]: {
                    barrelLength: PLAYER_RADIUS + 22,
                    barrelWidth: 3,
                    color: 0xAA44FF,
                    muzzleSize: 5,
                    scope: true
                },
                [GP.WeaponType.Melee]: {
                    barrelLength: PLAYER_RADIUS + 8,
                    barrelWidth: 10,
                    color: 0xD1D5DB,
                    muzzleSize: 0
                }
            };

            const config = weaponConfigs[player.weapon] || weaponConfigs[GP.WeaponType.Pistol];

            // Hide current weapon if it exists and is different
            if (gunContainer.currentWeapon && gunContainer.currentWeapon !== player.weapon) {
                const currentGraphic = gunContainer.cachedWeapons.get(gunContainer.currentWeapon);
                if (currentGraphic) currentGraphic.visible = false;
            }

            // Get or create the weapon graphic
            let weaponGraphic = gunContainer.cachedWeapons.get(player.weapon);

            if (!weaponGraphic) {
                // Create the weapon graphic once and cache it
                weaponGraphic = new PIXI.Container();

                // Base weapon shape
                const baseGraphic = new PIXI.Graphics();

                // Draw weapon barrel(s)
                if (config.barrelCount === 2) {
                    // Shotgun double barrel
                    baseGraphic.lineStyle(config.barrelWidth / 2, config.color);
                    baseGraphic.moveTo(0, -3);
                    baseGraphic.lineTo(config.barrelLength, -3);
                    baseGraphic.moveTo(0, 3);
                    baseGraphic.lineTo(config.barrelLength, 3);
                } else {
                    // Single barrel weapons
                    baseGraphic.lineStyle(config.barrelWidth, config.color);
                    baseGraphic.moveTo(0, 0);
                    baseGraphic.lineTo(config.barrelLength, 0);
                }

                // Add muzzle
                if (config.muzzleSize > 0) {
                    baseGraphic.beginFill(mixColors(config.color, 0x000000, 0.2));
                    baseGraphic.drawCircle(config.barrelLength, 0, config.muzzleSize / 2);
                    baseGraphic.endFill();

                    // Muzzle highlight
                    baseGraphic.beginFill(mixColors(config.color, 0xFFFFFF, 0.3), 0.5);
                    baseGraphic.drawCircle(config.barrelLength, 0, config.muzzleSize / 3);
                    baseGraphic.endFill();
                }

                // Sniper scope
                if (config.scope) {
                    baseGraphic.lineStyle(1, config.color, 0.7);
                    baseGraphic.drawCircle(config.barrelLength * 0.7, 0, 5);
                    baseGraphic.moveTo(config.barrelLength * 0.7 - 5, 0);
                    baseGraphic.lineTo(config.barrelLength * 0.7 + 5, 0);
                    baseGraphic.moveTo(config.barrelLength * 0.7, -5);
                    baseGraphic.lineTo(config.barrelLength * 0.7, 5);
                }

                weaponGraphic.addChild(baseGraphic);
                weaponGraphic.baseGraphic = baseGraphic;

                // Create damage boost overlay (initially invisible)
                const damageBoostOverlay = new PIXI.Graphics();
                damageBoostOverlay.visible = false;

                // Draw glowing overlay
                damageBoostOverlay.lineStyle(config.barrelWidth + 4, 0xFF6B6B, 0.3);
                if (config.barrelCount === 2) {
                    damageBoostOverlay.moveTo(0, -3);
                    damageBoostOverlay.lineTo(config.barrelLength, -3);
                    damageBoostOverlay.moveTo(0, 3);
                    damageBoostOverlay.lineTo(config.barrelLength, 3);
                } else {
                    damageBoostOverlay.moveTo(0, 0);
                    damageBoostOverlay.lineTo(config.barrelLength, 0);
                }

                // Power effect at muzzle
                const powerEffect = new PIXI.Graphics();
                powerEffect.beginFill(0xFF6B6B, 0.6);
                powerEffect.drawCircle(config.barrelLength, 0, config.muzzleSize * 0.8);
                powerEffect.endFill();
                damageBoostOverlay.addChild(powerEffect);
                damageBoostOverlay.powerEffect = powerEffect;

                weaponGraphic.addChild(damageBoostOverlay);
                weaponGraphic.damageBoostOverlay = damageBoostOverlay;
                weaponGraphic.config = config; // Store config for damage boost effect

                // Add to gun container and cache
                gunContainer.addChild(weaponGraphic);
                gunContainer.cachedWeapons.set(player.weapon, weaponGraphic);
            }

            // Show the current weapon
            weaponGraphic.visible = true;
            gunContainer.currentWeapon = player.weapon;

            // Handle damage boost effect
            if (player.damage_boost_remaining > 0) {
                weaponGraphic.damageBoostOverlay.visible = true;

                // Pulsing effect
                const pulse = Math.sin(Date.now() * 0.01) * 0.3 + 0.7;
                weaponGraphic.baseGraphic.tint = PIXI.utils.rgb2hex([1, pulse, pulse]);

                // Animate power effect
                if (weaponGraphic.damageBoostOverlay.powerEffect) {
                    const powerSize = weaponGraphic.config.muzzleSize * 0.8 + Math.sin(Date.now() * 0.015) * 2;
                    weaponGraphic.damageBoostOverlay.powerEffect.scale.set(powerSize / (weaponGraphic.config.muzzleSize * 0.8));
                }
            } else {
                weaponGraphic.damageBoostOverlay.visible = false;
                weaponGraphic.baseGraphic.tint = 0xFFFFFF;
            }
        }



        // OPTIMIZED: Update health bar using scale instead of redrawing
        function updatePlayerHealthBar(sprite, player) {
            if (!sprite.healthFg || !sprite.healthBarContainer) return;

            if (player.alive) {
                sprite.healthBarContainer.visible = true;
                const healthPercent = Math.max(0, Math.min(1, player.health / player.max_health));

                // Scale the health bar instead of redrawing
                sprite.healthFg.scale.x = healthPercent;

                // Update color via tint
                let healthColor;
                if (healthPercent > 0.6) {
                    healthColor = interpolateColor(0x22C55E, 0xFACC15, (healthPercent - 0.6) / 0.4);
                } else if (healthPercent > 0.3) {
                    healthColor = interpolateColor(0xFACC15, 0xEF4444, (healthPercent - 0.3) / 0.3);
                } else {
                    healthColor = 0xEF4444;
                }
                sprite.healthFg.tint = healthColor;

                // Pulse effect when low health
                if (healthPercent < 0.3) {
                    const pulse = Math.sin(Date.now() * 0.01) * 0.2 + 0.8;
                    sprite.healthFg.alpha = pulse;
                } else {
                    sprite.healthFg.alpha = 1;
                }
            } else {
                sprite.healthBarContainer.visible = false;
            }
        }

        // Helper function to interpolate colors
        function interpolateColor(color1, color2, factor) {
            const c1 = PIXI.Color.shared.setValue(color1).toRgbArray();
            const c2 = PIXI.Color.shared.setValue(color2).toRgbArray();
            const r = Math.floor(c1[0] * 255 * (1 - factor) + c2[0] * 255 * factor);
            const g = Math.floor(c1[1] * 255 * (1 - factor) + c2[1] * 255 * factor);
            const b = Math.floor(c1[2] * 255 * (1 - factor) + c2[2] * 255 * factor);
            return (r << 16) | (g << 8) | b;
        }

        // Enhanced createSpeedBoostEffect function
        function createSpeedBoostEffect() {
            const effect = new PIXI.Container();

            // Multiple trail lines
            for (let i = 0; i < 3; i++) {
                const trail = new PIXI.Graphics();
                trail.beginFill(0x00FFFF, 0.3);
                trail.drawRect(-2, -PLAYER_RADIUS * (1.5 + i * 0.3), 4, PLAYER_RADIUS * 0.5);
                trail.endFill();
                trail.rotation = (i - 1) * 0.2;
                effect.addChild(trail);
            }

            // Speed particles
            const particleContainer = new PIXI.Container();
            effect.addChild(particleContainer);
            effect.particleContainer = particleContainer;

            return effect;
        }

        function updateShieldVisual(sprite, current, max) {
            if (!sprite.shieldVisual) return;
            sprite.shieldVisual.clear();

            if (current > 0 && max > 0) {
                const shieldPercent = current / max;
                const shieldRadius = PLAYER_RADIUS + 8;

                // Hexagonal shield pattern
                const sides = 6;
                const alpha = 0.2 + shieldPercent * 0.3;

                // Outer shield layer
                sprite.shieldVisual.lineStyle(Math.max(2, 4 * shieldPercent), 0x00BFFF, alpha);
                sprite.shieldVisual.beginFill(0x00BFFF, alpha * 0.2);
                drawRegularPolygon(sprite.shieldVisual, 0, 0, shieldRadius + (5 * shieldPercent), sides);
                sprite.shieldVisual.endFill();

                // Inner shield segments
                if (shieldPercent > 0.3) {
                    sprite.shieldVisual.lineStyle(1, 0x00FFFF, alpha * 0.5);
                    const segmentAngle = (Math.PI * 2) / sides;
                    for (let i = 0; i < sides; i++) {
                        const angle = segmentAngle * i;
                        sprite.shieldVisual.moveTo(0, 0);
                        sprite.shieldVisual.lineTo(
                            Math.cos(angle) * shieldRadius,
                            Math.sin(angle) * shieldRadius
                        );
                    }
                }

                // Add shimmer effect
                const shimmer = Math.sin(Date.now() * 0.003) * 0.1;
                sprite.shieldVisual.alpha = 1 + shimmer;
            }
        }

        // FIXED: Enhanced projectile sprite with proper animation support
        function createProjectileSprite(projectile) {
            const container = new PIXI.Container();
            container.projectileId = projectile.id;

            const projectileConfigs = {
                [GP.WeaponType.Pistol]: {
                    color: 0xFFBF00,
                    glowColor: 0xFFFF00,
                    size: 8,
                    glowSize: 15,
                    shape: 'bullet'
                },
                [GP.WeaponType.Shotgun]: {
                    color: 0xFF4444,
                    glowColor: 0xFF6666,
                    size: 4,
                    glowSize: 8,
                    shape: 'pellet'
                },
                [GP.WeaponType.Rifle]: {
                    color: 0x4444FF,
                    glowColor: 0x6666FF,
                    size: 10,
                    glowSize: 18,
                    shape: 'laser'
                },
                [GP.WeaponType.Sniper]: {
                    color: 0xAA44FF,
                    glowColor: 0xFF00FF,
                    size: 12,
                    glowSize: 20,
                    shape: 'beam'
                }
            };

            const config = projectileConfigs[projectile.weapon_type] || projectileConfigs[GP.WeaponType.Pistol];

            // Outer glow effect - more prominent
            const glow = new PIXI.Graphics();
            glow.beginFill(config.glowColor, 0.4);
            glow.drawCircle(0, 0, config.glowSize);
            glow.endFill();
            glow.filters = [new PIXI.BlurFilter(4)];
            container.addChild(glow);

            // Core projectile
            const core = new PIXI.Graphics();

            switch (config.shape) {
                case 'pellet':
                    core.beginFill(config.color, 1);
                    core.drawCircle(0, 0, config.size);
                    core.endFill();
                    core.beginFill(0xFFFFFF, 0.8);
                    core.drawCircle(0, 0, config.size * 0.5);
                    core.endFill();
                    break;

                case 'laser':
                    core.beginFill(config.color, 0.9);
                    core.drawRoundedRect(-config.size * 1.5, -config.size / 3, config.size * 3, config.size * 0.66, config.size / 3);
                    core.endFill();
                    core.beginFill(0xFFFFFF, 1);
                    core.drawRoundedRect(-config.size * 1.2, -config.size / 6, config.size * 2.4, config.size / 3, config.size / 6);
                    core.endFill();
                    break;

                case 'beam':
                    core.beginFill(config.color, 0.8);
                    core.drawRect(-config.size * 3, -2, config.size * 6, 4);
                    core.endFill();
                    core.beginFill(0xFFFFFF, 1);
                    core.drawRect(-config.size * 3, -1, config.size * 6, 2);
                    core.endFill();
                    break;

                default: // bullet
                    core.beginFill(config.color, 1);
                    core.drawRoundedRect(-config.size / 2, -config.size / 3, config.size * 1.5, config.size * 0.66, config.size / 3);
                    core.endFill();
                    core.beginFill(0xFFFFFF, 1);
                    core.drawCircle(config.size * 0.5, 0, config.size / 3);
                    core.endFill();
            }

            container.addChild(core);

            // Add motion trail effect
            const trail = new PIXI.Graphics();
            trail.alpha = 0.5;
            container.addChildAt(trail, 0);
            container.trail = trail;

            // Store config for trail updates
            container.trailColor = config.glowColor;
            container.weaponType = projectile.weapon_type;

            // FIXED: Initialize position and velocity tracking
            container.lastPositions = [];
            container.velocity = {
                x: projectile.velocity_x || 0,
                y: projectile.velocity_y || 0
            };

            return container;
        }

        // FIXED: Enhanced projectile update with proper animation
        function updateProjectileSprite(sprite, projectile) {
            // Update position using interpolated values if available
            const newX = projectile.render_x !== undefined ? projectile.render_x : projectile.x;
            const newY = projectile.render_y !== undefined ? projectile.render_y : projectile.y;
            
            sprite.position.x = newX;
            sprite.position.y = newY;

            // Update rotation based on velocity
            if (projectile.velocity_x !== undefined && projectile.velocity_y !== undefined) {
                sprite.rotation = Math.atan2(projectile.velocity_y, projectile.velocity_x);
                // Update cached velocity
                sprite.velocity.x = projectile.velocity_x;
                sprite.velocity.y = projectile.velocity_y;
            }

            // Update trail effect
            if (sprite.trail && sprite.lastPositions) {
                sprite.trail.clear();
                
                // Add current position to history
                sprite.lastPositions.unshift({ x: newX, y: newY });
                if (sprite.lastPositions.length > 5) {
                    sprite.lastPositions.pop();
                }

                // Draw trail
                if (sprite.lastPositions.length > 1) {
                    sprite.trail.lineStyle(3, sprite.trailColor, 0.3);
                    
                    // Start from current position (local coordinates)
                    sprite.trail.moveTo(0, 0);
                    
                    for (let i = 1; i < sprite.lastPositions.length; i++) {
                        const alpha = (1 - i / sprite.lastPositions.length) * 0.3;
                        sprite.trail.lineStyle(3 - i * 0.5, sprite.trailColor, alpha);
                        sprite.trail.lineTo(
                            sprite.lastPositions[i].x - newX,
                            sprite.lastPositions[i].y - newY
                        );
                    }
                }
            }
        }
