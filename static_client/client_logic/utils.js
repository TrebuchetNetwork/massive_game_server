/**
 * Shared utility functions for game client
 * Dependencies: PIXI.js
 */

// Color utility functions
export function mixColors(color1, color2, amount) {
    const c1 = PIXI.Color.shared.setValue(color1).toRgbArray();
    const c2 = PIXI.Color.shared.setValue(color2).toRgbArray();
    const r = c1[0] * (1 - amount) + c2[0] * amount;
    const g = c1[1] * (1 - amount) + c2[1] * amount;
    const b = c1[2] * (1 - amount) + c2[2] * amount;
    return PIXI.Color.shared.setValue([r, g, b]).toNumber();
}

export function interpolateColor(color1, color2, factor) {
    const c1 = PIXI.Color.shared.setValue(color1).toRgbArray();
    const c2 = PIXI.Color.shared.setValue(color2).toRgbArray();
    const r = Math.floor(c1[0] * 255 * (1 - factor) + c2[0] * 255 * factor);
    const g = Math.floor(c1[1] * 255 * (1 - factor) + c2[1] * 255 * factor);
    const b = Math.floor(c1[2] * 255 * (1 - factor) + c2[2] * 255 * factor);
    return (r << 16) | (g << 8) | b;
}

// Shape drawing utilities
export function drawStar(graphics, x, y, points, outerRadius, innerRadius) {
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

export function drawRegularPolygon(graphics, x, y, radius, sides) {
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

// Visual effects utilities
export function createStarfield(app) {
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

export function updateStarfield(starfieldContainer, cameraX, cameraY, delta, app) {
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

export function createHealthVignette(app) {
    const vignette = new PIXI.Graphics();
    const radius = Math.max(app.screen.width, app.screen.height);
    
    // Create radial gradient effect
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

export function updateHealthVignette(vignette, healthPercent) {
    if (healthPercent < 0.3) {
        vignette.visible = true;
        vignette.alpha = (0.3 - healthPercent) / 0.3 * 0.5;
        // Pulse effect
        vignette.alpha += Math.sin(Date.now() * 0.01) * 0.1;
    } else {
        vignette.visible = false;
    }
}

export function applyScreenShake(container, duration, magnitude, app) {
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

export function createScreenFlash(app, color = 0xFFFFFF, duration = 15, maxAlpha = 0.7) {
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

// Initialize enhanced graphics system
export function initializeEnhancedGraphics(app, worldContainer, AudioManager, EffectsManager) {
    // Initialize managers with audio support
    const audioManager = new AudioManager();
    const effectsManager = new EffectsManager(app, worldContainer, audioManager);
    
    // Create starfield background
    const starfield = createStarfield(app);
    worldContainer.addChildAt(starfield, 0);
    
    // Create health vignette
    const healthVignette = createHealthVignette(app);
    app.stage.addChild(healthVignette);  // Add to main stage, not worldContainer
    
    return {
        audioManager,
        effectsManager,
        starfield,
        healthVignette
    };
}

// Team colors configuration
export const teamColors = {
    0: 0xA0A0A0, // Neutral/FFA - A distinct Grey
    1: 0xFF6B6B, // Team 1 - Red
    2: 0x4ECDC4, // Team 2 - Teal/Blue
};

export const defaultEnemyColor = 0xF87171;

// Weapon configurations
export const weaponNames = {
    Pistol: 'Pistol',
    Shotgun: 'Shotgun',
    Rifle: 'Rifle',
    Sniper: 'Sniper',
    Melee: 'Melee'
};

export const weaponColors = {
    Pistol: 0xFFBF00,
    Shotgun: 0xFF4444,
    Rifle: 0x4444FF,
    Sniper: 0xAA44FF,
    Melee: 0xD1D5DB
};

// Pickup configurations
export const pickupTypes = {
    Health: 'Health',
    Ammo: 'Ammo',
    WeaponCrate: 'Weapon',
    SpeedBoost: 'Speed',
    DamageBoost: 'Damage',
    Shield: 'Shield',
    FlagRed: 'Red Flag',
    FlagBlue: 'Blue Flag'
};

export const pickupColors = {
    Health: 0x10B981,
    Ammo: 0xF59E0B,
    WeaponCrate: 0x60A5FA,
    SpeedBoost: 0x00FFFF,
    DamageBoost: 0xFF6B6B,
    Shield: 0x00BFFF,
    FlagRed: 0xFF0000,
    FlagBlue: 0x0000FF
};

// Helper function for client-side max ammo logic
export function getMaxAmmoForWeaponClient(weaponType) {
    const GP = window.GP;
    switch (weaponType) {
        case GP.WeaponType.Pistol: return 10;
        case GP.WeaponType.Shotgun: return 5;
        case GP.WeaponType.Rifle: return 30;
        case GP.WeaponType.Sniper: return 5;
        case GP.WeaponType.Melee: return 0; // Melee has no ammo
        default: return 10; // Default fallback
    }
}

// HTML escape utility
export function escapeHtml(unsafe) {
    return unsafe
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&#039;");
}
