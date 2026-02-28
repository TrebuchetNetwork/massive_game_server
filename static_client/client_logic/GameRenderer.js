/**
 * GameRenderer.js - Rendering helper functions extracted from client.html
 *
 * Contains sprite creation, visual effects, pickup/flag rendering,
 * starfield, fog-of-war, health vignette, zone rendering, and
 * various rendering utility functions.
 */

export function createGameRenderer({
    PIXI,
    GP,
    PLAYER_RADIUS,
    teamColors,
    weaponNames,
    weaponColors,
    pickupColors,
}) {
    // ── Drawing helpers ──────────────────────────────────────────────

    function drawRegularPolygon(graphics, x, y, radius, sides) {
        const angle = (Math.PI * 2) / sides;
        const points = [];
        for (let i = 0; i < sides; i++) {
            points.push(
                x + radius * Math.cos(angle * i - Math.PI / 2),
                y + radius * Math.sin(angle * i - Math.PI / 2)
            );
        }
        graphics.drawPolygon(points);
    }

    function drawStar(graphics, x, y, points, outerRadius, innerRadius) {
        const angle = Math.PI / points;
        const polygon = [];
        for (let i = 0; i < points * 2; i++) {
            const radius = i % 2 === 0 ? outerRadius : innerRadius;
            polygon.push(
                x + radius * Math.cos(angle * i - Math.PI / 2),
                y + radius * Math.sin(angle * i - Math.PI / 2)
            );
        }
        graphics.drawPolygon(polygon);
    }

    function interpolateColor(color1, color2, factor) {
        const c1 = PIXI.Color.shared.setValue(color1).toRgbArray();
        const c2 = PIXI.Color.shared.setValue(color2).toRgbArray();
        const r = Math.floor(c1[0] * 255 * (1 - factor) + c2[0] * 255 * factor);
        const g = Math.floor(c1[1] * 255 * (1 - factor) + c2[1] * 255 * factor);
        const b = Math.floor(c1[2] * 255 * (1 - factor) + c2[2] * 255 * factor);
        return (r << 16) | (g << 8) | b;
    }

    function mixColors(color1, color2, amount) {
        const c1 = PIXI.Color.shared.setValue(color1).toRgbArray();
        const c2 = PIXI.Color.shared.setValue(color2).toRgbArray();
        const r = c1[0] * (1 - amount) + c2[0] * amount;
        const g = c1[1] * (1 - amount) + c2[1] * amount;
        const b = c1[2] * (1 - amount) + c2[2] * amount;
        return PIXI.Color.shared.setValue([r, g, b]).toNumber();
    }

    // ── Visual effect factories ──────────────────────────────────────

    function createSpeedBoostEffect() {
        const effect = new PIXI.Container();
        for (let i = 0; i < 3; i++) {
            const trail = new PIXI.Graphics();
            trail.beginFill(0x00FFFF, 0.3);
            trail.drawRect(-2, -PLAYER_RADIUS * (1.5 + i * 0.3), 4, PLAYER_RADIUS * 0.5);
            trail.endFill();
            trail.rotation = (i - 1) * 0.2;
            effect.addChild(trail);
        }
        const particleContainer = new PIXI.Container();
        effect.addChild(particleContainer);
        effect.particleContainer = particleContainer;
        return effect;
    }

    function createDodgeGlowEffect() {
        const effect = new PIXI.Graphics();
        effect.beginFill(0x88CCFF, 0.15);
        effect.drawCircle(0, 0, PLAYER_RADIUS * 2.0);
        effect.endFill();
        effect.beginFill(0xAADDFF, 0.25);
        effect.drawCircle(0, 0, PLAYER_RADIUS * 1.5);
        effect.endFill();
        effect.lineStyle(2, 0xFFFFFF, 0.5);
        effect.drawCircle(0, 0, PLAYER_RADIUS * 1.15);
        effect.blendMode = PIXI.BLEND_MODES.ADD;
        return effect;
    }

    function createWeaponSwapEffect() {
        const effect = new PIXI.Graphics();
        const r = PLAYER_RADIUS * 0.55;
        effect.lineStyle(2.5, 0xFFD700, 0.9);
        effect.moveTo(-r, -r * 0.3); effect.lineTo(r, -r * 0.3);
        effect.lineTo(r * 0.55, -r * 0.7);
        effect.moveTo(r, -r * 0.3); effect.lineTo(r * 0.55, r * 0.1);
        effect.moveTo(r, r * 0.3); effect.lineTo(-r, r * 0.3);
        effect.lineTo(-r * 0.55, -r * 0.1);
        effect.moveTo(-r, r * 0.3); effect.lineTo(-r * 0.55, r * 0.7);
        effect.position.set(0, -PLAYER_RADIUS - 4);
        return effect;
    }

    // ── Pickup sprite factory ────────────────────────────────────────

    function createPickupSprite(pickup) {
        const container = new PIXI.Container();
        container.pickupId = pickup.id;

        const pickupConfigs = {
            [GP.PickupType.Health]:      { color: 0x10B981, icon: '+',  shape: 'cross',   pulseColor: 0x34D399 },
            [GP.PickupType.Ammo]:        { color: 0xF59E0B, icon: 'o',  shape: 'hexagon', pulseColor: 0xFBBF24 },
            [GP.PickupType.WeaponCrate]: { color: 0x60A5FA, icon: 'W',  shape: 'crate',   pulseColor: 0x93C5FD },
            [GP.PickupType.SpeedBoost]:  { color: 0x00FFFF, icon: '>',  shape: 'arrow',   pulseColor: 0x67E8F9 },
            [GP.PickupType.DamageBoost]: { color: 0xFF6B6B, icon: '*',  shape: 'star',    pulseColor: 0xFCA5A5 },
            [GP.PickupType.Shield]:      { color: 0x00BFFF, icon: 'S',  shape: 'shield',  pulseColor: 0x60C5FF },
        };
        const config = pickupConfigs[pickup.pickup_type] || pickupConfigs[GP.PickupType.Health];

        const outerGlow = new PIXI.Graphics();
        outerGlow.beginFill(config.pulseColor, 0.15);
        outerGlow.drawCircle(0, 0, 28);
        outerGlow.endFill();
        container.addChild(outerGlow);
        container.outerGlow = outerGlow;

        const middleGlow = new PIXI.Graphics();
        middleGlow.beginFill(config.color, 0.25);
        middleGlow.drawCircle(0, 0, 22);
        middleGlow.endFill();
        container.addChild(middleGlow);

        const main = new PIXI.Graphics();
        main.lineStyle(3, config.color, 0.9);
        main.beginFill(config.color, 0.35);

        switch (config.shape) {
            case 'cross': {
                const crossSize = 15;
                const crossWidth = 6;
                main.drawRect(-crossWidth / 2, -crossSize, crossWidth, crossSize * 2);
                main.drawRect(-crossSize, -crossWidth / 2, crossSize * 2, crossWidth);
                break;
            }
            case 'hexagon':
                drawRegularPolygon(main, 0, 0, 18, 6);
                break;
            case 'crate':
                main.drawRoundedRect(-15, -15, 30, 30, 5);
                main.lineStyle(1, config.color, 0.5);
                main.moveTo(-15, 0); main.lineTo(15, 0);
                main.moveTo(0, -15); main.lineTo(0, 15);
                break;
            case 'arrow': {
                const arrowPoints = [0, -20, 10, -5, 5, -5, 5, 10, -5, 10, -5, -5, -10, -5];
                main.drawPolygon(arrowPoints);
                break;
            }
            case 'star':
                drawStar(main, 0, 0, 5, 20, 10);
                break;
            case 'shield': {
                const shieldPoints = [0, -20, 15, -10, 15, 5, 0, 20, -15, 5, -15, -10];
                main.drawPolygon(shieldPoints);
                break;
            }
            default:
                main.drawCircle(0, 0, 18);
        }
        main.endFill();
        container.addChild(main);

        let iconText = config.icon;
        if (pickup.pickup_type === GP.PickupType.WeaponCrate && pickup.weapon_type !== undefined) {
            iconText = weaponNames[pickup.weapon_type]?.[0] || 'W';
        }

        const iconStyle = new PIXI.TextStyle({
            fontFamily: 'Arial',
            fontSize: pickup.pickup_type === GP.PickupType.WeaponCrate ? 16 : 18,
            fill: 0xFFFFFF,
            fontWeight: 'bold',
            stroke: mixColors(config.color, 0x000000, 0.5),
            strokeThickness: 3,
            dropShadow: true,
            dropShadowColor: 0x000000,
            dropShadowBlur: 2,
            dropShadowDistance: 1
        });
        const icon = new PIXI.Text(iconText, iconStyle);
        icon.anchor.set(0.5);
        container.addChild(icon);

        container.particleEmitter = null;
        container.baseScale = 1;
        container.pulseTime = Math.random() * Math.PI * 2;
        container.floatOffset = Math.random() * Math.PI * 2;

        return container;
    }

    // ── Flag sprite factory ──────────────────────────────────────────

    function createFlagSprite(flagState) {
        const container = new PIXI.Container();
        container.flagTeamId = flagState.team_id;

        const baseColor = teamColors[flagState.team_id] || 0xFFFFFF;
        const darkerColor = mixColors(baseColor, 0x000000, 0.3);

        // Flag pole
        const pole = new PIXI.Graphics();
        pole.lineStyle(3, 0xBBBBBB, 1);
        pole.moveTo(0, 0);
        pole.lineTo(0, -50);
        pole.lineStyle(2, 0x888888, 0.6);
        pole.moveTo(-1, 0);
        pole.lineTo(-1, -50);
        container.addChild(pole);

        // Flag cloth
        const flag = new PIXI.Graphics();
        flag.beginFill(baseColor, 0.9);
        flag.lineStyle(2, darkerColor, 1);
        flag.moveTo(0, -50);
        flag.lineTo(25, -42);
        flag.lineTo(25, -26);
        flag.lineTo(0, -32);
        flag.closePath();
        flag.endFill();
        flag.beginFill(baseColor, 0.6);
        flag.drawRect(2, -48, 20, 3);
        flag.endFill();
        container.addChild(flag);
        container.flagGraphic = flag;

        // Base
        const base = new PIXI.Graphics();
        base.beginFill(0x888888, 0.8);
        base.drawEllipse(0, 0, 12, 5);
        base.endFill();
        base.beginFill(0x666666, 0.4);
        base.drawEllipse(0, 3, 14, 4);
        base.endFill();
        container.addChild(base);

        // Team label
        const teamLabel = new PIXI.Text(
            flagState.team_id === 1 ? 'R' : (flagState.team_id === 2 ? 'B' : '?'),
            { fontSize: 10, fill: baseColor, fontWeight: 'bold', stroke: 0x000000, strokeThickness: 2 }
        );
        teamLabel.anchor.set(0.5);
        teamLabel.position.set(12, -38);
        container.addChild(teamLabel);

        // Glow for dropped flags
        const droppedGlow = new PIXI.Graphics();
        droppedGlow.beginFill(baseColor, 0.2);
        droppedGlow.drawCircle(0, -20, 35);
        droppedGlow.endFill();
        droppedGlow.visible = false;
        container.addChild(droppedGlow);
        container.droppedGlow = droppedGlow;

        // Timer text for dropped
        const timerText = new PIXI.Text('', {
            fontSize: 12, fill: 0xFFFFFF,
            fontWeight: 'bold', stroke: 0x000000, strokeThickness: 3
        });
        timerText.anchor.set(0.5);
        timerText.position.set(0, 18);
        timerText.visible = false;
        container.addChild(timerText);
        container.timerText = timerText;

        return container;
    }

    // ── Starfield ────────────────────────────────────────────────────

    function createStarfield(app) {
        const starfieldContainer = new PIXI.Container();
        const starLayers = [
            { count: 100, scrollFactor: 0.1, minRadius: 0.5, maxRadius: 1, color: 0xFFFFFF },
            { count: 50,  scrollFactor: 0.3, minRadius: 1,   maxRadius: 1.5, color: 0xAAAAFF },
            { count: 30,  scrollFactor: 0.5, minRadius: 1.5, maxRadius: 2, color: 0xFFFFAA }
        ];

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

        starLayers.forEach((layerData) => {
            const layerContainer = new PIXI.Container();
            layerContainer.scrollFactor = layerData.scrollFactor;

            const starGraphics = new PIXI.Graphics();
            starGraphics.beginFill(0xFFFFFF);
            starGraphics.drawCircle(0, 0, 2);
            starGraphics.endFill();
            const starTexture = app.renderer.generateTexture(starGraphics);
            starGraphics.destroy();

            for (let i = 0; i < layerData.count; i++) {
                const star = new PIXI.Sprite(starTexture);
                star.anchor.set(0.5);
                const radius = Math.random() * (layerData.maxRadius - layerData.minRadius) + layerData.minRadius;
                star.scale.set(radius / 2);
                star.tint = layerData.color;
                star.alpha = Math.random() * 0.5 + 0.5;
                star.x = Math.random() * app.screen.width * 2;
                star.y = Math.random() * app.screen.height * 2;
                star.initialX = star.x;
                star.initialY = star.y;
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

    function updateStarfield(starfieldContainer, cameraX, cameraY, delta, frameNowMs, lowDetailMode, app) {
        starfieldContainer.children.forEach((layer) => {
            if (layer.scrollFactor !== undefined) {
                layer.x = -cameraX * layer.scrollFactor;
                layer.y = -cameraY * layer.scrollFactor;
                if (lowDetailMode) return;
                layer.children.forEach(star => {
                    if (star.twinkleSpeed) {
                        star.alpha = 0.5 + Math.sin(frameNowMs * star.twinkleSpeed + star.twinkleOffset) * 0.5;
                    }
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

    // ── Health vignette ──────────────────────────────────────────────

    function createHealthVignette(app) {
        const vignette = new PIXI.Graphics();
        const radius = Math.max(app.screen.width, app.screen.height);
        const center = new PIXI.Point(app.screen.width / 2, app.screen.height / 2);
        for (let i = 0; i < 10; i++) {
            const alpha = (i / 10) * 0.5;
            const currentRadius = radius * (1 - i / 10);
            vignette.beginFill(0xFF0000, alpha);
            vignette.drawCircle(center.x, center.y, currentRadius);
            vignette.endFill();
        }
        vignette.blendMode = PIXI.BLEND_MODES.MULTIPLY;
        vignette.visible = false;
        return vignette;
    }

    function updateHealthVignette(vignette, healthPercent, frameNowMs = Date.now()) {
        if (healthPercent < 0.3) {
            vignette.visible = true;
            vignette.alpha = (0.3 - healthPercent) / 0.3 * 0.5;
            vignette.alpha += Math.sin(frameNowMs * 0.01) * 0.1;
        } else {
            vignette.visible = false;
        }
    }

    // ── Fog of war ───────────────────────────────────────────────────

    function createFogOfWar(app, worldContainer) {
        const fogContainer = new PIXI.Container();
        const fogMask = new PIXI.Graphics();

        const fogOverlay = new PIXI.Graphics();
        fogOverlay.beginFill(0x06090F, 0.42);
        fogOverlay.drawRect(-10000, -10000, 20000, 20000);
        fogOverlay.endFill();
        fogOverlay.blendMode = PIXI.BLEND_MODES.MULTIPLY;

        const fogTexture = new PIXI.Graphics();
        for (let i = 0; i < 50; i++) {
            const x = (Math.random() - 0.5) * 2000;
            const y = (Math.random() - 0.5) * 2000;
            const size = Math.random() * 100 + 50;
            fogTexture.beginFill(0x0B1220, Math.random() * 0.04);
            fogTexture.drawCircle(x, y, size);
            fogTexture.endFill();
        }
        fogTexture.filters = [new PIXI.BlurFilter(20)];
        fogContainer.addChild(fogTexture);
        fogContainer.addChild(fogOverlay);
        fogContainer.mask = fogMask;
        worldContainer.addChild(fogContainer);

        return { fogContainer, fogMask };
    }

    function updateFogOfWar(fogMask, playerX, playerY, fogRadius, fadeDistance, localPlayerState, players, myPlayerId) {
        if (!fogMask) return;
        const maskWorldHalfSpan = 12000;
        const localRevealRadius = fogRadius + (fadeDistance * 0.55);
        fogMask.clear();
        fogMask.beginFill(0xFFFFFF, 1);
        fogMask.drawRect(-maskWorldHalfSpan, -maskWorldHalfSpan, maskWorldHalfSpan * 2, maskWorldHalfSpan * 2);
        fogMask.beginHole();
        fogMask.drawCircle(playerX, playerY, localRevealRadius);
        if (localPlayerState && localPlayerState.team_id !== 0) {
            players.forEach((player) => {
                if (player.id !== myPlayerId && player.team_id === localPlayerState.team_id && player.alive) {
                    fogMask.drawCircle(player.x, player.y, fogRadius * 0.4);
                }
            });
        }
        fogMask.endHole();
        fogMask.endFill();
    }

    // ── Ammo helper ──────────────────────────────────────────────────

    function getMaxAmmoForWeaponClient(weaponType) {
        switch (weaponType) {
            case GP.WeaponType.Pistol:  return 10;
            case GP.WeaponType.Shotgun: return 5;
            case GP.WeaponType.Rifle:   return 30;
            case GP.WeaponType.Sniper:  return 5;
            case GP.WeaponType.Melee:   return 0;
            default: return 10;
        }
    }

    // ── Screen effects ───────────────────────────────────────────────

    function applyScreenShake(gameScene, intensity, durationFrames) {
        let frame = 0;
        const originalX = gameScene.position.x;
        const originalY = gameScene.position.y;
        const doShake = () => {
            if (frame >= durationFrames) {
                gameScene.position.x = originalX;
                gameScene.position.y = originalY;
                return;
            }
            const decay = 1 - (frame / durationFrames);
            gameScene.position.x = originalX + (Math.random() - 0.5) * intensity * decay;
            gameScene.position.y = originalY + (Math.random() - 0.5) * intensity * decay;
            frame++;
            requestAnimationFrame(doShake);
        };
        doShake();
    }

    function createScreenFlash(app, color, durationFrames, maxAlpha) {
        const flash = new PIXI.Graphics();
        flash.beginFill(color, maxAlpha);
        flash.drawRect(0, 0, app.screen.width, app.screen.height);
        flash.endFill();
        app.stage.addChild(flash);
        let frame = 0;
        const doFlash = () => {
            if (frame >= durationFrames) {
                app.stage.removeChild(flash);
                flash.destroy();
                return;
            }
            flash.alpha = maxAlpha * (1 - frame / durationFrames);
            frame++;
            requestAnimationFrame(doFlash);
        };
        doFlash();
    }

    return {
        drawRegularPolygon,
        drawStar,
        interpolateColor,
        mixColors,
        createSpeedBoostEffect,
        createDodgeGlowEffect,
        createWeaponSwapEffect,
        createPickupSprite,
        createFlagSprite,
        createStarfield,
        updateStarfield,
        createHealthVignette,
        updateHealthVignette,
        createFogOfWar,
        updateFogOfWar,
        getMaxAmmoForWeaponClient,
        applyScreenShake,
        createScreenFlash,
    };
}
