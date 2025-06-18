// Part 4 - Sprite Updates and Networking

// Update all sprites based on game state
function updateSprites() {
    const viewportBounds = getViewportBounds();

    // Update player sprites
    players.forEach((player, playerId) => {
        const isInViewport = viewportBounds && viewportBounds.contains(
            player.render_x !== undefined ? player.render_x : player.x,
            player.render_y !== undefined ? player.render_y : player.y
        );

        let sprite = playerContainer.children.find(s => s.playerId === playerId);

        if (!sprite && isInViewport) {
            sprite = createPlayerSprite(player, playerId === myPlayerId);
            playerContainer.addChild(sprite);
            if (playerId === myPlayerId) {
                localPlayerSprite = sprite;
            }
        }

        if (sprite) {
            sprite.visible = isInViewport;
            if (isInViewport) {
                updatePlayerSprite(sprite, player);
            }
        }
    });

    // Remove sprites for disconnected players
    playerContainer.children.filter(s => !players.has(s.playerId)).forEach(s => {
        playerContainer.removeChild(s);
        s.destroy({ children: true });
    });

    // Update projectile sprites with proper interpolation
    projectiles.forEach((projectile, projId) => {
        const projX = projectile.render_x !== undefined ? projectile.render_x : projectile.x;
        const projY = projectile.render_y !== undefined ? projectile.render_y : projectile.y;
        
        const isInViewport = viewportBounds && viewportBounds.contains(projX, projY);

        let sprite = projectileContainer.children.find(s => s.projectileId === projId);

        if (!sprite && isInViewport) {
            sprite = createProjectileSprite(projectile);
            projectileContainer.addChild(sprite);
        }

        if (sprite) {
            sprite.visible = isInViewport;
            if (isInViewport) {
                updateProjectileSprite(sprite, projectile);
            }
        }
    });

    // Remove sprites for destroyed projectiles
    projectileContainer.children.filter(s => !projectiles.has(s.projectileId)).forEach(s => {
        projectileContainer.removeChild(s);
        s.destroy({ children: true });
    });

    // Update pickup sprites
    pickups.forEach((pickup, pickupId) => {
        const isInViewport = viewportBounds && viewportBounds.contains(pickup.x, pickup.y);

        let sprite = pickupContainer.children.find(s => s.pickupId === pickupId);

        if (!sprite && isInViewport) {
            sprite = createPickupSprite(pickup);
            pickupContainer.addChild(sprite);
        }

        if (sprite) {
            sprite.visible = isInViewport;
            if (isInViewport) {
                sprite.position.set(pickup.x, pickup.y);
            }
        }
    });

    // Remove sprites for collected pickups
    pickupContainer.children.filter(s => !pickups.has(s.pickupId)).forEach(s => {
        pickupContainer.removeChild(s);
        s.destroy({ children: true });
    });
}

// Update game stats UI
function updateGameStatsUI() {
    if (!localPlayerState) return;

    playerHealthSpan.textContent = Math.ceil(localPlayerState.health);
    playerShieldSpan.textContent = Math.ceil(localPlayerState.shield_current || 0);
    playerAmmoSpan.textContent = localPlayerState.ammo;

    const healthPercent = localPlayerState.health / localPlayerState.max_health;
    if (healthPercent <= 0.3) {
        playerHealthSpan.style.color = '#EF4444';
    } else if (healthPercent <= 0.6) {
        playerHealthSpan.style.color = '#F59E0B';
    } else {
        playerHealthSpan.style.color = '#10B981';
    }

    // Update health vignette effect
    if (healthVignette) {
        updateHealthVignette(healthVignette, healthPercent);
    }

    const maxAmmo = getMaxAmmoForWeaponClient(localPlayerState.weapon);
    if (localPlayerState.ammo === 0 && maxAmmo > 0) {
        reloadPromptSpan.textContent = '(Press R to reload)';
    } else {
        reloadPromptSpan.textContent = '';
    }

    playerWeaponSpan.textContent = weaponNames[localPlayerState.weapon] || 'Unknown';
    playerScoreSpan.textContent = localPlayerState.score;
    playerKillsSpan.textContent = localPlayerState.kills;
    playerDeathsSpan.textContent = localPlayerState.deaths;

    const teamName = localPlayerState.team_id === 0 ? 'FFA' :
        localPlayerState.team_id === 1 ? 'Red' :
            localPlayerState.team_id === 2 ? 'Blue' : 'None';
    playerTeamSpan.textContent = teamName;
    playerTeamSpan.className = localPlayerState.team_id === 1 ? 'text-red-400' :
        localPlayerState.team_id === 2 ? 'text-blue-400' : 'text-gray-100';

    playerCountSpan.textContent = players.size;
    pingDisplay.textContent = ping;

    // Update powerup status
    powerupStatusDiv.innerHTML = '';
    if (localPlayerState.speed_boost_remaining > 0) {
        const speedDiv = document.createElement('div');
        speedDiv.className = 'powerup-indicator';
        speedDiv.innerHTML = `<span class="icon">💨</span> Speed Boost: ${Math.ceil(localPlayerState.speed_boost_remaining)}s`;
        powerupStatusDiv.appendChild(speedDiv);
    }
    if (localPlayerState.damage_boost_remaining > 0) {
        const damageDiv = document.createElement('div');
        damageDiv.className = 'powerup-indicator';
        damageDiv.innerHTML = `<span class="icon">💥</span> Damage Boost: ${Math.ceil(localPlayerState.damage_boost_remaining)}s`;
        powerupStatusDiv.appendChild(damageDiv);
    }

    if (networkIndicator) {
        networkIndicator.updateQuality(ping);
    }
}

// Send inputs to server
function sendInputsToServer() {
    const now = Date.now();
    if (now - lastInputSendTime < 1000 / INPUT_SEND_RATE) return;
    if (!dataChannel || dataChannel.readyState !== 'open' || !localPlayerState || !localPlayerState.alive) return;

    const builder = new flatbuffers.Builder(256);
    const clientInput = GP.ClientInput.createClientInput(
        builder,
        ++inputSequence,
        inputState.move_forward,
        inputState.move_backward,
        inputState.move_left,
        inputState.move_right,
        inputState.shooting,
        inputState.reload,
        inputState.rotation,
        inputState.melee_attack,
        inputState.change_weapon_slot,
        inputState.use_ability_slot
    );
    builder.finish(clientInput);
    dataChannel.send(builder.asUint8Array());

    pendingInputs.push({
        sequenceNumber: inputSequence,
        input: { ...inputState },
        timestamp: now
    });

    if (pendingInputs.length > RECONCILIATION_BUFFER_SIZE) {
        pendingInputs.shift();
    }

    inputState.change_weapon_slot = 0;
    inputState.use_ability_slot = 0;

    lastInputSendTime = now;
}

// Initialize enhanced graphics
function initializeEnhancedGraphics(app, worldContainer) {
    class EffectsManager {
        constructor(app, worldContainer) {
            this.app = app;
            this.worldContainer = worldContainer;
            this.activeEffects = new Set();
        }

        createExplosion(x, y, config = {}) {
            const defaults = {
                color: 0xFF6B6B,
                size: 30,
                duration: 500,
                particles: 20
            };
            const settings = { ...defaults, ...config };

            // Main explosion flash
            const flash = new PIXI.Graphics();
            flash.beginFill(settings.color, 0.8);
            flash.drawCircle(0, 0, settings.size);
            flash.endFill();
            flash.position.set(x, y);
            this.worldContainer.addChild(flash);

            // Explosion ring
            const ring = new PIXI.Graphics();
            ring.lineStyle(3, settings.color, 0.6);
            ring.drawCircle(0, 0, settings.size * 0.5);
            ring.position.set(x, y);
            this.worldContainer.addChild(ring);

            // Particles
            const particles = [];
            for (let i = 0; i < settings.particles; i++) {
                const particle = new PIXI.Graphics();
                particle.beginFill(settings.color, 1);
                particle.drawCircle(0, 0, Math.random() * 3 + 1);
                particle.endFill();
                particle.position.set(x, y);

                const angle = (Math.PI * 2 * i) / settings.particles + Math.random() * 0.5;
                particle.velocity = {
                    x: Math.cos(angle) * (Math.random() * 5 + 2),
                    y: Math.sin(angle) * (Math.random() * 5 + 2)
                };
                particle.life = 1;

                this.worldContainer.addChild(particle);
                particles.push(particle);
            }

            // Animate explosion
            let elapsed = 0;
            const effect = {
                update: (deltaMs) => {
                    elapsed += deltaMs;
                    const progress = elapsed / settings.duration;

                    if (progress >= 1) {
                        this.worldContainer.removeChild(flash);
                        this.worldContainer.removeChild(ring);
                        particles.forEach(p => this.worldContainer.removeChild(p));
                        flash.destroy();
                        ring.destroy();
                        particles.forEach(p => p.destroy());
                        this.activeEffects.delete(effect);
                        return;
                    }

                    // Animate flash
                    flash.scale.set(1 + progress * 2);
                    flash.alpha = 1 - progress;

                    // Animate ring
                    ring.scale.set(1 + progress * 3);
                    ring.alpha = (1 - progress) * 0.6;

                    // Animate particles
                    particles.forEach(p => {
                        p.x += p.velocity.x;
                        p.y += p.velocity.y;
                        p.velocity.x *= 0.98;
                        p.velocity.y *= 0.98;
                        p.alpha = 1 - progress;
                        p.scale.set((1 - progress) * 1.5);
                    });
                }
            };

            this.activeEffects.add(effect);
            return effect;
        }

        createMuzzleFlash(x, y, rotation, weaponType) {
            const configs = {
                [GP.WeaponType.Pistol]: { color: 0xFFBF00, size: 15, duration: 100 },
                [GP.WeaponType.Shotgun]: { color: 0xFF4444, size: 25, duration: 150 },
                [GP.WeaponType.Rifle]: { color: 0x4444FF, size: 20, duration: 80 },
                [GP.WeaponType.Sniper]: { color: 0xAA44FF, size: 30, duration: 120 }
            };

            const config = configs[weaponType] || configs[GP.WeaponType.Pistol];

            const flash = new PIXI.Graphics();
            flash.beginFill(config.color, 0.8);
            flash.drawCircle(0, 0, config.size);
            flash.endFill();
            flash.position.set(x, y);
            flash.rotation = rotation;
            this.worldContainer.addChild(flash);

            let elapsed = 0;
            const effect = {
                update: (deltaMs) => {
                    elapsed += deltaMs;
                    const progress = elapsed / config.duration;

                    if (progress >= 1) {
                        this.worldContainer.removeChild(flash);
                        flash.destroy();
                        this.activeEffects.delete(effect);
                        return;
                    }

                    flash.scale.set(1 - progress * 0.5);
                    flash.alpha = 1 - progress;
                }
            };

            this.activeEffects.add(effect);
            return effect;
        }

        createHitEffect(x, y, damage) {
            // Impact flash
            const impact = new PIXI.Graphics();
            impact.beginFill(0xFFFFFF, 0.9);
            impact.drawCircle(0, 0, 10 + damage / 10);
            impact.endFill();
            impact.position.set(x, y);
            this.worldContainer.addChild(impact);

            // Blood splatter particles
            const bloodParticles = [];
            const particleCount = Math.min(15, 5 + damage / 5);

            for (let i = 0; i < particleCount; i++) {
                const particle = new PIXI.Graphics();
                particle.beginFill(0xFF0000, 0.8);
                particle.drawCircle(0, 0, Math.random() * 2 + 1);
                particle.endFill();
                particle.position.set(x, y);

                const angle = Math.random() * Math.PI * 2;
                const speed = Math.random() * 3 + 1;
                particle.velocity = {
                    x: Math.cos(angle) * speed,
                    y: Math.sin(angle) * speed + 1 // Add gravity
                };

                this.worldContainer.addChild(particle);
                bloodParticles.push(particle);
            }

            let elapsed = 0;
            const effect = {
                update: (deltaMs) => {
                    elapsed += deltaMs;
                    const progress = elapsed / 300;

                    if (progress >= 1) {
                        this.worldContainer.removeChild(impact);
                        bloodParticles.forEach(p => this.worldContainer.removeChild(p));
                        impact.destroy();
                        bloodParticles.forEach(p => p.destroy());
                        this.activeEffects.delete(effect);
                        return;
                    }

                    // Animate impact
                    impact.scale.set(1 + progress);
                    impact.alpha = 1 - progress;

                    // Animate blood particles
                    bloodParticles.forEach(p => {
                        p.x += p.velocity.x;
                        p.y += p.velocity.y;
                        p.velocity.y += 0.2; // Gravity
                        p.alpha = 1 - progress;
                    });
                }
            };

            this.activeEffects.add(effect);
            return effect;
        }

        update(deltaMs) {
            this.activeEffects.forEach(effect => effect.update(deltaMs));
        }
    }

    class AudioManager {
        constructor() {
            this.audioContext = new (window.AudioContext || window.webkitAudioContext)();
            this.masterVolume = 0.5;
            this.sounds = new Map();
            this.loadSounds();
        }

        loadSounds() {
            // Create procedural sounds
            this.sounds.set('shoot_pistol', this.createShootSound(800, 0.1));
            this.sounds.set('shoot_shotgun', this.createShootSound(400, 0.15, true));
            this.sounds.set('shoot_rifle', this.createShootSound(1200, 0.08));
            this.sounds.set('shoot_sniper', this.createShootSound(1500, 0.12));
            this.sounds.set('hit', this.createHitSound());
            this.sounds.set('explosion', this.createExplosionSound());
            this.sounds.set('pickup', this.createPickupSound());
            this.sounds.set('reload', this.createReloadSound());
        }

        createShootSound(frequency, duration, burst = false) {
            return () => {
                const oscillator = this.audioContext.createOscillator();
                const gainNode = this.audioContext.createGain();

                oscillator.connect(gainNode);
                gainNode.connect(this.audioContext.destination);

                oscillator.frequency.value = frequency;
                oscillator.type = burst ? 'sawtooth' : 'square';

                const now = this.audioContext.currentTime;
                gainNode.gain.setValueAtTime(this.masterVolume * 0.3, now);
                gainNode.gain.exponentialRampToValueAtTime(0.01, now + duration);

                oscillator.start(now);
                oscillator.stop(now + duration);
            };
        }

        createHitSound() {
            return () => {
                const oscillator = this.audioContext.createOscillator();
                const gainNode = this.audioContext.createGain();

                oscillator.connect(gainNode);
                gainNode.connect(this.audioContext.destination);

                oscillator.frequency.value = 200;
                oscillator.type = 'sawtooth';

                const now = this.audioContext.currentTime;
                gainNode.gain.setValueAtTime(this.masterVolume * 0.2, now);
                gainNode.gain.exponentialRampToValueAtTime(0.01, now + 0.05);

                oscillator.start(now);
                oscillator.stop(now + 0.05);
            };
        }

        createExplosionSound() {
            return () => {
                const noise = this.audioContext.createBufferSource();
                const buffer = this.audioContext.createBuffer(1, this.audioContext.sampleRate * 0.5, this.audioContext.sampleRate);
                const data = buffer.getChannelData(0);

                for (let i = 0; i < data.length; i++) {
                    data[i] = (Math.random() - 0.5) * 2;
                }

                noise.buffer = buffer;

                const filter = this.audioContext.createBiquadFilter();
                filter.type = 'lowpass';
                filter.frequency.value = 400;

                const gainNode = this.audioContext.createGain();

                noise.connect(filter);
                filter.connect(gainNode);
                gainNode.connect(this.audioContext.destination);

                const now = this.audioContext.currentTime;
                gainNode.gain.setValueAtTime(this.masterVolume * 0.5, now);
                gainNode.gain.exponentialRampToValueAtTime(0.01, now + 0.5);

                noise.start(now);
            };
        }

        createPickupSound() {
            return () => {
                const oscillator = this.audioContext.createOscillator();
                const gainNode = this.audioContext.createGain();

                oscillator.connect(gainNode);
                gainNode.connect(this.audioContext.destination);

                oscillator.type = 'sine';

                const now = this.audioContext.currentTime;
                oscillator.frequency.setValueAtTime(400, now);
                oscillator.frequency.exponentialRampToValueAtTime(800, now + 0.1);

                gainNode.gain.setValueAtTime(this.masterVolume * 0.2, now);
                gainNode.gain.exponentialRampToValueAtTime(0.01, now + 0.1);

                oscillator.start(now);
                oscillator.stop(now + 0.1);
            };
        }

        createReloadSound() {
            return () => {
                const oscillator = this.audioContext.createOscillator();
                const gainNode = this.audioContext.createGain();

                oscillator.connect(gainNode);
                gainNode.connect(this.audioContext.destination);

                oscillator.type = 'square';
                oscillator.frequency.value = 150;

                const now = this.audioContext.currentTime;
                gainNode.gain.setValueAtTime(0, now);
                gainNode.gain.linearRampToValueAtTime(this.masterVolume * 0.1, now + 0.05);
                gainNode.gain.linearRampToValueAtTime(0, now + 0.2);

                oscillator.start(now);
                oscillator.stop(now + 0.2);
            };
        }

        play(soundName, volume = 1.0) {
            if (!gameSettings.soundEnabled) return;
            const soundGenerator = this.sounds.get(soundName);
            if (soundGenerator) {
                this.masterVolume = gameSettings.soundVolume * volume;
                soundGenerator();
            }
        }

        setVolume(volume) {
            this.masterVolume = Math.max(0, Math.min(1, volume));
        }
    }

    const audioManager = new AudioManager();
    const effectsManager = new EffectsManager(app, worldContainer);
    const starfield = createStarfield(app);
    app.stage.addChildAt(starfield, 0);

    const healthVignette = createHealthVignette(app);
    app.stage.addChild(healthVignette);

    return { audioManager, effectsManager, starfield, healthVignette };
}

// Network message handlers will be added in the final part...
