// Part 2 - Helper Functions and Sprite Creation

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

// Create projectile sprite with proper animation
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

    // Store velocity for interpolation
    container.velocity = {
        x: projectile.velocity_x || 0,
        y: projectile.velocity_y || 0
    };

    return container;
}

// Update projectile sprite with proper animation
function updateProjectileSprite(sprite, projectile) {
    // Use render position if available, otherwise use actual position
    sprite.position.x = projectile.render_x !== undefined ? projectile.render_x : projectile.x;
    sprite.position.y = projectile.render_y !== undefined ? projectile.render_y : projectile.y;

    // Update rotation based on velocity
    if (projectile.velocity_x !== undefined && projectile.velocity_y !== undefined) {
        sprite.rotation = Math.atan2(projectile.velocity_y, projectile.velocity_x);
        // Update stored velocity for interpolation
        sprite.velocity.x = projectile.velocity_x;
        sprite.velocity.y = projectile.velocity_y;
    } else if (sprite.velocity.x !== 0 || sprite.velocity.y !== 0) {
        // Use stored velocity if server doesn't send it
        sprite.rotation = Math.atan2(sprite.velocity.y, sprite.velocity.x);
    }

    // Update trail effect
    if (sprite.trail && sprite.lastPositions) {
        sprite.trail.clear();
        sprite.trail.lineStyle(3, sprite.trailColor, 0.3);

        if (sprite.lastPositions.length > 1) {
            sprite.trail.moveTo(
                sprite.lastPositions[0].x - sprite.position.x,
                sprite.lastPositions[0].y - sprite.position.y
            );

            for (let i = 1; i < sprite.lastPositions.length; i++) {
                const alpha = (1 - i / sprite.lastPositions.length) * 0.3;
                sprite.trail.lineStyle(3 - i * 0.5, sprite.trailColor, alpha);
                sprite.trail.lineTo(
                    sprite.lastPositions[i].x - sprite.position.x,
                    sprite.lastPositions[i].y - sprite.position.y
                );
            }
        }
    }

    // Initialize or update position history
    if (!sprite.lastPositions) {
        sprite.lastPositions = [];
    }
    sprite.lastPositions.unshift({ x: sprite.position.x, y: sprite.position.y });
    if (sprite.lastPositions.length > 5) {
        sprite.lastPositions.pop();
    }
}

function createPickupSprite(pickup) {
    const container = new PIXI.Container();
    container.pickupId = pickup.id;

    const pickupConfigs = {
        [GP.PickupType.Health]: {
            color: 0x10B981,
            icon: '➕',
            shape: 'cross',
            pulseColor: 0x34D399
        },
        [GP.PickupType.Ammo]: {
            color: 0xF59E0B,
            icon: '⦿',
            shape: 'hexagon',
            pulseColor: 0xFBBF24
        },
        [GP.PickupType.WeaponCrate]: {
            color: 0x60A5FA,
            icon: '🔫',
            shape: 'crate',
            pulseColor: 0x93C5FD
        },
        [GP.PickupType.SpeedBoost]: {
            color: 0x00FFFF,
            icon: '💨',
            shape: 'arrow',
            pulseColor: 0x67E8F9
        },
        [GP.PickupType.DamageBoost]: {
            color: 0xFF6B6B,
            icon: '💥',
            shape: 'star',
            pulseColor: 0xFCA5A5
        },
        [GP.PickupType.Shield]: {
            color: 0x00BFFF,
            icon: '🛡️',
            shape: 'shield',
            pulseColor: 0x60C5FF
        }
    };

    const config = pickupConfigs[pickup.pickup_type] || pickupConfigs[GP.PickupType.Health];

    // Animated outer glow
    const outerGlow = new PIXI.Graphics();
    outerGlow.beginFill(config.pulseColor, 0.15);
    outerGlow.drawCircle(0, 0, 28);
    outerGlow.endFill();
    container.addChild(outerGlow);
    container.outerGlow = outerGlow;

    // Middle glow layer
    const middleGlow = new PIXI.Graphics();
    middleGlow.beginFill(config.color, 0.25);
    middleGlow.drawCircle(0, 0, 22);
    middleGlow.endFill();
    container.addChild(middleGlow);

    // Main pickup shape
    const main = new PIXI.Graphics();
    main.lineStyle(3, config.color, 0.9);
    main.beginFill(config.color, 0.35);

    switch (config.shape) {
        case 'cross':
            const crossSize = 15;
            const crossWidth = 6;
            main.drawRect(-crossWidth / 2, -crossSize, crossWidth, crossSize * 2);
            main.drawRect(-crossSize, -crossWidth / 2, crossSize * 2, crossWidth);
            break;

        case 'hexagon':
            drawRegularPolygon(main, 0, 0, 18, 6);
            break;

        case 'crate':
            main.drawRoundedRect(-15, -15, 30, 30, 5);
            main.lineStyle(1, config.color, 0.5);
            main.moveTo(-15, 0);
            main.lineTo(15, 0);
            main.moveTo(0, -15);
            main.lineTo(0, 15);
            break;

        case 'arrow':
            const arrowPoints = [0, -20, 10, -5, 5, -5, 5, 10, -5, 10, -5, -5, -10, -5];
            main.drawPolygon(arrowPoints);
            break;

        case 'star':
            drawStar(main, 0, 0, 5, 20, 10);
            break;

        case 'shield':
            const shieldPoints = [0, -20, 15, -10, 15, 5, 0, 20, -15, 5, -15, -10];
            main.drawPolygon(shieldPoints);
            break;

        default:
            main.drawCircle(0, 0, 18);
    }

    main.endFill();
    container.addChild(main);

    // Icon or weapon type indicator
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

    // Particle emitter placeholder for ambient particles
    container.particleEmitter = null;

    container.baseScale = 1;
    container.pulseTime = Math.random() * Math.PI * 2;
    container.floatOffset = Math.random() * Math.PI * 2;

    return container;
}

// Helper function to draw regular polygon
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

// Helper function to draw star
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

// Enhanced pickup animation in game loop
function animatePickups(delta) {
    pickupContainer.children.forEach(pickupSprite => {
        if (pickupSprite.visible) {
            // Floating animation
            pickupSprite.floatOffset += delta * 0.002;
            pickupSprite.y += Math.sin(pickupSprite.floatOffset) * 0.1;

            // Pulsing animation
            pickupSprite.pulseTime += delta * 0.003;
            const pulse = Math.sin(pickupSprite.pulseTime) * 0.1 + 0.95;
            pickupSprite.scale.set(pickupSprite.baseScale * pulse);

            // Rotation
            pickupSprite.rotation += 0.02;

            // Update outer glow
            if (pickupSprite.outerGlow) {
                const glowPulse = Math.sin(pickupSprite.pulseTime * 1.5) * 0.1 + 0.9;
                pickupSprite.outerGlow.scale.set(glowPulse);
                pickupSprite.outerGlow.alpha = 0.15 + Math.sin(pickupSprite.pulseTime * 2) * 0.05;
            }
        }
    });
}

// Enhanced flag animation
function animateFlags(delta) {
    flagContainer.children.forEach(flagSprite => {
        if (flagSprite.flagGraphic && flagSprite.visible) {
            // Waving animation
            const waveSpeed = 0.002;
            const waveAmount = 0.15;
            flagSprite.flagGraphic.skew.x = Math.sin(Date.now() * waveSpeed + flagSprite.flagTeamId) * waveAmount;

            // Slight vertical bob
            flagSprite.flagGraphic.y = -40 + Math.sin(Date.now() * 0.001) * 2;

            // Dropped flag effects
            if (flagSprite.droppedGlow) {
                const pulse = Math.sin(Date.now() * 0.003) * 0.3 + 0.7;
                flagSprite.droppedGlow.alpha = pulse;
                flagSprite.droppedGlow.scale.set(pulse);
            }

            // Update timer
            if (flagSprite.timerText) {
                const state = flagStates.get(flagSprite.flagTeamId);
                if (state && state.status === GP.FlagStatus.Dropped && state.respawn_timer > 0) {
                    flagSprite.timerText.text = Math.ceil(state.respawn_timer) + 's';
                    // Flash when time is low
                    if (state.respawn_timer < 3) {
                        flagSprite.timerText.alpha = Math.sin(Date.now() * 0.01) * 0.5 + 0.5;
                    }
                }
            }
        }
    });
}

// Low health vignette effect
function createHealthVignette(app) {
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

function updateHealthVignette(vignette, healthPercent) {
    if (healthPercent < 0.3) {
        vignette.visible = true;
        vignette.alpha = (0.3 - healthPercent) / 0.3 * 0.5;
        // Pulse effect
        vignette.alpha += Math.sin(Date.now() * 0.01) * 0.1;
    } else {
        vignette.visible = false;
    }
}

// Helper function for client-side max ammo logic
function getMaxAmmoForWeaponClient(weaponType) {
    switch (weaponType) {
        case GP.WeaponType.Pistol: return 10;
        case GP.WeaponType.Shotgun: return 5;
        case GP.WeaponType.Rifle: return 30;
        case GP.WeaponType.Sniper: return 5;
        case GP.WeaponType.Melee: return 0;
        default: return 10;
    }
}
