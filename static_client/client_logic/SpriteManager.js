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
        container._lastDamageBoost = false;
        container._lastHealthPercent = null;
        container._lastHealthAlive = null;
        container._lastShieldCurrent = null;
        container._lastShieldMax = null;
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
                createWeaponSwapEffect, buildCarriedFlagSprite } = ctx;

        const targetX = player.render_x !== undefined ? player.render_x : player.x;
        const targetY = player.render_y !== undefined ? player.render_y : player.y;
        if (sprite.position.x !== targetX) sprite.position.x = targetX;
        if (sprite.position.y !== targetY) sprite.position.y = targetY;

        const effectiveRotation = (player.render_rotation !== undefined ? player.render_rotation : player.rotation) + (Math.PI / 2);
        if (sprite.rotation !== effectiveRotation) sprite.rotation = effectiveRotation;

        const isLocalSprite = sprite.playerId === myPlayerId;
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
        const mainBodyColor = player.alive ? playerTeamColor : 0x6B7280;
        if (sprite._lastTeamId !== player.team_id || sprite._lastAlive !== player.alive) {
            sprite.body.tint = mainBodyColor;
            sprite._lastTeamId = player.team_id;
            sprite._lastAlive = player.alive;
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
                    sprite.weaponSwapEffect.rotation += 0.15;
                    sprite.weaponSwapEffect.alpha = 0.5 + 0.5 * player.weapon_swap_progress;
                }
            }
        } else if (sprite.weaponSwapEffect) {
            if (sprite.weaponSwapEffect.visible) sprite.weaponSwapEffect.visible = false;
            sprite.weaponSwapEffect.rotation = 0;
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
            return;
        }

        if (!gun.visible) gun.visible = true;
        const weapon = player.weapon ?? GP.WeaponType.Pistol;
        if (sprite._lastWeapon !== weapon) {
            gun.texture = renderAssetCache.gunTextures.get(weapon) || renderAssetCache.gunTextures.get(GP.WeaponType.Pistol);
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
        sprite._lastDamageBoost = hasDamageBoost;
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
            return;
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
            sprite._lastShieldCurrent = current;
            sprite._lastShieldMax = max;
            return;
        }

        const shieldPercent = Math.max(0, Math.min(1, current / max));
        sprite.shieldVisual.visible = true;
        sprite.shieldVisual.scale.set(1 + shieldPercent * 0.35);
        const shimmer = Math.sin(frameNowMs * 0.003) * 0.08;
        sprite.shieldVisual.alpha = 0.18 + shieldPercent * 0.35 + shimmer;
        sprite.shieldVisual.tint = shieldPercent > 0.5 ? 0x00BFFF : 0x60A5FA;

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
        if (
            Math.abs((sprite.scale.x || 1) - targetScale) > TRANSFORM_EPSILON ||
            Math.abs((sprite.scale.y || 1) - targetScale) > TRANSFORM_EPSILON
        ) {
            sprite.scale.set(targetScale, targetScale);
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
