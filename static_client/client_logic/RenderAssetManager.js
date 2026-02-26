/**
 * RenderAssetManager.js - Texture generation, atlas building, render asset cache
 *
 * Extracted from client.html. Contains buildRenderTexture, createGunTextureForWeapon,
 * createProjectileTextureForWeapon, initRenderAssetCache, and buildCarriedFlagSprite.
 * Uses getCtx callback pattern.
 */

export function createRenderAssetManager(getCtx) {

    function buildRenderTexture(drawFn, resolution = 2) {
        const { app } = getCtx();
        const graphics = new (getCtx().PIXI.Graphics)();
        drawFn(graphics);
        const texture = app.renderer.generateTexture(graphics, { resolution });
        graphics.destroy(true);
        return texture;
    }

    function createGunTextureForWeapon(weaponType) {
        const { GP, PLAYER_RADIUS } = getCtx();
        const configs = {
            [GP.WeaponType.Pistol]: { barrelLength: PLAYER_RADIUS + 12, barrelWidth: 4, muzzleSize: 4, barrelCount: 1, scope: false },
            [GP.WeaponType.Shotgun]: { barrelLength: PLAYER_RADIUS + 14, barrelWidth: 6, muzzleSize: 5, barrelCount: 2, scope: false },
            [GP.WeaponType.Rifle]: { barrelLength: PLAYER_RADIUS + 18, barrelWidth: 4, muzzleSize: 4, barrelCount: 1, scope: false },
            [GP.WeaponType.Sniper]: { barrelLength: PLAYER_RADIUS + 22, barrelWidth: 3, muzzleSize: 3, barrelCount: 1, scope: true },
            [GP.WeaponType.Melee]: { barrelLength: PLAYER_RADIUS + 8, barrelWidth: 8, muzzleSize: 0, barrelCount: 1, scope: false }
        };
        const cfg = configs[weaponType] || configs[GP.WeaponType.Pistol];
        return buildRenderTexture((g) => {
            g.beginFill(0xFFFFFF, 1);
            if (cfg.barrelCount === 2) {
                g.drawRoundedRect(0, -4, cfg.barrelLength, 3, 1.5);
                g.drawRoundedRect(0, 1, cfg.barrelLength, 3, 1.5);
            } else {
                g.drawRoundedRect(0, -cfg.barrelWidth * 0.5, cfg.barrelLength, cfg.barrelWidth, cfg.barrelWidth * 0.35);
            }
            g.endFill();

            if (cfg.muzzleSize > 0) {
                g.beginFill(0xFFFFFF, 1);
                g.drawCircle(cfg.barrelLength, 0, cfg.muzzleSize);
                g.endFill();
            }

            if (cfg.scope) {
                g.beginFill(0xFFFFFF, 0.9);
                g.drawCircle(cfg.barrelLength * 0.7, 0, 4);
                g.endFill();
            }
        });
    }

    function createProjectileTextureForWeapon(weaponType) {
        const { GP } = getCtx();
        return buildRenderTexture((g) => {
            g.beginFill(0xFFFFFF, 1);
            switch (weaponType) {
                case GP.WeaponType.Shotgun:
                    g.drawCircle(0, 0, 4);
                    break;
                case GP.WeaponType.Rifle:
                    g.drawRoundedRect(-10, -2.5, 20, 5, 2);
                    break;
                case GP.WeaponType.Sniper:
                    g.drawRoundedRect(-13, -2, 26, 4, 2);
                    break;
                default:
                    g.drawRoundedRect(-8, -3, 16, 6, 3);
                    break;
            }
            g.endFill();
        });
    }

    function initRenderAssetCache() {
        const ctx = getCtx();
        const {
            PIXI, GP, app, renderAssetCache, PLAYER_RADIUS, SHIP_POINTS,
            drawRegularPolygon, interpolateColor,
        } = ctx;

        if (renderAssetCache.initialized || !app || !app.renderer) return;

        renderAssetCache.shipTexture = buildRenderTexture((g) => {
            g.beginFill(0xFFFFFF, 1);
            g.drawPolygon(SHIP_POINTS);
            g.endFill();
        });

        renderAssetCache.shadowTexture = buildRenderTexture((g) => {
            // Soft shadow falloff baked into texture (no runtime blur filter cost).
            for (let i = 0; i < 5; i++) {
                const t = i / 4;
                const alpha = 0.24 * (1 - t);
                const rx = PLAYER_RADIUS * (1.05 + t * 0.55);
                const ry = PLAYER_RADIUS * (0.55 + t * 0.35);
                g.beginFill(0xFFFFFF, alpha);
                g.drawEllipse(0, 0, rx, ry);
                g.endFill();
            }
        }, 2.5);

        renderAssetCache.engineGlowTexture = buildRenderTexture((g) => {
            g.beginFill(0xFFFFFF, 1);
            g.drawCircle(0, 0, PLAYER_RADIUS * 0.3);
            g.endFill();
        });

        renderAssetCache.localIndicatorTexture = buildRenderTexture((g) => {
            g.lineStyle(2, 0xFFFFFF, 1);
            g.drawCircle(0, 0, PLAYER_RADIUS + 4);
        });

        renderAssetCache.shieldTexture = buildRenderTexture((g) => {
            g.lineStyle(2, 0xFFFFFF, 1);
            g.beginFill(0xFFFFFF, 0.25);
            drawRegularPolygon(g, 0, 0, PLAYER_RADIUS + 10, 6);
            g.endFill();
        });

        renderAssetCache.carriedFlagPoleTexture = buildRenderTexture((g) => {
            g.beginFill(0xFFFFFF, 1);
            g.drawRect(0, -PLAYER_RADIUS * 1.5, 3, PLAYER_RADIUS * 1.5);
            g.endFill();
        });
        renderAssetCache.carriedFlagClothTexture = buildRenderTexture((g) => {
            g.beginFill(0xFFFFFF, 1);
            g.drawRect(0, -PLAYER_RADIUS * 1.5, 15, 10);
            g.endFill();
        });

        const weaponTypes = [
            GP.WeaponType.Pistol,
            GP.WeaponType.Shotgun,
            GP.WeaponType.Rifle,
            GP.WeaponType.Sniper,
            GP.WeaponType.Melee
        ];
        weaponTypes.forEach((weaponType) => {
            renderAssetCache.gunTextures.set(weaponType, createGunTextureForWeapon(weaponType));
            renderAssetCache.projectileTextures.set(weaponType, createProjectileTextureForWeapon(weaponType));
        });

        // --- Texture Atlas: consolidate player visual components into a single RenderTexture ---
        // This reduces draw calls by letting sprites share one BaseTexture.
        {
            const atlasSize = 512;
            const atlasRT = PIXI.RenderTexture.create({ width: atlasSize, height: atlasSize, resolution: 2 });
            const atlasContainer = new PIXI.Container();
            const regions = {};

            // Ship body at (64, 64)
            const shipG = new PIXI.Graphics();
            shipG.beginFill(0xFFFFFF, 1);
            shipG.drawPolygon(SHIP_POINTS);
            shipG.endFill();
            shipG.position.set(64, 64);
            atlasContainer.addChild(shipG);
            regions.ship = { x: 64 - PLAYER_RADIUS * 1.3, y: 64 - PLAYER_RADIUS * 1.3, w: PLAYER_RADIUS * 2.6, h: PLAYER_RADIUS * 2.6 };

            // Engine glow at (160, 64)
            const engineG = new PIXI.Graphics();
            engineG.beginFill(0xFFFFFF, 1);
            engineG.drawCircle(0, 0, PLAYER_RADIUS * 0.3);
            engineG.endFill();
            engineG.position.set(160, 64);
            atlasContainer.addChild(engineG);
            regions.engineGlow = { x: 160 - PLAYER_RADIUS * 0.4, y: 64 - PLAYER_RADIUS * 0.4, w: PLAYER_RADIUS * 0.8, h: PLAYER_RADIUS * 0.8 };

            // Local indicator ring at (256, 64)
            const indG = new PIXI.Graphics();
            indG.lineStyle(2, 0xFFFFFF, 1);
            indG.drawCircle(0, 0, PLAYER_RADIUS + 4);
            indG.position.set(256, 64);
            atlasContainer.addChild(indG);
            const indSize = (PLAYER_RADIUS + 6) * 2;
            regions.localIndicator = { x: 256 - indSize / 2, y: 64 - indSize / 2, w: indSize, h: indSize };

            // Shield hex at (384, 64)
            const shieldG = new PIXI.Graphics();
            shieldG.lineStyle(2, 0xFFFFFF, 1);
            shieldG.beginFill(0xFFFFFF, 0.25);
            drawRegularPolygon(shieldG, 0, 0, PLAYER_RADIUS + 10, 6);
            shieldG.endFill();
            shieldG.position.set(384, 64);
            atlasContainer.addChild(shieldG);
            const shieldSize = (PLAYER_RADIUS + 12) * 2;
            regions.shield = { x: 384 - shieldSize / 2, y: 64 - shieldSize / 2, w: shieldSize, h: shieldSize };

            app.renderer.render(atlasContainer, { renderTexture: atlasRT });
            atlasContainer.destroy({ children: true });

            renderAssetCache.playerAtlasTexture = atlasRT;

            // Create sub-region Texture references from the single atlas BaseTexture.
            // Frame coordinates must stay in BaseTexture frame-space (logical width/height).
            const bt = atlasRT.baseTexture;
            const atlasW = Math.max(1, Math.floor(Number(bt && bt.width) || Number(atlasRT.width) || 1));
            const atlasH = Math.max(1, Math.floor(Number(bt && bt.height) || Number(atlasRT.height) || 1));
            const normalizeFrame = (x, y, w, h) => {
                const frameX = Math.max(0, Math.min(atlasW - 1, Math.floor(x)));
                const frameY = Math.max(0, Math.min(atlasH - 1, Math.floor(y)));
                const frameW = Math.max(1, Math.min(atlasW - frameX, Math.ceil(w)));
                const frameH = Math.max(1, Math.min(atlasH - frameY, Math.ceil(h)));
                if (frameW <= 0 || frameH <= 0) return null;
                return { x: frameX, y: frameY, w: frameW, h: frameH };
            };
            const toTexture = (frameDef) => {
                if (!frameDef) return null;
                try {
                    return new PIXI.Texture(bt, new PIXI.Rectangle(frameDef.x, frameDef.y, frameDef.w, frameDef.h));
                } catch (_) {
                    return null;
                }
            };
            for (const [name, r] of Object.entries(regions)) {
                const subFrame = normalizeFrame(r.x, r.y, r.w, r.h);
                let subTexture = toTexture(subFrame);
                if (!subTexture) {
                    subTexture = PIXI.Texture.EMPTY;
                    if (typeof console !== 'undefined' && typeof console.warn === 'function') {
                        console.warn(
                            `[RenderAssetManager] Failed to create atlas region "${name}"`,
                            { region: r, atlasW, atlasH }
                        );
                    }
                }
                renderAssetCache.playerAtlasRegions[name] = subTexture;
            }
        }

        // --- Bitmap Font: pre-generate for username labels (10x faster than PIXI.Text) ---
        PIXI.BitmapFont.from(renderAssetCache.bitmapFontName, {
            fontFamily: 'Arial',
            fontSize: 24,
            fill: '#FFFFFF',
            stroke: '#111827',
            strokeThickness: 4,
            fontWeight: 'normal'
        }, {
            // ASCII printable range plus Latin Extended for international player names
            chars: [
                [' ', '~'],         // ASCII printable (space through tilde)
                ['\u00C0', '\u00FF'] // Latin-1 Supplement (accented chars)
            ],
            resolution: 2,
            padding: 2
        });

        // --- Health Bar Textures: pre-render at 10% increments ---
        {
            const barWidth = PLAYER_RADIUS * 2;
            const barHeight = 6;
            const totalWidth = PLAYER_RADIUS * 2 + 4;
            const totalHeight = 10;

            // Background texture (dark)
            renderAssetCache.healthBarBgTexture = buildRenderTexture((g) => {
                g.beginFill(0x1F2937, 0.9);
                g.drawRoundedRect(0, 0, totalWidth, totalHeight, 5);
                g.endFill();
            });

            // Border texture
            renderAssetCache.healthBarBorderTexture = buildRenderTexture((g) => {
                g.lineStyle(1, 0x4B5563, 0.8);
                g.drawRoundedRect(0, 0, totalWidth, totalHeight, 5);
            });

            // 11 fill textures (0% through 100% in 10% steps)
            for (let i = 0; i <= 10; i++) {
                const healthPercent = i / 10;
                const currentWidth = barWidth * healthPercent;

                if (i === 0) {
                    // Empty texture for 0% health
                    renderAssetCache.healthBarTextures.push(PIXI.Texture.EMPTY);
                    continue;
                }

                let healthColor;
                if (healthPercent > 0.6) {
                    healthColor = interpolateColor(0x22C55E, 0xFACC15, (healthPercent - 0.6) / 0.4);
                } else if (healthPercent > 0.3) {
                    healthColor = interpolateColor(0xFACC15, 0xEF4444, (healthPercent - 0.3) / 0.3);
                } else {
                    healthColor = 0xEF4444;
                }

                const tex = buildRenderTexture((g) => {
                    g.beginFill(healthColor);
                    g.drawRoundedRect(0, 0, currentWidth, barHeight, 3);
                    g.endFill();
                    // Highlight strip on top
                    g.beginFill(0xFFFFFF, 0.3);
                    g.drawRoundedRect(0, 0, currentWidth, 2, 1);
                    g.endFill();
                });
                renderAssetCache.healthBarTextures.push(tex);
            }
        }

        renderAssetCache.initialized = true;
    }

    function buildCarriedFlagSprite(teamId) {
        const { PIXI, renderAssetCache, PLAYER_RADIUS, teamColors } = getCtx();
        const container = new PIXI.Container();

        const pole = new PIXI.Sprite(renderAssetCache.carriedFlagPoleTexture);
        pole.anchor.set(0, 0);
        pole.position.set(PLAYER_RADIUS * 0.6, 0);
        pole.tint = 0xFFFFFF;
        container.addChild(pole);

        const cloth = new PIXI.Sprite(renderAssetCache.carriedFlagClothTexture);
        cloth.anchor.set(0, 0);
        cloth.position.set(PLAYER_RADIUS * 0.6 + 3, 0);
        cloth.tint = teamColors[teamId] || 0xFFFFFF;
        container.addChild(cloth);

        return container;
    }

    return {
        buildRenderTexture,
        createGunTextureForWeapon,
        createProjectileTextureForWeapon,
        initRenderAssetCache,
        buildCarriedFlagSprite,
    };
}
