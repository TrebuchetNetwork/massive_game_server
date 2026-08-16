/**
 * Effects + audio runtime extracted from client.html.
 *
 * Keeps manager implementations unchanged while refreshing live runtime refs
 * before each method call so dynamic state (app/player/maps) stays current.
 */

import { emitClientLog } from './client_logger.js';

export function createEffectsAudioRuntime({
  PIXI: PIXIRef = globalThis.PIXI,
  GP: GPRef = globalThis.GP || {},
  DAMAGE_NUMBER_CONFIGS = Object.freeze({}),
  DAMAGE_NUMBER_BITMAP_FONT_BASE = 'mgs_damage_bitmap',
  DAMAGE_NUMBER_BITMAP_CHARS = '+-!~0123456789',
  DAMAGE_NUMBER_POOL_PREALLOC = 40,
  DAMAGE_NUMBER_POOL_MAX = 360,
  DAMAGE_NUMBER_BATCH_ENABLED = true,
  DAMAGE_NUMBER_BATCH_MAX_PENDING = 200,
  DAMAGE_NUMBER_BATCH_FLUSH_LIMIT_HARD = 120,
  DAMAGE_NUMBER_BATCH_FLUSH_LIMIT_SOFT = 72,
  DAMAGE_NUMBER_BATCH_WINDOW_MS_DESKTOP = 24,
  DAMAGE_NUMBER_BATCH_WINDOW_MS_MOBILE = 40,
  DAMAGE_NUMBER_BATCH_WINDOW_MS_SOFT = 48,
  DAMAGE_NUMBER_BATCH_WINDOW_MS_HARD = 64,
  DAMAGE_NUMBER_BATCH_CELL_SIZE_DESKTOP = 12,
  DAMAGE_NUMBER_BATCH_CELL_SIZE_MOBILE = 20,
  DAMAGE_NUMBER_ACTIVE_CAP_SOFT = 92,
  DAMAGE_NUMBER_ACTIVE_CAP_HARD = 52,
  DAMAGE_NUMBER_MERGE_MAX_KEYS_DESKTOP = 64,
  DAMAGE_NUMBER_MERGE_MAX_KEYS_MOBILE = 26,
  DAMAGE_NUMBER_MERGE_FALLBACK_BUCKETS_DESKTOP = 20,
  DAMAGE_NUMBER_MERGE_FALLBACK_BUCKETS_MOBILE = 12,
  DAMAGE_NUMBER_MERGE_CELL_MIN_DESKTOP = 10,
  DAMAGE_NUMBER_MERGE_CELL_MIN_MOBILE = 16,
  EFFECTS_UPDATE_SKIP_STRIDE_SOFT = 2,
  EFFECTS_UPDATE_SKIP_STRIDE_HARD = 3,
  TOURNAMENT_MODE_FORCED = false,
  BENCH_MODE = false,
  STABLE_MODE_FORCED = false,
  LOW_OVERHEAD_MODE = false,
  PLAYER_RADIUS = 15,
  getProjectiles = () => new Map(),
  getPlayers = () => new Map(),
  getWalls = () => new Map(),
  getZones = () => new Map(),
  getLocalPlayerState = () => null,
  getMyPlayerId = () => null,
  getGameSettings = () => ({}),
  getApp = () => null,
  getGameScene = () => null,
  getUltraPerformanceMode = () => false,
  getSmoothedFrameMs = () => 16,
  getDeviceClassification = () => 'desktop',
  applyScreenShake = () => {},
  createScreenFlash = () => {},
  drawStar = () => {},
  isMobileSoundBudget = false,
} = {}) {
  const PIXI = PIXIRef;
  const GP = GPRef;
  const DAMAGE_BATCH_ENABLED = !!DAMAGE_NUMBER_BATCH_ENABLED;

  // Keep legacy names expected by extracted class bodies.
  const mobileDynamicsEnabled = !!isMobileSoundBudget;
  const forceMobileClient = false;

  let projectiles = getProjectiles() || new Map();
  let players = getPlayers() || new Map();
  let walls = getWalls() || new Map();
  let zones = getZones() || new Map();
  let localPlayerState = getLocalPlayerState() || null;
  let myPlayerId = getMyPlayerId() || null;
  let gameSettings = getGameSettings() || {};
  let app = getApp() || null;
  let gameScene = getGameScene() || null;
  let ultraPerformanceMode = !!getUltraPerformanceMode();
  let smoothedFrameMs = Number(getSmoothedFrameMs()) || 16;
  let deviceClassification = getDeviceClassification() || 'desktop';

  const safeRefresh = (getter, fallback) => {
    try {
      const next = getter();
      return typeof next === 'undefined' ? fallback : next;
    } catch (_) {
      return fallback;
    }
  };

  const refreshRuntimeRefs = () => {
    projectiles = safeRefresh(getProjectiles, projectiles) || new Map();
    players = safeRefresh(getPlayers, players) || new Map();
    walls = safeRefresh(getWalls, walls) || new Map();
    zones = safeRefresh(getZones, zones) || new Map();
    localPlayerState = safeRefresh(getLocalPlayerState, localPlayerState) || null;
    myPlayerId = safeRefresh(getMyPlayerId, myPlayerId) || null;
    gameSettings = safeRefresh(getGameSettings, gameSettings) || {};
    app = safeRefresh(getApp, app) || null;
    gameScene = safeRefresh(getGameScene, gameScene) || null;
    ultraPerformanceMode = !!safeRefresh(getUltraPerformanceMode, ultraPerformanceMode);
    smoothedFrameMs = Number(safeRefresh(getSmoothedFrameMs, smoothedFrameMs)) || 16;
    deviceClassification = safeRefresh(getDeviceClassification, deviceClassification) || 'desktop';
  };

  const getEntityWorldPosition = (entity) => {
    if (!entity) return null;
    const x = Number.isFinite(Number(entity.render_x))
      ? Number(entity.render_x)
      : Number(entity.x);
    const y = Number.isFinite(Number(entity.render_y))
      ? Number(entity.render_y)
      : Number(entity.y);
    if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
    return { x, y };
  };

  const pointInRect = (x, y, rect) => {
    if (!rect) return false;
    const rectX = Number(rect.x);
    const rectY = Number(rect.y);
    const rectW = Number(rect.width);
    const rectH = Number(rect.height);
    if (!Number.isFinite(rectX) || !Number.isFinite(rectY) || !Number.isFinite(rectW) || !Number.isFinite(rectH)) {
      return false;
    }
    return x >= rectX && x <= (rectX + rectW) && y >= rectY && y <= (rectY + rectH);
  };

  const distancePointToRect = (x, y, rect) => {
    const rectX = Number(rect?.x);
    const rectY = Number(rect?.y);
    const rectW = Number(rect?.width);
    const rectH = Number(rect?.height);
    if (!Number.isFinite(rectX) || !Number.isFinite(rectY) || !Number.isFinite(rectW) || !Number.isFinite(rectH)) {
      return Number.POSITIVE_INFINITY;
    }
    const dx = Math.max(rectX - x, 0, x - (rectX + rectW));
    const dy = Math.max(rectY - y, 0, y - (rectY + rectH));
    return Math.sqrt(dx * dx + dy * dy);
  };

  const segmentIntersectsAABB = (start, end, rect) => {
    const rectX = Number(rect?.x);
    const rectY = Number(rect?.y);
    const rectW = Number(rect?.width);
    const rectH = Number(rect?.height);
    if (!Number.isFinite(rectX) || !Number.isFinite(rectY) || !Number.isFinite(rectW) || !Number.isFinite(rectH)) {
      return false;
    }

    const minX = rectX;
    const minY = rectY;
    const maxX = rectX + rectW;
    const maxY = rectY + rectH;
    const dx = end.x - start.x;
    const dy = end.y - start.y;
    let tMin = 0;
    let tMax = 1;

    if (Math.abs(dx) < 1e-5) {
      if (start.x < minX || start.x > maxX) return false;
    } else {
      const invDx = 1 / dx;
      let t1 = (minX - start.x) * invDx;
      let t2 = (maxX - start.x) * invDx;
      if (t1 > t2) [t1, t2] = [t2, t1];
      tMin = Math.max(tMin, t1);
      tMax = Math.min(tMax, t2);
      if (tMin > tMax) return false;
    }

    if (Math.abs(dy) < 1e-5) {
      if (start.y < minY || start.y > maxY) return false;
    } else {
      const invDy = 1 / dy;
      let t1 = (minY - start.y) * invDy;
      let t2 = (maxY - start.y) * invDy;
      if (t1 > t2) [t1, t2] = [t2, t1];
      tMin = Math.max(tMin, t1);
      tMax = Math.min(tMax, t2);
      if (tMin > tMax) return false;
    }

    return true;
  };

  const countWallIntersections = (listenerPos, soundPos, wallMap, maxIntersections = 3) => {
    if (!listenerPos || !soundPos || !wallMap || typeof wallMap.values !== 'function') {
      return 0;
    }
    const minSegX = Math.min(listenerPos.x, soundPos.x);
    const maxSegX = Math.max(listenerPos.x, soundPos.x);
    const minSegY = Math.min(listenerPos.y, soundPos.y);
    const maxSegY = Math.max(listenerPos.y, soundPos.y);
    let intersections = 0;

    for (const wall of wallMap.values()) {
      const wallX = Number(wall?.x);
      const wallY = Number(wall?.y);
      const wallW = Number(wall?.width);
      const wallH = Number(wall?.height);
      if (!Number.isFinite(wallX) || !Number.isFinite(wallY) || !Number.isFinite(wallW) || !Number.isFinite(wallH)) {
        continue;
      }
      if ((wallX + wallW) < minSegX || wallX > maxSegX || (wallY + wallH) < minSegY || wallY > maxSegY) {
        continue;
      }
      if (distancePointToRect(listenerPos.x, listenerPos.y, wall) < 20 || distancePointToRect(soundPos.x, soundPos.y, wall) < 20) {
        continue;
      }
      if (!segmentIntersectsAABB(listenerPos, soundPos, wall)) {
        continue;
      }
      intersections += 1;
      if (intersections >= maxIntersections) {
        return intersections;
      }
    }
    return intersections;
  };

  const getZoneReverbProfileKey = (zoneMap, playerState) => {
    const position = getEntityWorldPosition(playerState);
    if (!position || !zoneMap || typeof zoneMap.values !== 'function') {
      return null;
    }

    let matchedKey = null;
    for (const zone of zoneMap.values()) {
      if (!pointInRect(position.x, position.y, zone)) continue;
      const zoneType = Number(zone?.zone_type);
      if (zoneType === GP.ZoneType.DamageZone || zoneType === 1) {
        return 'damage';
      }
      if (zoneType === GP.ZoneType.BoostPad || zoneType === 2) {
        matchedKey = matchedKey || 'boost';
      } else if (zoneType === GP.ZoneType.SlowZone || zoneType === 0) {
        matchedKey = matchedKey || 'slow';
      }
    }
    return matchedKey;
  };

  const blurFilterCache = new Map();
  const getSharedBlurFilter = (strength) => {
    const key = Number(strength);
    if (!Number.isFinite(key) || key <= 0) return null;
    if (blurFilterCache.has(key)) {
      return blurFilterCache.get(key);
    }
    const filter = new PIXI.BlurFilter(key);
    blurFilterCache.set(key, filter);
    return filter;
  };
class EffectsManager {
    constructor(app, container, audioManager = null) {
this.app = app;
const targetContainer = container && typeof container.addChild === 'function'
    ? container
    : new PIXI.Container();
this.effectsContainer = new PIXI.Container();
targetContainer.addChild(this.effectsContainer);
this.activeEffects = [];
this.pendingTimers = new Set();
this.particlesEnabled = true;
this.audioManager = audioManager;  // Store audio manager reference
this.damageNumberTextStyles = new Map();
this.damageNumberPool = [];
this.damageNumberPoolSize = 0;
this.activeDamageNumberCount = 0;
this.pendingDamageNumberBatches = new Map();
this.activeDamageNumberEffectsByKey = new Map();
this.pendingDamageBatchCount = 0;
this.damageNumberUseBitmapText = !!(
    PIXI &&
    PIXI.BitmapText &&
    PIXI.BitmapFont &&
    typeof PIXI.BitmapFont.from === 'function'
);
this.damageNumberBitmapFontNames = {
    full: `${DAMAGE_NUMBER_BITMAP_FONT_BASE}_full`,
    lite: `${DAMAGE_NUMBER_BITMAP_FONT_BASE}_lite`,
    minimal: `${DAMAGE_NUMBER_BITMAP_FONT_BASE}_minimal`
};
this.damageNumberOnUpdate = this.updateDamageNumberEffect.bind(this);
this.damageNumberOnComplete = this.completeDamageNumberEffect.bind(this);
this.effectStats = {
    dropped: 0,
    evicted: 0
};
this.engineTrailPool = [];
this.engineTrailPoolCursor = 0;
this.engineTrailMidStrideCounter = 0;
this.performanceProfiles = {
    high: {
        maxActiveEffects: 2200,
        durationScale: 1,
        particleScale: 1,
        maxDeferredTimers: 140,
        allowDelayedBursts: true
    },
    medium: {
        maxActiveEffects: 1500,
        durationScale: 0.85,
        particleScale: 0.75,
        maxDeferredTimers: 96,
        allowDelayedBursts: true
    },
    dense: {
        maxActiveEffects: 950,
        durationScale: 0.72,
        particleScale: 0.55,
        maxDeferredTimers: 64,
        allowDelayedBursts: false
    },
    ultra: {
        maxActiveEffects: 560,
        durationScale: 0.55,
        particleScale: 0.36,
        maxDeferredTimers: 28,
        allowDelayedBursts: false
    }
};
this.performanceProfileName = 'high';
this.performanceProfile = this.performanceProfiles.high;
this.maxActiveEffects = this.performanceProfile.maxActiveEffects;
this.effectSpawnSequence = 0;
this.effectUpdateFrame = 0;
this.lastLoadTier = 0;
this.nearMissTriggerByProjectile = new Map();
this.nearMissScanAccumulatorMs = 0;
this.lastNearMissPruneAtMs = 0;
this.nearMissScanCursor = 0;
this.ensureDamageNumberBitmapFonts();
const preallocCount = Math.min(DAMAGE_NUMBER_POOL_PREALLOC, DAMAGE_NUMBER_POOL_MAX);
for (let i = 0; i < preallocCount; i += 1) {
    const entry = this.createDamageNumberPoolEntry();
    if (!entry) break;
    this.damageNumberPool.push(entry);
}

// Pre-generate particle textures
this.particleTextures = this.generateParticleTextures();
const defaultProfile = TOURNAMENT_MODE_FORCED
    ? 'dense'
    : ((BENCH_MODE || STABLE_MODE_FORCED) ? 'ultra' : (LOW_OVERHEAD_MODE ? 'dense' : 'high'));
this.setPerformanceProfile(defaultProfile);
    }

    ensureDamageNumberBitmapFonts() {
if (!this.damageNumberUseBitmapText) return;
const variants = [
    { key: 'full', fontSize: 24 },
    { key: 'lite', fontSize: 19 },
    { key: 'minimal', fontSize: 16 }
];
for (let i = 0; i < variants.length; i += 1) {
    const variant = variants[i];
    const fontName = this.damageNumberBitmapFontNames[variant.key];
    const available = PIXI.BitmapFont?.available;
    if (available && available[fontName]) continue;
    try {
        PIXI.BitmapFont.from(
            fontName,
            {
                fontFamily: 'Arial',
                fontSize: variant.fontSize,
                fontWeight: '700',
                fill: '#ffffff',
                stroke: '#000000',
                strokeThickness: variant.key === 'minimal' ? 1 : 2
            },
            {
                chars: DAMAGE_NUMBER_BITMAP_CHARS
            }
        );
    } catch (error) {
        emitClientLog('Bitmap font generation failed, falling back to PIXI.Text', 'warn', error);
        this.damageNumberUseBitmapText = false;
        return;
    }
}
    }

    createDamageNumberTextNode(variant = 'full') {
const normalizedVariant = variant === 'minimal' ? 'minimal' : (variant === 'lite' ? 'lite' : 'full');
if (this.damageNumberUseBitmapText) {
    const fontName = this.damageNumberBitmapFontNames[normalizedVariant];
    const node = new PIXI.BitmapText('', { fontName });
    if (node.anchor && typeof node.anchor.set === 'function') {
        node.anchor.set(0.5);
    }
    node.visible = false;
    return node;
}

const fallbackStyle = this.getDamageNumberTextStyle('enemyReceived', DAMAGE_NUMBER_CONFIGS.enemyReceived, normalizedVariant);
const node = new PIXI.Text('', fallbackStyle);
node.anchor.set(0.5);
node.resolution = 1;
node.visible = false;
return node;
    }


    generateParticleTextures() {
const textures = {};
const renderer = this.app && this.app.renderer && typeof this.app.renderer.generateTexture === 'function'
    ? this.app.renderer
    : null;
if (!renderer) {
    const fallbackTexture = PIXI.Texture?.WHITE || PIXI.Texture?.EMPTY;
    textures.spark = fallbackTexture;
    textures.sparkRed = fallbackTexture;
    textures.sparkOrange = fallbackTexture;
    textures.sparkBlue = fallbackTexture;
    textures.sparkWhite = fallbackTexture;
    textures.smoke = fallbackTexture;
    textures.smokeLight = fallbackTexture;
    textures.smokeDark = fallbackTexture;
    textures.debris = fallbackTexture;
    textures.debrisBrown = fallbackTexture;
    textures.debrisGray = fallbackTexture;
    textures.trailGlow = fallbackTexture;
    return textures;
}

const buildSparkTexture = (color) => {
    const graphics = new PIXI.Graphics();
    graphics.beginFill(color, 1);
    graphics.drawCircle(0, 0, 2);
    graphics.endFill();
    const texture = renderer.generateTexture(graphics);
    graphics.destroy();
    return texture;
};
const buildSmokeTexture = (color, alpha) => {
    const graphics = new PIXI.Graphics();
    graphics.beginFill(color, alpha);
    graphics.drawCircle(0, 0, 8);
    graphics.endFill();
    graphics.filters = [getSharedBlurFilter(3)];
    const texture = renderer.generateTexture(graphics);
    graphics.destroy();
    return texture;
};
const buildDebrisTexture = (color) => {
    const graphics = new PIXI.Graphics();
    graphics.beginFill(color, 1);
    graphics.drawRect(-3, -3, 6, 6);
    graphics.endFill();
    const texture = renderer.generateTexture(graphics);
    graphics.destroy();
    return texture;
};
const buildSoftGlowTexture = (color) => {
    // Soft radial falloff baked as concentric rings (no runtime blur cost).
    const graphics = new PIXI.Graphics();
    const rings = 5;
    for (let i = 0; i < rings; i += 1) {
        const t = i / (rings - 1);
        graphics.beginFill(color, 0.34 * (1 - t));
        graphics.drawCircle(0, 0, 2.5 + t * 5.5);
        graphics.endFill();
    }
    const texture = renderer.generateTexture(graphics);
    graphics.destroy();
    return texture;
};

textures.spark = buildSparkTexture(0xFFFFFF);
textures.sparkRed = buildSparkTexture(0xF87171);
textures.sparkOrange = buildSparkTexture(0xFB923C);
textures.sparkBlue = buildSparkTexture(0x60A5FA);
textures.sparkWhite = buildSparkTexture(0xF8FAFC);
textures.smoke = buildSmokeTexture(0x888888, 0.5);
textures.smokeLight = buildSmokeTexture(0xCBD5E1, 0.38);
textures.smokeDark = buildSmokeTexture(0x475569, 0.52);
textures.debris = buildDebrisTexture(0x444444);
textures.debrisBrown = buildDebrisTexture(0x8B5E3C);
textures.debrisGray = buildDebrisTexture(0x6B7280);
textures.trailGlow = buildSoftGlowTexture(0xFFFFFF);

return textures;
    }

    findPlayerSpriteById(instigatorId) {
const playerContainerRef = globalThis.playerContainer;
if (!playerContainerRef || !Array.isArray(playerContainerRef.children)) return null;
return (
    playerContainerRef.children.find(
        sprite => sprite && sprite.playerId === instigatorId
    ) || null
);
    }
    
    
    setParticlesEnabled(enabled) {
this.particlesEnabled = enabled;
    }

    setPerformanceProfile(profileName) {
const normalized = (typeof profileName === 'string' ? profileName.toLowerCase() : 'high');
const profile = this.performanceProfiles[normalized] || this.performanceProfiles.high;
this.performanceProfileName = this.performanceProfiles[normalized] ? normalized : 'high';
this.performanceProfile = profile;
this.maxActiveEffects = profile.maxActiveEffects;
let timerOverflow = this.pendingTimers.size - profile.maxDeferredTimers;
if (timerOverflow > 0) {
    for (const timerId of this.pendingTimers) {
        clearTimeout(timerId);
        this.pendingTimers.delete(timerId);
        this.effectStats.dropped += 1;
        timerOverflow -= 1;
        if (timerOverflow <= 0) break;
    }
}
this.dropOverflowEffects(0);
return this.performanceProfileName;
    }

    scaleEffectCount(baseCount, minimum = 1) {
const scaled = Math.round((Number(baseCount) || 0) * (this.performanceProfile.particleScale || 1));
return Math.max(minimum, scaled);
    }

    scaleDuration(durationMs, minimum = 60) {
const scaled = Math.round((Number(durationMs) || 0) * (this.performanceProfile.durationScale || 1));
return Math.max(minimum, scaled);
    }

    getLoadTier() {
const projectileCount = Number(projectiles?.size || 0);
const effectUtilization = this.maxActiveEffects > 0
    ? (this.activeEffects.length / this.maxActiveEffects)
    : 0;

if (
    ultraPerformanceMode ||
    smoothedFrameMs >= 30 ||
    projectileCount >= 120 ||
    effectUtilization >= 0.75
) {
    return 2;
}
if (smoothedFrameMs >= 22 || projectileCount >= 60 || effectUtilization >= 0.55) {
    return 1;
}
return 0;
    }

    getUpdateStrideForLoad(loadTier) {
if (loadTier >= 2) return EFFECTS_UPDATE_SKIP_STRIDE_HARD;
if (loadTier === 1) return EFFECTS_UPDATE_SKIP_STRIDE_SOFT;
return 1;
    }

    shouldEmitEffect(kind = 'generic') {
const loadTier = this.getLoadTier();
if (loadTier <= 0) return true;

const heavy = loadTier >= 2;
let stride = heavy ? 4 : 2;
switch (kind) {
    case 'damage':
        stride = heavy ? 3 : 2;
        break;
    case 'impact':
        stride = heavy ? 3 : 2;
        break;
    case 'muzzle':
        stride = heavy ? 5 : 3;
        break;
    case 'explosion':
        stride = heavy ? 6 : 3;
        break;
    case 'powerup':
        stride = heavy ? 8 : 4;
        break;
    case 'flag':
        stride = heavy ? 10 : 6;
        break;
    case 'movement':
        stride = heavy ? 6 : 3;
        break;
    default:
        break;
}

this.effectSpawnSequence = (this.effectSpawnSequence + 1) % 1000000;
return (this.effectSpawnSequence % stride) === 0;
    }

    createDamageNumberPoolEntry() {
if (this.damageNumberPoolSize >= DAMAGE_NUMBER_POOL_MAX) return null;
const container = new PIXI.Container();
container.visible = false;
container.renderable = false;

const glow = new PIXI.Graphics();
glow.beginFill(0xFFFFFF, 0.25);
glow.drawCircle(0, 0, 20);
glow.endFill();
glow.visible = false;
container.addChild(glow);

const criticalBurst = new PIXI.Graphics();
criticalBurst.lineStyle(2, 0xFFFF00, 0.8);
drawStar(criticalBurst, 0, 0, 8, 25, 15);
criticalBurst.visible = false;
container.addChild(criticalBurst);

const textNodes = {
    full: this.createDamageNumberTextNode('full'),
    lite: this.createDamageNumberTextNode('lite'),
    minimal: this.createDamageNumberTextNode('minimal')
};
container.addChild(textNodes.full);
container.addChild(textNodes.lite);
container.addChild(textNodes.minimal);
textNodes.full.visible = true;

const arrow = new PIXI.Graphics();
arrow.beginFill(0xFFFFFF, 0.7);
arrow.drawPolygon([-5, -25, 5, -25, 0, -30]);
arrow.endFill();
arrow.visible = false;
container.addChild(arrow);

const entry = {
    container,
    glow,
    criticalBurst,
    text: textNodes.full,
    textNodes,
    activeTextVariant: 'full',
    arrow,
    inUse: false,
    styleKey: '',
    lastText: ''
};
this.damageNumberPoolSize += 1;
return entry;
    }

    acquireDamageNumberEntry() {
if (this.damageNumberPool.length > 0) {
    return this.damageNumberPool.pop();
}
return this.createDamageNumberPoolEntry();
    }

    releaseDamageNumberEntry(entry) {
if (!entry) return;
const wasInUse = !!entry.inUse;
entry.inUse = false;
entry.lastText = '';
entry.styleKey = '';
entry.activeTextVariant = 'full';
if (entry.textNodes) {
    const variants = Object.keys(entry.textNodes);
    for (let i = 0; i < variants.length; i += 1) {
        const node = entry.textNodes[variants[i]];
        if (node) {
            node.visible = variants[i] === 'full';
            if (typeof node.text === 'string') {
                node.text = '';
            }
        }
    }
    entry.text = entry.textNodes.full || entry.text;
}
const container = entry.container;
if (container && !container.destroyed) {
    container.visible = false;
    container.renderable = false;
    container.rotation = 0;
    container.alpha = 1;
    container.scale.set(1, 1);
    if (container.parent) {
        container.parent.removeChild(container);
    }
}
if (wasInUse) {
    this.activeDamageNumberCount = Math.max(0, this.activeDamageNumberCount - 1);
}
if (this.damageNumberPool.length < DAMAGE_NUMBER_POOL_MAX) {
    this.damageNumberPool.push(entry);
}
    }

    getDamageNumberActiveLimit(loadTier) {
if (loadTier >= 2 || ultraPerformanceMode) return DAMAGE_NUMBER_ACTIVE_CAP_HARD;
if (loadTier >= 1 || smoothedFrameMs > 22) return DAMAGE_NUMBER_ACTIVE_CAP_SOFT;
return DAMAGE_NUMBER_POOL_MAX;
    }

    getDamageBatchWindowMs(loadTier) {
if (loadTier >= 2) return DAMAGE_NUMBER_BATCH_WINDOW_MS_HARD;
if (loadTier >= 1) return DAMAGE_NUMBER_BATCH_WINDOW_MS_SOFT;
return (mobileDynamicsEnabled || forceMobileClient)
    ? DAMAGE_NUMBER_BATCH_WINDOW_MS_MOBILE
    : DAMAGE_NUMBER_BATCH_WINDOW_MS_DESKTOP;
    }

    getDamageBatchCellSize(loadTier) {
const baseSize = (mobileDynamicsEnabled || forceMobileClient)
    ? DAMAGE_NUMBER_BATCH_CELL_SIZE_MOBILE
    : DAMAGE_NUMBER_BATCH_CELL_SIZE_DESKTOP;
if (loadTier >= 2) return Math.round(baseSize * 4.0);
if (loadTier >= 1) return Math.round(baseSize * 2.8);
return baseSize;
    }

    getDamageBatchFlushLimit(loadTier) {
if (loadTier >= 2) return DAMAGE_NUMBER_BATCH_FLUSH_LIMIT_HARD;
if (loadTier >= 1) return DAMAGE_NUMBER_BATCH_FLUSH_LIMIT_SOFT;
return 12;
    }

    isDamageMergePressure(loadTier) {
if (loadTier >= 1 || ultraPerformanceMode || smoothedFrameMs >= 22) return true;
if (window.__e2e?.fxStressActive) return true;
const mergeLimit = (mobileDynamicsEnabled || forceMobileClient)
    ? DAMAGE_NUMBER_MERGE_MAX_KEYS_MOBILE
    : DAMAGE_NUMBER_MERGE_MAX_KEYS_DESKTOP;
return this.activeDamageNumberEffectsByKey.size >= mergeLimit;
    }

    resolveDamageMergeKey(damageType, position, targetId = null, loadTier = 0) {
const resolvedType = DAMAGE_NUMBER_CONFIGS[damageType] ? damageType : 'enemyReceived';
const normalizedTargetId = Number.isFinite(Number(targetId)) ? Number(targetId) : null;
if (normalizedTargetId !== null) {
    return `${resolvedType}|t:${normalizedTargetId}`;
}

const x = Number(position?.x);
const y = Number(position?.y);
if (!Number.isFinite(x) || !Number.isFinite(y)) return null;

const mobileMerge = mobileDynamicsEnabled || forceMobileClient;
const underPressure = this.isDamageMergePressure(loadTier);
const baseCellSize = this.getDamageBatchCellSize(loadTier);
const mergeCellMin = mobileMerge
    ? DAMAGE_NUMBER_MERGE_CELL_MIN_MOBILE
    : DAMAGE_NUMBER_MERGE_CELL_MIN_DESKTOP;
const cellSize = underPressure ? Math.max(baseCellSize, mergeCellMin) : baseCellSize;
const cellX = Math.round(x / cellSize);
const cellY = Math.round(y / cellSize);
const baseKey = `${resolvedType}|p:${cellX},${cellY}`;
if (!underPressure) return baseKey;
if (this.activeDamageNumberEffectsByKey.has(baseKey) || this.pendingDamageNumberBatches.has(baseKey)) {
    return baseKey;
}

const mergeLimit = mobileMerge
    ? DAMAGE_NUMBER_MERGE_MAX_KEYS_MOBILE
    : DAMAGE_NUMBER_MERGE_MAX_KEYS_DESKTOP;
if (
    this.activeDamageNumberEffectsByKey.size < mergeLimit &&
    this.pendingDamageNumberBatches.size < (mergeLimit * 3)
) {
    return baseKey;
}

const fallbackBuckets = mobileMerge
    ? DAMAGE_NUMBER_MERGE_FALLBACK_BUCKETS_MOBILE
    : DAMAGE_NUMBER_MERGE_FALLBACK_BUCKETS_DESKTOP;
const fallbackCellSize = cellSize * 1.25;
const bucketX = Math.round(x / fallbackCellSize);
const bucketY = Math.round(y / fallbackCellSize);
const bucket = Math.abs(((bucketX * 73856093) ^ (bucketY * 19349663))) % fallbackBuckets;
return `${resolvedType}|g:${bucket}`;
    }

    queueDamageNumber(position, damage, damageType = 'enemyReceived', targetId = null, loadTierOverride = null) {
const x = Number(position?.x);
const y = Number(position?.y);
if (!Number.isFinite(x) || !Number.isFinite(y)) return false;

const roundedDamage = Math.max(0, Math.round(Number(damage) || 0));
if (roundedDamage <= 0) return false;

const resolvedType = DAMAGE_NUMBER_CONFIGS[damageType] ? damageType : 'enemyReceived';
const loadTier = Number.isFinite(loadTierOverride) ? Number(loadTierOverride) : this.getLoadTier();
const normalizedTargetId = Number.isFinite(Number(targetId)) ? Number(targetId) : null;
const key = this.resolveDamageMergeKey(resolvedType, { x, y }, normalizedTargetId, loadTier);
if (!key) return false;
const now = Date.now();
const existing = this.pendingDamageNumberBatches.get(key);
if (existing) {
    const nextCount = existing.count + 1;
    existing.count = nextCount;
    existing.damage = Math.min(9999, existing.damage + roundedDamage);
    existing.position.x = ((existing.position.x * (nextCount - 1)) + x) / nextCount;
    existing.position.y = ((existing.position.y * (nextCount - 1)) + y) / nextCount;
    existing.lastAt = now;
    return true;
}

if (!this.shouldEmitEffect('damage')) return false;

while (this.pendingDamageNumberBatches.size >= DAMAGE_NUMBER_BATCH_MAX_PENDING) {
    const oldestKey = this.pendingDamageNumberBatches.keys().next().value;
    if (typeof oldestKey === 'undefined') break;
    this.pendingDamageNumberBatches.delete(oldestKey);
    this.effectStats.dropped += 1;
}

this.pendingDamageNumberBatches.set(key, {
    mergeKey: key,
    position: { x, y },
    damage: roundedDamage,
    damageType: resolvedType,
    count: 1,
    firstAt: now,
    lastAt: now
});
this.pendingDamageBatchCount = this.pendingDamageNumberBatches.size;
return true;
    }

    flushQueuedDamageNumbers(force = false, loadTierOverride = null) {
if (this.pendingDamageNumberBatches.size <= 0) {
    this.pendingDamageBatchCount = 0;
    return;
}

const loadTier = Number.isFinite(loadTierOverride) ? Number(loadTierOverride) : this.getLoadTier();
const now = Date.now();
const batchWindowMs = this.getDamageBatchWindowMs(loadTier);
const activeLimit = this.getDamageNumberActiveLimit(loadTier);
const availableSlots = Math.max(0, activeLimit - this.activeDamageNumberCount);
if (!force && availableSlots <= 0) return;

let flushBudget = force
    ? this.pendingDamageNumberBatches.size
    : Math.min(this.getDamageBatchFlushLimit(loadTier), availableSlots);
if (flushBudget <= 0) return;

const readyKeys = [];
for (const [key, batch] of this.pendingDamageNumberBatches) {
    if (!batch) {
        readyKeys.push(key);
        continue;
    }
    if (!force && (now - batch.firstAt) < batchWindowMs) {
        continue;
    }
    readyKeys.push(key);
    if (readyKeys.length >= flushBudget) {
        break;
    }
}

for (let i = 0; i < readyKeys.length && flushBudget > 0; i += 1) {
    const key = readyKeys[i];
    const batch = this.pendingDamageNumberBatches.get(key);
    this.pendingDamageNumberBatches.delete(key);
    if (batch) {
        const created = this.spawnDamageNumberEffect(
            batch.position,
            batch.damage,
            batch.damageType,
            loadTier,
            { mergeKey: batch.mergeKey || key }
        );
        if (!created) {
            this.effectStats.dropped += 1;
        }
        flushBudget -= 1;
    }
}
this.pendingDamageBatchCount = this.pendingDamageNumberBatches.size;
    }

    updateDamageNumberEffect(progress, effect) {
const entry = effect?.damageEntry;
if (!entry || !entry.inUse) return;
const container = entry.container;
if (!container || container.destroyed) return;

container.y = effect.damageStartY + (effect.damageMoveDirection * progress * 50);
container.alpha = effect.damageVariant === 'minimal'
    ? (1 - progress * 0.9)
    : (1 - progress * 0.7);

if (effect.damageVariant === 'minimal') {
    container.scale.set((1 - progress * 0.05) * effect.damageScaleBase);
} else if (effect.damageIsDealt) {
    container.scale.set((1 + progress * 0.3) * effect.damageScaleBase);
} else {
    container.scale.set((1 - progress * 0.1) * effect.damageScaleBase);
}

if (effect.damageFriendlyFire) {
    container.rotation = Math.sin(progress * Math.PI * 4) * 0.1;
} else {
    container.rotation = 0;
}
    }

    completeDamageNumberEffect(effect) {
const mergeKey = effect?.damageMergeKey;
if (mergeKey) {
    const mapped = this.activeDamageNumberEffectsByKey.get(mergeKey);
    if (mapped === effect) {
        this.activeDamageNumberEffectsByKey.delete(mergeKey);
    }
}
const entry = effect?.damageEntry;
this.releaseDamageNumberEntry(entry);
    }

    destroyEffectObject(object) {
if (!object || object.destroyed) return;
if (object.parent) {
    object.parent.removeChild(object);
}
try {
    object.destroy({ children: true });
} catch (_) {
    try {
        object.destroy();
    } catch (_) {}
}
    }

    scheduleCallback(delayMs, callback) {
if (typeof callback !== 'function') return false;
const delay = Math.max(0, Math.floor(Number(delayMs) || 0));

if (!this.performanceProfile.allowDelayedBursts && delay > 0) {
    if (delay > 80) {
        this.effectStats.dropped += 1;
        return false;
    }
    callback();
    return true;
}

if (this.pendingTimers.size >= this.performanceProfile.maxDeferredTimers) {
    this.effectStats.dropped += 1;
    return false;
}

const timerId = setTimeout(() => {
    this.pendingTimers.delete(timerId);
    callback();
}, delay);
this.pendingTimers.add(timerId);
return true;
    }

    dropOverflowEffects(requiredSlots = 0) {
let overflow = (this.activeEffects.length + Math.max(0, requiredSlots)) - this.maxActiveEffects;
if (overflow <= 0) return;

const survivors = [];
for (let i = 0; i < this.activeEffects.length; i += 1) {
    const effect = this.activeEffects[i];
    const priority = effect.priority ?? 1;
    if (overflow > 0 && priority <= 1) {
        if (typeof effect.onAbort === 'function') {
            effect.onAbort(effect);
        } else {
            this.destroyEffectObject(effect.object);
        }
        this.effectStats.evicted += 1;
        overflow -= 1;
        continue;
    }
    survivors.push(effect);
}

while (overflow > 0 && survivors.length > 0) {
    const effect = survivors.shift();
    if (typeof effect.onAbort === 'function') {
        effect.onAbort(effect);
    } else {
        this.destroyEffectObject(effect.object);
    }
    this.effectStats.evicted += 1;
    overflow -= 1;
}
this.activeEffects = survivors;
    }

    getSurfaceImpactSoundName(surfaceType) {
const surface = Number(surfaceType) || 0;
const surfaceEnum = GP?.SurfaceType || {};
switch (surface) {
    case (surfaceEnum.Metal ?? 1):
        return 'impactMetal';
    case (surfaceEnum.Wood ?? 2):
        return 'impactWood';
    case (surfaceEnum.Glass ?? 3):
        return 'impactGlass';
    default:
        return 'impactConcrete';
}
    }

    getFootstepSoundName(surfaceType) {
const surface = Number(surfaceType) || 0;
const surfaceEnum = GP?.SurfaceType || {};
switch (surface) {
    case (surfaceEnum.Metal ?? 1):
        return 'footstepMetal';
    case (surfaceEnum.Wood ?? 2):
        return 'footstepWood';
    case (surfaceEnum.Glass ?? 3):
        return 'footstepGlass';
    default:
        return 'footstepConcrete';
}
    }

    processGameEvent(event) {
const GAME_EVENT_FOOTSTEP = GP?.GameEventType?.Footstep ?? 16;
if (
    !this.particlesEnabled &&
    event.event_type !== GP.GameEventType.PlayerDamageEffect &&
    event.event_type !== GP.GameEventType.WallImpact &&
    event.event_type !== GP.GameEventType.WeaponFire &&
    event.event_type !== (GP?.GameEventType?.WeaponMilestone ?? 17) &&
    event.event_type !== GAME_EVENT_FOOTSTEP
) return;
const registerCombatEventFeedback = globalThis.registerCombatEventFeedback;
if (typeof registerCombatEventFeedback === 'function') {
    registerCombatEventFeedback(event);
}
const onClientGameEvent = globalThis.onClientGameEvent;
if (typeof onClientGameEvent === 'function') {
    try {
        onClientGameEvent(event);
    } catch (_) {}
}

const pos = {
    x: Number(event?.position?.x) || 0,
    y: Number(event?.position?.y) || 0
};
const GAME_EVENT_SHIELD_BROKEN = GP?.GameEventType?.ShieldBroken ?? 14;
const GAME_EVENT_POWERUP_EXPIRING = GP?.GameEventType?.PowerupExpiring ?? 15;
switch (event.event_type) {
    case GP.GameEventType.BulletImpact:
        if (!this.shouldEmitEffect('impact')) break;
        this.createEnhancedBulletImpact(pos, event.weapon_type);
        if (this.audioManager) {
            this.audioManager.playSound('bulletImpact', pos, 0.5);
            this.audioManager.registerCombatEventIntensity(0.12);
        }
        break;
    case GP.GameEventType.WallImpact:
        if (this.shouldEmitEffect('impact')) {
            this.createEnhancedBulletImpact(pos, event.weapon_type);
        }
        if (this.audioManager) {
            this.audioManager.playSound(this.getSurfaceImpactSoundName(event.surface_type), pos, 0.5);
            this.audioManager.registerCombatEventIntensity(0.1);
        }
        break;
    case GAME_EVENT_FOOTSTEP:
        if (this.audioManager) {
            const instigatorId = event.instigator_id != null ? String(event.instigator_id) : '';
            const localId = myPlayerId != null ? String(myPlayerId) : '';
            if (!instigatorId || !localId || instigatorId !== localId) {
                this.audioManager.playSound(this.getFootstepSoundName(event.surface_type), pos, 0.24);
            }
        }
        break;
    case GP.GameEventType.Explosion:
        if (!this.shouldEmitEffect('explosion')) break;
        this.createEnhancedExplosion(pos, event.value);
        if (this.audioManager) {
            this.audioManager.playSound('explosion', pos);
            this.audioManager.registerCombatEventIntensity(0.45);
        }
        break;
    case GP.GameEventType.WeaponFire:
        if (event.weapon_type === GP.WeaponType.Melee) {
            this.createMeleeSwingEffect(pos, event.instigator_id);
            const instigatorIdString = event.instigator_id != null ? String(event.instigator_id) : '';
            const localIdString = myPlayerId != null ? String(myPlayerId) : '';
            const isLocalMelee = !!instigatorIdString && !!localIdString && instigatorIdString === localIdString;
            let shouldShowParryWindow = isLocalMelee;
            if (!shouldShowParryWindow && localPlayerState) {
                const localX = Number.isFinite(localPlayerState.render_x)
                    ? localPlayerState.render_x
                    : Number(localPlayerState.x);
                const localY = Number.isFinite(localPlayerState.render_y)
                    ? localPlayerState.render_y
                    : Number(localPlayerState.y);
                if (Number.isFinite(localX) && Number.isFinite(localY)) {
                    const dx = pos.x - localX;
                    const dy = pos.y - localY;
                    shouldShowParryWindow = (dx * dx + dy * dy) <= (420 * 420);
                }
            }
            if (shouldShowParryWindow) {
                this.createParryWindowIndicator(pos, event.instigator_id, isLocalMelee);
            }
        } else {
            if (!this.shouldEmitEffect('muzzle')) break;
            this.createEnhancedMuzzleFlash(pos, event.weapon_type, event.instigator_id);
            if (event.weapon_type === GP.WeaponType.Sniper) {
                this.createSniperTrailEffect(pos, event.instigator_id);
            }
        }
        if (this.audioManager) {
            this.audioManager.playWeaponSound(event.weapon_type, pos, event.instigator_id === myPlayerId);
            this.audioManager.registerCombatEventIntensity(event.weapon_type === GP.WeaponType.Sniper ? 0.36 : 0.24);
        }
        break;
    case GP.GameEventType.PlayerDamageEffect:
        // Determine damage type based on event context
        let damageType = 'enemy';
        const targetIdString = event.target_id != null ? String(event.target_id) : '';
        const localIdString = myPlayerId != null ? String(myPlayerId) : '';
        if (event.instigator_id && event.target_id) {
            const instigator = players.get(event.instigator_id);
            const target = players.get(event.target_id);
            if (instigator && target && localPlayerState) {
                if (target.id === myPlayerId) {
                    damageType = (instigator.team_id === target.team_id && instigator.team_id !== 0) ? 'friendlyFireReceived' : 'enemyReceived';
                } else if (instigator.id === myPlayerId) {
                    damageType = (instigator.team_id === target.team_id && instigator.team_id !== 0) ? 'friendlyFireDealt' : 'enemyDealt';
                } else if (localPlayerState.team_id !== 0 && instigator.team_id === localPlayerState.team_id && target.team_id !== localPlayerState.team_id) {
                    damageType = 'enemyDealt';
                } else if (instigator.team_id === target.team_id && instigator.team_id !== 0 && instigator.team_id !== localPlayerState.team_id) {
                    damageType = 'enemyFriendlyFire';
                }
            }
        }
        this.createEnhancedDamageNumbers(pos, event.value, damageType, { targetId: event.target_id });
        if (
            gameSettings?.screenShake &&
            gameScene &&
            localIdString &&
            targetIdString === localIdString
        ) {
            const incomingDamage = Number(event.value);
            const effectiveDamage =
                Number.isFinite(incomingDamage) && incomingDamage > 0
                    ? incomingDamage
                    : 35;
            if (effectiveDamage > 10) {
                const shakeIntensity = Math.min(140, 18 + effectiveDamage * 2.2);
                const shakeFrames = Math.min(9, 2 + Math.round(effectiveDamage / 14));
                applyScreenShake(gameScene, shakeIntensity, shakeFrames);
                const attacker = event.instigator_id ? players.get(event.instigator_id) : null;
                this.createKnockbackImpactEffect(
                    pos,
                    {
                        sourceX: Number(attacker?.x),
                        sourceY: Number(attacker?.y),
                        strength: Math.min(1.3, 0.3 + effectiveDamage / 80),
                    }
                );
            }
        }
        if (
            event.weapon_type === GP.WeaponType.Sniper &&
            event.instigator_id &&
            event.target_id &&
            event.instigator_id !== event.target_id
        ) {
            const attacker = players.get(event.instigator_id);
            if (attacker && Number.isFinite(attacker.x) && Number.isFinite(attacker.y)) {
                this.createSniperTrailEffect(
                    { x: Number(attacker.x), y: Number(attacker.y) },
                    event.instigator_id,
                    pos
                );
            }
        }
        if (this.audioManager) {
            this.audioManager.playSound('playerHit', pos);
            this.audioManager.registerCombatEventIntensity(0.28);
        }
        break;
    case GP.GameEventType.WallDestroyed:
        if (!this.shouldEmitEffect('explosion')) break;
        this.createEnhancedWallDestructionEffect(pos);
        if (this.audioManager) {
            this.audioManager.playSound('explosion', pos, 0.7);
            this.audioManager.playSound('wallRumble', pos, 0.42);
            this.audioManager.registerCombatEventIntensity(0.3);
        }
        break;
    case GP.GameEventType.PowerupActivated:
        if (!this.shouldEmitEffect('powerup')) break;
        this.createEnhancedPowerupCollectEffect(pos);
        if (this.audioManager) {
            this.audioManager.playSound('powerupCollect', pos);
        }
        break;
    case GAME_EVENT_SHIELD_BROKEN:
        this.createShieldBreakEffect(pos);
        if (this.audioManager) {
            this.audioManager.playSound('shieldBreak', pos, 0.78);
        }
        break;
    case GAME_EVENT_POWERUP_EXPIRING:
        this.createPowerupExpiringEffect(pos, event);
        if (this.audioManager) {
            this.audioManager.playSound('powerupWarning', pos, 0.42, { prioritizeLocal: event.instigator_id === myPlayerId });
        }
        break;
    case GP.GameEventType.FlagCaptured:
        if (!this.shouldEmitEffect('flag')) break;
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
}
    }

    createShieldBreakEffect(position) {
if (!position || !Number.isFinite(position.x) || !Number.isFinite(position.y)) return;
const loadTier = this.getLoadTier();
const ring = new PIXI.Graphics();
ring.position.set(position.x, position.y);
ring.lineStyle(3.2, 0x7DD3FC, 0.95);
ring.drawCircle(0, 0, 14);
this.effectsContainer.addChild(ring);
this.animateEffect(ring, {
    duration: this.scaleDuration(280, 130),
    onUpdate: (progress) => {
        ring.clear();
        ring.lineStyle(3 - progress * 2.2, 0xBAE6FD, 0.9 * (1 - progress));
        ring.drawCircle(0, 0, 14 + progress * 36);
    },
    onComplete: () => ring.destroy()
});

let shardCount = this.scaleEffectCount(10, 4);
if (loadTier >= 2) shardCount = Math.max(4, Math.floor(shardCount * 0.6));
for (let i = 0; i < shardCount; i += 1) {
    const shard = new PIXI.Sprite(i % 2 === 0 ? this.particleTextures.sparkWhite : this.particleTextures.sparkBlue);
    shard.anchor.set(0.5);
    shard.position.set(position.x, position.y);
    shard.scale.set(0.4 + Math.random() * 0.4);
    const angle = Math.random() * Math.PI * 2;
    const speed = 2.5 + Math.random() * 4.2;
    const vx = Math.cos(angle) * speed;
    const vy = Math.sin(angle) * speed;
    this.effectsContainer.addChild(shard);
    this.animateEffect(shard, {
        duration: this.scaleDuration(280 + Math.random() * 120, 120),
        onUpdate: (progress) => {
            shard.x += vx * (1 - progress * 0.4);
            shard.y += vy * (1 - progress * 0.4);
            shard.alpha = 0.9 * (1 - progress);
        },
        onComplete: () => shard.destroy()
    });
}
    }

    createPowerupExpiringEffect(position, event = null) {
if (!position || !Number.isFinite(position.x) || !Number.isFinite(position.y)) return;
const seconds = Math.max(0, Number(event?.value) || 0);
const warningColor = seconds <= 1.1 ? 0xF87171 : 0xFBBF24;
const pulse = new PIXI.Graphics();
pulse.position.set(position.x, position.y);
pulse.beginFill(warningColor, 0.22);
pulse.drawCircle(0, 0, 16);
pulse.endFill();
this.effectsContainer.addChild(pulse);
this.animateEffect(pulse, {
    duration: this.scaleDuration(360, 180),
    onUpdate: (progress) => {
        pulse.scale.set(1 + progress * 2.3);
        pulse.alpha = 0.42 * (1 - progress);
    },
    onComplete: () => pulse.destroy()
});
    }

    createKillConfirmationMarker(position, options = null) {
if (!position || !Number.isFinite(position.x) || !Number.isFinite(position.y)) return;
if (!this.shouldEmitEffect('impact')) return;
const isHeadshot = !!options?.isHeadshot;
const mark = new PIXI.Text(isHeadshot ? 'CRIT' : 'ELIM', {
    fontFamily: 'Arial',
    fontSize: isHeadshot ? 16 : 14,
    fill: isHeadshot ? 0xFDE68A : 0xF8FAFC,
    fontWeight: '700',
    stroke: isHeadshot ? 0x92400E : 0x111827,
    strokeThickness: 3,
    dropShadow: true,
    dropShadowColor: isHeadshot ? 0xF59E0B : 0x38BDF8,
    dropShadowBlur: 6,
    dropShadowDistance: 0,
});
mark.anchor.set(0.5);
mark.position.set(position.x, position.y - 10);
mark.alpha = 0.96;
this.effectsContainer.addChild(mark);
this.animateEffect(mark, {
    duration: this.scaleDuration(260, 130),
    onUpdate: (progress) => {
        mark.y = position.y - 10 - progress * 18;
        mark.alpha = 0.96 * (1 - progress);
        mark.scale.set(1 + progress * 0.14);
    },
    onComplete: () => mark.destroy()
});
    }

    createKnockbackImpactEffect(position, options = {}) {
if (!position || !Number.isFinite(position.x) || !Number.isFinite(position.y)) return;
if (!this.shouldEmitEffect('impact')) return;
const sourceX = Number(options.sourceX);
const sourceY = Number(options.sourceY);
const strength = Math.max(0.12, Math.min(1.4, Number(options.strength) || 0.45));
const baseAngle = (Number.isFinite(sourceX) && Number.isFinite(sourceY))
    ? Math.atan2(position.y - sourceY, position.x - sourceX)
    : (Math.random() * Math.PI * 2);
const streakCount = this.scaleEffectCount(8, 3);
for (let i = 0; i < streakCount; i += 1) {
    const streak = new PIXI.Graphics();
    const angle = baseAngle + (Math.random() - 0.5) * 0.8;
    const length = 16 + Math.random() * 34 * strength;
    streak.lineStyle(1.4 + Math.random() * 1.1, 0xFCA5A5, 0.62);
    streak.moveTo(position.x, position.y);
    streak.lineTo(
        position.x + Math.cos(angle) * length,
        position.y + Math.sin(angle) * length
    );
    this.effectsContainer.addChild(streak);
    this.animateEffect(streak, {
        duration: this.scaleDuration(140 + Math.random() * 70, 70),
        onUpdate: (progress) => {
            streak.alpha = 0.74 * (1 - progress);
        },
        onComplete: () => streak.destroy()
    });
}
    }

    createSniperTrailEffect(origin, instigatorId = null, impact = null) {
if (!origin || !Number.isFinite(origin.x) || !Number.isFinite(origin.y)) return;
if (!this.shouldEmitEffect('impact')) return;

let endX = Number(impact?.x);
let endY = Number(impact?.y);
if (!Number.isFinite(endX) || !Number.isFinite(endY)) {
    const shooter = instigatorId ? players.get(instigatorId) : null;
    const rotation = Number.isFinite(shooter?.rotation) ? Number(shooter.rotation) : 0;
    endX = origin.x + Math.cos(rotation) * 220;
    endY = origin.y + Math.sin(rotation) * 220;
}
const beam = new PIXI.Graphics();
beam.lineStyle(2.2, 0xFF6BFF, 0.62);
beam.moveTo(origin.x, origin.y);
beam.lineTo(endX, endY);
this.effectsContainer.addChild(beam);
this.animateEffect(beam, {
    duration: this.scaleDuration(210, 110),
    onUpdate: (progress) => {
        beam.alpha = 0.66 * (1 - progress);
    },
    onComplete: () => beam.destroy()
});
    }

    createEnhancedBulletImpact(position, weaponType) {
if (!this.shouldEmitEffect('impact')) return;
const loadTier = this.getLoadTier();
const impactConfigs = {
    [GP.WeaponType.Pistol]: { size: 4, sparkCount: 3, color: 0xFFFF00 },
    [GP.WeaponType.Shotgun]: { size: 3, sparkCount: 2, color: 0xFF6600 },
    [GP.WeaponType.Rifle]: { size: 5, sparkCount: 4, color: 0x6666FF },
    [GP.WeaponType.Sniper]: { size: 8, sparkCount: 6, color: 0xFF00FF }
};

const config = impactConfigs[weaponType] || impactConfigs[GP.WeaponType.Pistol];

// Impact flash
const impact = new PIXI.Graphics();
impact.beginFill(config.color, 0.9);
impact.drawCircle(0, 0, config.size);
impact.endFill();
impact.position.set(position.x, position.y);
if (loadTier === 0) {
    impact.filters = [getSharedBlurFilter(2)];
}
this.effectsContainer.addChild(impact);

if (loadTier >= 2) {
    this.animateEffect(impact, {
        duration: 120,
        onUpdate: p => {
            impact.scale.set(1 + p * 2);
            impact.alpha = 1 - p;
        },
        onComplete: () => impact.destroy()
    });
    return;
}

// Sparks
let sparkCount = this.scaleEffectCount(config.sparkCount, 1);
if (loadTier === 1) {
    sparkCount = Math.max(1, Math.floor(sparkCount * 0.5));
}
for (let i = 0; i < sparkCount; i++) {
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

// Impact streak trails (high/medium quality only).
if (loadTier <= 1) {
    let streakCount = this.scaleEffectCount(5, 2);
    if (loadTier === 1) {
        streakCount = Math.max(2, Math.floor(streakCount * 0.6));
    }
    for (let i = 0; i < streakCount; i += 1) {
        const streak = new PIXI.Graphics();
        const angle = Math.random() * Math.PI * 2;
        const length = config.size * (2.8 + Math.random() * 3.2);
        const sx = Math.cos(angle) * (config.size * 0.25);
        const sy = Math.sin(angle) * (config.size * 0.25);
        const ex = Math.cos(angle) * length;
        const ey = Math.sin(angle) * length;
        streak.lineStyle(1.6 + Math.random() * 1.1, config.color, 0.72);
        streak.moveTo(sx, sy);
        streak.lineTo(ex, ey);
        streak.position.set(position.x, position.y);
        this.effectsContainer.addChild(streak);

        this.animateEffect(streak, {
            duration: 170 + Math.random() * 90,
            onUpdate: (p) => {
                streak.alpha = 0.76 * (1 - p);
                streak.scale.set(1 + p * 0.34);
            },
            onComplete: () => streak.destroy()
        });
    }
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
if (!this.shouldEmitEffect('muzzle')) return;
const loadTier = this.getLoadTier();
const playerSprite = this.findPlayerSpriteById(instigatorId);
if (!playerSprite) return;
if (!playerSprite.gun || typeof playerSprite.gun.addChild !== 'function') return;

const flashConfigs = {
    [GP.WeaponType.Pistol]: { size: 15, color: 0xFFFF66, points: 4 },
    [GP.WeaponType.Shotgun]: { size: 22, color: 0xFF6600, points: 6 },
    [GP.WeaponType.Rifle]: { size: 18, color: 0x6666FF, points: 5 },
    [GP.WeaponType.Sniper]: { size: 25, color: 0xFF66FF, points: 8 }
};

const config = flashConfigs[weaponType] || flashConfigs[GP.WeaponType.Pistol];

// Multi-layered flash
const flashContainer = new PIXI.Container();
const gunLength = PLAYER_RADIUS + 15; // Approximate gun length
flashContainer.position.set(gunLength, 0); // Position at the tip of the gun sprite
flashContainer.rotation = Math.random() * Math.PI * 2; // Random rotation for variety
playerSprite.gun.addChild(flashContainer); // Add to the gun sprite of the player

if (loadTier >= 2) {
    const flashLite = new PIXI.Graphics();
    flashLite.beginFill(config.color, 0.65);
    flashLite.drawCircle(0, 0, Math.max(6, config.size * 0.45));
    flashLite.endFill();
    flashContainer.addChild(flashLite);

    this.animateEffect(flashContainer, {
        duration: 80,
        onUpdate: p => {
            flashContainer.scale.set(0.7 + 0.4 * (1 - p));
            flashContainer.alpha = 1 - p;
        },
        onComplete: () => flashContainer.destroy()
    });
    return;
}

// Outer glow
if (loadTier === 0) {
    const glow = new PIXI.Graphics();
    glow.beginFill(config.color, 0.3);
    glow.drawCircle(0, 0, config.size * 1.5);
    glow.endFill();
    glow.filters = [getSharedBlurFilter(4)];
    flashContainer.addChild(glow);
}

// Main flash
const flash = new PIXI.Graphics();
flash.beginFill(config.color, 0.8);
const starPoints = loadTier === 1 ? Math.max(4, config.points - 2) : config.points;
drawStar(flash, 0, 0, starPoints, config.size, config.size * 0.4);
flash.endFill();

// Core
flash.beginFill(0xFFFFFF, 1);
flash.drawCircle(0, 0, config.size * 0.3);
flash.endFill();

flashContainer.addChild(flash);

this.animateEffect(flashContainer, {
    duration: 100,
    onUpdate: p => {
        flashContainer.scale.set(0.5 + 0.5 * (1 - p));
        flashContainer.alpha = 1 - p;
    },
    onComplete: () => flashContainer.destroy()
});
    }

    triggerMovementAbilityBurst(payload = {}) {
const type = payload.type === 'dodge' ? 'dodge' : 'dash';
const position = payload.position;
const isLocalPlayer = !!payload.isLocalPlayer;
if (!position || !Number.isFinite(position.x) || !Number.isFinite(position.y)) return;
if (!this.shouldEmitEffect('movement')) return;

const rotation = Number.isFinite(payload.rotation) ? payload.rotation : 0;
const loadTier = this.getLoadTier();
const effectColor = type === 'dash' ? 0x66DDFF : 0x77FFCC;
const accentColor = type === 'dash' ? 0xC6F5FF : 0xD4FFE8;
const ringStartRadius = type === 'dash' ? 18 : 14;
const ringEndRadius = type === 'dash' ? 72 : 56;

const ring = new PIXI.Graphics();
ring.lineStyle(3, effectColor, 0.85);
ring.drawCircle(0, 0, ringStartRadius);
ring.position.set(position.x, position.y);
this.effectsContainer.addChild(ring);
this.animateEffect(ring, {
    duration: this.scaleDuration(type === 'dash' ? 230 : 190, 90),
    onUpdate: (progress) => {
        const radius = ringStartRadius + (ringEndRadius - ringStartRadius) * progress;
        ring.clear();
        ring.lineStyle(3 - progress * 2, effectColor, (1 - progress) * 0.9);
        ring.drawCircle(0, 0, radius);
        ring.alpha = 1 - progress;
    },
    onComplete: () => ring.destroy()
});

const streakCount = this.scaleEffectCount(type === 'dash' ? 12 : 8, type === 'dash' ? 4 : 3);
for (let i = 0; i < streakCount; i += 1) {
    if (loadTier >= 2 && i % 2 === 1) continue;
    const streak = new PIXI.Graphics();
    const lateral = (Math.random() - 0.5) * (type === 'dash' ? 18 : 22);
    const length = (type === 'dash' ? 34 : 24) + Math.random() * 18;
    const startDist = (type === 'dash' ? 7 : 5) + Math.random() * 6;
    const dirJitter = (Math.random() - 0.5) * (type === 'dash' ? 0.24 : 0.4);
    const forward = rotation + dirJitter;
    const right = forward + Math.PI * 0.5;
    const originX = position.x + Math.cos(right) * lateral - Math.cos(forward) * startDist;
    const originY = position.y + Math.sin(right) * lateral - Math.sin(forward) * startDist;
    const endX = originX - Math.cos(forward) * length;
    const endY = originY - Math.sin(forward) * length;
    streak.lineStyle(2, accentColor, 0.65);
    streak.moveTo(originX, originY);
    streak.lineTo(endX, endY);
    this.effectsContainer.addChild(streak);
    this.animateEffect(streak, {
        duration: this.scaleDuration(type === 'dash' ? 180 : 150, 75),
        delay: i * 8,
        onUpdate: (progress) => {
            streak.alpha = 0.85 * (1 - progress);
        },
        onComplete: () => streak.destroy()
    });
}

if (this.audioManager) {
    this.audioManager.playSound(
        type === 'dash' ? 'dashWhoosh' : 'dodgeWhoosh',
        position,
        isLocalPlayer ? 1.0 : 0.72
    );
}

if (isLocalPlayer && gameSettings.screenShake && gameScene) {
    applyScreenShake(gameScene, type === 'dash' ? 72 : 56, 2);
}
    }

    getDamageNumberTextStyle(damageType, config, variant = 'full') {
const normalizedVariant = variant === 'minimal' ? 'minimal' : (variant === 'lite' ? 'lite' : 'full');
const styleKey = `${String(damageType || 'enemyReceived')}:${normalizedVariant}`;
if (this.damageNumberTextStyles.has(styleKey)) {
    return this.damageNumberTextStyles.get(styleKey);
}
const isLite = normalizedVariant !== 'full';
const isMinimal = normalizedVariant === 'minimal';
const styleOptions = {
    fontSize: (isMinimal ? 15 : (isLite ? 17 : 20)) * config.scale,
    fontWeight: 'bold',
    fill: isLite ? (Array.isArray(config.textColor) ? config.textColor[0] : config.textColor) : config.textColor,
    stroke: config.strokeColor,
    strokeThickness: isMinimal ? 1 : (isLite ? 2 : 4),
    dropShadow: !isLite
};
if (!isLite) {
    styleOptions.fillGradientType = PIXI.TEXT_GRADIENT.LINEAR_VERTICAL;
    styleOptions.dropShadowColor = 0x000000;
    styleOptions.dropShadowBlur = 4;
    styleOptions.dropShadowDistance = 2;
}
const style = new PIXI.TextStyle(styleOptions);
this.damageNumberTextStyles.set(styleKey, style);
return style;
    }

    getDamageNumberAnimationDuration(useMinimalVariant, useLiteVariant) {
if (useMinimalVariant) return 520;
if (useLiteVariant) return 760;
return 1000;
    }

    getDamageNumberUpdateStride(loadTier, useMinimalVariant, useLiteVariant) {
if (useMinimalVariant || loadTier >= 2) return 3;
if (useLiteVariant || loadTier >= 1 || mobileDynamicsEnabled || forceMobileClient) return 2;
return 1;
    }

    applyDamageNumberEntryVisual(entry, resolvedType, config, styleVariant, damageValue) {
if (!entry || !config) return;
const normalizedVariant = styleVariant === 'minimal' ? 'minimal' : (styleVariant === 'lite' ? 'lite' : 'full');
const useLiteVariant = normalizedVariant !== 'full';
const useMinimalVariant = normalizedVariant === 'minimal';
const glow = entry.glow;
const criticalBurst = entry.criticalBurst;
const arrow = entry.arrow;
const roundedDamage = Math.max(0, Math.round(Number(damageValue) || 0));
const textColor = Array.isArray(config.textColor) ? config.textColor[0] : config.textColor;
const textValue = config.prefix + roundedDamage;

const textNodes = entry.textNodes || null;
let textNode = entry.text;
if (textNodes) {
    if (entry.activeTextVariant !== normalizedVariant) {
        entry.activeTextVariant = normalizedVariant;
        entry.styleKey = '';
        entry.lastText = '';
    }
    const variants = ['full', 'lite', 'minimal'];
    for (let i = 0; i < variants.length; i += 1) {
        const key = variants[i];
        const node = textNodes[key];
        if (!node) continue;
        const shouldShow = key === normalizedVariant;
        if (node.visible !== shouldShow) {
            node.visible = shouldShow;
        }
        if (shouldShow) {
            textNode = node;
        }
    }
    entry.text = textNode;
}
if (!textNode) return;

if (entry.lastText !== textValue || textNode.text !== textValue) {
    textNode.text = textValue;
    entry.lastText = textValue;
}
const resolvedTextTint = typeof textColor === 'number' ? textColor : 0xFFFFFF;
if (textNode.tint !== resolvedTextTint) {
    textNode.tint = resolvedTextTint;
}

if (!this.damageNumberUseBitmapText) {
    const styleKey = `${resolvedType}:${normalizedVariant}`;
    if (entry.styleKey !== styleKey) {
        textNode.style = this.getDamageNumberTextStyle(resolvedType, config, normalizedVariant);
        entry.styleKey = styleKey;
    }
} else {
    entry.styleKey = `${resolvedType}:${normalizedVariant}:bitmap`;
}

const showGlow = !useLiteVariant;
glow.visible = showGlow;
glow.tint = config.glowColor;
glow.alpha = showGlow ? 1 : 0;

const showArrow = !useMinimalVariant && (resolvedType === 'enemyReceived' || resolvedType === 'friendlyFireReceived');
arrow.visible = showArrow;
arrow.tint = config.glowColor;

const showCritical = !useLiteVariant && roundedDamage > 50;
criticalBurst.visible = showCritical;
    }

    createEnhancedDamageNumbers(position, damage, damageType = 'enemy', options = {}) {
const loadTier = this.getLoadTier();
const resolvedType = DAMAGE_NUMBER_CONFIGS[damageType] ? damageType : 'enemyReceived';
const normalizedTargetId = Number.isFinite(Number(options?.targetId)) ? Number(options.targetId) : null;
const syntheticStressActive = Boolean(window.__e2e?.fxStressActive);
const allowMerge = DAMAGE_BATCH_ENABLED;
const mergeKey = allowMerge
    ? this.resolveDamageMergeKey(resolvedType, position, normalizedTargetId, loadTier)
    : null;
const shouldUseBatch = DAMAGE_BATCH_ENABLED && (
    loadTier >= 1 ||
    ultraPerformanceMode ||
    mobileDynamicsEnabled ||
    forceMobileClient ||
    syntheticStressActive
);
if (!shouldUseBatch) {
    if (!this.shouldEmitEffect('damage')) return false;
    return this.spawnDamageNumberEffect(position, damage, damageType, loadTier, { mergeKey });
}
if (options && options.immediate) {
    if (!this.shouldEmitEffect('damage')) return false;
    return this.spawnDamageNumberEffect(position, damage, damageType, loadTier, { ...options, mergeKey });
}
return this.queueDamageNumber(position, damage, damageType, normalizedTargetId, loadTier);
    }

    spawnDamageNumberEffect(position, damage, damageType = 'enemy', loadTierOverride = null, options = null) {
const loadTier = Number.isFinite(loadTierOverride) ? Number(loadTierOverride) : this.getLoadTier();
const useLiteVariant = loadTier >= 1;
const useMinimalVariant = loadTier >= 2;
const resolvedType = DAMAGE_NUMBER_CONFIGS[damageType] ? damageType : 'enemyReceived';
const config = DAMAGE_NUMBER_CONFIGS[resolvedType];
const roundedDamage = Math.max(0, Math.round(Number(damage) || 0));
if (roundedDamage <= 0) return false;
const mergeKey = (options && typeof options.mergeKey === 'string') ? options.mergeKey : null;
const isDealt = resolvedType === 'enemyDealt' || resolvedType === 'friendlyFireDealt';
const moveDirection = isDealt ? 1 : -1; // Dealt damage moves up, received damage moves down
const styleVariant = useMinimalVariant ? 'minimal' : (useLiteVariant ? 'lite' : 'full');
const animationDuration = this.getDamageNumberAnimationDuration(useMinimalVariant, useLiteVariant);
const damageUpdateStride = this.getDamageNumberUpdateStride(loadTier, useMinimalVariant, useLiteVariant);

if (mergeKey) {
    const existingEffect = this.activeDamageNumberEffectsByKey.get(mergeKey);
    if (existingEffect) {
        const existingEntry = existingEffect.damageEntry;
        const existingContainer = existingEntry?.container;
        if (existingEntry && existingEntry.inUse && existingContainer && !existingContainer.destroyed) {
            const mergedDamage = Math.min(9999, Math.max(0, Math.round(Number(existingEffect.damageTotal) || 0)) + roundedDamage);
            existingEffect.damageTotal = mergedDamage;
            this.applyDamageNumberEntryVisual(existingEntry, resolvedType, config, styleVariant, mergedDamage);
            existingContainer.position.set(position.x, position.y + config.offsetY);
            existingContainer.scale.set(config.scale);
            existingContainer.alpha = 1;
            existingContainer.rotation = 0;
            existingContainer.visible = true;
            existingContainer.renderable = true;
            existingEffect.damageStartY = position.y + config.offsetY;
            existingEffect.damageMoveDirection = moveDirection;
            existingEffect.damageScaleBase = config.scale;
            existingEffect.damageVariant = styleVariant;
            existingEffect.damageIsDealt = isDealt;
            existingEffect.damageFriendlyFire = resolvedType === 'friendlyFireDealt' || resolvedType === 'friendlyFireReceived';
            existingEffect.duration = animationDuration;
            existingEffect.updateStride = damageUpdateStride;
            existingEffect.updateStrideTick = 0;
            const now = Date.now();
            existingEffect.startTime = now;
            existingEffect.actualStartTime = now;
            existingEffect.started = true;
            return true;
        }
        this.activeDamageNumberEffectsByKey.delete(mergeKey);
    }
}

const activeLimit = this.getDamageNumberActiveLimit(loadTier);
if (this.activeDamageNumberCount >= activeLimit) {
    this.effectStats.dropped += 1;
    return false;
}

const poolEntry = this.acquireDamageNumberEntry();
if (!poolEntry) {
    this.effectStats.dropped += 1;
    return false;
}

const container = poolEntry.container;
this.applyDamageNumberEntryVisual(poolEntry, resolvedType, config, styleVariant, roundedDamage);

container.position.set(position.x, position.y + config.offsetY);
container.scale.set(config.scale);
container.alpha = 1;
container.rotation = 0;
container.visible = true;
container.renderable = true;

this.effectsContainer.addChild(container);
poolEntry.inUse = true;
this.activeDamageNumberCount += 1;

const queued = this.animateEffect(container, {
    duration: animationDuration,
    priority: 3,
    damageTotal: roundedDamage,
    damageMergeKey: mergeKey,
    damageEntry: poolEntry,
    damageStartY: position.y + config.offsetY,
    damageMoveDirection: moveDirection,
    damageScaleBase: config.scale,
    damageVariant: styleVariant,
    damageIsDealt: isDealt,
    damageFriendlyFire: resolvedType === 'friendlyFireDealt' || resolvedType === 'friendlyFireReceived',
    updateStride: damageUpdateStride,
    updateStrideTick: 0,
    preserveObjectOnDrop: true,
    onUpdate: this.damageNumberOnUpdate,
    onComplete: this.damageNumberOnComplete,
    onAbort: this.damageNumberOnComplete
});
if (!queued) {
    this.releaseDamageNumberEntry(poolEntry);
    return false;
}
if (mergeKey) {
    this.activeDamageNumberEffectsByKey.set(mergeKey, queued);
}
return true;
    }

    createEnhancedExplosion(position, radius = 30) {
if (!this.shouldEmitEffect('explosion')) return;
const loadTier = this.getLoadTier();
const explosionContainer = new PIXI.Container();
explosionContainer.position.set(position.x, position.y);
this.effectsContainer.addChild(explosionContainer);

// Shockwave ring
const shockwave = new PIXI.Graphics();
shockwave.lineStyle(3, 0xFFAA00, 0.8);
shockwave.drawCircle(0, 0, 10);
explosionContainer.addChild(shockwave);

if (loadTier >= 2) {
    const blastLite = new PIXI.Graphics();
    blastLite.beginFill(0xFFAA33, 0.55);
    blastLite.drawCircle(0, 0, Math.max(12, radius * 0.45));
    blastLite.endFill();
    explosionContainer.addChild(blastLite);

    this.animateEffect(blastLite, {
        duration: 240,
        onUpdate: (progress) => {
            blastLite.scale.set(1 + progress * 1.2);
            blastLite.alpha = 0.55 * (1 - progress);
        },
        onComplete: () => blastLite.destroy()
    });
    this.animateEffect(shockwave, {
        duration: 320,
        onUpdate: (progress) => {
            shockwave.scale.set(1 + progress * 2.8);
            shockwave.alpha = 0.7 * (1 - progress);
        },
        onComplete: () => explosionContainer.destroy({ children: true })
    });
    return;
}

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
if (loadTier === 0) {
    explosion.filters = [getSharedBlurFilter(3)];
}
explosionContainer.addChild(explosion);

// Particles
let particleCount = this.scaleEffectCount(20 + Math.floor(radius / 10), 4);
if (loadTier === 1) {
    particleCount = Math.max(2, Math.floor(particleCount * 0.35));
}
for (let i = 0; i < particleCount; i++) {
    const particleTexturePool = [
        this.particleTextures.sparkWhite,
        this.particleTextures.sparkOrange,
        this.particleTextures.sparkRed,
    ];
    const particle = new PIXI.Sprite(particleTexturePool[Math.floor(Math.random() * particleTexturePool.length)] || this.particleTextures.spark);
    particle.anchor.set(0.5);
    particle.position.set(0, 0);
    
    const angle = (Math.PI * 2 * i) / particleCount + Math.random() * 0.5;
    const speed = 3 + Math.random() * 5;
    particle.velocity = {
        x: Math.cos(angle) * speed,
        y: Math.sin(angle) * speed - 2
    };
    particle.angularVelocity = (Math.random() - 0.5) * 0.3;
    
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
dustCloud.filters = [getSharedBlurFilter(8)];
this.effectsContainer.addChild(dustCloud);

// Debris pieces
const debrisCount = this.scaleEffectCount(15, 5);
for (let i = 0; i < debrisCount; i++) {
    const debris = new PIXI.Sprite(Math.random() > 0.45 ? this.particleTextures.debrisGray : this.particleTextures.debrisBrown);
    debris.anchor.set(0.5);
    debris.position.set(position.x, position.y);
    
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
const dustCount = this.scaleEffectCount(10, 3);
for (let i = 0; i < dustCount; i++) {
    const dust = new PIXI.Sprite(Math.random() > 0.5 ? this.particleTextures.smokeDark : this.particleTextures.smokeLight);
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

    createEnhancedPlayerDeathEffect(position, options = {}) {
if (!position || !Number.isFinite(position.x) || !Number.isFinite(position.y)) return;
if (!this.shouldEmitEffect('explosion')) return;

const teamId = Number(options.teamId) || 0;
const isLocalVictim = !!options.isLocalVictim;
const isLocalKiller = !!options.isLocalKiller;
const isHeadshot = !!options.isHeadshot;
const loadTier = this.getLoadTier();
const baseColor = teamId === 1 ? 0xFF6B6B : (teamId === 2 ? 0x6BB8FF : 0xFFD166);
const accentColor = isHeadshot ? 0xFFD84A : baseColor;

const container = new PIXI.Container();
container.position.set(position.x, position.y);
this.effectsContainer.addChild(container);

const core = new PIXI.Graphics();
core.beginFill(0xFFFFFF, 0.92);
core.drawCircle(0, 0, 8);
core.endFill();
container.addChild(core);

const ring = new PIXI.Graphics();
ring.lineStyle(3, accentColor, 0.9);
ring.drawCircle(0, 0, 18);
container.addChild(ring);

const pulse = new PIXI.Graphics();
pulse.beginFill(accentColor, 0.26);
pulse.drawCircle(0, 0, 16);
pulse.endFill();
if (loadTier === 0) {
    pulse.filters = [getSharedBlurFilter(4)];
}
container.addChildAt(pulse, 0);

let critBurst = null;
if (isHeadshot && loadTier <= 1) {
    critBurst = new PIXI.Graphics();
    critBurst.lineStyle(2.2, 0xFFF0B0, 0.9);
    drawStar(critBurst, 0, 0, 8, 22, 11);
    container.addChild(critBurst);
    this.animateEffect(critBurst, {
        duration: this.scaleDuration(280, 140),
        onUpdate: (progress) => {
            critBurst.scale.set(0.65 + progress * 2.5);
            critBurst.rotation = progress * Math.PI * 1.35;
            critBurst.alpha = 0.92 * (1 - progress);
        },
        onComplete: () => critBurst.destroy()
    });
}

this.animateEffect(core, {
    duration: this.scaleDuration(170, 90),
    onUpdate: (progress) => {
        core.scale.set(1 + progress * 1.8);
        core.alpha = 0.92 * (1 - progress);
    },
    onComplete: () => core.destroy()
});

this.animateEffect(ring, {
    duration: this.scaleDuration(260, 120),
    onUpdate: (progress) => {
        ring.scale.set(1 + progress * 2.4);
        ring.alpha = 0.95 * (1 - progress);
    },
    onComplete: () => ring.destroy()
});

this.animateEffect(pulse, {
    duration: this.scaleDuration(260, 120),
    onUpdate: (progress) => {
        pulse.scale.set(1 + progress * 1.8);
        pulse.alpha = 0.28 * (1 - progress);
    },
    onComplete: () => pulse.destroy()
});

let sparkCount = this.scaleEffectCount(12, 4);
if (isHeadshot && loadTier <= 1) sparkCount += this.scaleEffectCount(4, 2);
if (loadTier >= 2) sparkCount = Math.max(3, Math.floor(sparkCount * 0.6));
for (let i = 0; i < sparkCount; i += 1) {
    const spark = new PIXI.Sprite(this.particleTextures.spark);
    spark.anchor.set(0.5);
    spark.position.set(0, 0);
    spark.tint = i % 3 === 0
        ? 0xFFFFFF
        : (isHeadshot && i % 2 === 0 ? 0xFFE598 : accentColor);
    spark.scale.set(0.45 + Math.random() * 0.55);
    const angle = Math.random() * Math.PI * 2;
    const speed = 2.5 + Math.random() * 4.5;
    const vx = Math.cos(angle) * speed;
    const vy = Math.sin(angle) * speed - 1.2;
    container.addChild(spark);
    this.animateEffect(spark, {
        duration: this.scaleDuration(360 + Math.random() * 120, 160),
        onUpdate: (progress) => {
            spark.x += vx * (1 - progress * 0.5);
            spark.y += vy + progress * 12;
            spark.alpha = 0.95 * (1 - progress);
        },
        onComplete: () => spark.destroy()
    });
}

this.animateEffect(container, {
    duration: this.scaleDuration(420, 180),
    onUpdate: () => {},
    onComplete: () => container.destroy({ children: true })
});

if (isLocalVictim && gameSettings.screenShake && gameScene) {
    applyScreenShake(gameScene, 120, 4);
}
if (isLocalVictim && app) {
    createScreenFlash(app, isHeadshot ? 0xFFE8B0 : 0xFFFFFF, 16, isHeadshot ? 0.38 : 0.34);
} else if (isLocalKiller && app) {
    createScreenFlash(app, isHeadshot ? 0xFFDF78 : accentColor, isHeadshot ? 14 : 10, isHeadshot ? 0.26 : 0.2);
}
if (this.audioManager) {
    this.audioManager.playSound('explosion', position, isLocalVictim ? 0.65 : 0.42);
    if (isHeadshot && isLocalKiller) {
        this.audioManager.playSound('hitMarkerHeadshot', position, 0.2);
    }
}
    }

    createEnhancedPowerupCollectEffect(position) {
if (!this.shouldEmitEffect('powerup')) return;
const loadTier = this.getLoadTier();
const container = new PIXI.Container();
container.position.set(position.x, position.y);
this.effectsContainer.addChild(container);

if (loadTier >= 2) {
    const pulse = new PIXI.Graphics();
    pulse.beginFill(0x00FF88, 0.5);
    pulse.drawCircle(0, 0, 14);
    pulse.endFill();
    container.addChild(pulse);
    this.animateEffect(pulse, {
        duration: 260,
        onUpdate: (progress) => {
            pulse.scale.set(1 + progress * 1.8);
            pulse.alpha = 0.5 * (1 - progress);
        },
        onComplete: () => container.destroy({ children: true })
    });
    return;
}

// Energy burst
const burst = new PIXI.Graphics();
burst.beginFill(0x00FF00, 0.6);
drawStar(burst, 0, 0, 8, 30, 15);
burst.endFill();
container.addChild(burst);

// Ring waves
let ringCount = this.scaleEffectCount(3, 1);
if (loadTier === 1) {
    ringCount = Math.max(1, Math.floor(ringCount * 0.5));
}
for (let i = 0; i < ringCount; i++) {
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
let sparkleCount = this.scaleEffectCount(12, 2);
if (loadTier === 1) {
    sparkleCount = Math.max(2, Math.floor(sparkleCount * 0.35));
}
for (let i = 0; i < sparkleCount; i++) {
    const sparkle = new PIXI.Graphics();
    sparkle.beginFill(0xFFFFFF, 0.9);
    sparkle.drawCircle(0, 0, 2);
    sparkle.endFill();
    
    const angle = (Math.PI * 2 * i) / sparkleCount;
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

    createEnhancedFlagCaptureEffect(position) {
if (!this.shouldEmitEffect('flag')) return;
const loadTier = this.getLoadTier();
const container = new PIXI.Container();
container.position.set(position.x, position.y);
this.effectsContainer.addChild(container);

if (loadTier >= 1) {
    const flashLite = new PIXI.Graphics();
    flashLite.beginFill(0xFFD447, 0.7);
    drawStar(flashLite, 0, 0, 6, 20, 10);
    flashLite.endFill();
    container.addChild(flashLite);

    this.animateEffect(flashLite, {
        duration: loadTier >= 2 ? 320 : 480,
        onUpdate: (progress) => {
            flashLite.scale.set(0.8 + progress * 1.6);
            flashLite.alpha = 0.7 * (1 - progress);
        },
        onComplete: () => container.destroy({ children: true })
    });
    return;
}

// Fireworks effect
const colors = [0xFF0000, 0x0000FF, 0xFFFF00, 0x00FF00, 0xFF00FF];

const burstCount = this.scaleEffectCount(3, 1);
for (let burst = 0; burst < burstCount; burst++) {
    this.scheduleCallback(burst * 200, () => {
        const burstContainer = new PIXI.Container();
        container.addChild(burstContainer);
        
        // Central flash
        const flash = new PIXI.Graphics();
        flash.beginFill(0xFFFFFF, 0.8);
        flash.drawCircle(0, 0, 15);
        flash.endFill();
        burstContainer.addChild(flash);
        
        // Firework particles
        const particleCount = this.scaleEffectCount(30, 8);
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
    });
}

// Clean up container after all effects
this.scheduleCallback(2000, () => {
    if (container.parent) {
        container.destroy();
    }
});
    }

    createParryWindowIndicator(position, instigatorId, isLocalPlayer = false) {
if (!position || !Number.isFinite(position.x) || !Number.isFinite(position.y)) return;
if (!isLocalPlayer && !this.shouldEmitEffect('movement')) return;

const player = players.get(instigatorId);
const facing = Number.isFinite(player?.rotation) ? player.rotation : 0;
const windowMs = 220;
const radius = PLAYER_RADIUS + 20;
const halfArc = Math.PI * 0.42;

const container = new PIXI.Container();
container.position.set(position.x, position.y);
this.effectsContainer.addChild(container);

const arc = new PIXI.Graphics();
const core = new PIXI.Graphics();
const sparkColor = isLocalPlayer ? 0x7EE8FF : 0xB8FFF2;
container.addChild(arc);
container.addChild(core);

for (let i = 0; i < 3; i += 1) {
    const tick = new PIXI.Graphics();
    tick.lineStyle(2, sparkColor, 0.8 - i * 0.15);
    const ang = facing + (i - 1) * 0.32;
    const inner = radius - 8;
    const outer = radius + 2 + i * 3;
    tick.moveTo(Math.cos(ang) * inner, Math.sin(ang) * inner);
    tick.lineTo(Math.cos(ang) * outer, Math.sin(ang) * outer);
    container.addChild(tick);
    this.animateEffect(tick, {
        duration: this.scaleDuration(windowMs, 90),
        onUpdate: (progress) => {
            tick.alpha = (0.8 - i * 0.15) * (1 - progress);
            tick.scale.set(1 + progress * 0.2);
        },
        onComplete: () => tick.destroy()
    });
}

this.animateEffect(arc, {
    duration: this.scaleDuration(windowMs, 90),
    priority: 2,
    onUpdate: (progress) => {
        arc.clear();
        arc.lineStyle(3, sparkColor, 0.9 * (1 - progress));
        arc.arc(
            0,
            0,
            radius + progress * 3,
            facing - halfArc,
            facing + halfArc,
            false
        );
    },
    onComplete: () => arc.destroy()
});

this.animateEffect(core, {
    duration: this.scaleDuration(windowMs, 90),
    priority: 2,
    onUpdate: (progress) => {
        core.clear();
        core.lineStyle(1.8, 0xFFFFFF, 0.7 * (1 - progress));
        core.drawCircle(0, 0, PLAYER_RADIUS + 3 + progress * 4);
    },
    onComplete: () => core.destroy()
});

this.animateEffect(container, {
    duration: this.scaleDuration(windowMs + 40, 110),
    onUpdate: () => {},
    onComplete: () => container.destroy({ children: true })
});
    }

    animateEffect(object, config = {}) {
if (!object || object.destroyed) return false;

const duration = this.scaleDuration(config.duration || 120, 40);
const delay = Math.max(0, Math.floor(Number(config.delay) || 0));

this.dropOverflowEffects(1);
if (this.activeEffects.length >= this.maxActiveEffects) {
    this.effectStats.dropped += 1;
    if (!config.preserveObjectOnDrop) {
        this.destroyEffectObject(object);
    }
    return false;
}

const effect = {
    ...config,
    object,
    duration,
    startTime: Date.now() + delay,
    started: false,
    updateStride: Math.max(1, Math.floor(Number(config.updateStride) || 1)),
    updateStrideTick: 0,
    priority: Number.isFinite(config.priority) ? Number(config.priority) : 1,
    onUpdate: typeof config.onUpdate === 'function' ? config.onUpdate : (() => {}),
    onAbort: typeof config.onAbort === 'function' ? config.onAbort : null
};
this.activeEffects.push(effect);
return effect;
    }

    emitEngineTrail(x, y, color = 0xFFFFFF, intensity = 0.6) {
if (!this.particlesEnabled) return false;
if (!Number.isFinite(x) || !Number.isFinite(y)) return false;

// Device-class gating: full rate on desktop/high, half rate on mid, off on low.
if (deviceClassification === 'low') return false;
if (deviceClassification === 'mid') {
    this.engineTrailMidStrideCounter = (this.engineTrailMidStrideCounter + 1) % 2;
    if (this.engineTrailMidStrideCounter !== 0) return false;
}
// Shared load-based stride gating (movement effects drop first under load).
if (!this.shouldEmitEffect('movement')) return false;

const clampedIntensity = Math.max(0.2, Math.min(1, Number(intensity) || 0));

// Ring-buffer pool of reusable glow sprites (priority 1 = evicted first).
const pool = this.engineTrailPool;
const cap = Math.max(32, Math.round(240 * (this.performanceProfile.particleScale || 1)));
let sprite = null;
for (let i = 0; i < pool.length; i += 1) {
    this.engineTrailPoolCursor = (this.engineTrailPoolCursor + 1) % pool.length;
    const candidate = pool[this.engineTrailPoolCursor];
    if (candidate && !candidate._trailActive && !candidate.destroyed) {
        sprite = candidate;
        break;
    }
}
if (!sprite) {
    if (pool.length >= cap) {
        this.effectStats.dropped += 1;
        return false;
    }
    sprite = new PIXI.Sprite(this.particleTextures.trailGlow || PIXI.Texture.WHITE);
    sprite.anchor.set(0.5);
    sprite.blendMode = PIXI.BLEND_MODES.ADD;
    sprite.visible = false;
    sprite._trailActive = false;
    this.effectsContainer.addChild(sprite);
    pool.push(sprite);
}

const baseScale = 0.7 + clampedIntensity * 0.85;
sprite.position.set(x + (Math.random() - 0.5) * 3, y + (Math.random() - 0.5) * 3);
sprite.tint = color;
sprite.scale.set(baseScale);
sprite.alpha = 0.65;
sprite.visible = true;
sprite._trailActive = true;

const release = () => {
    if (sprite.destroyed) return;
    sprite._trailActive = false;
    sprite.visible = false;
};
const started = this.animateEffect(sprite, {
    duration: this.scaleDuration(400, 130),
    priority: 1,
    preserveObjectOnDrop: true,
    onUpdate: (progress) => {
        sprite.alpha = 0.65 * (1 - progress);
        sprite.scale.set(baseScale * (1 - progress * 0.55));
    },
    onComplete: release,
    onAbort: release
});
if (!started) {
    release();
    return false;
}
return true;
    }

    emitNearMissStreak(localX, localY, velocityX, velocityY, proximity = 0.5) {
if (!this.shouldEmitEffect('movement')) return;
const speed = Math.hypot(velocityX, velocityY);
if (!Number.isFinite(speed) || speed < 1) return;

const dirX = velocityX / speed;
const dirY = velocityY / speed;
const normalX = -dirY;
const normalY = dirX;
const offset = (Math.random() < 0.5 ? -1 : 1) * (16 + Math.random() * 16);
const streakLength = 22 + proximity * 34;

const startX = localX + normalX * offset - dirX * streakLength * 0.55;
const startY = localY + normalY * offset - dirY * streakLength * 0.55;
const endX = localX + normalX * offset + dirX * streakLength * 0.45;
const endY = localY + normalY * offset + dirY * streakLength * 0.45;

const streak = new PIXI.Graphics();
streak.lineStyle(2.4, 0xFFFFFF, 0.82);
streak.moveTo(startX, startY);
streak.lineTo(endX, endY);
this.effectsContainer.addChild(streak);

this.animateEffect(streak, {
    duration: this.scaleDuration(110, 55),
    priority: 2,
    onUpdate: (progress) => {
        streak.alpha = 0.86 * (1 - progress);
        streak.scale.set(1 + progress * 0.24);
    },
    onComplete: () => streak.destroy()
});
    }

    processProjectileNearMissFeedback(deltaMS = 16.67) {
if (!this.audioManager || !localPlayerState || !localPlayerState.alive || localPlayerState.is_spectator) {
    return;
}
if (!projectiles || projectiles.size === 0) return;

const localX = Number.isFinite(localPlayerState.render_x)
    ? localPlayerState.render_x
    : Number(localPlayerState.x);
const localY = Number.isFinite(localPlayerState.render_y)
    ? localPlayerState.render_y
    : Number(localPlayerState.y);
if (!Number.isFinite(localX) || !Number.isFinite(localY)) return;

const loadTier = this.getLoadTier();
const scanIntervalMs = loadTier >= 2 ? 66 : (loadTier === 1 ? 50 : 33);
this.nearMissScanAccumulatorMs += Math.max(0, Number(deltaMS) || 16.67);
if (this.nearMissScanAccumulatorMs < scanIntervalMs) {
    return;
}

const scanStepMs = this.nearMissScanAccumulatorMs;
this.nearMissScanAccumulatorMs = 0;
const dtSec = Math.min(0.1, Math.max(0.016, scanStepMs / 1000));
const nearMissRadius = loadTier >= 2 ? 48 : 56;
const nearMissRadiusSq = nearMissRadius * nearMissRadius;
const maxChecks = loadTier >= 2 ? 140 : (loadTier === 1 ? 260 : 420);
const nowMs = Date.now();
const localId = myPlayerId != null ? String(myPlayerId) : '';
const projectileCount = projectiles.size;
if (projectileCount <= 0) return;

if (nowMs - this.lastNearMissPruneAtMs > 1200) {
    this.lastNearMissPruneAtMs = nowMs;
    this.nearMissTriggerByProjectile.forEach((triggerAt, projectileId) => {
        if ((nowMs - triggerAt) > 2500 || !projectiles.has(projectileId)) {
            this.nearMissTriggerByProjectile.delete(projectileId);
        }
    });
}

let checked = 0;
let triggered = 0;
let scanProcessed = 0;
const startIndex = this.nearMissScanCursor % projectileCount;
const processProjectileCandidate = (projectileId, projectile) => {
    if (checked >= maxChecks || triggered >= 2) return false;
    checked += 1;
    if (!projectile || this.nearMissTriggerByProjectile.has(projectileId)) return false;

    const ownerId = projectile.owner_id != null ? String(projectile.owner_id) : '';
    if (ownerId && localId && ownerId === localId) return false;

    const px = Number.isFinite(projectile.render_x) ? projectile.render_x : Number(projectile.x);
    const py = Number.isFinite(projectile.render_y) ? projectile.render_y : Number(projectile.y);
    const vx = Number(projectile.velocity_x) || 0;
    const vy = Number(projectile.velocity_y) || 0;
    if (!Number.isFinite(px) || !Number.isFinite(py)) return false;
    const speedSq = vx * vx + vy * vy;
    if (!Number.isFinite(speedSq) || speedSq < 1) return false;

    const relX = localX - px;
    const relY = localY - py;
    if ((relX * vx + relY * vy) <= 0) return false;

    const nextX = px + vx * dtSec * 1.25;
    const nextY = py + vy * dtSec * 1.25;
    const segX = nextX - px;
    const segY = nextY - py;
    const segLenSq = segX * segX + segY * segY;
    if (!Number.isFinite(segLenSq) || segLenSq <= 1e-4) return false;

    let t = ((localX - px) * segX + (localY - py) * segY) / segLenSq;
    t = Math.max(0, Math.min(1, t));
    const closestX = px + segX * t;
    const closestY = py + segY * t;
    const dx = localX - closestX;
    const dy = localY - closestY;
    const distSq = dx * dx + dy * dy;
    if (!Number.isFinite(distSq) || distSq > nearMissRadiusSq) return false;

    const dist = Math.sqrt(Math.max(0, distSq));
    const proximity = 1 - Math.min(1, dist / nearMissRadius);
    const volume = 0.2 + proximity * 0.34;
    this.audioManager.playSound('bulletWhiz', { x: px, y: py }, volume);
    this.emitNearMissStreak(localX, localY, vx, vy, proximity);
    this.nearMissTriggerByProjectile.set(projectileId, nowMs);
    triggered += 1;
    return true;
};

let index = 0;
for (const [projectileId, projectile] of projectiles) {
    if (checked >= maxChecks || triggered >= 2) break;
    if (index < startIndex) {
        index += 1;
        continue;
    }
    processProjectileCandidate(projectileId, projectile);
    index += 1;
    scanProcessed += 1;
}

if (checked < maxChecks && triggered < 2 && startIndex > 0) {
    index = 0;
    for (const [projectileId, projectile] of projectiles) {
        if (checked >= maxChecks || triggered >= 2 || index >= startIndex) break;
        processProjectileCandidate(projectileId, projectile);
        index += 1;
        scanProcessed += 1;
    }
}
this.nearMissScanCursor = (startIndex + scanProcessed) % Math.max(1, projectileCount);
    }

    update(deltaMS) {
if (this.activeEffects.length > this.maxActiveEffects) {
    this.dropOverflowEffects(0);
}
this.processProjectileNearMissFeedback(deltaMS);
if (this.audioManager && localPlayerState) {
    const maxHealth = Math.max(1, Number(localPlayerState.max_health) || 100);
    const healthNow = Math.max(0, Number(localPlayerState.health) || 0);
    this.audioManager.updateLowHealthWarning(healthNow / maxHealth);
}
const loadTier = this.getLoadTier();
this.lastLoadTier = loadTier;
this.flushQueuedDamageNumbers(false, loadTier);
const updateStride = this.getUpdateStrideForLoad(loadTier);
if (updateStride > 1) {
    this.effectUpdateFrame = (this.effectUpdateFrame + 1) % updateStride;
    if (this.effectUpdateFrame !== 0) {
        return;
    }
}
const now = Date.now();
let writeIndex = 0;
for (let readIndex = 0; readIndex < this.activeEffects.length; readIndex += 1) {
    const effect = this.activeEffects[readIndex];
    if (!effect || !effect.object || effect.object.destroyed) {
        if (effect && typeof effect.onAbort === 'function') {
            effect.onAbort(effect);
        }
        continue;
    }
    if (now < effect.startTime) {
        this.activeEffects[writeIndex] = effect;
        writeIndex += 1;
        continue;
    }
    if (!effect.started) {
        effect.started = true;
        effect.actualStartTime = now;
    }

    const elapsed = now - effect.actualStartTime;
    const progress = Math.min(elapsed / effect.duration, 1);

    if (progress < 1 && effect.updateStride > 1) {
        effect.updateStrideTick = (effect.updateStrideTick + 1) % effect.updateStride;
        if (effect.updateStrideTick !== 0) {
            this.activeEffects[writeIndex] = effect;
            writeIndex += 1;
            continue;
        }
    }

    effect.onUpdate(progress, effect);

    if (progress >= 1) {
        if (effect.onComplete && effect.object && !effect.object.destroyed) {
            effect.onComplete(effect);
        }
        continue;
    }

    this.activeEffects[writeIndex] = effect;
    writeIndex += 1;
}
if (writeIndex < this.activeEffects.length) {
    this.activeEffects.length = writeIndex;
}
    }

    createMeleeSwingEffect(position, instigatorId) {
const playerSprite = this.findPlayerSpriteById(instigatorId);
if (!playerSprite) return;

const player = players.get(instigatorId);
if (!player) return;

const container = new PIXI.Container();
container.position.set(position.x, position.y);
this.effectsContainer.addChild(container);

// Enhanced melee parameters
const arcRadius = PLAYER_RADIUS + 40;
const arcAngle = Math.PI * 0.75; // 135 degree arc for wider swing
const startAngle = player.rotation - arcAngle / 2;
const windupDelayMs = 90;

// Create energy charge effect before swing
const chargeEffect = new PIXI.Graphics();
chargeEffect.lineStyle(2, 0x00FFFF, 0.6);
for (let i = 0; i < 8; i++) {
    const angle = (Math.PI * 2 * i) / 8;
    chargeEffect.moveTo(
        Math.cos(angle) * PLAYER_RADIUS,
        Math.sin(angle) * PLAYER_RADIUS
    );
    chargeEffect.lineTo(
        Math.cos(angle) * (PLAYER_RADIUS + 15),
        Math.sin(angle) * (PLAYER_RADIUS + 15)
    );
}
container.addChild(chargeEffect);

const windupGuide = new PIXI.Graphics();
windupGuide.lineStyle(2, 0x66FFFF, 0.45);
windupGuide.moveTo(0, 0);
windupGuide.lineTo(
    Math.cos(player.rotation) * (PLAYER_RADIUS + 28),
    Math.sin(player.rotation) * (PLAYER_RADIUS + 28)
);
container.addChild(windupGuide);

// Animate charge effect
this.animateEffect(chargeEffect, {
    duration: windupDelayMs,
    onUpdate: (progress) => {
        chargeEffect.scale.set(1 + progress * 0.5);
        chargeEffect.alpha = 0.6 * (1 - progress);
        chargeEffect.rotation = progress * Math.PI / 4;
    },
    onComplete: () => chargeEffect.destroy()
});
this.animateEffect(windupGuide, {
    duration: windupDelayMs,
    onUpdate: (progress) => {
        windupGuide.alpha = 0.45 * (1 - progress);
        windupGuide.scale.set(1 + progress * 0.2);
    },
    onComplete: () => windupGuide.destroy()
});

// Motion blur container
const blurContainer = new PIXI.Container();
blurContainer.alpha = 0;
container.addChild(blurContainer);

// Create enhanced motion blur layers
const blurLayerCount = this.scaleEffectCount(8, 3);
for (let b = 0; b < blurLayerCount; b++) {
    const blurArc = new PIXI.Graphics();
    const blurAlpha = 0.2 - b * 0.025;
    const blurOffset = b * 0.1;
    
    // Create gradient blur effect
    const gradient = [0xFFFFFF, 0xE0E0E0, 0xC0C0C0][b % 3];
    blurArc.beginFill(gradient, blurAlpha);
    blurArc.moveTo(0, 0);
    blurArc.arc(0, 0, arcRadius + b * 2, startAngle - blurOffset, startAngle + arcAngle - blurOffset, false);
    blurArc.closePath();
    blurArc.endFill();
    
    blurContainer.addChild(blurArc);
}

// Main swing arc with multi-layer gradient
const arcContainer = new PIXI.Container();
arcContainer.alpha = 0;

// Energy field layer
const energyField = new PIXI.Graphics();
energyField.beginFill(0x00FFFF, 0.2);
energyField.moveTo(0, 0);
energyField.arc(0, 0, arcRadius + 10, startAngle - 0.1, startAngle + arcAngle + 0.1, false);
energyField.closePath();
energyField.endFill();
energyField.filters = [getSharedBlurFilter(5)];
arcContainer.addChild(energyField);

// Main arc layers
const arcLayers = [
    { radius: arcRadius, color: 0xB0B0B0, alpha: 0.3 },
    { radius: arcRadius * 0.9, color: 0xD0D0D0, alpha: 0.5 },
    { radius: arcRadius * 0.8, color: 0xE0E0E0, alpha: 0.7 },
    { radius: arcRadius * 0.7, color: 0xFFFFFF, alpha: 0.9 }
];

arcLayers.forEach(layer => {
    const arc = new PIXI.Graphics();
    arc.beginFill(layer.color, layer.alpha);
    arc.moveTo(0, 0);
    arc.arc(0, 0, layer.radius, startAngle, startAngle + arcAngle, false);
    arc.closePath();
    arc.endFill();
    arcContainer.addChild(arc);
});

container.addChild(arcContainer);

// Enhanced energy trail particles
const trailCount = this.scaleEffectCount(20, 6);
for (let i = 0; i < trailCount; i++) {
    const trailAngle = startAngle + (arcAngle * i / trailCount);
    const trailRadius = arcRadius - 5 + Math.random() * 10;
    
    // Create trail with glow
    const trailContainer = new PIXI.Container();
    
    const trailGlow = new PIXI.Graphics();
    trailGlow.beginFill(0x00FFFF, 0.3);
    trailGlow.drawCircle(0, 0, 8);
    trailGlow.endFill();
    trailGlow.filters = [getSharedBlurFilter(3)];
    trailContainer.addChild(trailGlow);
    
    const trail = new PIXI.Graphics();
    trail.beginFill(0xFFFFFF, 0.9);
    trail.drawCircle(0, 0, 3);
    trail.endFill();
    trailContainer.addChild(trail);
    
    trailContainer.position.set(
        Math.cos(trailAngle) * trailRadius,
        Math.sin(trailAngle) * trailRadius
    );
    trailContainer.alpha = 0;
    
    container.addChild(trailContainer);
    
    // Animate trail with spiral motion
    this.animateEffect(trailContainer, {
        duration: 400,
        delay: windupDelayMs + i * 10,
        onUpdate: (progress) => {
            const spiralFactor = 1 + progress * 0.5;
            const currentRadius = trailRadius + progress * 30;
            const currentAngle = trailAngle + progress * 0.5;
            
            trailContainer.position.set(
                Math.cos(currentAngle) * currentRadius * spiralFactor,
                Math.sin(currentAngle) * currentRadius * spiralFactor
            );
            trailContainer.alpha = 0.9 * (1 - progress);
            trailContainer.scale.set(1 + progress * 3);
        },
        onComplete: () => trailContainer.destroy()
    });
}

// Create cutting edge effect
const cuttingEdge = new PIXI.Graphics();
cuttingEdge.lineStyle(2, 0xFFFFFF, 1);
const edgePoints = [];
for (let i = 0; i <= 10; i++) {
    const angle = startAngle + (arcAngle * i / 10);
    edgePoints.push(
        Math.cos(angle) * arcRadius,
        Math.sin(angle) * arcRadius
    );
}
cuttingEdge.drawPolygon(edgePoints);
cuttingEdge.alpha = 0;
container.addChild(cuttingEdge);

// Animate cutting edge
this.animateEffect(cuttingEdge, {
    duration: 200,
    delay: windupDelayMs,
    onUpdate: (progress) => {
        cuttingEdge.alpha = 1 - progress;
        cuttingEdge.scale.set(1 + progress * 0.2);
    },
    onComplete: () => cuttingEdge.destroy()
});

// Enhanced slash lines with energy effect
const slashCount = this.scaleEffectCount(8, 3);
for (let i = 0; i < slashCount; i++) {
    const slashContainer = new PIXI.Container();
    const slashAngle = startAngle + (arcAngle * i / slashCount) + arcAngle / (slashCount * 2);
    const innerRadius = PLAYER_RADIUS + 10;
    
    // Slash glow
    const slashGlow = new PIXI.Graphics();
    slashGlow.lineStyle(8, 0x00FFFF, 0.3);
    slashGlow.moveTo(
        Math.cos(slashAngle) * innerRadius,
        Math.sin(slashAngle) * innerRadius
    );
    slashGlow.lineTo(
        Math.cos(slashAngle) * arcRadius,
        Math.sin(slashAngle) * arcRadius
    );
    slashGlow.filters = [getSharedBlurFilter(2)];
    slashContainer.addChild(slashGlow);
    
    // Main slash
    const slash = new PIXI.Graphics();
    slash.lineStyle(3 - i * 0.3, 0xFFFFFF, 1 - i * 0.1);
    slash.moveTo(
        Math.cos(slashAngle) * innerRadius,
        Math.sin(slashAngle) * innerRadius
    );
    slash.lineTo(
        Math.cos(slashAngle) * arcRadius,
        Math.sin(slashAngle) * arcRadius
    );
    slashContainer.addChild(slash);
    
    container.addChild(slashContainer);
    
    // Animate slash with delay
    slashContainer.alpha = 0;
    this.animateEffect(slashContainer, {
        duration: 250,
        delay: windupDelayMs + i * 15,
        onUpdate: (progress) => {
            slashContainer.alpha = (1 - progress) * (1 - i * 0.1);
            slashContainer.scale.set(1 + progress * 0.3);
        },
        onComplete: () => slashContainer.destroy()
    });
}

// Multiple impact shockwaves
const shockwaveCount = this.scaleEffectCount(3, 1);
for (let w = 0; w < shockwaveCount; w++) {
    this.scheduleCallback(windupDelayMs + w * 50, () => {
        const shockwave = new PIXI.Graphics();
        shockwave.lineStyle(4 - w, 0xFFFFFF, 0.8 - w * 0.2);
        shockwave.drawCircle(0, 0, arcRadius * (0.7 + w * 0.1));
        container.addChild(shockwave);
        
        this.animateEffect(shockwave, {
            duration: 400,
            onUpdate: (progress) => {
                shockwave.scale.set(0.8 + progress * (0.8 + w * 0.2));
                shockwave.alpha = (0.8 - w * 0.2) * (1 - progress);
            },
            onComplete: () => shockwave.destroy()
        });
    });
}

// Enhanced impact sparks with physics
const sparkCount = this.scaleEffectCount(25, 8);
for (let i = 0; i < sparkCount; i++) {
    const sparkContainer = new PIXI.Container();
    
    // Spark glow
    const sparkGlow = new PIXI.Graphics();
    const sparkColor = [0xFFFF88, 0xFFFFFF, 0x88DDFF, 0xFF88FF][i % 4];
    sparkGlow.beginFill(sparkColor, 0.5);
    sparkGlow.drawCircle(0, 0, 6);
    sparkGlow.endFill();
    sparkGlow.filters = [getSharedBlurFilter(2)];
    sparkContainer.addChild(sparkGlow);
    
    // Spark core
    const spark = new PIXI.Graphics();
    spark.beginFill(0xFFFFFF, 1);
    spark.drawCircle(0, 0, 2);
    spark.endFill();
    sparkContainer.addChild(spark);
    
    const sparkAngle = startAngle + Math.random() * arcAngle;
    const sparkRadius = arcRadius - 10 + Math.random() * 20;
    sparkContainer.position.set(
        Math.cos(sparkAngle) * sparkRadius,
        Math.sin(sparkAngle) * sparkRadius
    );
    sparkContainer.alpha = 0;
    
    container.addChild(sparkContainer);
    
    const velocity = {
        x: Math.cos(sparkAngle) * (6 + Math.random() * 8),
        y: Math.sin(sparkAngle) * (6 + Math.random() * 8) - 3
    };
    
    this.animateEffect(sparkContainer, {
        duration: 600 + Math.random() * 300,
        delay: windupDelayMs,
        onUpdate: (progress) => {
            // Physics-based motion
            sparkContainer.x += velocity.x * (1 - progress * 0.8);
            sparkContainer.y += velocity.y * (1 - progress * 0.8) + progress * 15; // Gravity
            sparkContainer.alpha = 1 * (1 - progress);
            sparkContainer.scale.set((1 - progress * 0.5) * (1 + Math.sin(progress * Math.PI * 6) * 0.3));
            sparkContainer.rotation += 0.2;
        },
        onComplete: () => sparkContainer.destroy()
    });
}

// Animate the main arc
this.animateEffect(arcContainer, {
    duration: 300,
    delay: windupDelayMs,
    onUpdate: (progress) => {
        arcContainer.scale.set(0.6 + progress * 0.6);
        arcContainer.alpha = 1 * (1 - progress * 0.8);
        arcContainer.rotation = progress * 0.4;
    },
    onComplete: () => {
        arcContainer.destroy();
    }
});

// Animate motion blur
this.animateEffect(blurContainer, {
    duration: 350,
    delay: windupDelayMs,
    onUpdate: (progress) => {
        blurContainer.alpha = 1 - progress;
        blurContainer.scale.set(1 + progress * 0.3);
        blurContainer.rotation = progress * 0.2;
    },
    onComplete: () => {
        blurContainer.destroy();
        if (container.children.length === 0) {
            container.destroy();
        }
    }
});

// Enhanced screen effects for local player
if (instigatorId === myPlayerId) {
    this.scheduleCallback(windupDelayMs, () => {
        if (gameSettings.screenShake) {
            applyScreenShake(gameScene, 200, 8);
        }
        createScreenFlash(app, 0xFFFFFF, 8, 0.5);
    });
    this.scheduleCallback(windupDelayMs + 50, () => createScreenFlash(app, 0x88DDFF, 15, 0.3));
}
    }

    createWallRespawnEffect(position, wallData) {
const container = new PIXI.Container();
container.position.set(position.x, position.y);
this.effectsContainer.addChild(container);

const wallWidth = wallData.width;
const wallHeight = wallData.height;

// Enhanced Phase 0: Energy gathering announcement
// Create energy vortex effect
const vortex = new PIXI.Graphics();
vortex.lineStyle(3, 0x00FFFF, 0.3);
vortex.drawCircle(0, 0, Math.max(wallWidth, wallHeight) * 0.8);
container.addChild(vortex);

this.animateEffect(vortex, {
    duration: 800,
    onUpdate: (progress) => {
        vortex.scale.set(2 - progress * 1.5);
        vortex.alpha = 0.3 * (1 - progress);
        vortex.rotation = progress * Math.PI * 2;
    },
    onComplete: () => vortex.destroy()
});

// Phase 1: Enhanced energy particles converging with trails
const particleCount = this.scaleEffectCount(30, 8);
const energyColors = [0x00FFFF, 0x00DDFF, 0x66FFFF, 0x00AAFF];

for (let i = 0; i < particleCount; i++) {
    const particleContainer = new PIXI.Container();
    
    // Main particle
    const particle = new PIXI.Graphics();
    const color = energyColors[i % energyColors.length];
    particle.beginFill(color, 0.9);
    particle.drawCircle(0, 0, 2 + Math.random() * 2);
    particle.endFill();
    
    // Particle glow
    const glow = new PIXI.Graphics();
    glow.beginFill(color, 0.3);
    glow.drawCircle(0, 0, 8);
    glow.endFill();
    glow.filters = [getSharedBlurFilter(3)];
    
    particleContainer.addChild(glow);
    particleContainer.addChild(particle);
    
    // Start from random positions in a wider area
    const angle = (Math.PI * 2 * i) / particleCount + Math.random() * 0.5;
    const distance = 80 + Math.random() * 80;
    particleContainer.position.set(
        Math.cos(angle) * distance,
        Math.sin(angle) * distance
    );
    
    container.addChild(particleContainer);
    
    // Create trailing effect
    const trail = new PIXI.Graphics();
    container.addChildAt(trail, 0);
    
    let lastPositions = [{x: particleContainer.x, y: particleContainer.y}];
    
    // Animate particles with spiral converging motion
    this.animateEffect(particleContainer, {
        duration: 800,
        delay: i * 20,
        onUpdate: (progress) => {
            const easeProgress = 1 - Math.pow(1 - progress, 3);
            const spiralAngle = angle + progress * Math.PI * 2;
            const currentDistance = distance * (1 - easeProgress);
            
            particleContainer.x = Math.cos(spiralAngle) * currentDistance;
            particleContainer.y = Math.sin(spiralAngle) * currentDistance;
            particleContainer.alpha = 0.9 + Math.sin(progress * Math.PI * 6) * 0.1;
            
            // Update trail
            lastPositions.push({x: particleContainer.x, y: particleContainer.y});
            if (lastPositions.length > 10) lastPositions.shift();
            
            trail.clear();
            if (lastPositions.length > 1) {
                for (let j = 0; j < lastPositions.length - 1; j++) {
                    const alpha = (j / lastPositions.length) * 0.3 * (1 - progress);
                    trail.lineStyle(2, color, alpha);
                    if (j === 0) {
                        trail.moveTo(lastPositions[j].x, lastPositions[j].y);
                    }
                    trail.lineTo(lastPositions[j + 1].x, lastPositions[j + 1].y);
                }
            }
        },
        onComplete: () => {
            particleContainer.destroy();
            trail.destroy();
        }
    });
}

// Phase 2: Enhanced wireframe with scanning effect
this.scheduleCallback(500, () => {
    const wireframeContainer = new PIXI.Container();
    container.addChild(wireframeContainer);
    
    // Main wireframe
    const wireframe = new PIXI.Graphics();
    wireframe.lineStyle(3, 0x00FFFF, 0.9);
    wireframe.drawRect(-wallWidth/2, -wallHeight/2, wallWidth, wallHeight);
    
    // Grid lines with animation
    const gridSize = Math.min(20, Math.min(wallWidth, wallHeight) / 4);
    const gridLines = new PIXI.Graphics();
    
    wireframeContainer.addChild(wireframe);
    wireframeContainer.addChild(gridLines);
    
    // Corner highlights
    const corners = new PIXI.Graphics();
    corners.lineStyle(4, 0x00FFFF, 1);
    const cornerSize = 10;
    // Top-left
    corners.moveTo(-wallWidth/2, -wallHeight/2 + cornerSize);
    corners.lineTo(-wallWidth/2, -wallHeight/2);
    corners.lineTo(-wallWidth/2 + cornerSize, -wallHeight/2);
    // Top-right
    corners.moveTo(wallWidth/2 - cornerSize, -wallHeight/2);
    corners.lineTo(wallWidth/2, -wallHeight/2);
    corners.lineTo(wallWidth/2, -wallHeight/2 + cornerSize);
    // Bottom-left
    corners.moveTo(-wallWidth/2, wallHeight/2 - cornerSize);
    corners.lineTo(-wallWidth/2, wallHeight/2);
    corners.lineTo(-wallWidth/2 + cornerSize, wallHeight/2);
    // Bottom-right
    corners.moveTo(wallWidth/2 - cornerSize, wallHeight/2);
    corners.lineTo(wallWidth/2, wallHeight/2);
    corners.lineTo(wallWidth/2, wallHeight/2 - cornerSize);
    
    wireframeContainer.addChild(corners);
    
    // Scanning line effect
    const scanLine = new PIXI.Graphics();
    scanLine.lineStyle(2, 0x00FFFF, 0.8);
    scanLine.moveTo(-wallWidth/2, 0);
    scanLine.lineTo(wallWidth/2, 0);
    scanLine.position.y = -wallHeight/2;
    wireframeContainer.addChild(scanLine);
    
    this.animateEffect(wireframeContainer, {
        duration: 600,
        onUpdate: (progress) => {
            wireframeContainer.alpha = Math.min(1, progress * 2);
            wireframeContainer.scale.set(1.1 - progress * 0.1);
            
            // Animate scan line
            scanLine.position.y = -wallHeight/2 + wallHeight * progress;
            
            // Animate grid appearance
            gridLines.clear();
            gridLines.lineStyle(1, 0x00FFFF, 0.4 * progress);
            const visibleGridLines = Math.floor(progress * (wallWidth / gridSize));
            for (let i = 1; i <= visibleGridLines; i++) {
                const x = -wallWidth/2 + i * gridSize;
                if (x < wallWidth/2) {
                    gridLines.moveTo(x, -wallHeight/2);
                    gridLines.lineTo(x, wallHeight/2);
                }
            }
            const visibleHGridLines = Math.floor(progress * (wallHeight / gridSize));
            for (let i = 1; i <= visibleHGridLines; i++) {
                const y = -wallHeight/2 + i * gridSize;
                if (y < wallHeight/2) {
                    gridLines.moveTo(-wallWidth/2, y);
                    gridLines.lineTo(wallWidth/2, y);
                }
            }
        },
        onComplete: () => wireframeContainer.destroy()
    });
});

// Phase 3: Enhanced fill effect with energy waves
this.scheduleCallback(900, () => {
    const fillContainer = new PIXI.Container();
    container.addChild(fillContainer);
    
    // Create multiple fill layers for depth
    const fillLayers = [];
    for (let i = 0; i < 3; i++) {
        const fill = new PIXI.Graphics();
        fillLayers.push(fill);
        fillContainer.addChild(fill);
    }
    
    // Energy wave rings
    const waveCount = this.scaleEffectCount(3, 1);
    for (let i = 0; i < waveCount; i++) {
        this.scheduleCallback(i * 100, () => {
            const wave = new PIXI.Graphics();
            wave.lineStyle(2, 0x00FFFF, 0.6);
            wave.drawRect(-wallWidth/2 - 5, -wallHeight/2 - 5, wallWidth + 10, wallHeight + 10);
            fillContainer.addChild(wave);
            
            this.animateEffect(wave, {
                duration: 400,
                onUpdate: (progress) => {
                    wave.scale.set(1 + progress * 0.2);
                    wave.alpha = 0.6 * (1 - progress);
                },
                onComplete: () => wave.destroy()
            });
        });
    }
    
    this.animateEffect(fillContainer, {
        duration: 700,
        onUpdate: (progress) => {
            // Multi-layer fill effect
            fillLayers.forEach((fill, index) => {
                fill.clear();
                const layerProgress = Math.min(1, progress * (1 + index * 0.2));
                const alpha = 0.3 + index * 0.2;
                const color = interpolateColor(0x00FFFF, 0x374151, layerProgress);
                
                fill.beginFill(color, alpha * layerProgress);
                
                // Different fill patterns for each layer
                if (index === 0) {
                    // Edge to center fill
                    const inset = (1 - layerProgress) * Math.min(wallWidth, wallHeight) / 2;
                    fill.drawRect(
                        -wallWidth/2 + inset,
                        -wallHeight/2 + inset,
                        wallWidth - inset * 2,
                        wallHeight - inset * 2
                    );
                } else if (index === 1) {
                    // Horizontal sweep
                    fill.drawRect(
                        -wallWidth/2,
                        -wallHeight/2,
                        wallWidth * layerProgress,
                        wallHeight
                    );
                } else {
                    // Vertical sweep
                    fill.drawRect(
                        -wallWidth/2,
                        -wallHeight/2,
                        wallWidth,
                        wallHeight * layerProgress
                    );
                }
                fill.endFill();
            });
        },
        onComplete: () => fillContainer.destroy()
    });
});

// Phase 4: Enhanced final materialization
this.scheduleCallback(1200, () => {
    // Pre-flash energy burst
    const burst = new PIXI.Graphics();
    burst.lineStyle(3, 0x00FFFF, 0.8);
    for (let i = 0; i < 8; i++) {
        const angle = (Math.PI * 2 * i) / 8;
        burst.moveTo(0, 0);
        burst.lineTo(
            Math.cos(angle) * Math.max(wallWidth, wallHeight) * 0.7,
            Math.sin(angle) * Math.max(wallWidth, wallHeight) * 0.7
        );
    }
    container.addChild(burst);
    
    // Multiple flash layers
    const flashLayers = [];
    for (let i = 0; i < 3; i++) {
        const flash = new PIXI.Graphics();
        flash.beginFill(0x00FFFF, 0.6 - i * 0.15);
        flash.drawRect(
            -wallWidth/2 - 15 - i * 5,
            -wallHeight/2 - 15 - i * 5,
            wallWidth + 30 + i * 10,
            wallHeight + 30 + i * 10
        );
        flash.endFill();
        flash.filters = [getSharedBlurFilter(3 + i * 2)];
        container.addChild(flash);
        flashLayers.push(flash);
    }
    
    // Final solidification particles
    const finalParticleCount = this.scaleEffectCount(20, 6);
    for (let i = 0; i < finalParticleCount; i++) {
        const particle = new PIXI.Graphics();
        particle.beginFill(0x374151, 0.8);
        particle.drawCircle(0, 0, 2);
        particle.endFill();
        
        const startX = (Math.random() - 0.5) * wallWidth;
        const startY = (Math.random() - 0.5) * wallHeight;
        particle.position.set(startX, startY);
        container.addChild(particle);
        
        this.animateEffect(particle, {
            duration: 300,
            delay: Math.random() * 100,
            onUpdate: (progress) => {
                particle.alpha = 0.8 * (1 - progress);
                particle.y = startY - progress * 20;
            },
            onComplete: () => particle.destroy()
        });
    }
    
    // Animate burst
    this.animateEffect(burst, {
        duration: 200,
        onUpdate: (progress) => {
            burst.scale.set(1 + progress * 0.5);
            burst.alpha = 0.8 * (1 - progress);
            burst.rotation = progress * 0.2;
        },
        onComplete: () => burst.destroy()
    });
    
    // Animate flash layers
    flashLayers.forEach((flash, index) => {
        this.animateEffect(flash, {
            duration: 250 + index * 50,
            onUpdate: (progress) => {
                flash.scale.set(1 + progress * (0.3 + index * 0.1));
                flash.alpha = (0.6 - index * 0.15) * (1 - progress);
            },
            onComplete: () => {
                flash.destroy();
                if (index === flashLayers.length - 1) {
                    container.destroy();
                }
            }
        });
    });
    
    // Enhanced sound effect
    if (this.audioManager) {
        this.audioManager.playSound('powerupCollect', position, 0.8);
        // Additional materialization sound
        this.scheduleCallback(100, () => {
            this.audioManager.playSound('powerupCollect', position, 0.4);
        });
    }
    
    // Screen effect for nearby walls
    if (localPlayerState) {
        const dx = position.x - localPlayerState.x;
        const dy = position.y - localPlayerState.y;
        const distance = Math.sqrt(dx * dx + dy * dy);
        if (distance < 300 && gameSettings.screenShake) {
            applyScreenShake(gameScene, 50, 2);
        }
    }
});
    }

    clearAllEffects() {
this.pendingTimers.forEach(timerId => clearTimeout(timerId));
this.pendingTimers.clear();
this.activeEffects.forEach(effect => {
    if (effect && typeof effect.onAbort === 'function') {
        effect.onAbort(effect);
    } else if (effect) {
        this.destroyEffectObject(effect.object);
    }
});
this.activeEffects = [];
this.effectsContainer.removeChildren().forEach(c => this.destroyEffectObject(c));
// Pooled trail sprites live in effectsContainer and were just destroyed;
// reset the pool so destroyed entries don't count toward the pool cap.
this.engineTrailPool.length = 0;
this.engineTrailPoolCursor = 0;
this.activeDamageNumberCount = 0;
this.pendingDamageNumberBatches.clear();
this.activeDamageNumberEffectsByKey.clear();
this.pendingDamageBatchCount = 0;
    }
}



// Audio Manager
// Audio Manager

class AudioManager {
    constructor() {
        this.soundEnabled = true;
        this.globalVolume = 0.5;
        this.audioContext = null;
        this.masterOutputNode = null;
        this.masterGainNode = null;
        this.compressorNode = null;
        try {
            this.audioContext = new (window.AudioContext || window.webkitAudioContext)();
        } catch (e) {
            emitClientLog("Web Audio API not supported.", "warn");
        }

this.sounds = {
    pistolFire: { freq: [800, 600], duration: 0.05, type: 'triangle', vol: 0.3 },
    shotgunFire: { freq: [400, 200], duration: 0.15, type: 'sawtooth', vol: 0.5 },
    rifleFire: { freq: [700, 500], duration: 0.07, type: 'square', vol: 0.35 },
    sniperFire: { freq: [1000, 300], duration: 0.2, type: 'sine', vol: 0.6 },
    meleeSwing: { freq: [300, 500], duration: 0.1, type: 'sine', vol: 0.2 },
    bulletImpact: { freq: [200, 100], duration: 0.08, type: 'noise', vol: 0.25 },
    impactConcrete: { freq: [220, 120], duration: 0.08, type: 'noise', vol: 0.24 },
    impactMetal: { freq: [1200, 760], duration: 0.08, type: 'triangle', vol: 0.22 },
    impactWood: { freq: [320, 180], duration: 0.09, type: 'noise', vol: 0.2 },
    impactGlass: { freq: [2000, 1400], duration: 0.07, type: 'sine', vol: 0.18 },
    footstepConcrete: { freq: [180, 140], duration: 0.04, type: 'noise', vol: 0.08 },
    footstepMetal: { freq: [540, 380], duration: 0.04, type: 'triangle', vol: 0.08 },
    footstepWood: { freq: [240, 180], duration: 0.04, type: 'noise', vol: 0.08 },
    footstepGlass: { freq: [900, 620], duration: 0.035, type: 'sine', vol: 0.06 },
    explosion: { freq: [300, 50], duration: 0.5, type: 'noise', vol: 0.7 },
    powerupCollect: { freq: [600, 1200], duration: 0.2, type: 'sine', vol: 0.4 },
    playerHit: { freq: [250, 150], duration: 0.1, type: 'sawtooth', vol: 0.3 },
    flagCapture: { freq: [800, 1000, 1200], duration: 0.4, type: 'square', vol: 0.5 },
    chatMessage: { freq: [1000, 1200], duration: 0.1, type: 'sine', vol: 0.2 },
    outOfAmmo: { freq: [150, 100], duration: 0.15, type: 'square', vol: 0.3 },
    reloadStart: { freq: [400, 300], duration: 0.1, type: 'sawtooth', vol: 0.25 },
    reloadNeeded: { freq: [200], duration: 0.1, type: 'sine', vol: 0.35 }, // freq[1] is undefined
    flagGrabbed: { freq: [700, 900], duration: 0.2, type: 'triangle', vol: 0.45 },
    flagDropped: { freq: [600, 400], duration: 0.25, type: 'sawtooth', vol: 0.4 },
    flagReturned: { freq: [500, 800, 600], duration: 0.3, type: 'sine', vol: 0.5 },
    hitMarker: { freq: [1400, 980], duration: 0.05, type: 'square', vol: 0.2 },
    hitMarkerHeadshot: { freq: [1600, 2300, 1800], duration: 0.09, type: 'triangle', vol: 0.28 },
    announcerHeadshot: { freq: [980, 1320, 1660], duration: 0.26, type: 'square', vol: 0.36 },
    announcerDoubleKill: { freq: [540, 760, 980], duration: 0.34, type: 'triangle', vol: 0.38 },
    announcerTripleKill: { freq: [620, 920, 1260], duration: 0.4, type: 'triangle', vol: 0.42 },
    announcerRampage: { freq: [360, 520, 760], duration: 0.55, type: 'sawtooth', vol: 0.44 },
    spawnChime: { freq: [620, 900, 1180], duration: 0.28, type: 'sine', vol: 0.36 },
    flagFanfare: { freq: [760, 980, 1240, 1560], duration: 0.45, type: 'triangle', vol: 0.5 },
    bulletWhiz: { freq: [1800, 900], duration: 0.09, type: 'sine', vol: 0.26 },
    dashWhoosh: { freq: [220, 520], duration: 0.18, type: 'sawtooth', vol: 0.3 },
    dodgeWhoosh: { freq: [280, 640], duration: 0.16, type: 'triangle', vol: 0.28 },
    weaponSwap: { freq: [1400, 980], duration: 0.08, type: 'triangle', vol: 0.22 },
    killConfirm: { freq: [260, 540, 860], duration: 0.16, type: 'triangle', vol: 0.36 },
    shieldBreak: { freq: [920, 640, 420], duration: 0.22, type: 'sawtooth', vol: 0.4 },
    powerupWarning: { freq: [640, 860], duration: 0.12, type: 'square', vol: 0.24 },
    heartbeatPulse: { freq: [110, 82], duration: 0.12, type: 'sine', vol: 0.16 },
    ambientCombat: { freq: [380, 180], duration: 0.28, type: 'noise', vol: 0.12 },
    wallRumble: { freq: [72, 44], duration: 0.34, type: 'noise', vol: 0.18 },
    zoneSlowEnter: { freq: [240, 180, 130], duration: 0.22, type: 'triangle', vol: 0.18 },
    zoneSlowExit: { freq: [160, 220], duration: 0.16, type: 'sine', vol: 0.12 },
    zoneDamageEnter: { freq: [780, 560, 420], duration: 0.18, type: 'sawtooth', vol: 0.2 },
    zoneDamageExit: { freq: [520, 680], duration: 0.14, type: 'triangle', vol: 0.12 },
    zoneBoostEnter: { freq: [420, 760, 1080], duration: 0.18, type: 'sine', vol: 0.18 },
    zoneBoostExit: { freq: [1080, 760], duration: 0.14, type: 'triangle', vol: 0.12 },
    countdownBeep: { freq: [920, 780], duration: 0.12, type: 'square', vol: 0.24 },
    victorySting: { freq: [720, 980, 1320], duration: 0.58, type: 'triangle', vol: 0.46 },
    defeatSting: { freq: [560, 420, 280], duration: 0.62, type: 'sawtooth', vol: 0.44 },
};

this.soundSamples = Object.freeze({
    pistolFire: 'sfx/pistol_fire.wav',
    shotgunFire: 'sfx/shotgun_fire.wav',
    rifleFire: 'sfx/rifle_fire.wav',
    sniperFire: 'sfx/sniper_fire.wav',
    meleeSwing: 'sfx/melee_swing.wav',
    bulletImpact: 'sfx/bullet_impact.wav',
    explosion: 'sfx/explosion.wav',
    powerupCollect: 'sfx/powerup_collect.wav',
    playerHit: 'sfx/player_hit.wav',
    flagCapture: 'sfx/flag_capture.wav',
    flagGrabbed: 'sfx/flag_grabbed.wav',
    flagDropped: 'sfx/flag_dropped.wav',
    flagReturned: 'sfx/flag_returned.wav',
    hitMarker: 'sfx/hit_marker.wav',
    hitMarkerHeadshot: 'sfx/hit_marker_headshot.wav',
    announcerHeadshot: 'sfx/announcer_headshot.wav',
    announcerDoubleKill: 'sfx/announcer_double_kill.wav',
    announcerTripleKill: 'sfx/announcer_triple_kill.wav',
    announcerRampage: 'sfx/announcer_rampage.wav',
    spawnChime: 'sfx/spawn_chime.wav',
    flagFanfare: 'sfx/flag_fanfare.wav',
    bulletWhiz: 'sfx/bullet_whiz.wav',
    dashWhoosh: 'sfx/dash_whoosh.wav',
    dodgeWhoosh: 'sfx/dodge_whoosh.wav',
    weaponSwap: 'sfx/weapon_swap.wav',
    countdownBeep: 'sfx/countdown_beep.wav',
    victorySting: 'sfx/victory_sting.wav',
    defeatSting: 'sfx/defeat_sting.wav',
    outOfAmmo: 'sfx/out_of_ammo.wav',
    reloadStart: 'sfx/reload_start.wav',
    reloadNeeded: 'sfx/reload_needed.wav',
    chatMessage: 'sfx/chat_message.wav',
});
this.sampleBuffers = new Map();
this.sampleLoadPromises = new Map();
this.sampleLoadFailures = new Set();
this.warnedSampleOnlyFallback = new Set();
this.pendingSounds = [];
this.maxPendingSounds = 48;
this.flushingPendingSounds = false;
this.criticalCombatSamples = Object.freeze([
    'pistolFire',
    'shotgunFire',
    'rifleFire',
    'sniperFire',
    'meleeSwing',
    'playerHit',
    'hitMarker',
    'hitMarkerHeadshot',
]);
this.criticalSamplesLoaded = false;

this.resumeInFlight = false;
this.soundActivity = new Map();
this.voicePool = [];
this.voiceCursor = 0;
this.noiseBufferCache = new Map();
this.mobileSoundBudget = mobileDynamicsEnabled || forceMobileClient;
this.soundBudgetScale = this.mobileSoundBudget ? 1.8 : 1.0;
this.defaultSoundLimit = Object.freeze({
    minIntervalMs: 12,
    windowMs: 1000,
    maxPerWindow: 72,
    maxConcurrent: 8
});
this.soundLimits = Object.freeze({
    pistolFire: { minIntervalMs: 16, windowMs: 1000, maxPerWindow: 42, maxConcurrent: 5 },
    rifleFire: { minIntervalMs: 14, windowMs: 1000, maxPerWindow: 52, maxConcurrent: 6 },
    shotgunFire: { minIntervalMs: 24, windowMs: 1000, maxPerWindow: 28, maxConcurrent: 4 },
    sniperFire: { minIntervalMs: 72, windowMs: 1000, maxPerWindow: 12, maxConcurrent: 2 },
    meleeSwing: { minIntervalMs: 48, windowMs: 1000, maxPerWindow: 14, maxConcurrent: 2 },
    bulletImpact: { minIntervalMs: 48, windowMs: 1000, maxPerWindow: 18, maxConcurrent: 3 },
    impactConcrete: { minIntervalMs: 48, windowMs: 1000, maxPerWindow: 18, maxConcurrent: 3 },
    impactMetal: { minIntervalMs: 56, windowMs: 1000, maxPerWindow: 16, maxConcurrent: 3 },
    impactWood: { minIntervalMs: 52, windowMs: 1000, maxPerWindow: 16, maxConcurrent: 3 },
    impactGlass: { minIntervalMs: 64, windowMs: 1000, maxPerWindow: 12, maxConcurrent: 2 },
    footstepConcrete: { minIntervalMs: 85, windowMs: 1000, maxPerWindow: 12, maxConcurrent: 2 },
    footstepMetal: { minIntervalMs: 85, windowMs: 1000, maxPerWindow: 12, maxConcurrent: 2 },
    footstepWood: { minIntervalMs: 85, windowMs: 1000, maxPerWindow: 12, maxConcurrent: 2 },
    footstepGlass: { minIntervalMs: 95, windowMs: 1000, maxPerWindow: 10, maxConcurrent: 2 },
    playerHit: { minIntervalMs: 52, windowMs: 1000, maxPerWindow: 16, maxConcurrent: 3 },
    explosion: { minIntervalMs: 120, windowMs: 1000, maxPerWindow: 8, maxConcurrent: 2 },
    hitMarker: { minIntervalMs: 55, windowMs: 1000, maxPerWindow: 14, maxConcurrent: 2 },
    hitMarkerHeadshot: { minIntervalMs: 180, windowMs: 1000, maxPerWindow: 5, maxConcurrent: 1 },
    bulletWhiz: { minIntervalMs: 130, windowMs: 1000, maxPerWindow: 8, maxConcurrent: 2 },
    dashWhoosh: { minIntervalMs: 220, windowMs: 1000, maxPerWindow: 4, maxConcurrent: 1 },
    dodgeWhoosh: { minIntervalMs: 220, windowMs: 1000, maxPerWindow: 4, maxConcurrent: 1 },
    killConfirm: { minIntervalMs: 90, windowMs: 1000, maxPerWindow: 8, maxConcurrent: 2 },
    shieldBreak: { minIntervalMs: 150, windowMs: 1000, maxPerWindow: 5, maxConcurrent: 2 },
    powerupWarning: { minIntervalMs: 180, windowMs: 1000, maxPerWindow: 5, maxConcurrent: 1 },
    heartbeatPulse: { minIntervalMs: 240, windowMs: 1000, maxPerWindow: 6, maxConcurrent: 1 },
    ambientCombat: { minIntervalMs: 320, windowMs: 1000, maxPerWindow: 5, maxConcurrent: 1 },
    wallRumble: { minIntervalMs: 280, windowMs: 1000, maxPerWindow: 3, maxConcurrent: 1 },
    zoneSlowEnter: { minIntervalMs: 500, windowMs: 2000, maxPerWindow: 2, maxConcurrent: 1 },
    zoneSlowExit: { minIntervalMs: 500, windowMs: 2000, maxPerWindow: 2, maxConcurrent: 1 },
    zoneDamageEnter: { minIntervalMs: 500, windowMs: 2000, maxPerWindow: 2, maxConcurrent: 1 },
    zoneDamageExit: { minIntervalMs: 500, windowMs: 2000, maxPerWindow: 2, maxConcurrent: 1 },
    zoneBoostEnter: { minIntervalMs: 500, windowMs: 2000, maxPerWindow: 2, maxConcurrent: 1 },
    zoneBoostExit: { minIntervalMs: 500, windowMs: 2000, maxPerWindow: 2, maxConcurrent: 1 },
    spawnChime: { minIntervalMs: 400, windowMs: 1000, maxPerWindow: 2, maxConcurrent: 1 },
    flagFanfare: { minIntervalMs: 1200, windowMs: 2000, maxPerWindow: 1, maxConcurrent: 1 },
    weaponSwap: { minIntervalMs: 80, windowMs: 1000, maxPerWindow: 8, maxConcurrent: 2 },
    countdownBeep: { minIntervalMs: 480, windowMs: 2000, maxPerWindow: 4, maxConcurrent: 1 },
    victorySting: { minIntervalMs: 1800, windowMs: 4000, maxPerWindow: 1, maxConcurrent: 1 },
    defeatSting: { minIntervalMs: 1800, windowMs: 4000, maxPerWindow: 1, maxConcurrent: 1 },
    announcerHeadshot: { minIntervalMs: 300, windowMs: 1000, maxPerWindow: 3, maxConcurrent: 1 },
    announcerDoubleKill: { minIntervalMs: 500, windowMs: 1000, maxPerWindow: 2, maxConcurrent: 1 },
    announcerTripleKill: { minIntervalMs: 500, windowMs: 1000, maxPerWindow: 2, maxConcurrent: 1 },
    announcerRampage: { minIntervalMs: 600, windowMs: 1000, maxPerWindow: 2, maxConcurrent: 1 }
});
this.mobileSoundLimits = Object.freeze({
    pistolFire: { minIntervalMs: 24, windowMs: 1000, maxPerWindow: 24, maxConcurrent: 3 },
    rifleFire: { minIntervalMs: 22, windowMs: 1000, maxPerWindow: 28, maxConcurrent: 3 },
    shotgunFire: { minIntervalMs: 40, windowMs: 1000, maxPerWindow: 14, maxConcurrent: 2 },
    bulletImpact: { minIntervalMs: 80, windowMs: 1000, maxPerWindow: 8, maxConcurrent: 2 },
    impactConcrete: { minIntervalMs: 110, windowMs: 1000, maxPerWindow: 6, maxConcurrent: 1 },
    impactMetal: { minIntervalMs: 110, windowMs: 1000, maxPerWindow: 6, maxConcurrent: 1 },
    impactWood: { minIntervalMs: 110, windowMs: 1000, maxPerWindow: 6, maxConcurrent: 1 },
    impactGlass: { minIntervalMs: 140, windowMs: 1000, maxPerWindow: 4, maxConcurrent: 1 },
    footstepConcrete: { minIntervalMs: 150, windowMs: 1000, maxPerWindow: 6, maxConcurrent: 1 },
    footstepMetal: { minIntervalMs: 150, windowMs: 1000, maxPerWindow: 6, maxConcurrent: 1 },
    footstepWood: { minIntervalMs: 150, windowMs: 1000, maxPerWindow: 6, maxConcurrent: 1 },
    footstepGlass: { minIntervalMs: 180, windowMs: 1000, maxPerWindow: 4, maxConcurrent: 1 },
    playerHit: { minIntervalMs: 88, windowMs: 1000, maxPerWindow: 8, maxConcurrent: 2 },
    explosion: { minIntervalMs: 180, windowMs: 1000, maxPerWindow: 4, maxConcurrent: 1 },
    bulletWhiz: { minIntervalMs: 180, windowMs: 1000, maxPerWindow: 4, maxConcurrent: 1 },
    dashWhoosh: { minIntervalMs: 320, windowMs: 1000, maxPerWindow: 2, maxConcurrent: 1 },
    dodgeWhoosh: { minIntervalMs: 320, windowMs: 1000, maxPerWindow: 2, maxConcurrent: 1 },
    killConfirm: { minIntervalMs: 160, windowMs: 1000, maxPerWindow: 4, maxConcurrent: 1 },
    shieldBreak: { minIntervalMs: 220, windowMs: 1000, maxPerWindow: 3, maxConcurrent: 1 },
    powerupWarning: { minIntervalMs: 280, windowMs: 1000, maxPerWindow: 3, maxConcurrent: 1 },
    heartbeatPulse: { minIntervalMs: 340, windowMs: 1000, maxPerWindow: 3, maxConcurrent: 1 },
    ambientCombat: { minIntervalMs: 420, windowMs: 1000, maxPerWindow: 2, maxConcurrent: 1 },
    wallRumble: { minIntervalMs: 420, windowMs: 1000, maxPerWindow: 2, maxConcurrent: 1 },
    zoneSlowEnter: { minIntervalMs: 900, windowMs: 2000, maxPerWindow: 1, maxConcurrent: 1 },
    zoneSlowExit: { minIntervalMs: 900, windowMs: 2000, maxPerWindow: 1, maxConcurrent: 1 },
    zoneDamageEnter: { minIntervalMs: 900, windowMs: 2000, maxPerWindow: 1, maxConcurrent: 1 },
    zoneDamageExit: { minIntervalMs: 900, windowMs: 2000, maxPerWindow: 1, maxConcurrent: 1 },
    zoneBoostEnter: { minIntervalMs: 900, windowMs: 2000, maxPerWindow: 1, maxConcurrent: 1 },
    zoneBoostExit: { minIntervalMs: 900, windowMs: 2000, maxPerWindow: 1, maxConcurrent: 1 },
    spawnChime: { minIntervalMs: 600, windowMs: 1000, maxPerWindow: 1, maxConcurrent: 1 },
    flagFanfare: { minIntervalMs: 1400, windowMs: 2000, maxPerWindow: 1, maxConcurrent: 1 },
    weaponSwap: { minIntervalMs: 140, windowMs: 1000, maxPerWindow: 4, maxConcurrent: 1 },
    countdownBeep: { minIntervalMs: 650, windowMs: 2000, maxPerWindow: 3, maxConcurrent: 1 },
    victorySting: { minIntervalMs: 2200, windowMs: 4000, maxPerWindow: 1, maxConcurrent: 1 },
    defeatSting: { minIntervalMs: 2200, windowMs: 4000, maxPerWindow: 1, maxConcurrent: 1 },
    reloadStart: { minIntervalMs: 120, windowMs: 1000, maxPerWindow: 6, maxConcurrent: 1 },
    hitMarker: { minIntervalMs: 80, windowMs: 1000, maxPerWindow: 7, maxConcurrent: 1 },
    hitMarkerHeadshot: { minIntervalMs: 220, windowMs: 1000, maxPerWindow: 3, maxConcurrent: 1 },
    announcerHeadshot: { minIntervalMs: 500, windowMs: 1000, maxPerWindow: 1, maxConcurrent: 1 },
    announcerDoubleKill: { minIntervalMs: 700, windowMs: 1000, maxPerWindow: 1, maxConcurrent: 1 },
    announcerTripleKill: { minIntervalMs: 700, windowMs: 1000, maxPerWindow: 1, maxConcurrent: 1 },
    announcerRampage: { minIntervalMs: 850, windowMs: 1000, maxPerWindow: 1, maxConcurrent: 1 }
});
this.localWeaponEchoSuppressMs = 110;
this.recentPredictedWeaponSoundAt = new Map();
this.maxVoices = this.mobileSoundBudget ? 8 : 20;
this.lowHealthWarningActive = false;
this.lowHealthHeartbeatNextAt = 0;
this.lowHealthFilterNode = null;
this.ambientCombatEnergy = 0;
this.lastAmbientEnergyAt = 0;
this.nextAmbientCombatAt = 0;
this.zoneCueKey = null;
this.lastZoneCueCheckAt = 0;
this.zoneDryGainNode = null;
this.zoneWetGainNode = null;
this.zoneConvolverNode = null;
this.zoneImpulseBuffers = new Map();
this.zoneReverbKey = null;
this.lastZoneReverbCheckAt = 0;
this.initializeOutputChain();
this.initializeVoicePool();
this.preloadSoundSamples();
    }

    setGlobalVolume(volume) {
this.globalVolume = Math.max(0, Math.min(1, volume));
    }

    setMuted(muted) {
this.soundEnabled = !muted;
    }

    initializeOutputChain() {
if (!this.audioContext) return;
try {
    const masterGain = this.audioContext.createGain();
    masterGain.gain.value = 0.84;
    const lowHealthFilter = typeof this.audioContext.createBiquadFilter === 'function'
        ? this.audioContext.createBiquadFilter()
        : null;
    if (lowHealthFilter) {
        lowHealthFilter.type = 'lowpass';
        lowHealthFilter.frequency.value = 20000;
        lowHealthFilter.Q.value = 0.5;
    }
    const routeInputNode = lowHealthFilter || masterGain;
    let outputNode = this.audioContext.destination;
    if (typeof this.audioContext.createDynamicsCompressor === 'function') {
        const compressor = this.audioContext.createDynamicsCompressor();
        compressor.threshold.value = -22;
        compressor.knee.value = 14;
        compressor.ratio.value = 9;
        compressor.attack.value = 0.003;
        compressor.release.value = 0.22;
        compressor.connect(this.audioContext.destination);
        outputNode = compressor;
        this.compressorNode = compressor;
    }
    if (lowHealthFilter) {
        masterGain.connect(lowHealthFilter);
    }

    const dryGain = this.audioContext.createGain();
    dryGain.gain.value = 1;
    routeInputNode.connect(dryGain);
    dryGain.connect(outputNode);

    let wetGain = null;
    let convolver = null;
    if (!this.mobileSoundBudget && !ultraPerformanceMode && typeof this.audioContext.createConvolver === 'function') {
        wetGain = this.audioContext.createGain();
        wetGain.gain.value = 0.0001;
        convolver = this.audioContext.createConvolver();
        convolver.normalize = true;
        routeInputNode.connect(wetGain);
        wetGain.connect(convolver);
        convolver.connect(outputNode);
        this.zoneImpulseBuffers = new Map([
            ['slow', this.createZoneImpulseResponse(2.2, 2.6, 0.65)],
            ['damage', this.createZoneImpulseResponse(1.35, 3.2, 0.32)],
            ['boost', this.createZoneImpulseResponse(0.9, 1.8, 0.85)],
        ]);
    }

    this.masterGainNode = masterGain;
    this.masterOutputNode = outputNode;
    this.lowHealthFilterNode = lowHealthFilter;
    this.zoneDryGainNode = dryGain;
    this.zoneWetGainNode = wetGain;
    this.zoneConvolverNode = convolver;
} catch (error) {
    emitClientLog('Audio output chain init failed, using direct destination', 'warn', error);
    this.masterOutputNode = this.audioContext.destination;
    this.masterGainNode = null;
    this.compressorNode = null;
    this.lowHealthFilterNode = null;
    this.zoneDryGainNode = null;
    this.zoneWetGainNode = null;
    this.zoneConvolverNode = null;
    this.zoneImpulseBuffers = new Map();
}
    }

    createZoneImpulseResponse(durationSec, decayPower, shimmerAmount = 0.5) {
if (!this.audioContext) return null;
const length = Math.max(1, Math.floor(this.audioContext.sampleRate * Math.max(0.2, durationSec)));
const buffer = this.audioContext.createBuffer(2, length, this.audioContext.sampleRate);
for (let channel = 0; channel < buffer.numberOfChannels; channel += 1) {
    const data = buffer.getChannelData(channel);
    for (let i = 0; i < length; i += 1) {
        const timeNorm = i / Math.max(1, length - 1);
        const decay = Math.pow(1 - timeNorm, Math.max(1.1, decayPower));
        const noise = (Math.random() * 2) - 1;
        const shimmer = Math.sin(i * (0.014 + shimmerAmount * 0.01) + channel * 0.7) * shimmerAmount;
        data[i] = (noise * (1 - shimmerAmount * 0.45) + shimmer * 0.35) * decay;
    }
}
return buffer;
    }

    syncEnvironmentalReverb(nowMs = 0) {
if (
    !this.audioContext ||
    !this.zoneDryGainNode ||
    !this.zoneConvolverNode ||
    !this.zoneWetGainNode ||
    this.mobileSoundBudget ||
    ultraPerformanceMode
) {
    return;
}
if ((nowMs - this.lastZoneReverbCheckAt) < 220) {
    return;
}
this.lastZoneReverbCheckAt = nowMs;

const zoneKey = getZoneReverbProfileKey(zones, localPlayerState);
if (zoneKey !== this.zoneReverbKey) {
    this.zoneReverbKey = zoneKey;
    this.zoneConvolverNode.buffer = zoneKey ? (this.zoneImpulseBuffers.get(zoneKey) || null) : null;
}

const wetTarget = zoneKey === 'damage'
    ? 0.34
    : zoneKey === 'slow'
        ? 0.24
        : zoneKey === 'boost'
            ? 0.18
            : 0.0001;
const dryTarget = zoneKey ? 0.92 : 1;
const now = this.audioContext.currentTime;
try {
    this.zoneWetGainNode.gain.setTargetAtTime(wetTarget, now, 0.18);
    this.zoneDryGainNode.gain.setTargetAtTime(dryTarget, now, 0.12);
} catch (_) {}
    }

    getZoneTransitionSoundName(zoneKey, entering) {
if (zoneKey === 'damage') return entering ? 'zoneDamageEnter' : 'zoneDamageExit';
if (zoneKey === 'boost') return entering ? 'zoneBoostEnter' : 'zoneBoostExit';
if (zoneKey === 'slow') return entering ? 'zoneSlowEnter' : 'zoneSlowExit';
return null;
    }

    syncZoneAudio(nowMs = 0) {
if (!this.audioContext || !localPlayerState) {
    this.zoneCueKey = null;
    return;
}
if ((nowMs - this.lastZoneCueCheckAt) < 140) {
    return;
}
this.lastZoneCueCheckAt = nowMs;

const nextZoneKey = getZoneReverbProfileKey(zones, localPlayerState);
if (nextZoneKey === this.zoneCueKey) {
    return;
}

const playerPos = getEntityWorldPosition(localPlayerState);
const previousZoneKey = this.zoneCueKey;
this.zoneCueKey = nextZoneKey;

if (previousZoneKey) {
    const exitSound = this.getZoneTransitionSoundName(previousZoneKey, false);
    if (exitSound) {
        this.playSound(exitSound, playerPos, 0.8, {
            prioritizeLocal: true,
            bypassLimiter: true,
        });
    }
}
if (nextZoneKey) {
    const enterSound = this.getZoneTransitionSoundName(nextZoneKey, true);
    if (enterSound) {
        this.playSound(enterSound, playerPos, 0.92, {
            prioritizeLocal: true,
            bypassLimiter: true,
        });
    }
}
    }

    updateAmbientState(nowMs = 0) {
this.syncEnvironmentalReverb(nowMs);
this.syncZoneAudio(nowMs);
    }

    decodeAudioData(arrayBuffer) {
if (!this.audioContext || !(arrayBuffer instanceof ArrayBuffer)) {
    return Promise.resolve(null);
}

const decodeInput = arrayBuffer.slice(0);
try {
    const maybePromise = this.audioContext.decodeAudioData(decodeInput);
    if (maybePromise && typeof maybePromise.then === 'function') {
        return maybePromise;
    }
} catch (_) {
    // Fall back to callback-style decodeAudioData below.
}

return new Promise((resolve, reject) => {
    try {
        this.audioContext.decodeAudioData(decodeInput, resolve, reject);
    } catch (error) {
        reject(error);
    }
});
    }

    preloadSoundSamples() {
if (!this.audioContext || typeof fetch !== 'function') return;
const criticalLoads = this.criticalCombatSamples
    .filter((soundName) => !!this.soundSamples[soundName])
    .map((soundName) => this.loadSampleBuffer(soundName).catch(() => null));
if (criticalLoads.length > 0) {
    Promise.allSettled(criticalLoads).finally(() => {
        this.criticalSamplesLoaded = true;
    });
} else {
    this.criticalSamplesLoaded = true;
}
Object.keys(this.soundSamples).forEach((soundName) => {
    this.loadSampleBuffer(soundName).catch(() => {
        // Errors are surfaced in loadSampleBuffer; keep preloading fire-and-forget.
    });
});
    }

    queuePendingSound(soundName, position = null, volumeMultiplier = 1.0, options = null) {
const queuedPosition = position && typeof position === 'object'
    ? { ...position }
    : position;
const queuedOptions = options && typeof options === 'object'
    ? { ...options }
    : options;
if (this.pendingSounds.length >= this.maxPendingSounds) {
    this.pendingSounds.shift();
}
this.pendingSounds.push({
    soundName,
    position: queuedPosition,
    volumeMultiplier,
    options: queuedOptions,
});
    }

    flushPendingSounds() {
if (
    this.flushingPendingSounds ||
    !this.audioContext ||
    this.audioContext.state !== 'running' ||
    this.pendingSounds.length === 0
) {
    return;
}
const queuedSounds = this.pendingSounds.splice(0, this.pendingSounds.length);
this.flushingPendingSounds = true;
try {
    queuedSounds.forEach((queued) => {
        this.playSound(
            queued.soundName,
            queued.position,
            queued.volumeMultiplier,
            queued.options
        );
    });
} finally {
    this.flushingPendingSounds = false;
}
    }

    requestAudioResume() {
if (!this.audioContext || this.resumeInFlight || this.audioContext.state !== 'suspended') {
    return;
}
this.resumeInFlight = true;
this.audioContext.resume()
    .then(() => {
        this.flushPendingSounds();
    })
    .catch((e) => emitClientLog("AudioContext resume failed", "warn", e))
    .finally(() => {
        this.resumeInFlight = false;
    });
    }

    loadSampleBuffer(soundName) {
const cached = this.sampleBuffers.get(soundName);
if (cached) {
    return Promise.resolve(cached);
}
if (!this.audioContext || typeof fetch !== 'function') {
    return Promise.resolve(null);
}

const samplePath = this.soundSamples[soundName];
if (!samplePath) {
    return Promise.resolve(null);
}

const inFlight = this.sampleLoadPromises.get(soundName);
if (inFlight) {
    return inFlight;
}

const loadPromise = fetch(samplePath, { cache: 'force-cache' })
    .then((response) => {
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}`);
        }
        return response.arrayBuffer();
    })
    .then((audioBytes) => this.decodeAudioData(audioBytes))
    .then((buffer) => {
        if (buffer) {
            this.sampleBuffers.set(soundName, buffer);
            this.sampleLoadFailures.delete(soundName);
            this.warnedSampleOnlyFallback.delete(soundName);
            return buffer;
        }
        return null;
    })
    .catch((error) => {
        this.sampleLoadFailures.add(soundName);
        emitClientLog(
            `Failed to load sound sample '${soundName}' (${samplePath})`,
            'warn',
            error
        );
        return null;
    })
    .finally(() => {
        this.sampleLoadPromises.delete(soundName);
    });

this.sampleLoadPromises.set(soundName, loadPromise);
return loadPromise;
    }

    tryPlaySample(soundName, volume, panValue, nowMs, prioritizeLocal = false, options = null) {
if (!this.audioContext) return false;
const sampleBuffer = this.sampleBuffers.get(soundName);
if (!sampleBuffer) return false;

const basePitchScale = Number(options?.pitchScale);
const pitchJitter = Number(options?.pitchJitter);
let playbackRate = Number.isFinite(basePitchScale) && basePitchScale > 0
    ? basePitchScale
    : 1;
if (Number.isFinite(pitchJitter) && pitchJitter > 0) {
    playbackRate *= 1 + ((Math.random() * 2 - 1) * Math.min(0.4, pitchJitter));
}
if (!Number.isFinite(playbackRate) || playbackRate <= 0.05) playbackRate = 1;
playbackRate = Math.max(0.55, Math.min(1.75, playbackRate));

const sampleDurationSec = Math.max(0.015, Number(sampleBuffer.duration) || 0.015);
const adjustedDurationSec = Math.max(0.015, sampleDurationSec / playbackRate);
const voice = this.acquireVoiceSlot(soundName, adjustedDurationSec, nowMs, prioritizeLocal);
if (!voice) {
    return false;
}

const now = this.audioContext.currentTime;
const gainNode = voice.gainNode;
let effectNode = null;
try {
    const attackSec = Math.max(0.002, Math.min(0.02, adjustedDurationSec * 0.18));
    const releaseStartOffset = Math.max(attackSec, adjustedDurationSec - 0.03);
    gainNode.gain.cancelScheduledValues(now);
    gainNode.gain.setValueAtTime(0.0001, now);
    gainNode.gain.exponentialRampToValueAtTime(Math.max(0.001, volume), now + attackSec);
    gainNode.gain.setValueAtTime(Math.max(0.001, volume), now + releaseStartOffset);
    gainNode.gain.exponentialRampToValueAtTime(0.001, now + adjustedDurationSec);
} catch (_) {
    this.markVoiceReleased(voice);
    return false;
}

if (voice.pannerNode) {
    try {
        voice.pannerNode.pan.cancelScheduledValues(now);
        voice.pannerNode.pan.setValueAtTime(Math.max(-1, Math.min(1, Number(panValue) || 0)), now);
    } catch (_) {}
}

const sourceNode = this.audioContext.createBufferSource();
sourceNode.buffer = sampleBuffer;
try {
    sourceNode.playbackRate.setValueAtTime(playbackRate, now);
} catch (_) {}
const lowpassHz = Number(options?.lowpassHz);
if (
    Number.isFinite(lowpassHz) &&
    lowpassHz > 40 &&
    lowpassHz < 18000 &&
    typeof this.audioContext.createBiquadFilter === 'function'
) {
    effectNode = this.audioContext.createBiquadFilter();
    effectNode.type = 'lowpass';
    effectNode.frequency.value = lowpassHz;
    effectNode.Q.value = 0.35;
    sourceNode.connect(effectNode);
    effectNode.connect(gainNode);
} else {
    sourceNode.connect(gainNode);
}
voice.sourceNode = sourceNode;
sourceNode.onended = () => {
    if (effectNode) {
        try {
            effectNode.disconnect();
        } catch (_) {}
    }
    this.markVoiceReleased(voice, sourceNode);
};

try {
    sourceNode.start(now);
    sourceNode.stop(now + adjustedDurationSec + 0.02);
    return true;
} catch (_) {
    if (effectNode) {
        try {
            effectNode.disconnect();
        } catch (_) {}
    }
    this.markVoiceReleased(voice, sourceNode);
    return false;
}
    }

    playWeaponSound(weaponType, position, isLocalPlayer, options = null) {
let soundName;
switch (weaponType) {
    case GP.WeaponType.Pistol: soundName = 'pistolFire'; break;
    case GP.WeaponType.Shotgun: soundName = 'shotgunFire'; break;
    case GP.WeaponType.Rifle: soundName = 'rifleFire'; break;
    case GP.WeaponType.Sniper: soundName = 'sniperFire'; break;
    case GP.WeaponType.Melee: soundName = 'meleeSwing'; break;
    default: return;
}
const nowMs = (typeof performance !== 'undefined' && typeof performance.now === 'function')
    ? performance.now()
    : Date.now();
const predicted = !!(options && options.predicted);
if (isLocalPlayer && predicted) {
    this.recentPredictedWeaponSoundAt.set(soundName, nowMs);
} else if (isLocalPlayer) {
    const lastPredictedAt = this.recentPredictedWeaponSoundAt.get(soundName) || -1e9;
    if ((nowMs - lastPredictedAt) < this.localWeaponEchoSuppressMs) {
        return;
    }
}
const baseVolumeScale = options && Number.isFinite(options.volumeScale)
    ? options.volumeScale
    : (isLocalPlayer ? 1.0 : 0.7);
let adjustedVolumeScale = baseVolumeScale;
if (!isLocalPlayer) {
    const activePlayers = Number(players?.size) || 0;
    if (activePlayers > 10) {
        const densePenalty = Math.min(0.45, (activePlayers - 10) * 0.012);
        adjustedVolumeScale *= (1 - densePenalty);
    }
    if (smoothedFrameMs > 22) {
        adjustedVolumeScale *= 0.82;
    }
    if (ultraPerformanceMode) {
        adjustedVolumeScale *= 0.74;
    }
    if (adjustedVolumeScale < 0.09) {
        return;
    }
}
const pitchJitter = (weaponType === GP.WeaponType.Shotgun || weaponType === GP.WeaponType.Sniper) ? 0.05 : 0.08;
const volumeJitter = 0.1;
const randomizedScale = adjustedVolumeScale * (1 + ((Math.random() * 2 - 1) * volumeJitter));
const finalScale = Math.max(0.04, randomizedScale);
this.playSound(soundName, position, finalScale, {
    prioritizeLocal: !!isLocalPlayer,
    bypassLimiter: !!(options && options.bypassLimiter),
    pitchJitter,
});
if (weaponType === GP.WeaponType.Rifle || weaponType === GP.WeaponType.Sniper) {
    const clickVolume = Math.max(0.05, finalScale * (weaponType === GP.WeaponType.Sniper ? 0.16 : 0.12));
    const clickDelayMs = 20;
    setTimeout(() => {
        this.playSound('weaponSwap', position, clickVolume, {
            bypassLimiter: true,
            prioritizeLocal: !!isLocalPlayer,
            pitchScale: 0.74 + Math.random() * 0.12,
        });
    }, clickDelayMs);
}
    }

    registerCombatEventIntensity(weight = 0.18) {
const nowMs = (typeof performance !== 'undefined' && typeof performance.now === 'function')
    ? performance.now()
    : Date.now();
const elapsedMs = Math.max(0, nowMs - (this.lastAmbientEnergyAt || nowMs));
this.lastAmbientEnergyAt = nowMs;
if (elapsedMs > 0) {
    this.ambientCombatEnergy = Math.max(0, this.ambientCombatEnergy - elapsedMs * 0.00045);
}
this.ambientCombatEnergy = Math.min(1.8, this.ambientCombatEnergy + Math.max(0, Number(weight) || 0));
if (!this.soundEnabled || !this.audioContext) return;
if (this.ambientCombatEnergy < 0.28) return;
if (nowMs < this.nextAmbientCombatAt) return;
const energyNorm = Math.max(0, Math.min(1, this.ambientCombatEnergy / 1.5));
const intervalMs = 1400 - energyNorm * 820;
const volume = 0.08 + energyNorm * 0.12;
this.nextAmbientCombatAt = nowMs + intervalMs;
this.playSound('ambientCombat', null, volume, {
    bypassLimiter: true,
    skipAmbientEnergy: true,
    pitchScale: 0.84 + Math.random() * 0.28,
});
    }

    updateLowHealthWarning(healthRatio) {
if (!this.audioContext || !this.soundEnabled) return;
const ratio = Math.max(0, Math.min(1, Number(healthRatio) || 0));
const nowMs = (typeof performance !== 'undefined' && typeof performance.now === 'function')
    ? performance.now()
    : Date.now();

if (ratio <= 0.3) {
    const urgency = Math.max(0, Math.min(1, (0.3 - ratio) / 0.2));
    const bpm = 60 + urgency * 60;
    const intervalMs = 60000 / Math.max(30, bpm);
    if (nowMs >= this.lowHealthHeartbeatNextAt) {
        const pitchScale = 0.72 + urgency * 0.55;
        const volume = 0.12 + urgency * 0.18;
        this.playSound('heartbeatPulse', null, volume, {
            bypassLimiter: true,
            pitchScale,
        });
        this.lowHealthHeartbeatNextAt = nowMs + intervalMs;
    }
    this.lowHealthWarningActive = true;
} else if (this.lowHealthWarningActive) {
    this.lowHealthWarningActive = false;
}

if (this.lowHealthFilterNode) {
    const targetCutoffHz = ratio < 0.15
        ? 900 + (ratio / 0.15) * 2800
        : 20000;
    try {
        this.lowHealthFilterNode.frequency.setTargetAtTime(
            targetCutoffHz,
            this.audioContext.currentTime,
            0.08
        );
    } catch (_) {}
}
    }

    getSoundLimiterConfig(soundName) {
const mobileConfig = this.mobileSoundBudget ? this.mobileSoundLimits[soundName] : null;
const rawConfig = mobileConfig || this.soundLimits[soundName] || this.defaultSoundLimit;
const scale = this.soundBudgetScale;
return {
    minIntervalMs: Math.max(0, Math.round((Number(rawConfig.minIntervalMs) || 0) * scale)),
    windowMs: Math.max(250, Math.round(Number(rawConfig.windowMs) || 1000)),
    maxPerWindow: Math.max(1, Math.floor((Number(rawConfig.maxPerWindow) || 1) / scale)),
    maxConcurrent: Math.max(1, Math.floor((Number(rawConfig.maxConcurrent) || 1) / scale))
};
    }

    getSoundActivityState(soundName, nowMs = 0) {
let state = this.soundActivity.get(soundName);
if (!state) {
    state = {
        lastPlayAtMs: -1e9,
        windowStartMs: nowMs,
        windowCount: 0,
        activeCount: 0
    };
    this.soundActivity.set(soundName, state);
}
return state;
    }

    shouldPlaySoundNow(soundName, nowMs) {
const limit = this.getSoundLimiterConfig(soundName);
const state = this.getSoundActivityState(soundName, nowMs);
if (state.activeCount >= limit.maxConcurrent) {
    return false;
}
if ((nowMs - state.lastPlayAtMs) < limit.minIntervalMs) {
    return false;
}
if ((nowMs - state.windowStartMs) >= limit.windowMs) {
    state.windowStartMs = nowMs;
    state.windowCount = 0;
}
if (state.windowCount >= limit.maxPerWindow) {
    return false;
}
state.lastPlayAtMs = nowMs;
state.windowCount += 1;
return true;
    }

    initializeVoicePool() {
if (!this.audioContext) return;
for (let i = 0; i < this.maxVoices; i += 1) {
    try {
        const gainNode = this.audioContext.createGain();
        gainNode.gain.value = 0.0001;
        const outputNode = this.masterGainNode || this.masterOutputNode || this.audioContext.destination;
        let pannerNode = null;
        if (typeof this.audioContext.createStereoPanner === 'function') {
            pannerNode = this.audioContext.createStereoPanner();
            pannerNode.pan.value = 0;
            gainNode.connect(pannerNode);
            pannerNode.connect(outputNode);
        } else {
            gainNode.connect(outputNode);
        }
        this.voicePool.push({
            gainNode,
            pannerNode,
            sourceNode: null,
            soundName: '',
            busyUntilMs: 0
        });
    } catch (error) {
        emitClientLog('Audio voice pool init failed', 'warn', error);
        this.voicePool = [];
        break;
    }
}
    }

    sweepExpiredVoices(nowMs) {
if (!this.voicePool.length) return;
for (let i = 0; i < this.voicePool.length; i += 1) {
    const voice = this.voicePool[i];
    if (!voice?.sourceNode) continue;
    if (nowMs >= (voice.busyUntilMs + 2)) {
        this.markVoiceReleased(voice, voice.sourceNode);
    }
}
    }

    acquireVoiceSlot(soundName, durationSec, nowMs, allowSteal = false) {
if (!this.voicePool.length) return null;
const voiceCount = this.voicePool.length;
for (let offset = 0; offset < voiceCount; offset += 1) {
    const index = (this.voiceCursor + offset) % voiceCount;
    const voice = this.voicePool[index];
    if (voice.sourceNode) {
        continue;
    }
    this.voiceCursor = (index + 1) % voiceCount;
    voice.soundName = soundName;
    voice.busyUntilMs = nowMs + Math.ceil(durationSec * 1000) + 48;
    const state = this.getSoundActivityState(soundName, nowMs);
    state.activeCount += 1;
    return voice;
}
if (!allowSteal) return null;

let bestVoice = null;
for (let i = 0; i < voiceCount; i += 1) {
    const voice = this.voicePool[i];
    if (!voice) continue;
    if (!bestVoice || voice.busyUntilMs < bestVoice.busyUntilMs) {
        bestVoice = voice;
    }
}
if (!bestVoice) return null;

this.markVoiceReleased(bestVoice);
bestVoice.soundName = soundName;
bestVoice.busyUntilMs = nowMs + Math.ceil(durationSec * 1000) + 48;
const state = this.getSoundActivityState(soundName, nowMs);
state.activeCount += 1;
return bestVoice;
    }

    markVoiceReleased(voice, sourceNode = null) {
if (!voice) return;
if (sourceNode && voice.sourceNode && voice.sourceNode !== sourceNode) {
    return;
}

const activeSoundName = voice.soundName;
const currentSource = voice.sourceNode;
voice.sourceNode = null;
voice.soundName = '';
voice.busyUntilMs = 0;

if (currentSource) {
    try {
        currentSource.onended = null;
    } catch (_) {}
}

if (activeSoundName) {
    const state = this.soundActivity.get(activeSoundName);
    if (state) {
        state.activeCount = Math.max(0, state.activeCount - 1);
    }
}
    }

    getNoiseBuffer(durationSec) {
if (!this.audioContext) return null;
const durationMs = Math.max(20, Math.round(durationSec * 1000));
const cacheKey = `${this.audioContext.sampleRate}:${durationMs}`;
const cached = this.noiseBufferCache.get(cacheKey);
if (cached) {
    return cached;
}

const frameCount = Math.max(1, Math.floor(this.audioContext.sampleRate * (durationMs / 1000)));
const buffer = this.audioContext.createBuffer(1, frameCount, this.audioContext.sampleRate);
const output = buffer.getChannelData(0);
for (let i = 0; i < frameCount; i += 1) {
    output[i] = (Math.random() * 2) - 1;
}
this.noiseBufferCache.set(cacheKey, buffer);
if (this.noiseBufferCache.size > 24) {
    const oldestKey = this.noiseBufferCache.keys().next().value;
    if (typeof oldestKey !== 'undefined') {
        this.noiseBufferCache.delete(oldestKey);
    }
}
return buffer;
    }

    playSound(soundName, position = null, volumeMultiplier = 1.0, options = null) {
if (!this.soundEnabled || !this.audioContext || !this.sounds[soundName]) return;
if (this.audioContext.state === 'running' && this.pendingSounds.length > 0 && !this.flushingPendingSounds) {
    this.flushPendingSounds();
}
if (this.audioContext.state === 'suspended') {
    this.queuePendingSound(soundName, position, volumeMultiplier, options);
    this.requestAudioResume();
    return;
}

const soundProfile = this.sounds[soundName];
const durationSec = Number(soundProfile.duration);
if (!Number.isFinite(durationSec) || durationSec <= 0) {
    emitClientLog(`Invalid duration for sound: ${soundName}`, 'warn', soundProfile.duration);
    return;
}

const baseVolume = (soundProfile.vol !== undefined ? soundProfile.vol : 0.5) * this.globalVolume * volumeMultiplier;
if (baseVolume <= 0.001) return;

let finalVolume = baseVolume;
let panValue = 0;
let lowpassHz = null;
if (position && localPlayerState && app && gameScene) {
    const listenerWorldPos = getEntityWorldPosition(localPlayerState);
    const soundWorldPosData = getEntityWorldPosition(position) || {
        x: Number(position?.x),
        y: Number(position?.y),
    };
    const viewCenter = { x: app.screen.width / 2, y: app.screen.height / 2 };
    const soundWorldPos = gameScene.toGlobal(position);
    const dx = soundWorldPos.x - viewCenter.x;
    const dy = soundWorldPos.y - viewCenter.y;
    const worldDx = listenerWorldPos && Number.isFinite(soundWorldPosData.x)
        ? soundWorldPosData.x - listenerWorldPos.x
        : 0;
    const worldDy = listenerWorldPos && Number.isFinite(soundWorldPosData.y)
        ? soundWorldPosData.y - listenerWorldPos.y
        : 0;
    const worldDistance = Math.sqrt(worldDx * worldDx + worldDy * worldDy);
    const maxAudibleDistance = 900;

    if (worldDistance > maxAudibleDistance) return;
    const attenuation = 1 / (1 + Math.pow(worldDistance / 400, 2));
    finalVolume *= attenuation;
    if (finalVolume <= 0.001) return;
    panValue = Math.max(-1, Math.min(1, dx / (app.screen.width / 2)));
    if (worldDistance > 500) {
        const farRatio = Math.max(0, Math.min(1, (worldDistance - 500) / 400));
        lowpassHz = 8200 - farRatio * 6600;
    }
    if (
        !ultraPerformanceMode &&
        !this.mobileSoundBudget &&
        worldDistance > 100 &&
        listenerWorldPos &&
        Number.isFinite(soundWorldPosData.x) &&
        Number.isFinite(soundWorldPosData.y)
    ) {
        const wallHits = countWallIntersections(listenerWorldPos, soundWorldPosData, walls, 3);
        if (wallHits > 0) {
            finalVolume *= Math.max(0.16, 1 - wallHits * 0.3);
            const occludedCutoffHz = Math.max(900, 5200 - wallHits * 1200);
            lowpassHz = Number.isFinite(lowpassHz)
                ? Math.min(lowpassHz, occludedCutoffHz)
                : occludedCutoffHz;
        }
    }
}

const nowMs = (typeof performance !== 'undefined' && typeof performance.now === 'function')
    ? performance.now()
    : Date.now();
this.updateAmbientState(nowMs);
const prioritizeLocal = !!(options && options.prioritizeLocal);
const bypassLimiter = !!(options && options.bypassLimiter);
this.sweepExpiredVoices(nowMs);
if (!bypassLimiter && !this.shouldPlaySoundNow(soundName, nowMs)) {
    return;
}
if (this.tryPlaySample(soundName, finalVolume, panValue, nowMs, prioritizeLocal, {
    pitchScale: options?.pitchScale,
    pitchJitter: options?.pitchJitter,
    lowpassHz: Number.isFinite(Number(options?.lowpassHz)) ? Number(options.lowpassHz) : lowpassHz,
})) {
    return;
}
const hasSampleMapping = !!this.soundSamples[soundName];
if (hasSampleMapping) {
    this.loadSampleBuffer(soundName).catch(() => {
        // loadSampleBuffer already logs detailed failures.
    });
    const allowToneFallback = !this.criticalSamplesLoaded || this.sampleLoadFailures.has(soundName);
    if (this.sampleLoadFailures.has(soundName) && !this.warnedSampleOnlyFallback.has(soundName)) {
        this.warnedSampleOnlyFallback.add(soundName);
        emitClientLog(
            `Falling back to synthesized '${soundName}' because the sample is unavailable.`,
            'warn'
        );
    }
    if (!allowToneFallback) {
        return;
    }
}
this._playTone(
    soundName,
    soundProfile,
    finalVolume,
    panValue,
    durationSec,
    nowMs,
    prioritizeLocal,
    {
        pitchScale: options?.pitchScale,
        pitchJitter: options?.pitchJitter,
        lowpassHz: Number.isFinite(Number(options?.lowpassHz)) ? Number(options.lowpassHz) : lowpassHz,
    }
);
    }

    _playTone(soundName, profile, volume, panValue, durationSec, nowMs, prioritizeLocal = false, options = null) {
if (!this.audioContext) return;
const basePitchScale = Number(options?.pitchScale);
const pitchJitter = Number(options?.pitchJitter);
let pitchScale = Number.isFinite(basePitchScale) && basePitchScale > 0
    ? basePitchScale
    : 1;
if (Number.isFinite(pitchJitter) && pitchJitter > 0) {
    pitchScale *= 1 + ((Math.random() * 2 - 1) * Math.min(0.35, pitchJitter));
}
if (!Number.isFinite(pitchScale) || pitchScale <= 0.05) pitchScale = 1;
pitchScale = Math.max(0.55, Math.min(1.7, pitchScale));
const adjustedDurationSec = Math.max(0.015, durationSec / pitchScale);
const voice = this.acquireVoiceSlot(soundName, adjustedDurationSec, nowMs, prioritizeLocal);
if (!voice) {
    return;
}

const now = this.audioContext.currentTime;
const gainNode = voice.gainNode;
try {
    gainNode.gain.cancelScheduledValues(now);
    const attackSec = Math.max(0.002, Math.min(0.01, adjustedDurationSec * 0.22));
    gainNode.gain.setValueAtTime(0.0001, now);
    gainNode.gain.exponentialRampToValueAtTime(Math.max(0.001, volume), now + attackSec);
    gainNode.gain.exponentialRampToValueAtTime(0.001, now + adjustedDurationSec);
} catch (_) {
    this.markVoiceReleased(voice);
    return;
}

if (voice.pannerNode) {
    try {
        voice.pannerNode.pan.cancelScheduledValues(now);
        voice.pannerNode.pan.setValueAtTime(Math.max(-1, Math.min(1, Number(panValue) || 0)), now);
    } catch (_) {}
}

let sourceNode = null;
const lowpassHz = Number(options?.lowpassHz);
let filterNode = null;
if (profile.type === 'noise') {
    const buffer = this.getNoiseBuffer(adjustedDurationSec);
    if (!buffer) {
        this.markVoiceReleased(voice);
        return;
    }
    sourceNode = this.audioContext.createBufferSource();
    sourceNode.buffer = buffer;
    try {
        sourceNode.playbackRate.setValueAtTime(pitchScale, now);
    } catch (_) {}
} else {
    const oscillator = this.audioContext.createOscillator();
    oscillator.type = profile.type || 'sine';
    if (Array.isArray(profile.freq)) {
        if (!Number.isFinite(profile.freq[0])) {
            emitClientLog("Invalid profile.freq[0]", "warn", profile);
            this.markVoiceReleased(voice);
            return;
        }
        oscillator.frequency.setValueAtTime(profile.freq[0] * pitchScale, now);
        if (profile.freq.length > 1 && Number.isFinite(profile.freq[1])) {
            oscillator.frequency.linearRampToValueAtTime(profile.freq[1] * pitchScale, now + adjustedDurationSec * 0.8);
        }
        if (profile.freq.length > 2 && Number.isFinite(profile.freq[2])) {
            oscillator.frequency.linearRampToValueAtTime(profile.freq[2] * pitchScale, now + adjustedDurationSec);
        }
    } else if (Number.isFinite(profile.freq)) {
        oscillator.frequency.setValueAtTime(profile.freq * pitchScale, now);
    } else {
        emitClientLog("Invalid profile.freq", "warn", profile);
        this.markVoiceReleased(voice);
        return;
    }
    sourceNode = oscillator;
}

if (
    Number.isFinite(lowpassHz) &&
    lowpassHz > 40 &&
    lowpassHz < 18000 &&
    typeof this.audioContext.createBiquadFilter === 'function'
) {
    filterNode = this.audioContext.createBiquadFilter();
    filterNode.type = 'lowpass';
    filterNode.frequency.value = lowpassHz;
    filterNode.Q.value = 0.35;
    sourceNode.connect(filterNode);
    filterNode.connect(gainNode);
} else {
    sourceNode.connect(gainNode);
}
voice.sourceNode = sourceNode;
sourceNode.onended = () => {
    if (filterNode) {
        try {
            filterNode.disconnect();
        } catch (_) {}
    }
    this.markVoiceReleased(voice, sourceNode);
};

try {
    sourceNode.start(now);
    sourceNode.stop(now + adjustedDurationSec);
} catch (_) {
    if (filterNode) {
        try {
            filterNode.disconnect();
        } catch (_) {}
    }
    this.markVoiceReleased(voice, sourceNode);
}
    }
}

  const wrapRuntimeAwareMethods = (ClassCtor) => {
    const proto = ClassCtor?.prototype;
    if (!proto) return;
    for (const key of Object.getOwnPropertyNames(proto)) {
      if (key === 'constructor') continue;
      const descriptor = Object.getOwnPropertyDescriptor(proto, key);
      if (!descriptor || typeof descriptor.value !== 'function') continue;
      const original = descriptor.value;
      Object.defineProperty(proto, key, {
        ...descriptor,
        value: function wrappedRuntimeMethod(...args) {
          refreshRuntimeRefs();
          return original.apply(this, args);
        },
      });
    }
  };

  wrapRuntimeAwareMethods(EffectsManager);
  wrapRuntimeAwareMethods(AudioManager);

  return {
    EffectsManager,
    AudioManager,
  };
}
