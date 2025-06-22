/**
 * EffectsManager - Handles visual effects like explosions, impacts, etc.
 * Dependencies: PIXI.js, GameProtocol
 */

export class EffectsManager {
    constructor(app, container, audioManager = null) {
        this.app = app;
        this.effectsContainer = new PIXI.Container();
        container.addChild(this.effectsContainer);
        this.activeEffects = [];
        this.particlesEnabled = true;
        this.audioManager = audioManager;  // Store audio manager reference
        
        // Pre-generate particle textures
        this.particleTextures = this.generateParticleTextures();
    }

    generateParticleTextures() {
        const textures = {};
        
        // Spark particle
        const sparkGraphics = new PIXI.Graphics();
        sparkGraphics.beginFill(0xFFFFFF);
        sparkGraphics.drawCircle(0, 0, 2);
        sparkGraphics.endFill();
        textures.spark = this.app.renderer.generateTexture(sparkGraphics);
        
        // Smoke particle
        const smokeGraphics = new PIXI.Graphics();
        smokeGraphics.beginFill(0x888888, 0.5);
        smokeGraphics.drawCircle(0, 0, 8);
        smokeGraphics.endFill();
        smokeGraphics.filters = [new PIXI.BlurFilter(3)];
        textures.smoke = this.app.renderer.generateTexture(smokeGraphics);
        
        // Debris particle
        const debrisGraphics = new PIXI.Graphics();
        debrisGraphics.beginFill(0x444444);
        debrisGraphics.drawRect(-3, -3, 6, 6);
        debrisGraphics.endFill();
        textures.debris = this.app.renderer.generateTexture(debrisGraphics);
        
        // Clean up
        sparkGraphics.destroy();
        smokeGraphics.destroy();
        debrisGraphics.destroy();
        
        return textures;
    }
    
    setParticlesEnabled(enabled) {
        this.particlesEnabled = enabled;
    }

    processGameEvent(event, GameProtocol) {
        // Use window.GP if GameProtocol is not provided
        const GP = GameProtocol || window.GP;
        if (!GP) {
            console.error('GameProtocol not available in processGameEvent');
            return;
        }
        
        // Check if GameEventType exists
        if (!GP.GameEventType) {
            console.warn('GP.GameEventType not available, trying numeric event types');
            // If GameEventType enum is not available, use numeric values directly
            // This allows the system to work even if the enum hasn't loaded yet
            const eventTypes = {
                BulletImpact: 0,
                Explosion: 1,
                WeaponFire: 2,
                PlayerDamageEffect: 3,
                WallDestroyed: 4,
                PowerupActivated: 5,
                FlagCaptured: 6,
                FlagGrabbed: 7,
                FlagDropped: 8,
                FlagReturned: 9,
                MeleeAttack: 10
            };
            
            if (!this.particlesEnabled && event.event_type !== eventTypes.PlayerDamageEffect) return;
            
            const pos = { x: event.position?.x || 0, y: event.position?.y || 0 };
            switch (event.event_type) {
                case eventTypes.BulletImpact:
                    this.createEnhancedBulletImpact(pos, event.weapon_type);
                    if (this.audioManager) {
                        this.audioManager.playSound('bulletImpact', pos, 0.5);
                    }
                    break;
                case eventTypes.Explosion:
                    this.createEnhancedExplosion(pos, event.value);
                    if (this.audioManager) {
                        this.audioManager.playSound('explosion', pos);
                    }
                    break;
                case eventTypes.WeaponFire:
                    this.createEnhancedMuzzleFlash(pos, event.weapon_type, event.instigator_id);
                    if (this.audioManager) {
                        this.audioManager.playWeaponSound(event.weapon_type, pos, event.instigator_id === window.myPlayerId);
                    }
                    break;
                case eventTypes.PlayerDamageEffect:
                    this.createEnhancedDamageNumbers(pos, event.value);
                    if (this.audioManager) {
                        this.audioManager.playSound('playerHit', pos);
                    }
                    break;
                case eventTypes.WallDestroyed:
                    this.createEnhancedWallDestructionEffect(pos);
                    if (this.audioManager) {
                        this.audioManager.playSound('explosion', pos, 0.7);
                    }
                    break;
                case eventTypes.PowerupActivated:
                    this.createEnhancedPowerupCollectEffect(pos);
                    if (this.audioManager) {
                        this.audioManager.playSound('powerupCollect', pos);
                    }
                    break;
                case eventTypes.FlagCaptured:
                    this.createEnhancedFlagCaptureEffect(pos);
                    if (this.audioManager) {
                        this.audioManager.playSound('flagCapture', pos);
                    }
                    break;
                case eventTypes.FlagGrabbed:
                    if (this.audioManager) this.audioManager.playSound('flagGrabbed', pos, 0.6);
                    break;
                case eventTypes.FlagDropped:
                    if (this.audioManager) this.audioManager.playSound('flagDropped', pos, 0.5);
                    break;
                case eventTypes.FlagReturned:
                    if (this.audioManager) this.audioManager.playSound('flagReturned', pos, 0.7);
                    break;
                case eventTypes.MeleeAttack:
                    this.createMeleeEffect(pos, event.instigator_id);
                    if (this.audioManager) {
                        this.audioManager.playSound('meleeSwing', pos, 0.7);
                    }
                    break;
            }
            return;
        }
        
        if (!this.particlesEnabled && (event.event_type !== GP.GameEventType.PlayerDamageEffect)) return;

        const pos = { x: event.position?.x || 0, y: event.position?.y || 0 };
        switch (event.event_type) {
            case GP.GameEventType.BulletImpact:
                this.createEnhancedBulletImpact(pos, event.weapon_type);
                if (this.audioManager) {
                    this.audioManager.playSound('bulletImpact', pos, 0.5);
                }
                break;
            case GP.GameEventType.Explosion:
                this.createEnhancedExplosion(pos, event.value);
                if (this.audioManager) {
                    this.audioManager.playSound('explosion', pos);
                }
                break;
            case GP.GameEventType.WeaponFire:
                this.createEnhancedMuzzleFlash(pos, event.weapon_type, event.instigator_id);
                if (this.audioManager) {
                    this.audioManager.playWeaponSound(event.weapon_type, pos, event.instigator_id === window.myPlayerId);
                }
                break;
            case GP.GameEventType.PlayerDamageEffect:
                this.createEnhancedDamageNumbers(pos, event.value);
                if (this.audioManager) {
                    this.audioManager.playSound('playerHit', pos);
                }
                break;
            case GP.GameEventType.WallDestroyed:
                this.createEnhancedWallDestructionEffect(pos);
                if (this.audioManager) {
                    this.audioManager.playSound('explosion', pos, 0.7);
                }
                break;
            case GP.GameEventType.PowerupActivated:
                this.createEnhancedPowerupCollectEffect(pos);
                if (this.audioManager) {
                    this.audioManager.playSound('powerupCollect', pos);
                }
                break;
            case GP.GameEventType.FlagCaptured:
                this.createEnhancedFlagCaptureEffect(pos);
                if (this.audioManager) {
                    this.audioManager.playSound('flagCapture', pos);
                }
                break;
            case GP.GameEventType.FlagGrabbed:
                if (this.audioManager) this.audioManager.playSound('flagGrabbed', pos, 0.6);
                break;
            case GP.GameEventType.FlagDropped:
                 if (this.audioManager) this.audioManager.playSound('flagDropped', pos, 0.5);
                break;
            case GP.GameEventType.FlagReturned:
                if (this.audioManager) this.audioManager.playSound('flagReturned', pos, 0.7);
                break;
            case GP.GameEventType.MeleeAttack:
                this.createMeleeEffect(pos, event.instigator_id);
                if (this.audioManager) {
                    this.audioManager.playSound('meleeSwing', pos, 0.7);
                }
                break;
        }
    }

    createEnhancedBulletImpact(position, weaponType) {
        const impactConfigs = {
            [window.GP.WeaponType.Pistol]: { size: 4, sparkCount: 3, color: 0xFFFF00 },
            [window.GP.WeaponType.Shotgun]: { size: 3, sparkCount: 2, color: 0xFF6600 },
            [window.GP.WeaponType.Rifle]: { size: 5, sparkCount: 4, color: 0x6666FF },
            [window.GP.WeaponType.Sniper]: { size: 8, sparkCount: 6, color: 0xFF00FF }
        };
        
        const config = impactConfigs[weaponType] || impactConfigs[window.GP.WeaponType.Pistol];
        
        // Impact flash
        const impact = new PIXI.Graphics();
        impact.beginFill(config.color, 0.9);
        impact.drawCircle(0, 0, config.size);
        impact.endFill();
        impact.position.set(position.x, position.y);
        impact.filters = [new PIXI.BlurFilter(2)];
        this.effectsContainer.addChild(impact);
        
        // Sparks
        for (let i = 0; i < config.sparkCount; i++) {
            const spark = new PIXI.Sprite(this.particleTextures.spark);
            spark.anchor.set(0.5);
            spark.position.set(position.x, position.y);
            spark.tint = config.color;
            spark.scale.set(0.5 + Math.random() * 0.5);
            
            const angle = Math.random() * Math.PI * 2;
            const speed = 2 + Math.random() * 4;
            spark.velocity = {
                x: Math.cos(angle) * speed,
                y: Math.sin(angle) * speed
            };
            
            this.effectsContainer.addChild(spark);
            
            this.animateEffect(spark, {
                duration: 300,
                onUpdate: p => {
                    spark.x += spark.velocity.x * (1 - p);
                    spark.y += spark.velocity.y * (1 - p);
                    spark.alpha = 1 - p;
                    spark.scale.set(spark.scale.x * 0.98);
                },
                onComplete: () => spark.destroy()
            });
        }
        
        this.animateEffect(impact, {
            duration: 150,
            onUpdate: p => {
                impact.scale.set(1 + p * 3);
                impact.alpha = 1 - p;
            },
            onComplete: () => impact.destroy()
        });
    }

    createEnhancedMuzzleFlash(position, weaponType, instigatorId) {
        const playerSprite = window.playerContainer?.children.find(s => s.playerId === instigatorId);
        if (!playerSprite) return;

        const flashConfigs = {
            [window.GP.WeaponType.Pistol]: { size: 15, color: 0xFFFF66, points: 4 },
            [window.GP.WeaponType.Shotgun]: { size: 22, color: 0xFF6600, points: 6 },
            [window.GP.WeaponType.Rifle]: { size: 18, color: 0x6666FF, points: 5 },
            [window.GP.WeaponType.Sniper]: { size: 25, color: 0xFF66FF, points: 8 }
        };
        
        const config = flashConfigs[weaponType] || flashConfigs[window.GP.WeaponType.Pistol];
        
        // Multi-layered flash
        const flashContainer = new PIXI.Container();
        
        // Outer glow
        const glow = new PIXI.Graphics();
        glow.beginFill(config.color, 0.3);
        glow.drawCircle(0, 0, config.size * 1.5);
        glow.endFill();
        glow.filters = [new PIXI.BlurFilter(4)];
        flashContainer.addChild(glow);
        
        // Main flash
        const flash = new PIXI.Graphics();
        flash.beginFill(config.color, 0.8);
        this.drawStar(flash, 0, 0, config.points, config.size, config.size * 0.4);
        flash.endFill();
        
        // Core
        flash.beginFill(0xFFFFFF, 1);
        flash.drawCircle(0, 0, config.size * 0.3);
        flash.endFill();
        
        flashContainer.addChild(flash);
        
        const gunLength = 15 + 15; // PLAYER_RADIUS + 15
        flashContainer.position.set(gunLength, 0);
        flashContainer.rotation = Math.random() * Math.PI * 2;
        playerSprite.gun.addChild(flashContainer);
        
        this.animateEffect(flashContainer, {
            duration: 100,
            onUpdate: p => {
                flashContainer.scale.set(0.5 + 0.5 * (1 - p));
                flashContainer.alpha = 1 - p;
            },
            onComplete: () => flashContainer.destroy()
        });
    }

    createEnhancedDamageNumbers(position, damage) {
        const container = new PIXI.Container();
        
        // Background glow
        const glow = new PIXI.Graphics();
        glow.beginFill(0xFF0000, 0.3);
        glow.drawCircle(0, 0, 20);
        glow.endFill();
        glow.filters = [new PIXI.BlurFilter(5)];
        container.addChild(glow);
        
        // Damage text with gradient
        const textStyle = new PIXI.TextStyle({
            fontSize: 20,
            fontWeight: 'bold',
            fill: [0xFFFFFF, 0xFF4444],
            fillGradientType: PIXI.TEXT_GRADIENT.LINEAR_VERTICAL,
            stroke: 0x660000,
            strokeThickness: 4,
            dropShadow: true,
            dropShadowColor: 0x000000,
            dropShadowBlur: 4,
            dropShadowDistance: 2
        });
        
        const text = new PIXI.Text('-' + Math.round(damage), textStyle);
        text.anchor.set(0.5);
        container.addChild(text);
        
        container.position.set(position.x, position.y - 15); // PLAYER_RADIUS
        this.effectsContainer.addChild(container);
        
        // Critical hit effect for high damage
        if (damage > 50) {
            text.style.fontSize = 24;
            const criticalBurst = new PIXI.Graphics();
            criticalBurst.lineStyle(2, 0xFFFF00, 0.8);
            this.drawStar(criticalBurst, 0, 0, 8, 25, 15);
            container.addChildAt(criticalBurst, 1);
        }
        
        this.animateEffect(container, {
            duration: 1000,
            onUpdate: p => {
                container.y = position.y - 15 - p * 50;
                container.alpha = 1 - p * 0.7;
                container.scale.set(1 + p * 0.3);
            },
            onComplete: () => container.destroy()
        });
    }

    createEnhancedExplosion(position, radius = 30) {
        const explosionContainer = new PIXI.Container();
        explosionContainer.position.set(position.x, position.y);
        this.effectsContainer.addChild(explosionContainer);
        
        // Shockwave ring
        const shockwave = new PIXI.Graphics();
        shockwave.lineStyle(3, 0xFFAA00, 0.8);
        shockwave.drawCircle(0, 0, 10);
        explosionContainer.addChild(shockwave);
        
        // Main explosion
        const explosion = new PIXI.Graphics();
        explosion.beginFill(0xFFFF00, 0.8);
        explosion.drawCircle(0, 0, radius * 0.5);
        explosion.endFill();
        explosion.beginFill(0xFF6600, 0.6);
        explosion.drawCircle(0, 0, radius * 0.7);
        explosion.endFill();
        explosion.beginFill(0xFF0000, 0.4);
        explosion.drawCircle(0, 0, radius);
        explosion.endFill();
        explosion.filters = [new PIXI.BlurFilter(3)];
        explosionContainer.addChild(explosion);
        
        // Particles
        const particleCount = 20 + Math.floor(radius / 10);
        for (let i = 0; i < particleCount; i++) {
            const particle = new PIXI.Sprite(this.particleTextures.spark);
            particle.anchor.set(0.5);
            particle.position.set(0, 0);
            
            const angle = (Math.PI * 2 * i) / particleCount + Math.random() * 0.5;
            const speed = 3 + Math.random() * 5;
            particle.velocity = {
                x: Math.cos(angle) * speed,
                y: Math.sin(angle) * speed - 2
            };
            particle.angularVelocity = (Math.random() - 0.5) * 0.3;
            
            particle.tint = [0xFFFF00, 0xFF6600, 0xFF0000][Math.floor(Math.random() * 3)];
            particle.scale.set(0.5 + Math.random());
            
            explosionContainer.addChild(particle);
            
            this.animateEffect(particle, {
                duration: 800 + Math.random() * 400,
                gravity: 0.3,
                onUpdate: (progress) => {
                    particle.x += particle.velocity.x * (1 - progress * 0.5);
                    particle.y += particle.velocity.y + progress * 20;
                    particle.rotation += particle.angularVelocity;
                    particle.alpha = 1 - progress;
                    particle.scale.set(particle.scale.x * 0.98);
                },
                onComplete: () => particle.destroy()
            });
        }
        
        // Animate main explosion
        this.animateEffect(explosion, {
            duration: 400,
            onUpdate: (progress) => {
                explosion.scale.set(0.5 + progress * 1);
                explosion.alpha = 1 - progress * 0.8;
            },
            onComplete: () => explosion.destroy()
        });
        
        // Animate shockwave
        this.animateEffect(shockwave, {
            duration: 600,
            onUpdate: (progress) => {
                shockwave.scale.set(1 + progress * 4);
                shockwave.alpha = 1 - progress;
            },
            onComplete: () => {
                shockwave.destroy();
                if (explosionContainer.children.length === 0) {
                    explosionContainer.destroy();
                }
            }
        });
    }

    createEnhancedWallDestructionEffect(position) {
        // Dust cloud
        const dustCloud = new PIXI.Graphics();
        dustCloud.beginFill(0x666666, 0.5);
        dustCloud.drawCircle(0, 0, 40);
        dustCloud.endFill();
        dustCloud.position.set(position.x, position.y);
        dustCloud.filters = [new PIXI.BlurFilter(8)];
        this.effectsContainer.addChild(dustCloud);
        
        // Debris pieces
        for (let i = 0; i < 15; i++) {
            const debris = new PIXI.Sprite(this.particleTextures.debris);
            debris.anchor.set(0.5);
            debris.position.set(position.x, position.y);
            debris.tint = [0x374151, 0x4B5563, 0x6B7280][Math.floor(Math.random() * 3)];
            
            const angle = Math.random() * Math.PI * 2;
            const speed = Math.random() * 6 + 2;
            const velocityX = Math.cos(angle) * speed;
            const velocityY = Math.sin(angle) * speed - 5;
            const angularVelocity = (Math.random() - 0.5) * 0.4;
            
            this.effectsContainer.addChild(debris);
            
            this.animateEffect(debris, {
                duration: 1200,
                velocityX,
                velocityY,
                gravity: 0.4,
                onUpdate: (progress) => {
                    debris.position.x += velocityX * (1 - progress * 0.5);
                    debris.position.y += velocityY + progress * 25;
                    debris.rotation += angularVelocity;
                    debris.alpha = 1 - progress * 0.7;
                },
                onComplete: () => debris.destroy()
            });
        }
        
        // Dust particles
        for (let i = 0; i < 10; i++) {
            const dust = new PIXI.Sprite(this.particleTextures.smoke);
            dust.anchor.set(0.5);
            dust.position.set(
                position.x + (Math.random() - 0.5) * 30,
                position.y + (Math.random() - 0.5) * 30
            );
            dust.scale.set(0.5 + Math.random() * 0.5);
            dust.alpha = 0.5;
            
            this.effectsContainer.addChild(dust);
            
            this.animateEffect(dust, {
                duration: 2000,
                onUpdate: (progress) => {
                    dust.y -= progress * 30;
                    dust.scale.set(dust.scale.x * 1.01);
                    dust.alpha = 0.5 * (1 - progress);
                },
                onComplete: () => dust.destroy()
            });
        }
        
        this.animateEffect(dustCloud, {
            duration: 800,
            onUpdate: (progress) => {
                dustCloud.scale.set(1 + progress);
                dustCloud.alpha = 0.5 * (1 - progress);
            },
            onComplete: () => dustCloud.destroy()
        });
    }

    createEnhancedPowerupCollectEffect(position) {
        const container = new PIXI.Container();
        container.position.set(position.x, position.y);
        this.effectsContainer.addChild(container);
        
        // Energy burst
        const burst = new PIXI.Graphics();
        burst.beginFill(0x00FF00, 0.6);
        this.drawStar(burst, 0, 0, 8, 30, 15);
        burst.endFill();
        container.addChild(burst);
        
        // Ring waves
        for (let i = 0; i < 3; i++) {
            const ring = new PIXI.Graphics();
            ring.lineStyle(2, 0x00FF00, 0.8);
            ring.drawCircle(0, 0, 10);
            container.addChild(ring);
            
            this.animateEffect(ring, {
                duration: 600,
                delay: i * 100,
                onUpdate: (progress) => {
                    ring.scale.set(1 + progress * 3);
                    ring.alpha = 0.8 * (1 - progress);
                },
                onComplete: () => ring.destroy()
            });
        }
        
        // Sparkles
        for (let i = 0; i < 12; i++) {
            const sparkle = new PIXI.Graphics();
            sparkle.beginFill(0xFFFFFF, 0.9);
            sparkle.drawCircle(0, 0, 2);
            sparkle.endFill();
            
            const angle = (Math.PI * 2 * i) / 12;
            const distance = 20;
            sparkle.position.set(
                Math.cos(angle) * distance,
                Math.sin(angle) * distance
            );
            
            container.addChild(sparkle);
            
            this.animateEffect(sparkle, {
                duration: 500,
                onUpdate: (progress) => {
                    const currentDistance = distance * (1 + progress);
                    sparkle.position.set(
                        Math.cos(angle) * currentDistance,
                        Math.sin(angle) * currentDistance
                    );
                    sparkle.alpha = 1 - progress;
                    sparkle.scale.set(1 - progress * 0.5);
                },
                onComplete: () => sparkle.destroy()
            });
        }
        
        this.animateEffect(burst, {
            duration: 400,
            onUpdate: (progress) => {
                burst.scale.set(0.5 + progress * 1.5);
                burst.alpha = 0.6 * (1 - progress);
                burst.rotation = progress * Math.PI;
            },
            onComplete: () => {
                burst.destroy();
                if (container.children.length === 0) {
                    container.destroy();
                }
            }
        });
    }

    createMeleeEffect(position, instigatorId) {
        const playerSprite = window.playerContainer?.children.find(s => s.playerId === instigatorId);
        if (!playerSprite) return;

        // Generate a random color for each melee attack
        const meleeColors = [
            0xFF0066, // Hot Pink
            0x00FFFF, // Cyan
            0xFF6600, // Orange
            0x66FF00, // Lime
            0xFF00FF, // Magenta
            0x0066FF, // Blue
            0xFFFF00, // Yellow
            0x00FF66  // Spring Green
        ];
        const color = meleeColors[Math.floor(Math.random() * meleeColors.length)];

        // Create melee slash effect
        const slashContainer = new PIXI.Container();
        slashContainer.position.set(position.x, position.y);
        this.effectsContainer.addChild(slashContainer);

        // Arc slash
        const slash = new PIXI.Graphics();
        slash.lineStyle(4, color, 0.9);
        
        // Draw arc based on player rotation
        const playerRotation = playerSprite.rotation - Math.PI / 2;
        const startAngle = playerRotation - Math.PI / 3;
        const endAngle = playerRotation + Math.PI / 3;
        const radius = 25;
        
        // Draw arc
        for (let i = 0; i <= 20; i++) {
            const angle = startAngle + (endAngle - startAngle) * (i / 20);
            const x = Math.cos(angle) * radius;
            const y = Math.sin(angle) * radius;
            if (i === 0) {
                slash.moveTo(x, y);
            } else {
                slash.lineTo(x, y);
            }
        }
        
        slashContainer.addChild(slash);

        // Energy trails
        for (let i = 0; i < 5; i++) {
            const trail = new PIXI.Graphics();
            trail.lineStyle(2, color, 0.5);
            
            const trailRadius = radius + i * 3;
            const trailDelay = i * 30;
            
            this.animateEffect(trail, {
                duration: 200,
                delay: trailDelay,
                onUpdate: (progress) => {
                    trail.clear();
                    trail.lineStyle(2, color, 0.5 * (1 - progress));
                    
                    const arcProgress = progress;
                    const currentStart = startAngle + (endAngle - startAngle) * arcProgress * 0.3;
                    const currentEnd = startAngle + (endAngle - startAngle) * (arcProgress * 0.7 + 0.3);
                    
                    for (let j = 0; j <= 10; j++) {
                        const angle = currentStart + (currentEnd - currentStart) * (j / 10);
                        const x = Math.cos(angle) * trailRadius;
                        const y = Math.sin(angle) * trailRadius;
                        if (j === 0) {
                            trail.moveTo(x, y);
                        } else {
                            trail.lineTo(x, y);
                        }
                    }
                },
                onComplete: () => trail.destroy()
            });
            
            slashContainer.addChild(trail);
        }

        // Impact particles
        const particleCount = 8;
        for (let i = 0; i < particleCount; i++) {
            const particle = new PIXI.Graphics();
            particle.beginFill(color, 0.8);
            particle.drawCircle(0, 0, 2);
            particle.endFill();
            
            const angle = startAngle + (endAngle - startAngle) * (i / (particleCount - 1));
            const speed = 2 + Math.random() * 3;
            const particleX = Math.cos(angle) * radius;
            const particleY = Math.sin(angle) * radius;
            particle.position.set(particleX, particleY);
            
            slashContainer.addChild(particle);
            
            this.animateEffect(particle, {
                duration: 400,
                onUpdate: (progress) => {
                    particle.x = particleX + Math.cos(angle) * speed * progress * 20;
                    particle.y = particleY + Math.sin(angle) * speed * progress * 20;
                    particle.alpha = 0.8 * (1 - progress);
                    particle.scale.set(1 - progress * 0.5);
                },
                onComplete: () => particle.destroy()
            });
        }

        // Main slash animation
        this.animateEffect(slash, {
            duration: 150,
            onUpdate: (progress) => {
                slash.alpha = 1 - progress * 0.3;
                slash.scale.set(1 + progress * 0.2);
            },
            onComplete: () => {
                slash.destroy();
                if (slashContainer.children.length === 0) {
                    slashContainer.destroy();
                }
            }
        });
    }

    createEnhancedFlagCaptureEffect(position) {
        const container = new PIXI.Container();
        container.position.set(position.x, position.y);
        this.effectsContainer.addChild(container);
        
        // Fireworks effect
        const colors = [0xFF0000, 0x0000FF, 0xFFFF00, 0x00FF00, 0xFF00FF];
        
        for (let burst = 0; burst < 3; burst++) {
            setTimeout(() => {
                const burstContainer = new PIXI.Container();
                container.addChild(burstContainer);
                
                // Central flash
                const flash = new PIXI.Graphics();
                flash.beginFill(0xFFFFFF, 0.8);
                flash.drawCircle(0, 0, 15);
                flash.endFill();
                burstContainer.addChild(flash);
                
                // Firework particles
                const particleCount = 30;
                for (let i = 0; i < particleCount; i++) {
                    const particle = new PIXI.Graphics();
                    const color = colors[Math.floor(Math.random() * colors.length)];
                    particle.beginFill(color);
                    particle.drawCircle(0, 0, 3);
                    particle.endFill();
                    
                    const angle = (Math.PI * 2 * i) / particleCount;
                    const speed = 5 + Math.random() * 5;
                    const velocityX = Math.cos(angle) * speed;
                    const velocityY = Math.sin(angle) * speed - 10;
                    
                    burstContainer.addChild(particle);
                    
                    // Add trail
                    const trail = new PIXI.Graphics();
                    trail.lineStyle(2, color, 0.5);
                    burstContainer.addChildAt(trail, 0);
                    
                    let lastX = 0, lastY = 0;
                    
                    this.animateEffect(particle, {
                        duration: 1500,
                        velocityX,
                        velocityY,
                        gravity: 0.4,
                        onUpdate: (progress) => {
                            particle.x += velocityX * (1 - progress * 0.5);
                            particle.y += velocityY + progress * 30;
                            particle.alpha = 1 - progress;
                            
                            // Update trail
                            trail.clear();
                            trail.lineStyle(2, color, 0.5 * (1 - progress));
                            trail.moveTo(lastX, lastY);
                            trail.lineTo(particle.x, particle.y);
                            lastX = particle.x;
                            lastY = particle.y;
                        },
                        onComplete: () => {
                            particle.destroy();
                            trail.destroy();
                        }
                    });
                }
                
                this.animateEffect(flash, {
                    duration: 200,
                    onUpdate: (progress) => {
                        flash.scale.set(1 + progress * 2);
                        flash.alpha = 0.8 * (1 - progress);
                    },
                    onComplete: () => {
                        flash.destroy();
                        if (burstContainer.children.length === 0) {
                            burstContainer.destroy();
                        }
                    }
                });
            }, burst * 200);
        }
        
        // Clean up container after all effects
        setTimeout(() => {
            if (container.parent) {
                container.destroy();
            }
        }, 2000);
    }

    animateEffect(object, config) {
        const effect = {
            object,
            startTime: Date.now() + (config.delay || 0),
            started: false,
            ...config
        };
        this.activeEffects.push(effect);
    }

    update(deltaMS) {
        const now = Date.now();
        this.activeEffects = this.activeEffects.filter(effect => {
            if (now < effect.startTime) return true;
            if (!effect.started) {
                effect.started = true;
                effect.actualStartTime = now;
            }
            
            const elapsed = now - effect.actualStartTime;
            const progress = Math.min(elapsed / effect.duration, 1);
            
            if (effect.object && !effect.object.destroyed) {
                effect.onUpdate(progress);
            }
            
            if (progress >= 1) {
                if (effect.onComplete && effect.object && !effect.object.destroyed) {
                    effect.onComplete();
                }
                return false;
            }
            return true;
        });
    }

    clearAllEffects() {
        this.activeEffects.forEach(effect => {
            if (effect.object && !effect.object.destroyed) effect.object.destroy();
        });
        this.activeEffects = [];
        this.effectsContainer.removeChildren().forEach(c => c.destroy());
    }

    // Helper function to draw star
    drawStar(graphics, x, y, points, outerRadius, innerRadius) {
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
}
