// Part 1 - Constants and Imports
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
