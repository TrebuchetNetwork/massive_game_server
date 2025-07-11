import * as flatbuffers from 'flatbuffers';
import { GameProtocol } from './generated_js/game.js';
const GP = GameProtocol;

// Game constants
const INTERPOLATION_DELAY = 100; // ms
const INPUT_SEND_RATE = 60; // Hz
const RECONCILIATION_BUFFER_SIZE = 120;
const PLAYER_RADIUS = 15;
const PICKUP_RADIUS = 20;
const MIN_PLAYERS_TO_START = 2;
const MAX_CHAT_MESSAGE_LENGTH = 100;
const SERVER_TICK_RATE = 60;
const VIEW_DISTANCE_BUFFER = 150; // Pixels outside viewport to keep updating

// Team colors
const teamColors = {
    0: 0xA0A0A0, // Neutral/FFA - A distinct Grey
    1: 0xFF6B6B, // Team 1 - Red
    2: 0x4ECDC4, // Team 2 - Teal/Blue
};
const defaultEnemyColor = 0xF87171; // Less critical if all players get team colors

// Weapon data
const weaponNames = {
    [GP.WeaponType.Pistol]: 'Pistol',
    [GP.WeaponType.Shotgun]: 'Shotgun',
    [GP.WeaponType.Rifle]: 'Rifle',
    [GP.WeaponType.Sniper]: 'Sniper',
    [GP.WeaponType.Melee]: 'Melee'
};

const weaponColors = {
    [GP.WeaponType.Pistol]: 0xFFBF00,
    [GP.WeaponType.Shotgun]: 0xFF4444,
    [GP.WeaponType.Rifle]: 0x4444FF,
    [GP.WeaponType.Sniper]: 0xAA44FF,
    [GP.WeaponType.Melee]: 0xD1D5DB
};

// Pickup data
const pickupTypes = {
    [GP.PickupType.Health]: 'Health',
    [GP.PickupType.Ammo]: 'Ammo',
    [GP.PickupType.WeaponCrate]: 'Weapon',
    [GP.PickupType.SpeedBoost]: 'Speed',
    [GP.PickupType.DamageBoost]: 'Damage',
    [GP.PickupType.Shield]: 'Shield',
    [GP.PickupType.FlagRed]: 'Red Flag',
    [GP.PickupType.FlagBlue]: 'Blue Flag'
};

const pickupColors = {
    [GP.PickupType.Health]: 0x10B981,
    [GP.PickupType.Ammo]: 0xF59E0B,
    [GP.PickupType.WeaponCrate]: 0x60A5FA,
    [GP.PickupType.SpeedBoost]: 0x00FFFF,
    [GP.PickupType.DamageBoost]: 0xFF6B6B,
    [GP.PickupType.Shield]: 0x00BFFF,
    [GP.PickupType.FlagRed]: 0xFF0000,
    [GP.PickupType.FlagBlue]: 0x0000FF
};

// DOM Elements
const wsUrlInput = document.getElementById('wsUrl');
const connectButton = document.getElementById('connectButton');
const chatInput = document.getElementById('chatInput');
const sendChatButton = document.getElementById('sendChatButton');
const logOutput = document.getElementById('log');
const controlsDiv = document.getElementById('controls');
const killFeedDiv = document.getElementById('killFeed');
const chatDisplayDiv = document.getElementById('chatDisplay');
const chatInputArea = document.getElementById('chatInputArea'); // Fixed chat area
const matchInfoDiv = document.getElementById('matchInfo');
const pingDisplay = document.getElementById('pingDisplay');
const networkQualityIndicatorDiv = document.getElementById('networkQualityIndicator');
const settingsButton = document.getElementById('settingsButton');
const settingsMenuDiv = document.getElementById('settingsMenu');
const saveSettingsButton = document.getElementById('saveSettingsButton');
const cancelSettingsButton = document.getElementById('cancelSettingsButton');
const scoreboardDiv = document.getElementById('scoreboard');
const fpsCounterDiv = document.getElementById('fpsCounter');
const fpsValueSpan = document.getElementById('fpsValue');

// Game Stats UI Elements
const myPlayerIdSpan = document.getElementById('myPlayerIdSpan');
const playerTeamSpan = document.getElementById('playerTeam');
const playerHealthSpan = document.getElementById('playerHealth');
const playerShieldSpan = document.getElementById('playerShield');
const playerAmmoSpan = document.getElementById('playerAmmo');
const reloadPromptSpan = document.getElementById('reloadPrompt');
const playerWeaponSpan = document.getElementById('playerWeapon');
const playerScoreSpan = document.getElementById('playerScore');
const playerKillsSpan = document.getElementById('playerKills');
const playerDeathsSpan = document.getElementById('playerDeaths');
const playerCountSpan = document.getElementById('playerCount');
const powerupStatusDiv = document.getElementById('powerupStatus');

let starfield;
let healthVignette;

// WebRTC & WebSocket Variables
let signalingSocket;
let peerConnection;
let dataChannel;

// PIXI.js Application
let app;
let gameScene;
let worldContainer;
let hudContainer;
let wallGraphics;
let pickupContainer;
let projectileContainer;
let playerContainer;
let flagContainer;
let localPlayerSprite;

// Game State
let myPlayerId = null;
let players = new Map();
let projectiles = new Map();
let walls = new Map();
let pickups = new Map();
let flagStates = new Map();
let killFeed = [];
let chatMessages = [];
let matchInfo = null;
let currentMapName = "Unknown Map";

// Client-side prediction state
let inputSequence = 0;
let pendingInputs = [];
let lastProcessedInput = 0;
let localPlayerState = null;

// Interpolation state
let serverUpdates = [];
let renderTimestamp = 0;

// Input state
let inputState = {
    move_forward: false,
    move_backward: false,
    move_left: false,
    move_right: false,
    shooting: false,
    reload: false,
    rotation: 0,
    melee_attack: false,
    change_weapon_slot: 0,
    use_ability_slot: 0
};

// Timing
let lastInputSendTime = 0;
let pingStartTime = 0;
let ping = 0;
let frameCount = 0;
let lastFPSUpdate = 0;

// Managers
let effectsManager;
let audioManager;
let minimap;
let networkIndicator;

// Game Settings
let gameSettings = {
    soundEnabled: true,
    soundVolume: 0.5,
    musicEnabled: false,
    musicVolume: 0.3,
    graphicsQuality: 'medium',
    showFPS: false,
    sensitivity: 1.0,
    particleEffects: true,
    screenShake: true
};

const peerConnectionConfig = {
    'iceServers': [{ 'urls': 'stun:stun.l.google.com:19302' }]
};

// Utility function to mix colors
function mixColors(color1, color2, amount) {
    const c1 = PIXI.Color.shared.setValue(color1).toRgbArray();
    const c2 = PIXI.Color.shared.setValue(color2).toRgbArray();
    const r = c1[0] * (1 - amount) + c2[0] * amount;
    const g = c1[1] * (1 - amount) + c2[1] * amount;
    const b = c1[2] * (1 - amount) + c2[2] * amount;
    return PIXI.Color.shared.setValue([r, g, b]).toNumber();
}

function log(message, type = 'info') {
    const entry = document.createElement('div');
    entry.textContent = `[${new Date().toLocaleTimeString()}] ${message}`;
    entry.classList.add('log-entry', `log-${type}`);
    logOutput.appendChild(entry);
    logOutput.scrollTop = logOutput.scrollHeight;
}

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
    window.app = app;
    window.gameScene = gameScene;
    window.localPlayerState = localPlayerState;
    window.myPlayerId = myPlayerId;
    window.playerContainer = playerContainer;
    window.GP = GP;

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
