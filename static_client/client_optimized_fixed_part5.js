// Part 5 - Networking and Final Setup

// Handle WebRTC data channel messages
function handleDataChannelMessage(event) {
    const data = new Uint8Array(event.data);
    const buf = new flatbuffers.ByteBuffer(data);

    const messageWrapper = GP.MessageWrapper.getRootAsMessageWrapper(buf);
    const messageType = messageWrapper.messageType();

    switch (messageType) {
        case GP.MessageType.Ping:
            handlePingMessage(messageWrapper);
            break;
        case GP.MessageType.GameState:
            handleGameStateMessage(messageWrapper);
            break;
        case GP.MessageType.ServerMessage:
            handleServerMessage(messageWrapper);
            break;
        case GP.MessageType.InitialState:
            handleInitialStateMessage(messageWrapper);
            break;
        case GP.MessageType.Event:
            handleEventMessage(messageWrapper);
            break;
        default:
            console.warn('Unknown message type:', messageType);
    }
}

function handlePingMessage(messageWrapper) {
    const pingMsg = messageWrapper.message(new GP.Ping());
    const now = Date.now();
    ping = now - pingStartTime;
}

function handleGameStateMessage(messageWrapper) {
    const gameState = messageWrapper.message(new GP.GameState());
    const timestamp = Date.now();

    const playerUpdates = new Map();
    const projectileUpdates = new Map();

    // Update players
    for (let i = 0; i < gameState.playersLength(); i++) {
        const player = gameState.players(i);
        const playerId = player.id();

        const playerData = {
            id: playerId,
            username: player.username(),
            team_id: player.teamId(),
            x: player.x(),
            y: player.y(),
            rotation: player.rotation(),
            velocity_x: player.velocityX(),
            velocity_y: player.velocityY(),
            health: player.health(),
            max_health: player.maxHealth(),
            shield_current: player.shieldCurrent(),
            shield_max: player.shieldMax(),
            weapon: player.weapon(),
            ammo: player.ammo(),
            score: player.score(),
            kills: player.kills(),
            deaths: player.deaths(),
            alive: player.alive(),
            respawn_timer: player.respawnTimer(),
            speed_boost_remaining: player.speedBoostRemaining(),
            damage_boost_remaining: player.damageBoostRemaining(),
            is_carrying_flag_team_id: player.isCarryingFlagTeamId(),
        };

        players.set(playerId, playerData);
        playerUpdates.set(playerId, { ...playerData });

        if (playerId === myPlayerId) {
            if (player.lastProcessedInput() > lastProcessedInput) {
                lastProcessedInput = player.lastProcessedInput();
                pendingInputs = pendingInputs.filter(input => input.sequenceNumber > lastProcessedInput);
            }

            localPlayerState = playerData;
            
            // Apply reconciliation
            const serverPos = { x: playerData.x, y: playerData.y };
            let predictedPos = { ...serverPos };
            
            pendingInputs.forEach(pendingInput => {
                const deltaTime = 1 / SERVER_TICK_RATE;
                let moveX = 0, moveY = 0;
                
                if (pendingInput.input.move_forward) moveY -= 1;
                if (pendingInput.input.move_backward) moveY += 1;
                if (pendingInput.input.move_left) moveX -= 1;
                if (pendingInput.input.move_right) moveX += 1;
                
                if (moveX !== 0 || moveY !== 0) {
                    const magnitude = Math.sqrt(moveX * moveX + moveY * moveY);
                    const effectiveSpeed = playerData.speed_boost_remaining > 0 ? 225 : 150;
                    predictedPos.x += (moveX / magnitude) * effectiveSpeed * deltaTime;
                    predictedPos.y += (moveY / magnitude) * effectiveSpeed * deltaTime;
                }
            });
            
            localPlayerState.x = predictedPos.x;
            localPlayerState.y = predictedPos.y;
            localPlayerState.render_x = predictedPos.x;
            localPlayerState.render_y = predictedPos.y;
            localPlayerState.render_rotation = inputState.rotation;
        }
    }

    // Update projectiles with velocity for smooth interpolation
    for (let i = 0; i < gameState.projectilesLength(); i++) {
        const proj = gameState.projectiles(i);
        const projId = proj.id();

        const projectileData = {
            id: projId,
            x: proj.x(),
            y: proj.y(),
            velocity_x: proj.velocityX(),
            velocity_y: proj.velocityY(),
            weapon_type: proj.weaponType(),
            shooter_id: proj.shooterId(),
            damage_multiplier: proj.damageMultiplier(),
        };

        projectiles.set(projId, projectileData);
        projectileUpdates.set(projId, { ...projectileData });
    }

    // Remove destroyed projectiles
    const currentProjectileIds = new Set();
    for (let i = 0; i < gameState.projectilesLength(); i++) {
        currentProjectileIds.add(gameState.projectiles(i).id());
    }
    projectiles.forEach((_, projId) => {
        if (!currentProjectileIds.has(projId)) {
            projectiles.delete(projId);
        }
    });

    // Update pickups
    pickups.clear();
    for (let i = 0; i < gameState.pickupsLength(); i++) {
        const pickup = gameState.pickups(i);
        pickups.set(pickup.id(), {
            id: pickup.id(),
            x: pickup.x(),
            y: pickup.y(),
            pickup_type: pickup.pickupType(),
            weapon_type: pickup.weaponType(),
        });
    }

    // Update flags
    const newFlagStates = [];
    for (let i = 0; i < gameState.flagStatesLength(); i++) {
        const flag = gameState.flagStates(i);
        newFlagStates.push({
            team_id: flag.teamId(),
            status: flag.status(),
            position: { x: flag.positionX(), y: flag.positionY() },
            respawn_timer: flag.respawnTimer()
        });
    }
    if (newFlagStates.length > 0) {
        updateFlags(newFlagStates);
    }

    // Update match info
    if (gameState.matchInfo()) {
        const mi = gameState.matchInfo();
        matchInfo = {
            game_mode: mi.gameMode(),
            map_name: mi.mapName(),
            time_remaining: mi.timeRemaining(),
            score_limit: mi.scoreLimit(),
            team_scores: []
        };
        
        for (let i = 0; i < mi.teamScoresLength(); i++) {
            const ts = mi.teamScores(i);
            matchInfo.team_scores.push({
                team_id: ts.teamId(),
                score: ts.score()
            });
        }
        
        currentMapName = matchInfo.map_name || "Unknown Map";
        updateMatchInfoUI();
    }

    // Store update for interpolation
    serverUpdates.push({
        timestamp,
        players: playerUpdates,
        projectiles: projectileUpdates
    });

    // Keep only recent updates
    if (serverUpdates.length > 60) {
        serverUpdates.shift();
    }
}

function handleInitialStateMessage(messageWrapper) {
    const initialState = messageWrapper.message(new GP.InitialState());
    
    myPlayerId = initialState.playerId();
    myPlayerIdSpan.textContent = myPlayerId;
    log(`Connected! Your player ID is: ${myPlayerId}`, 'success');
    
    // Load walls
    walls.clear();
    for (let i = 0; i < initialState.wallsLength(); i++) {
        const wall = initialState.walls(i);
        walls.set(wall.id(), {
            id: wall.id(),
            x: wall.x(),
            y: wall.y(),
            width: wall.width(),
            height: wall.height(),
            is_destructible: wall.isDestructible(),
            current_health: wall.currentHealth(),
            max_health: wall.maxHealth(),
        });
    }
    drawWalls();
    
    controlsDiv.classList.remove('hidden');
    killFeedDiv.classList.remove('hidden');
    chatDisplayDiv.classList.remove('hidden');
    chatInputArea.classList.remove('hidden');
    matchInfoDiv.classList.remove('hidden');
}

function handleServerMessage(messageWrapper) {
    const serverMsg = messageWrapper.message(new GP.ServerMessage());
    const message = serverMsg.message();
    
    log(`Server: ${message}`, 'info');
    
    // Also add to chat for important messages
    if (message.includes('joined') || message.includes('left') || message.includes('started')) {
        addChatMessage('System', message, true);
    }
}

function handleEventMessage(messageWrapper) {
    const event = messageWrapper.message(new GP.Event());
    const eventType = event.eventType();
    
    switch (eventType) {
        case GP.EventType.Kill:
            handleKillEvent(event);
            break;
        case GP.EventType.ChatMessage:
            handleChatMessageEvent(event);
            break;
        case GP.EventType.Shoot:
            handleShootEvent(event);
            break;
        case GP.EventType.PickupCollected:
            handlePickupCollectedEvent(event);
            break;
        case GP.EventType.Reload:
            handleReloadEvent(event);
            break;
        case GP.EventType.WallDamaged:
            handleWallDamagedEvent(event);
            break;
        case GP.EventType.WallDestroyed:
            handleWallDestroyedEvent(event);
            break;
        case GP.EventType.FlagAction:
            handleFlagActionEvent(event);
            break;
        case GP.EventType.Hit:
            handleHitEvent(event);
            break;
        case GP.EventType.PlayerRespawn:
            handlePlayerRespawnEvent(event);
            break;
    }
}

// Event handlers
function handleKillEvent(event) {
    const killEvent = event.data(new GP.KillEvent());
    const killerName = killEvent.killerName() || 'Unknown';
    const victimName = killEvent.victimName() || 'Unknown';
    const weaponType = killEvent.weaponType();
    
    addKillFeedEntry(killerName, victimName, weaponType);
    
    if (killEvent.killerId() === myPlayerId) {
        createScreenFlash(app, 0x00FF00, 10, 0.3);
    } else if (killEvent.victimId() === myPlayerId) {
        createScreenFlash(app, 0xFF0000, 20, 0.5);
    }
}

function handleChatMessageEvent(event) {
    const chatEvent = event.data(new GP.ChatMessageEvent());
    addChatMessage(chatEvent.username(), chatEvent.message(), false);
}

function handleShootEvent(event) {
    const shootEvent = event.data(new GP.ShootEvent());
    const shooterId = shootEvent.shooterId();
    const weaponType = shootEvent.weaponType();
    
    const shooter = players.get(shooterId);
    if (shooter && effectsManager) {
        const gunOffset = 20;
        const muzzleX = shooter.x + Math.cos(shooter.rotation) * gunOffset;
        const muzzleY = shooter.y + Math.sin(shooter.rotation) * gunOffset;
        
        effectsManager.createMuzzleFlash(muzzleX, muzzleY, shooter.rotation, weaponType);
        
        if (audioManager && shooterId === myPlayerId) {
            const soundMap = {
                [GP.WeaponType.Pistol]: 'shoot_pistol',
                [GP.WeaponType.Shotgun]: 'shoot_shotgun',
                [GP.WeaponType.Rifle]: 'shoot_rifle',
                [GP.WeaponType.Sniper]: 'shoot_sniper',
            };
            const soundName = soundMap[weaponType];
            if (soundName) audioManager.play(soundName);
        }
    }
}

function handlePickupCollectedEvent(event) {
    const pickupEvent = event.data(new GP.PickupCollectedEvent());
    
    if (pickupEvent.playerId() === myPlayerId && audioManager) {
        audioManager.play('pickup');
    }
}

function handleReloadEvent(event) {
    const reloadEvent = event.data(new GP.ReloadEvent());
    
    if (reloadEvent.playerId() === myPlayerId && audioManager) {
        audioManager.play('reload');
    }
}

function handleWallDamagedEvent(event) {
    const wallEvent = event.data(new GP.WallDamagedEvent());
    const wall = walls.get(wallEvent.wallId());
    
    if (wall) {
        wall.current_health = wallEvent.newHealth();
        drawWalls();
        
        if (effectsManager) {
            const centerX = wall.x + wall.width / 2;
            const centerY = wall.y + wall.height / 2;
            effectsManager.createExplosion(centerX, centerY, {
                color: 0x808080,
                size: 20,
                duration: 300,
                particles: 10
            });
        }
    }
}

function handleWallDestroyedEvent(event) {
    const wallEvent = event.data(new GP.WallDestroyedEvent());
    const wall = walls.get(wallEvent.wallId());
    
    if (wall) {
        wall.current_health = 0;
        drawWalls();
        
        if (effectsManager && gameSettings.screenShake) {
            const centerX = wall.x + wall.width / 2;
            const centerY = wall.y + wall.height / 2;
            effectsManager.createExplosion(centerX, centerY, {
                color: 0xFF6B6B,
                size: 50,
                duration: 500,
                particles: 30
            });
            applyScreenShake(gameScene, 30, 10);
        }
        
        if (audioManager) {
            audioManager.play('explosion');
        }
    }
}

function handleFlagActionEvent(event) {
    const flagEvent = event.data(new GP.FlagActionEvent());
    const action = flagEvent.action();
    const playerName = flagEvent.playerName();
    const teamId = flagEvent.teamId();
    
    let message = '';
    switch (action) {
        case GP.FlagActionType.Pickup:
            message = `${playerName} picked up the ${teamId === 1 ? 'Red' : 'Blue'} flag!`;
            break;
        case GP.FlagActionType.Drop:
            message = `${playerName} dropped the ${teamId === 1 ? 'Red' : 'Blue'} flag!`;
            break;
        case GP.FlagActionType.Return:
            message = `${playerName} returned the ${teamId === 1 ? 'Red' : 'Blue'} flag!`;
            break;
        case GP.FlagActionType.Capture:
            message = `${playerName} captured the ${teamId === 1 ? 'Red' : 'Blue'} flag!`;
            createScreenFlash(app, teamId === 1 ? 0xFF0000 : 0x0000FF, 20, 0.5);
            break;
    }
    
    if (message) {
        addChatMessage('System', message, true);
    }
}

function handleHitEvent(event) {
    const hitEvent = event.data(new GP.HitEvent());
    const targetId = hitEvent.targetId();
    const damage = hitEvent.damage();
    
    const target = players.get(targetId);
    if (target && effectsManager) {
        effectsManager.createHitEffect(target.x, target.y, damage);
        
        if (targetId === myPlayerId) {
            if (gameSettings.screenShake) {
                applyScreenShake(gameScene, 15, 5);
            }
            createScreenFlash(app, 0xFF0000, 10, 0.2);
        }
        
        if (audioManager && (targetId === myPlayerId || hitEvent.shooterId() === myPlayerId)) {
            audioManager.play('hit');
        }
    }
}

function handlePlayerRespawnEvent(event) {
    const respawnEvent = event.data(new GP.PlayerRespawnEvent());
    
    if (respawnEvent.playerId() === myPlayerId) {
        createScreenFlash(app, 0xFFFFFF, 15, 0.3);
    }
}

// UI Update functions
function addKillFeedEntry(killer, victim, weaponType) {
    const entry = document.createElement('div');
    entry.className = 'kill-entry';
    
    const weaponIcon = weaponType === GP.WeaponType.Melee ? '🗡️' : '💀';
    entry.innerHTML = `<span style="color: #FF6B6B">${killer}</span> ${weaponIcon} <span style="color: #9CA3AF">${victim}</span>`;
    
    killFeedDiv.insertBefore(entry, killFeedDiv.firstChild);
    killFeed.push(entry);
    
    if (killFeed.length > 5) {
        const oldEntry = killFeed.shift();
        killFeedDiv.removeChild(oldEntry);
    }
    
    setTimeout(() => {
        if (killFeedDiv.contains(entry)) {
            entry.style.opacity = '0';
            setTimeout(() => {
                if (killFeedDiv.contains(entry)) {
                    killFeedDiv.removeChild(entry);
                }
                const index = killFeed.indexOf(entry);
                if (index > -1) {
                    killFeed.splice(index, 1);
                }
            }, 500);
        }
    }, 5000);
}

function addChatMessage(username, message, isSystem = false) {
    const entry = document.createElement('div');
    entry.className = 'chat-entry';
    
    if (isSystem) {
        entry.style.color = '#60A5FA';
        entry.innerHTML = `<span style="font-style: italic">${message}</span>`;
    } else {
        entry.innerHTML = `<span class="username">${username}:</span> ${message}`;
    }
    
    chatDisplayDiv.appendChild(entry);
    chatMessages.push(entry);
    
    if (chatMessages.length > 50) {
        const oldEntry = chatMessages.shift();
        chatDisplayDiv.removeChild(oldEntry);
    }
    
    chatDisplayDiv.scrollTop = chatDisplayDiv.scrollHeight;
}

function updateMatchInfoUI() {
    if (!matchInfo) return;
    
    let html = '';
    
    if (matchInfo.game_mode === GP.GameMode.FreeForAll) {
        html = `<div>FFA - ${currentMapName}</div>`;
        if (matchInfo.time_remaining > 0) {
            const minutes = Math.floor(matchInfo.time_remaining / 60);
            const seconds = Math.floor(matchInfo.time_remaining % 60);
            html += `<div>${minutes}:${seconds.toString().padStart(2, '0')}</div>`;
        }
    } else if (matchInfo.game_mode === GP.GameMode.TeamDeathmatch || 
               matchInfo.game_mode === GP.GameMode.CaptureTheFlag) {
        html = `<div>${matchInfo.game_mode === GP.GameMode.TeamDeathmatch ? 'TDM' : 'CTF'} - ${currentMapName}</div>`;
        html += '<div class="team-scores">';
        
        matchInfo.team_scores.forEach(ts => {
            const teamClass = ts.team_id === 1 ? 'team-red' : 'team-blue';
            const teamName = ts.team_id === 1 ? 'Red' : 'Blue';
            html += `<span class="team-score ${teamClass}">${teamName}: ${ts.score}</span>`;
        });
        
        html += '</div>';
        
        if (matchInfo.time_remaining > 0) {
            const minutes = Math.floor(matchInfo.time_remaining / 60);
            const seconds = Math.floor(matchInfo.time_remaining % 60);
            html += `<div>${minutes}:${seconds.toString().padStart(2, '0')}</div>`;
        }
    }
    
    matchInfoDiv.innerHTML = html;
}

// Complete all missing references
window.Minimap = Minimap;
window.NetworkIndicator = NetworkIndicator;
window.toggleScoreboard = toggleScoreboard;

// Initialize everything when DOM is loaded
document.addEventListener('DOMContentLoaded', initializeGame);
