/**
 * SpriteManager.js - Player and projectile sprite lifecycle management
 *
 * Extracted from client.html. Contains creation, update, and destruction
 * of player and projectile PIXI sprites. Uses getCtx callback pattern
 * to access shared game state.
 */

export function createSpriteManager(getCtx) {

    function createPlayerSprite(player, isLocal = false) {
        const ctx = getCtx();
        const { PIXI, GP, renderAssetCache, GLOBAL_LIGHT_DIR, PLAYER_SHADOW_BASE_OFFSET,
                PLAYER_RADIUS, STABLE_MODE_FORCED, weaponColors } = ctx;

        const container = new PIXI.Container();
        container.playerId = player.id;
        const lightweightRemote = STABLE_MODE_FORCED && !isLocal;
        container._lightweightRemote = lightweightRemote;

        if (!lightweightRemote) {
            const shadow = new PIXI.Sprite(renderAssetCache.shadowTexture);
            shadow.anchor.set(0.5);
            shadow.position.set(GLOBAL_LIGHT_DIR.x * PLAYER_SHADOW_BASE_OFFSET, GLOBAL_LIGHT_DIR.y * PLAYER_SHADOW_BASE_OFFSET);
            shadow.tint = 0x000000;
            shadow.alpha = 0.38;
            shadow.blendMode = PIXI.BLEND_MODES.MULTIPLY;
            container.addChild(shadow);
            container.shadowSprite = shadow;
        }

        const body = new PIXI.Sprite(renderAssetCache.shipTexture);
        body.anchor.set(0.5);
        container.addChild(body);
        container.body = body;

        if (!lightweightRemote) {
            const engineGlow = new PIXI.Sprite(renderAssetCache.engineGlowTexture);
            engineGlow.anchor.set(0.5);
            engineGlow.position.set(0, PLAYER_RADIUS * 0.8);
            engineGlow.tint = 0x00FFFF;
            engineGlow.alpha = 0;
            engineGlow.blendMode = PIXI.BLEND_MODES.ADD;
            body.addChildAt(engineGlow, 0);
            container.engineGlow = engineGlow;
        }

        if (isLocal) {
            const localIndicator = new PIXI.Sprite(renderAssetCache.localIndicatorTexture);
            localIndicator.anchor.set(0.5);
            localIndicator.tint = 0xFFD700;
            localIndicator.alpha = 0.7;
            container.addChild(localIndicator);
            container.localIndicator = localIndicator;
        }

        const gun = new PIXI.Sprite(renderAssetCache.gunTextures.get(player.weapon) || renderAssetCache.gunTextures.get(GP.WeaponType.Pistol));
        gun.anchor.set(0, 0.5);
        gun.rotation = -Math.PI / 2;
        container.addChild(gun);
        container.gun = gun;

        if (!lightweightRemote) {
            const healthBarContainer = new PIXI.Container();
            healthBarContainer.position.set(0, -PLAYER_RADIUS - 15);
            const healthBg = new PIXI.Sprite(renderAssetCache.healthBarBgTexture);
            healthBg.anchor.set(0.5);
            healthBg.position.set(0, 3);
            healthBarContainer.addChild(healthBg);
            const healthBorder = new PIXI.Sprite(renderAssetCache.healthBarBorderTexture);
            healthBorder.anchor.set(0.5);
            healthBorder.position.set(0, 3);
            healthBarContainer.addChild(healthBorder);
            const healthFg = new PIXI.Sprite(renderAssetCache.healthBarTextures[10] || PIXI.Texture.EMPTY);
            healthFg.anchor.set(0, 0.5);
            healthFg.position.set(-PLAYER_RADIUS, 3);
            healthBarContainer.addChild(healthFg);
            container.addChild(healthBarContainer);
            container.healthFg = healthFg;
            container.healthBarContainer = healthBarContainer;
            container._healthBarIndex = 10;

            const shieldVisual = new PIXI.Sprite(renderAssetCache.shieldTexture);
            shieldVisual.anchor.set(0.5);
            shieldVisual.tint = 0x00BFFF;
            shieldVisual.blendMode = PIXI.BLEND_MODES.ADD;
            shieldVisual.visible = false;
            container.addChildAt(shieldVisual, 1);
            container.shieldVisual = shieldVisual;

            const shieldImpactRing = new PIXI.Graphics();
            shieldImpactRing.visible = false;
            shieldImpactRing.zIndex = 3;
            container.addChild(shieldImpactRing);
            container.shieldImpactRing = shieldImpactRing;

            const shieldCrackOverlay = new PIXI.Graphics();
            shieldCrackOverlay.visible = false;
            shieldCrackOverlay.zIndex = 4;
            container.addChild(shieldCrackOverlay);
            container.shieldCrackOverlay = shieldCrackOverlay;

            const reloadArc = new PIXI.Graphics();
            reloadArc.visible = false;
            reloadArc.position.set(0, -PLAYER_RADIUS - 27);
            reloadArc.zIndex = 5;
            container.addChild(reloadArc);
            container.reloadArc = reloadArc;

            const statusIconContainer = new PIXI.Container();
            statusIconContainer.visible = false;
            statusIconContainer.position.set(0, -PLAYER_RADIUS - 42);
            statusIconContainer.zIndex = 6;
            const iconStyle = new PIXI.TextStyle({
                fontSize: 10,
                fontWeight: 'bold',
                fill: 0xE2E8F0,
                stroke: 0x0F172A,
                strokeThickness: 3,
                align: 'center',
            });
            const speedIcon = new PIXI.Text('SPD', iconStyle);
            speedIcon.anchor.set(0.5);
            speedIcon.visible = false;
            speedIcon.x = -18;
            const damageIcon = new PIXI.Text('DMG', iconStyle);
            damageIcon.anchor.set(0.5);
            damageIcon.visible = false;
            damageIcon.x = 0;
            const shieldIcon = new PIXI.Text('SHD', iconStyle);
            shieldIcon.anchor.set(0.5);
            shieldIcon.visible = false;
            shieldIcon.x = 18;
            statusIconContainer.addChild(speedIcon, damageIcon, shieldIcon);
            container.addChild(statusIconContainer);
            container.statusIconContainer = statusIconContainer;
            container.speedStatusIcon = speedIcon;
            container.damageStatusIcon = damageIcon;
            container.shieldStatusIcon = shieldIcon;

            const usernameText = new PIXI.BitmapText(player.username || 'Player', {
                fontName: renderAssetCache.bitmapFontName,
                fontSize: 12,
                align: 'center'
            });
            usernameText.anchor.set(0.5);
            usernameText.position.y = -PLAYER_RADIUS - 28;
            container.addChild(usernameText);
            container.usernameText = usernameText;
        }

        container.speedBoostEffect = null;
        container.damageBoostEffect = null;
        container.dodgeGlowEffect = null;
        container.weaponSwapEffect = null;
        container._lastTeamId = null;
        container._lastAlive = null;
        container._lastWeapon = null;
        container._weaponSwapPunchUntilMs = 0;
        container._weaponSwapPunchDurationMs = 170;
        container._lastDamageBoost = false;
        container._lastHealthPercent = null;
        container._lastHealthAlive = null;
        container._lastHealthValue = Number.isFinite(player.health) ? Number(player.health) : null;
        container._damageFlashUntilMs = 0;
        container._lastDamageFlashActive = false;
        container._lastAliveTransitionState = !!player.alive;
        container._lastShieldCurrent = null;
        container._lastShieldMax = null;
        container._shieldImpactUntilMs = 0;
        container._shieldImpactStrength = 0;
        container._shieldBreakFlashUntilMs = 0;
        container._lastCarryingFlagTeam = 0;
        container._lastFarDetail = null;
        container._lastDenseVisualMode = null;

        updatePlayerGun(container, player);
        updatePlayerHealthBar(container, player);
        updateShieldVisual(container, player.shield_current || 0, player.shield_max || 0);

        return container;
    }

    function hidePlayerSprite(sprite) {
        if (!sprite) return;
        sprite.visible = false;
        if (sprite.usernameText) sprite.usernameText.visible = false;
        if (sprite.healthBarContainer) sprite.healthBarContainer.visible = false;
        if (sprite.engineGlow) sprite.engineGlow.visible = false;
        if (sprite.shadowSprite) sprite.shadowSprite.visible = false;
        if (sprite.shieldVisual) sprite.shieldVisual.visible = false;
        if (sprite.speedBoostEffect) sprite.speedBoostEffect.visible = false;
        if (sprite.damageBoostEffect) sprite.damageBoostEffect.visible = false;
        if (sprite.dodgeGlowEffect) sprite.dodgeGlowEffect.visible = false;
        if (sprite.weaponSwapEffect) sprite.weaponSwapEffect.visible = false;
        if (sprite.carriedFlagSprite) sprite.carriedFlagSprite.visible = false;
        if (sprite.respawnText) sprite.respawnText.visible = false;
        if (sprite.reloadArc) sprite.reloadArc.visible = false;
        if (sprite.statusIconContainer) sprite.statusIconContainer.visible = false;
        if (sprite.shieldImpactRing) sprite.shieldImpactRing.visible = false;
        if (sprite.shieldCrackOverlay) sprite.shieldCrackOverlay.visible = false;
    }

    function destroyPlayerSprite(playerId) {
        const ctx = getCtx();
        const { playerSprites, playerContainer } = ctx;
        const sprite = playerSprites.get(playerId);
        if (!sprite) return;
        if (ctx.localPlayerSprite === sprite) {
            ctx.setLocalPlayerSprite(null);
        }
        playerContainer.removeChild(sprite);
        sprite.destroy({ children: true });
        playerSprites.delete(playerId);
    }

    function removePlayerClientState(playerId) {
        if (!playerId) return;
        const ctx = getCtx();
        const { players, serverUpdates, cullWorkerResult } = ctx;
        destroyPlayerSprite(playerId);
        players.delete(playerId);
        for (let i = 0; i < serverUpdates.length; i += 1) {
            const snapshot = serverUpdates[i];
            if (snapshot && snapshot.players) {
                snapshot.players.delete(playerId);
            }
        }
        if (cullWorkerResult && cullWorkerResult.playerSet) {
            cullWorkerResult.playerSet.delete(playerId);
        }
    }

    function updatePlayerSprite(sprite, player, localAnchorX, localAnchorY, updateContext, skipVisibilityCheck = false) {
        const ctx = getCtx();
        const { myPlayerId, PIXI, GP, teamColors, ALPHA_EPSILON, TRANSFORM_EPSILON,
                GLOBAL_LIGHT_DIR, PLAYER_SHADOW_BASE_OFFSET, PLAYER_RADIUS,
                PLAYER_CULL_MARGIN_WORLD, FAR_DETAIL_DISTANCE_SQ,
                HIGH_POPULATION_PLAYER_COUNT, ultraPerformanceMode, STABLE_MODE_FORCED,
                frameNowMs, frameCounter, RESPAWN_WORLD_TEXT_ENABLED,
                isWorldPointVisible, createSpeedBoostEffect, createDodgeGlowEffect,
                createWeaponSwapEffect, buildCarriedFlagSprite,
                emitPlayerDeathEffect, emitPlayerRespawnEffect } = ctx;

        const targetX = player.render_x !== undefined ? player.render_x : player.x;
        const targetY = player.render_y !== undefined ? player.render_y : player.y;
        if (sprite.position.x !== targetX) sprite.position.x = targetX;
        if (sprite.position.y !== targetY) sprite.position.y = targetY;

        const effectiveRotation = (player.render_rotation !== undefined ? player.render_rotation : player.rotation) + (Math.PI / 2);
        if (sprite.rotation !== effectiveRotation) sprite.rotation = effectiveRotation;

        const isLocalSprite = sprite.playerId === myPlayerId;
        if (sprite._lastAliveTransitionState !== player.alive) {
            const transitionVisible = isLocalSprite || isWorldPointVisible(targetX, targetY, PLAYER_CULL_MARGIN_WORLD);
            if (
                transitionVisible &&
                sprite._lastAliveTransitionState &&
                !player.alive &&
                typeof emitPlayerDeathEffect === 'function'
            ) {
                emitPlayerDeathEffect(targetX, targetY, player.team_id, isLocalSprite);
            } else if (
                transitionVisible &&
                !sprite._lastAliveTransitionState &&
                player.alive &&
                typeof emitPlayerRespawnEffect === 'function'
            ) {
                emitPlayerRespawnEffect(targetX, targetY, player.team_id, isLocalSprite);
            }
            sprite._lastAliveTransitionState = !!player.alive;
        }

        const totalPlayerCount = updateContext.totalPlayerCount;
        const denseVisualMode = updateContext.denseVisualMode;
        const detailTickDivisor = updateContext.detailTickDivisor;
        const playerLodTier = updateContext.playerLodTier || 'full';
        const lowLodTier = playerLodTier === 'low' || playerLodTier === 'dot';
        sprite._lightweightRemote = !isLocalSprite && lowLodTier;
        const hideRemoteFxByDensity = !isLocalSprite && (denseVisualMode || lowLodTier || !!sprite._lightweightRemote);
        if (!isLocalSprite && !skipVisibilityCheck && !isWorldPointVisible(targetX, targetY, PLAYER_CULL_MARGIN_WORLD)) {
            hidePlayerSprite(sprite);
            return;
        }

        if (sprite.shadowSprite) {
            const shadowVisible = !hideRemoteFxByDensity;
            if (sprite.shadowSprite.visible !== shadowVisible) {
                sprite.shadowSprite.visible = shadowVisible;
            }
            if (shadowVisible) {
                const distance = PLAYER_SHADOW_BASE_OFFSET;
                const worldOffsetX = GLOBAL_LIGHT_DIR.x * distance;
                const worldOffsetY = GLOBAL_LIGHT_DIR.y * distance;
                const cos = Math.cos(-effectiveRotation);
                const sin = Math.sin(-effectiveRotation);
                const shadowX = worldOffsetX * cos - worldOffsetY * sin;
                const shadowY = worldOffsetX * sin + worldOffsetY * cos;
                if (sprite.shadowSprite.position.x !== shadowX) sprite.shadowSprite.position.x = shadowX;
                if (sprite.shadowSprite.position.y !== shadowY) sprite.shadowSprite.position.y = shadowY;
                const shadowRotation = -effectiveRotation;
                if (sprite.shadowSprite.rotation !== shadowRotation) sprite.shadowSprite.rotation = shadowRotation;
                const shadowAlpha = player.alive ? 0.38 : 0.24;
                if (Math.abs(sprite.shadowSprite.alpha - shadowAlpha) > ALPHA_EPSILON) {
                    sprite.shadowSprite.alpha = shadowAlpha;
                }
            }
        }

        const playerTeamColor = teamColors[player.team_id] || teamColors[0];
        const damageFlashActive = player.alive && sprite._damageFlashUntilMs > frameNowMs;
        const mainBodyColor = damageFlashActive ? 0xFFFFFF : (player.alive ? playerTeamColor : 0x6B7280);
        if (
            sprite._lastTeamId !== player.team_id ||
            sprite._lastAlive !== player.alive ||
            sprite._lastDamageFlashActive !== damageFlashActive
        ) {
            sprite.body.tint = mainBodyColor;
            sprite._lastTeamId = player.team_id;
            sprite._lastAlive = player.alive;
            sprite._lastDamageFlashActive = damageFlashActive;
        }

        if (sprite.localIndicator) {
            const localIndicatorVisible = isLocalSprite && player.alive;
            if (sprite.localIndicator.visible !== localIndicatorVisible) {
                sprite.localIndicator.visible = localIndicatorVisible;
            }
        }

        if (sprite.engineGlow) {
            if (hideRemoteFxByDensity) {
                if (sprite.engineGlow.visible) sprite.engineGlow.visible = false;
            } else {
                const moving = player.velocity_x !== 0 || player.velocity_y !== 0;
                if (player.alive && moving) {
                    if (!sprite.engineGlow.visible) sprite.engineGlow.visible = true;
                    const speed = Math.sqrt(player.velocity_x * player.velocity_x + player.velocity_y * player.velocity_y);
                    const intensity = Math.min(1, speed / 150);
                    const glowAlpha = 0.35 + intensity * 0.45;
                    if (Math.abs(sprite.engineGlow.alpha - glowAlpha) > ALPHA_EPSILON) {
                        sprite.engineGlow.alpha = glowAlpha;
                    }
                    const glowScale = 0.8 + intensity * 0.4;
                    if (Math.abs((sprite.engineGlow.scale.x || 0) - glowScale) > TRANSFORM_EPSILON) {
                        sprite.engineGlow.scale.set(glowScale);
                    }
                } else if (sprite.engineGlow.visible) {
                    sprite.engineGlow.visible = false;
                }
            }
        }

        const playerVisible = player.alive || (player.respawn_timer !== undefined && player.respawn_timer > 0);
        if (sprite.visible !== playerVisible) sprite.visible = playerVisible;
        let playerAlpha = player.alive ? 1 : 0.55;
        if (!isLocalSprite) {
            if (playerLodTier === 'dot') {
                playerAlpha = player.alive ? 0.68 : 0.44;
            } else if (playerLodTier === 'low') {
                playerAlpha = player.alive ? 0.82 : 0.5;
            } else if (playerLodTier === 'medium') {
                playerAlpha = player.alive ? 0.92 : 0.54;
            }
        }
        if (Math.abs(sprite.alpha - playerAlpha) > ALPHA_EPSILON) {
            sprite.alpha = playerAlpha;
        }
        if (!isLocalSprite) {
            let spriteScale = 1;
            if (playerLodTier === 'dot') {
                spriteScale = 0.46;
            } else if (playerLodTier === 'low') {
                spriteScale = 0.7;
            } else if (playerLodTier === 'medium') {
                spriteScale = 0.88;
            }
            if (
                Math.abs((sprite.scale.x || 1) - spriteScale) > TRANSFORM_EPSILON ||
                Math.abs((sprite.scale.y || 1) - spriteScale) > TRANSFORM_EPSILON
            ) {
                sprite.scale.set(spriteScale, spriteScale);
            }
        } else if (
            Math.abs((sprite.scale.x || 1) - 1) > TRANSFORM_EPSILON ||
            Math.abs((sprite.scale.y || 1) - 1) > TRANSFORM_EPSILON
        ) {
            sprite.scale.set(1, 1);
        }

        const localX = Number.isFinite(localAnchorX) ? localAnchorX : targetX;
        const localY = Number.isFinite(localAnchorY) ? localAnchorY : targetY;
        const dx = targetX - localX;
        const dy = targetY - localY;
        const farDetailMode =
            ultraPerformanceMode ||
            totalPlayerCount > HIGH_POPULATION_PLAYER_COUNT ||
            lowLodTier ||
            ((STABLE_MODE_FORCED && !isLocalSprite) || ((dx * dx + dy * dy) > FAR_DETAIL_DISTANCE_SQ));

        if (sprite._lastFarDetail !== farDetailMode || sprite._lastDenseVisualMode !== hideRemoteFxByDensity) {
            sprite._lastFarDetail = farDetailMode;
            sprite._lastDenseVisualMode = hideRemoteFxByDensity;
            if (sprite.usernameText) {
                const usernameVisible = (!farDetailMode && !hideRemoteFxByDensity) || isLocalSprite;
                if (sprite.usernameText.visible !== usernameVisible) {
                    sprite.usernameText.visible = usernameVisible;
                }
            }
            if (sprite.healthBarContainer) {
                const healthVisible = ((!farDetailMode && !hideRemoteFxByDensity) || isLocalSprite) && player.alive;
                if (sprite.healthBarContainer.visible !== healthVisible) {
                    sprite.healthBarContainer.visible = healthVisible;
                }
            }
        }

        const shouldSkipDetailTick = farDetailMode && !isLocalSprite && (frameCounter % detailTickDivisor !== 0);
        if (!shouldSkipDetailTick) {
            updatePlayerGun(sprite, player);
            updatePlayerHealthBar(sprite, player);
            updateShieldVisual(sprite, player.shield_current || 0, player.shield_max || 0);
            updatePlayerReloadArc(sprite, player, hideRemoteFxByDensity, farDetailMode);
            updateStatusEffectIcons(sprite, player, hideRemoteFxByDensity, farDetailMode);
        }

        if (!shouldSkipDetailTick && sprite.usernameText && sprite.usernameText.text !== (player.username || 'Player')) {
            sprite.usernameText.text = player.username || 'Player';
        }

        if (RESPAWN_WORLD_TEXT_ENABLED && !isLocalSprite && !player.alive && player.respawn_timer > 0) {
            if (!sprite.respawnText) {
                const respawnStyle = new PIXI.TextStyle({ fontSize: 14, fill: 0xFFFFFF, stroke: 0x000000, strokeThickness: 3, fontWeight: 'bold' });
                sprite.respawnText = new PIXI.Text('', respawnStyle);
                sprite.respawnText.anchor.set(0.5);
                sprite.respawnText.position.y = PLAYER_RADIUS + 10;
                sprite.addChild(sprite.respawnText);
            }
            sprite.respawnText.text = Math.ceil(player.respawn_timer) + 's';
            if (!sprite.respawnText.visible) sprite.respawnText.visible = true;
        } else if (sprite.respawnText && sprite.respawnText.visible) {
            sprite.respawnText.visible = false;
        }

        if (player.speed_boost_remaining > 0 && player.alive) {
            if (!sprite.speedBoostEffect && !farDetailMode) {
                sprite.speedBoostEffect = createSpeedBoostEffect();
                sprite.addChildAt(sprite.speedBoostEffect, 0);
            }
            if (sprite.speedBoostEffect) {
                const speedBoostVisible = !hideRemoteFxByDensity && !farDetailMode && !shouldSkipDetailTick;
                if (sprite.speedBoostEffect.visible !== speedBoostVisible) {
                    sprite.speedBoostEffect.visible = speedBoostVisible;
                }
            }
        } else if (sprite.speedBoostEffect && sprite.speedBoostEffect.visible) {
            sprite.speedBoostEffect.visible = false;
        }

        if (player.invulnerable_remaining > 0 && player.alive) {
            if (!sprite.dodgeGlowEffect && !farDetailMode) {
                sprite.dodgeGlowEffect = createDodgeGlowEffect();
                sprite.addChildAt(sprite.dodgeGlowEffect, 0);
            }
            if (sprite.dodgeGlowEffect) {
                const glowVisible = !hideRemoteFxByDensity && !farDetailMode;
                if (sprite.dodgeGlowEffect.visible !== glowVisible) {
                    sprite.dodgeGlowEffect.visible = glowVisible;
                }
                if (glowVisible) {
                    const pulse = 0.6 + 0.4 * Math.sin(frameNowMs * 0.012);
                    const fadeOut = Math.min(1, player.invulnerable_remaining * 4);
                    sprite.dodgeGlowEffect.alpha = pulse * fadeOut * 0.7;
                    const glowScale = 1.0 + 0.15 * Math.sin(frameNowMs * 0.008);
                    sprite.dodgeGlowEffect.scale.set(glowScale);
                }
            }
        } else if (sprite.dodgeGlowEffect && sprite.dodgeGlowEffect.visible) {
            sprite.dodgeGlowEffect.visible = false;
        }

        if (player.weapon_swap_progress > 0 && player.alive) {
            if (!sprite.weaponSwapEffect && !farDetailMode) {
                sprite.weaponSwapEffect = createWeaponSwapEffect();
                sprite.addChild(sprite.weaponSwapEffect);
            }
            if (sprite.weaponSwapEffect) {
                const swapVisible = !hideRemoteFxByDensity && !farDetailMode;
                if (sprite.weaponSwapEffect.visible !== swapVisible) {
                    sprite.weaponSwapEffect.visible = swapVisible;
                }
                if (swapVisible) {
                    const swapPunchRemaining = Math.max(
                        0,
                        (Number(sprite._weaponSwapPunchUntilMs) || 0) - frameNowMs
                    );
                    const swapPunchDuration = Math.max(1, Number(sprite._weaponSwapPunchDurationMs) || 170);
                    const swapPunchProgress = 1 - (swapPunchRemaining / swapPunchDuration);
                    const swapPunch = swapPunchRemaining > 0
                        ? Math.sin(Math.max(0, Math.min(1, swapPunchProgress)) * Math.PI)
                        : 0;
                    sprite.weaponSwapEffect.rotation += 0.15 + swapPunch * 0.06;
                    sprite.weaponSwapEffect.alpha = Math.min(
                        1,
                        0.42 + 0.58 * player.weapon_swap_progress + swapPunch * 0.28
                    );
                    const swapScale = 1 + swapPunch * 0.45;
                    if (
                        Math.abs((sprite.weaponSwapEffect.scale.x || 1) - swapScale) > TRANSFORM_EPSILON ||
                        Math.abs((sprite.weaponSwapEffect.scale.y || 1) - swapScale) > TRANSFORM_EPSILON
                    ) {
                        sprite.weaponSwapEffect.scale.set(swapScale);
                    }
                }
            }
        } else if (sprite.weaponSwapEffect) {
            if (sprite.weaponSwapEffect.visible) sprite.weaponSwapEffect.visible = false;
            sprite.weaponSwapEffect.rotation = 0;
            if (
                Math.abs((sprite.weaponSwapEffect.scale.x || 1) - 1) > TRANSFORM_EPSILON ||
                Math.abs((sprite.weaponSwapEffect.scale.y || 1) - 1) > TRANSFORM_EPSILON
            ) {
                sprite.weaponSwapEffect.scale.set(1);
            }
        }

        const carryingTeam = player.is_carrying_flag_team_id > 0 && player.alive ? player.is_carrying_flag_team_id : 0;
        if (carryingTeam > 0) {
            if (!sprite.carriedFlagSprite || sprite._lastCarryingFlagTeam !== carryingTeam) {
                if (sprite.carriedFlagSprite) {
                    sprite.removeChild(sprite.carriedFlagSprite);
                    sprite.carriedFlagSprite.destroy({ children: true });
                }
                sprite.carriedFlagSprite = buildCarriedFlagSprite(carryingTeam);
                sprite.addChild(sprite.carriedFlagSprite);
                sprite._lastCarryingFlagTeam = carryingTeam;
            }
            const carriedFlagVisible = !hideRemoteFxByDensity && !farDetailMode && !shouldSkipDetailTick;
            if (sprite.carriedFlagSprite.visible !== carriedFlagVisible) {
                sprite.carriedFlagSprite.visible = carriedFlagVisible;
            }
        } else if (sprite.carriedFlagSprite) {
            if (sprite.carriedFlagSprite.visible) sprite.carriedFlagSprite.visible = false;
            sprite._lastCarryingFlagTeam = 0;
        }
    }

    function updatePlayerGun(sprite, player) {
        const ctx = getCtx();
        const { GP, renderAssetCache, weaponColors, PIXI, ALPHA_EPSILON, frameNowMs } = ctx;
        const gun = sprite.gun;
        if (!gun) return;
        if (!player.alive) {
            if (gun.visible) gun.visible = false;
            if (
                Math.abs((gun.scale.x || 1) - 1) > ALPHA_EPSILON ||
                Math.abs((gun.scale.y || 1) - 1) > ALPHA_EPSILON
            ) {
                gun.scale.set(1, 1);
            }
            return;
        }

        if (!gun.visible) gun.visible = true;
        const weapon = player.weapon ?? GP.WeaponType.Pistol;
        if (sprite._lastWeapon !== weapon) {
            gun.texture = renderAssetCache.gunTextures.get(weapon) || renderAssetCache.gunTextures.get(GP.WeaponType.Pistol);
            if (sprite._lastWeapon !== null && sprite._lastWeapon !== undefined) {
                const punchDurationMs = 170;
                sprite._weaponSwapPunchDurationMs = punchDurationMs;
                sprite._weaponSwapPunchUntilMs = frameNowMs + punchDurationMs;
            }
            sprite._lastWeapon = weapon;
        }

        const baseTint = weaponColors[weapon] || 0xFFFFFF;
        const hasDamageBoost = player.damage_boost_remaining > 0;
        if (hasDamageBoost) {
            const pulse = Math.sin(frameNowMs * 0.01) * 0.25 + 0.75;
            const boostedTint = PIXI.utils.rgb2hex([1, pulse, pulse]);
            if (gun.tint !== boostedTint) {
                gun.tint = boostedTint;
            }
            const boostedAlpha = 0.85 + 0.15 * pulse;
            if (Math.abs(gun.alpha - boostedAlpha) > ALPHA_EPSILON) {
                gun.alpha = boostedAlpha;
            }
        } else {
            if (gun.tint !== baseTint) gun.tint = baseTint;
            if (gun.alpha !== 1) gun.alpha = 1;
        }

        const punchRemaining = Math.max(0, (Number(sprite._weaponSwapPunchUntilMs) || 0) - frameNowMs);
        const punchDuration = Math.max(1, Number(sprite._weaponSwapPunchDurationMs) || 170);
        const punchProgress = 1 - (punchRemaining / punchDuration);
        const swapPunch = punchRemaining > 0
            ? Math.sin(Math.max(0, Math.min(1, punchProgress)) * Math.PI)
            : 0;
        const reloadProgress = Math.max(0, Math.min(1, Number(player.reload_progress) || 0));
        const reloadTilt = reloadProgress > 0 && reloadProgress <= 1
            ? Math.sin(reloadProgress * Math.PI) * 0.26
            : 0;
        const targetGunRotation = (-Math.PI / 2) + reloadTilt;
        if (Math.abs((Number(gun.rotation) || 0) - targetGunRotation) > 0.001) {
            gun.rotation = targetGunRotation;
        }
        const targetScaleX = 1 + swapPunch * 0.2;
        const targetScaleY = 1 - swapPunch * 0.09;
        if (
            Math.abs((gun.scale.x || 1) - targetScaleX) > ALPHA_EPSILON ||
            Math.abs((gun.scale.y || 1) - targetScaleY) > ALPHA_EPSILON
        ) {
            gun.scale.set(targetScaleX, targetScaleY);
        }
        if (swapPunch > 0) {
            const punchedAlpha = Math.min(1, (Number(gun.alpha) || 1) + swapPunch * 0.14);
            if (Math.abs((Number(gun.alpha) || 1) - punchedAlpha) > ALPHA_EPSILON) {
                gun.alpha = punchedAlpha;
            }
        }
        sprite._lastDamageBoost = hasDamageBoost;
    }

    function updatePlayerReloadArc(sprite, player, hideRemoteFxByDensity, farDetailMode) {
        const ctx = getCtx();
        const { myPlayerId, frameNowMs } = ctx;
        const arc = sprite.reloadArc;
        if (!arc) return;
        const isLocal = sprite.playerId === myPlayerId;
        const reloadProgress = Number(player.reload_progress);
        const active = player.alive && Number.isFinite(reloadProgress) && reloadProgress >= 0 && reloadProgress <= 1;
        const visible = active && !hideRemoteFxByDensity && !farDetailMode;
        if (!visible) {
            if (arc.visible) arc.visible = false;
            return;
        }
        const progress = Math.max(0, Math.min(1, reloadProgress));
        const pulse = 0.72 + 0.28 * Math.sin(frameNowMs * 0.018);
        const start = -Math.PI / 2;
        const end = start + (Math.PI * 2 * progress);
        arc.visible = true;
        arc.clear();
        arc.lineStyle(2.1, isLocal ? 0x67E8F9 : 0xC4B5FD, 0.92 * pulse);
        arc.arc(0, 0, 14, start, end);
    }

    function updateStatusEffectIcons(sprite, player, hideRemoteFxByDensity, farDetailMode) {
        const ctx = getCtx();
        const { frameNowMs } = ctx;
        const iconContainer = sprite.statusIconContainer;
        if (!iconContainer) return;
        const speedIcon = sprite.speedStatusIcon;
        const damageIcon = sprite.damageStatusIcon;
        const shieldIcon = sprite.shieldStatusIcon;
        if (!speedIcon || !damageIcon || !shieldIcon) return;

        const hasSpeed = Number(player.speed_boost_remaining) > 0.01;
        const hasDamage = Number(player.damage_boost_remaining) > 0.01;
        const hasShield = Number(player.invulnerable_remaining) > 0.01;
        const anyActive = player.alive && (hasSpeed || hasDamage || hasShield);
        const show = anyActive && !hideRemoteFxByDensity && !farDetailMode;
        if (!show) {
            if (iconContainer.visible) iconContainer.visible = false;
            return;
        }

        const pulse = 0.72 + 0.28 * Math.sin(frameNowMs * 0.014);
        iconContainer.visible = true;
        speedIcon.visible = hasSpeed;
        damageIcon.visible = hasDamage;
        shieldIcon.visible = hasShield;
        speedIcon.tint = 0x22D3EE;
        damageIcon.tint = 0xF87171;
        shieldIcon.tint = 0xFDE047;
        speedIcon.alpha = hasSpeed ? pulse : 0;
        damageIcon.alpha = hasDamage ? pulse : 0;
        shieldIcon.alpha = hasShield ? pulse : 0;
    }

    function updatePlayerHealthBar(sprite, player) {
        const ctx = getCtx();
        const { myPlayerId, renderAssetCache, frameNowMs } = ctx;
        if (!sprite.healthFg) return;
        const alive = !!player.alive;
        if (sprite.healthBarContainer) {
            const hideByDensity = (sprite._lastFarDetail || sprite._lastDenseVisualMode) && sprite.playerId !== myPlayerId;
            sprite.healthBarContainer.visible = alive && !hideByDensity;
        }
        if (!alive) {
            sprite._lastHealthAlive = false;
            sprite._lastHealthValue = Number.isFinite(player.health) ? Number(player.health) : sprite._lastHealthValue;
            return;
        }

        const currentHealthValue = Number(player.health);
        if (Number.isFinite(currentHealthValue)) {
            const previousHealthValue = Number(sprite._lastHealthValue);
            if (
                Number.isFinite(previousHealthValue) &&
                currentHealthValue < previousHealthValue &&
                player.alive
            ) {
                sprite._damageFlashUntilMs = frameNowMs + 60;
            }
            sprite._lastHealthValue = currentHealthValue;
        }

        const healthPercent = Math.max(0, Math.min(1, player.health / Math.max(1, player.max_health)));
        const barIndex = Math.round(healthPercent * 10);
        const shouldSwap = sprite._lastHealthAlive !== alive || sprite._healthBarIndex !== barIndex;

        if (shouldSwap) {
            const tex = renderAssetCache.healthBarTextures[barIndex];
            if (tex) {
                sprite.healthFg.texture = tex;
            }
            sprite._healthBarIndex = barIndex;
            sprite._lastHealthPercent = healthPercent;
            sprite._lastHealthAlive = alive;
        }

        if (healthPercent < 0.3) {
            const pulse = Math.sin(frameNowMs * 0.01) * 0.2 + 0.8;
            sprite.healthFg.alpha = pulse;
        } else {
            sprite.healthFg.alpha = 1;
        }
    }

    function updateShieldVisual(sprite, current, max) {
        const ctx = getCtx();
        const { myPlayerId, frameNowMs } = ctx;
        if (!sprite.shieldVisual) return;

        const hideByDensity = sprite.playerId !== myPlayerId && !!sprite._lastDenseVisualMode;

        if (current <= 0 || max <= 0 || hideByDensity) {
            sprite.shieldVisual.visible = false;
            if (sprite.shieldImpactRing) sprite.shieldImpactRing.visible = false;
            if (sprite.shieldCrackOverlay) sprite.shieldCrackOverlay.visible = false;
            sprite._lastShieldCurrent = current;
            sprite._lastShieldMax = max;
            return;
        }

        const prevShieldCurrent = Number(sprite._lastShieldCurrent);
        if (Number.isFinite(prevShieldCurrent) && current < prevShieldCurrent) {
            const maxShield = Math.max(1, Number(max) || 1);
            const hitStrength = Math.max(0.15, Math.min(1.35, (prevShieldCurrent - current) / maxShield * 5));
            sprite._shieldImpactUntilMs = frameNowMs + 220;
            sprite._shieldImpactStrength = hitStrength;
            if (current <= 0 && prevShieldCurrent > 0) {
                sprite._shieldBreakFlashUntilMs = frameNowMs + 320;
            }
        }

        const shieldPercent = Math.max(0, Math.min(1, current / max));
        sprite.shieldVisual.visible = true;
        sprite.shieldVisual.scale.set(1 + shieldPercent * 0.35);
        const shimmer = Math.sin(frameNowMs * 0.003) * 0.08;
        sprite.shieldVisual.alpha = 0.18 + shieldPercent * 0.35 + shimmer;
        sprite.shieldVisual.tint = shieldPercent > 0.5 ? 0x00BFFF : 0x60A5FA;

        if (sprite.shieldCrackOverlay) {
            const crackAlpha = Math.max(0, (1 - shieldPercent) * 0.7);
            if (crackAlpha > 0.05) {
                const crack = sprite.shieldCrackOverlay;
                crack.visible = true;
                crack.clear();
                crack.lineStyle(1.2, 0xE0F2FE, crackAlpha);
                crack.drawCircle(0, 0, PLAYER_RADIUS + 5);
                for (let i = 0; i < 4; i += 1) {
                    const angle = ((i * Math.PI * 2) / 4) + (i * 0.35);
                    const inner = PLAYER_RADIUS - 2;
                    const outer = PLAYER_RADIUS + 9 + (i % 2) * 3;
                    crack.moveTo(Math.cos(angle) * inner, Math.sin(angle) * inner);
                    crack.lineTo(Math.cos(angle + 0.25) * outer, Math.sin(angle + 0.25) * outer);
                }
            } else {
                sprite.shieldCrackOverlay.visible = false;
            }
        }

        if (sprite.shieldImpactRing) {
            const ring = sprite.shieldImpactRing;
            const remainingMs = Math.max(
                0,
                Math.max(
                    Number(sprite._shieldImpactUntilMs) || 0,
                    Number(sprite._shieldBreakFlashUntilMs) || 0
                ) - frameNowMs
            );
            if (remainingMs <= 0) {
                ring.visible = false;
            } else {
                const durationMs = (Number(sprite._shieldBreakFlashUntilMs) || 0) > frameNowMs ? 320 : 220;
                const progress = 1 - (remainingMs / durationMs);
                const impactStrength = Math.max(0.15, Number(sprite._shieldImpactStrength) || 0.3);
                const radius = PLAYER_RADIUS + 8 + progress * (18 + impactStrength * 10);
                const alpha = Math.max(0, (1 - progress) * (0.45 + impactStrength * 0.45));
                ring.visible = true;
                ring.clear();
                ring.lineStyle(2.2 + impactStrength * 1.8, 0xE0F2FE, alpha);
                ring.drawCircle(0, 0, radius);
            }
        }

        sprite._lastShieldCurrent = current;
        sprite._lastShieldMax = max;
    }

    function createProjectileSprite(projectile) {
        const ctx = getCtx();
        const { PIXI, GP, renderAssetCache, weaponColors,
                projectileSpritePool, projectileSpritePoolStats } = ctx;
        const weaponType = projectile.weapon_type ?? GP.WeaponType.Pistol;
        const texture = renderAssetCache.projectileTextures.get(weaponType) || renderAssetCache.projectileTextures.get(GP.WeaponType.Pistol);
        const reusedFromPool = projectileSpritePool.length > 0;
        const sprite = reusedFromPool ? projectileSpritePool.pop() : new PIXI.Sprite(texture);
        if (reusedFromPool) {
            projectileSpritePoolStats.reused += 1;
        } else {
            projectileSpritePoolStats.created += 1;
        }
        if (sprite.texture !== texture) {
            sprite.texture = texture;
        }
        sprite.anchor.set(0.5);
        sprite.projectileId = projectile.id;
        sprite.weaponType = weaponType;
        sprite.tint = weaponColors[weaponType] || 0xFFFFFF;
        sprite.alpha = 0.92;
        sprite.blendMode = weaponType === GP.WeaponType.Sniper ? PIXI.BLEND_MODES.ADD : PIXI.BLEND_MODES.NORMAL;
        sprite._lastVelX = Number.NaN;
        sprite._lastVelY = Number.NaN;
        if (!Array.isArray(sprite._trailSprites) || sprite._trailSprites.length !== 2) {
            sprite._trailSprites = [];
            for (let i = 0; i < 2; i += 1) {
                const trail = new PIXI.Sprite(texture);
                trail.anchor.set(0.5);
                trail.visible = false;
                trail.alpha = i === 0 ? 0.4 : 0.2;
                trail.blendMode = PIXI.BLEND_MODES.NORMAL;
                trail.tint = sprite.tint;
                trail._trailIndex = i;
                sprite.addChild(trail);
                sprite._trailSprites.push(trail);
            }
        } else {
            for (const trail of sprite._trailSprites) {
                if (trail.texture !== texture) {
                    trail.texture = texture;
                }
                trail.tint = sprite.tint;
                trail.visible = false;
            }
        }

        if (!sprite._projectileHalo) {
            const haloTexture = renderAssetCache.engineGlowTexture || texture;
            const halo = new PIXI.Sprite(haloTexture);
            halo.anchor.set(0.5);
            halo.visible = false;
            halo.alpha = 0.24;
            halo.blendMode = PIXI.BLEND_MODES.ADD;
            halo.tint = sprite.tint;
            sprite.addChild(halo);
            sprite._projectileHalo = halo;
        } else if (renderAssetCache.engineGlowTexture && sprite._projectileHalo.texture !== renderAssetCache.engineGlowTexture) {
            sprite._projectileHalo.texture = renderAssetCache.engineGlowTexture;
            sprite._projectileHalo.visible = false;
        }
        return sprite;
    }

    function releaseProjectileSprite(sprite) {
        if (!sprite) return;
        const ctx = getCtx();
        const { GP, PIXI, weaponColors, projectileSpritePool, projectileSpritePoolStats,
                PROJECTILE_SPRITE_POOL_LIMIT } = ctx;
        projectileSpritePoolStats.released += 1;
        sprite.visible = false;
        sprite.projectileId = null;
        sprite.weaponType = GP.WeaponType.Pistol;
        sprite.rotation = 0;
        sprite.alpha = 0.92;
        sprite.scale.set(1, 1);
        sprite.tint = weaponColors[GP.WeaponType.Pistol] || 0xFFFFFF;
        sprite.blendMode = PIXI.BLEND_MODES.NORMAL;
        sprite._lastVelX = Number.NaN;
        sprite._lastVelY = Number.NaN;
        if (Array.isArray(sprite._trailSprites)) {
            for (const trail of sprite._trailSprites) {
                trail.visible = false;
                trail.position.set(0, 0);
                trail.scale.set(1, 1);
            }
        }
        if (sprite._projectileHalo) {
            sprite._projectileHalo.visible = false;
            sprite._projectileHalo.position.set(0, 0);
            sprite._projectileHalo.scale.set(1, 1);
        }

        if (projectileSpritePool.length < PROJECTILE_SPRITE_POOL_LIMIT) {
            projectileSpritePool.push(sprite);
        } else {
            projectileSpritePoolStats.destroyed += 1;
            sprite.destroy({ children: true });
        }
    }

    function removeProjectileClientState(projectileId) {
        if (projectileId === undefined || projectileId === null) return;
        const ctx = getCtx();
        const { projectileSprites, projectileContainer, projectiles, serverUpdates,
                cullWorkerResult, projectileWorkerCullGraceUntil } = ctx;
        const sprite = projectileSprites.get(projectileId);
        if (sprite) {
            projectileContainer.removeChild(sprite);
            releaseProjectileSprite(sprite);
            projectileSprites.delete(projectileId);
        }
        projectiles.delete(projectileId);
        for (let i = 0; i < serverUpdates.length; i += 1) {
            const snapshot = serverUpdates[i];
            if (snapshot && snapshot.projectiles) {
                snapshot.projectiles.delete(projectileId);
            }
        }
        if (cullWorkerResult && cullWorkerResult.projectileSet) {
            cullWorkerResult.projectileSet.delete(projectileId);
        }
        projectileWorkerCullGraceUntil.delete(projectileId);
    }

    function updateProjectileSprite(
        sprite,
        projectile,
        px = null,
        py = null,
        skipVisibilityCheck = false,
        denseVisualMode = false,
        lodTier = 'full'
    ) {
        const ctx = getCtx();
        const { GP, PIXI, ALPHA_EPSILON, TRANSFORM_EPSILON, PROJECTILE_CULL_MARGIN_WORLD,
                ultraPerformanceMode, STABLE_MODE_FORCED, frameNowMs,
                isWorldPointVisible } = ctx;

        const worldX = px !== null ? px : (projectile.render_x !== undefined ? projectile.render_x : projectile.x);
        const worldY = py !== null ? py : (projectile.render_y !== undefined ? projectile.render_y : projectile.y);
        if (sprite.position.x !== worldX) sprite.position.x = worldX;
        if (sprite.position.y !== worldY) sprite.position.y = worldY;

        if (!skipVisibilityCheck && !isWorldPointVisible(worldX, worldY, PROJECTILE_CULL_MARGIN_WORLD)) {
            if (sprite.visible) sprite.visible = false;
            return false;
        }

        if (!sprite.visible) sprite.visible = true;
        if (projectile.velocity_x !== undefined && projectile.velocity_y !== undefined) {
            const vx = projectile.velocity_x || 0;
            const vy = projectile.velocity_y || 0;
            if (vx !== sprite._lastVelX || vy !== sprite._lastVelY) {
                sprite.rotation = Math.atan2(vy, vx);
                sprite._lastVelX = vx;
                sprite._lastVelY = vy;
            }
        }

        const desiredBlendMode =
            !denseVisualMode && lodTier === 'full' && sprite.weaponType === GP.WeaponType.Sniper
                ? PIXI.BLEND_MODES.ADD
                : PIXI.BLEND_MODES.NORMAL;
        if (sprite.blendMode !== desiredBlendMode) {
            sprite.blendMode = desiredBlendMode;
        }

        const dotLod = lodTier === 'dot';
        const lowLod = lodTier === 'low';
        const mediumLod = lodTier === 'medium';
        let targetScale = 1;
        if (dotLod) {
            targetScale = 0.54;
        } else if (lowLod) {
            targetScale = 0.72;
        } else if (mediumLod) {
            targetScale = 0.86;
        }
        const vx = Number(projectile.velocity_x) || 0;
        const vy = Number(projectile.velocity_y) || 0;
        const projectileSpeed = Math.hypot(vx, vy);
        let scaleX = targetScale;
        let scaleY = targetScale;
        // Full-detail projectiles get a stretched silhouette for a cheap trail-like read.
        if (
            !dotLod &&
            !lowLod &&
            !mediumLod &&
            !ultraPerformanceMode &&
            !STABLE_MODE_FORCED &&
            !denseVisualMode &&
            projectileSpeed > 0
        ) {
            const stretch = Math.min(2.4, 1 + projectileSpeed / 480);
            scaleX *= stretch;
            scaleY *= 0.78;
        }
        if (
            Math.abs((sprite.scale.x || 1) - scaleX) > TRANSFORM_EPSILON ||
            Math.abs((sprite.scale.y || 1) - scaleY) > TRANSFORM_EPSILON
        ) {
            sprite.scale.set(scaleX, scaleY);
        }

        const fullVisualFx =
            !dotLod &&
            !lowLod &&
            !mediumLod &&
            !ultraPerformanceMode &&
            !STABLE_MODE_FORCED &&
            !denseVisualMode;

        const showTrailFx = fullVisualFx && projectileSpeed > 40;
        if (Array.isArray(sprite._trailSprites)) {
            const trailStep = Math.min(18, Math.max(5, projectileSpeed * 0.018));
            for (let i = 0; i < sprite._trailSprites.length; i += 1) {
                const trail = sprite._trailSprites[i];
                if (!showTrailFx) {
                    if (trail.visible) trail.visible = false;
                    continue;
                }
                if (!trail.visible) trail.visible = true;
                trail.tint = sprite.tint;
                const alpha = i === 0 ? 0.4 : 0.2;
                if (Math.abs(trail.alpha - alpha) > ALPHA_EPSILON) {
                    trail.alpha = alpha;
                }
                trail.position.set(-(i + 1) * trailStep, 0);
                const trailScaleX = Math.max(0.55, scaleX * (0.82 - i * 0.2));
                const trailScaleY = Math.max(0.48, scaleY * (0.74 - i * 0.16));
                trail.scale.set(trailScaleX, trailScaleY);
            }
        }

        const haloEnabledWeapon = sprite.weaponType === GP.WeaponType.Sniper || sprite.weaponType === GP.WeaponType.Rifle;
        const showHaloFx = fullVisualFx && haloEnabledWeapon;
        if (sprite._projectileHalo) {
            if (!showHaloFx) {
                if (sprite._projectileHalo.visible) sprite._projectileHalo.visible = false;
            } else {
                if (!sprite._projectileHalo.visible) sprite._projectileHalo.visible = true;
                const idSeed = typeof sprite.projectileId === 'string' ? sprite.projectileId.length : (Number(sprite.projectileId) || 0);
                const haloPulse = 0.18 + 0.12 * (0.5 + 0.5 * Math.sin(frameNowMs * 0.028 + idSeed));
                sprite._projectileHalo.alpha = haloPulse;
                sprite._projectileHalo.tint = sprite.tint;
                const haloScale = Math.max(1.4, 1.6 + Math.min(0.9, projectileSpeed / 800));
                sprite._projectileHalo.scale.set(haloScale);
            }
        }

        if (ultraPerformanceMode || STABLE_MODE_FORCED || denseVisualMode) {
            const denseAlpha = dotLod ? 0.6 : (lowLod ? 0.74 : (mediumLod ? 0.82 : 0.88));
            if (Math.abs(sprite.alpha - denseAlpha) > ALPHA_EPSILON) {
                sprite.alpha = denseAlpha;
            }
            return true;
        }

        if (dotLod) {
            if (Math.abs(sprite.alpha - 0.62) > ALPHA_EPSILON) {
                sprite.alpha = 0.62;
            }
            return true;
        }
        if (lowLod) {
            if (Math.abs(sprite.alpha - 0.74) > ALPHA_EPSILON) {
                sprite.alpha = 0.74;
            }
            return true;
        }
        if (mediumLod) {
            if (Math.abs(sprite.alpha - 0.84) > ALPHA_EPSILON) {
                sprite.alpha = 0.84;
            }
            return true;
        }

        if (sprite.weaponType === GP.WeaponType.Sniper || sprite.weaponType === GP.WeaponType.Rifle) {
            const idSeed = typeof sprite.projectileId === 'string' ? sprite.projectileId.length : (Number(sprite.projectileId) || 0);
            const pulse = Math.sin(frameNowMs * 0.02 + idSeed) * 0.08 + 0.92;
            if (Math.abs(sprite.alpha - pulse) > ALPHA_EPSILON) {
                sprite.alpha = pulse;
            }
        } else {
            if (Math.abs(sprite.alpha - 0.9) > ALPHA_EPSILON) {
                sprite.alpha = 0.9;
            }
        }
        return true;
    }

    return {
        createPlayerSprite,
        hidePlayerSprite,
        destroyPlayerSprite,
        removePlayerClientState,
        updatePlayerSprite,
        updatePlayerGun,
        updatePlayerHealthBar,
        updateShieldVisual,
        createProjectileSprite,
        releaseProjectileSprite,
        removeProjectileClientState,
        updateProjectileSprite,
    };
}
