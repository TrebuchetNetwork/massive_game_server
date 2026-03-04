/**
 * Effects + audio runtime extracted from client.html.
 *
 * Keeps manager implementations unchanged while refreshing live runtime refs
 * before each method call so dynamic state (app/player/maps) stays current.
 */

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
  getLocalPlayerState = () => null,
  getMyPlayerId = () => null,
  getGameSettings = () => ({}),
  getApp = () => null,
  getGameScene = () => null,
  getUltraPerformanceMode = () => false,
  getSmoothedFrameMs = () => 16,
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
  let localPlayerState = getLocalPlayerState() || null;
  let myPlayerId = getMyPlayerId() || null;
  let gameSettings = getGameSettings() || {};
  let app = getApp() || null;
  let gameScene = getGameScene() || null;
  let ultraPerformanceMode = !!getUltraPerformanceMode();
  let smoothedFrameMs = Number(getSmoothedFrameMs()) || 16;

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
    localPlayerState = safeRefresh(getLocalPlayerState, localPlayerState) || null;
    myPlayerId = safeRefresh(getMyPlayerId, myPlayerId) || null;
    gameSettings = safeRefresh(getGameSettings, gameSettings) || {};
    app = safeRefresh(getApp, app) || null;
    gameScene = safeRefresh(getGameScene, gameScene) || null;
    ultraPerformanceMode = !!safeRefresh(getUltraPerformanceMode, ultraPerformanceMode);
    smoothedFrameMs = Number(safeRefresh(getSmoothedFrameMs, smoothedFrameMs)) || 16;
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
        console.warn('Bitmap font generation failed, falling back to PIXI.Text:', error);
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
    textures.smoke = fallbackTexture;
    textures.debris = fallbackTexture;
    return textures;
}

// Spark particle
const sparkGraphics = new PIXI.Graphics();
sparkGraphics.beginFill(0xFFFFFF);
sparkGraphics.drawCircle(0, 0, 2);
sparkGraphics.endFill();
textures.spark = renderer.generateTexture(sparkGraphics);

// Smoke particle
const smokeGraphics = new PIXI.Graphics();
smokeGraphics.beginFill(0x888888, 0.5);
smokeGraphics.drawCircle(0, 0, 8);
smokeGraphics.endFill();
smokeGraphics.filters = [getSharedBlurFilter(3)];
textures.smoke = renderer.generateTexture(smokeGraphics);

// Debris particle
const debrisGraphics = new PIXI.Graphics();
debrisGraphics.beginFill(0x444444);
debrisGraphics.drawRect(-3, -3, 6, 6);
debrisGraphics.endFill();
textures.debris = renderer.generateTexture(debrisGraphics);

// Clean up
sparkGraphics.destroy();
smokeGraphics.destroy();
debrisGraphics.destroy();

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

    processGameEvent(event) {
if (!this.particlesEnabled && (event.event_type !== GP.GameEventType.PlayerDamageEffect)) return;
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
switch (event.event_type) {
    case GP.GameEventType.BulletImpact:
        if (!this.shouldEmitEffect('impact')) break;
        this.createEnhancedBulletImpact(pos, event.weapon_type);
        if (this.audioManager) {
            this.audioManager.playSound('bulletImpact', pos, 0.5);
        }
        break;
    case GP.GameEventType.WallImpact:
        if (!this.shouldEmitEffect('impact')) break;
        this.createEnhancedBulletImpact(pos, event.weapon_type);
        if (this.audioManager) {
            this.audioManager.playSound('bulletImpact', pos, 0.5);
        }
        break;
    case GP.GameEventType.Explosion:
        if (!this.shouldEmitEffect('explosion')) break;
        this.createEnhancedExplosion(pos, event.value);
        if (this.audioManager) {
            this.audioManager.playSound('explosion', pos);
        }
        break;
    case GP.GameEventType.WeaponFire:
        if (event.weapon_type === GP.WeaponType.Melee) {
            this.createMeleeSwingEffect(pos, event.instigator_id);
        } else {
            if (!this.shouldEmitEffect('muzzle')) break;
            this.createEnhancedMuzzleFlash(pos, event.weapon_type, event.instigator_id);
        }
        if (this.audioManager) {
            this.audioManager.playWeaponSound(event.weapon_type, pos, event.instigator_id === myPlayerId);
        }
        break;
    case GP.GameEventType.PlayerDamageEffect:
        // Determine damage type based on event context
        let damageType = 'enemy'; // default
        const targetIdString = event.target_id != null ? String(event.target_id) : '';
        const localIdString = myPlayerId != null ? String(myPlayerId) : '';
        if (event.instigator_id && event.target_id) {
            const instigator = players.get(event.instigator_id);
            const target = players.get(event.target_id);
            if (instigator && target && localPlayerState) {
                if (target.id === myPlayerId) {
                    // Damage received by local player
                    damageType = (instigator.team_id === target.team_id && instigator.team_id !== 0) ? 'friendlyFireReceived' : 'enemyReceived';
                } else if (instigator.id === myPlayerId) {
                    // Damage dealt by local player
                    damageType = (instigator.team_id === target.team_id && instigator.team_id !== 0) ? 'friendlyFireDealt' : 'enemyDealt';
                } else if (localPlayerState.team_id !== 0 && instigator.team_id === localPlayerState.team_id && target.team_id !== localPlayerState.team_id) {
                    // Teammate damaging enemy - also green (beneficial for our team)
                    damageType = 'enemyDealt';
                } else if (instigator.team_id === target.team_id && instigator.team_id !== 0 && instigator.team_id !== localPlayerState.team_id) {
                    // Enemy team friendly fire (their team damaging their own teammates)
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
            // Local damage should feel punchy but remain bounded for readability.
            const incomingDamage = Number(event.value);
            const effectiveDamage =
                Number.isFinite(incomingDamage) && incomingDamage > 0
                    ? incomingDamage
                    : 35;
            if (effectiveDamage > 10) {
                const shakeIntensity = Math.min(140, 18 + effectiveDamage * 2.2);
                const shakeFrames = Math.min(9, 2 + Math.round(effectiveDamage / 14));
                applyScreenShake(gameScene, shakeIntensity, shakeFrames);
            }
        }
        if (this.audioManager) {
            this.audioManager.playSound('playerHit', pos);
        }
        break;
    case GP.GameEventType.WallDestroyed:
        if (!this.shouldEmitEffect('explosion')) break;
        this.createEnhancedWallDestructionEffect(pos);
        if (this.audioManager) {
            this.audioManager.playSound('explosion', pos, 0.7);
        }
        break;
    case GP.GameEventType.PowerupActivated:
        if (!this.shouldEmitEffect('powerup')) break;
        this.createEnhancedPowerupCollectEffect(pos);
        if (this.audioManager) {
            this.audioManager.playSound('powerupCollect', pos);
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
dustCloud.filters = [getSharedBlurFilter(8)];
this.effectsContainer.addChild(dustCloud);

// Debris pieces
const debrisCount = this.scaleEffectCount(15, 5);
for (let i = 0; i < debrisCount; i++) {
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
const dustCount = this.scaleEffectCount(10, 3);
for (let i = 0; i < dustCount; i++) {
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

    update(deltaMS) {
if (this.activeEffects.length > this.maxActiveEffects) {
    this.dropOverflowEffects(0);
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

// Animate charge effect
this.animateEffect(chargeEffect, {
    duration: 150,
    onUpdate: (progress) => {
        chargeEffect.scale.set(1 + progress * 0.5);
        chargeEffect.alpha = 0.6 * (1 - progress);
        chargeEffect.rotation = progress * Math.PI / 4;
    },
    onComplete: () => chargeEffect.destroy()
});

// Motion blur container
const blurContainer = new PIXI.Container();
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
    
    container.addChild(trailContainer);
    
    // Animate trail with spiral motion
    this.animateEffect(trailContainer, {
        duration: 400,
        delay: i * 10,
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
container.addChild(cuttingEdge);

// Animate cutting edge
this.animateEffect(cuttingEdge, {
    duration: 200,
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
        delay: i * 15,
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
    this.scheduleCallback(w * 50, () => {
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
    
    container.addChild(sparkContainer);
    
    const velocity = {
        x: Math.cos(sparkAngle) * (6 + Math.random() * 8),
        y: Math.sin(sparkAngle) * (6 + Math.random() * 8) - 3
    };
    
    this.animateEffect(sparkContainer, {
        duration: 600 + Math.random() * 300,
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
    if (gameSettings.screenShake) {
        applyScreenShake(gameScene, 200, 8);
    }
    // Multiple screen flashes
    createScreenFlash(app, 0xFFFFFF, 8, 0.5);
    this.scheduleCallback(50, () => createScreenFlash(app, 0x88DDFF, 15, 0.3));
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
            console.warn("Web Audio API not supported.");
        }

this.sounds = {
    pistolFire: { freq: [800, 600], duration: 0.05, type: 'triangle', vol: 0.3 },
    shotgunFire: { freq: [400, 200], duration: 0.15, type: 'sawtooth', vol: 0.5 },
    rifleFire: { freq: [700, 500], duration: 0.07, type: 'square', vol: 0.35 },
    sniperFire: { freq: [1000, 300], duration: 0.2, type: 'sine', vol: 0.6 },
    meleeSwing: { freq: [300, 500], duration: 0.1, type: 'sine', vol: 0.2 },
    bulletImpact: { freq: [200, 100], duration: 0.08, type: 'noise', vol: 0.25 },
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
    playerHit: { minIntervalMs: 52, windowMs: 1000, maxPerWindow: 16, maxConcurrent: 3 },
    explosion: { minIntervalMs: 120, windowMs: 1000, maxPerWindow: 8, maxConcurrent: 2 },
    hitMarker: { minIntervalMs: 55, windowMs: 1000, maxPerWindow: 14, maxConcurrent: 2 },
    hitMarkerHeadshot: { minIntervalMs: 180, windowMs: 1000, maxPerWindow: 5, maxConcurrent: 1 },
    bulletWhiz: { minIntervalMs: 130, windowMs: 1000, maxPerWindow: 8, maxConcurrent: 2 },
    dashWhoosh: { minIntervalMs: 220, windowMs: 1000, maxPerWindow: 4, maxConcurrent: 1 },
    dodgeWhoosh: { minIntervalMs: 220, windowMs: 1000, maxPerWindow: 4, maxConcurrent: 1 },
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
    playerHit: { minIntervalMs: 88, windowMs: 1000, maxPerWindow: 8, maxConcurrent: 2 },
    explosion: { minIntervalMs: 180, windowMs: 1000, maxPerWindow: 4, maxConcurrent: 1 },
    bulletWhiz: { minIntervalMs: 180, windowMs: 1000, maxPerWindow: 4, maxConcurrent: 1 },
    dashWhoosh: { minIntervalMs: 320, windowMs: 1000, maxPerWindow: 2, maxConcurrent: 1 },
    dodgeWhoosh: { minIntervalMs: 320, windowMs: 1000, maxPerWindow: 2, maxConcurrent: 1 },
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
    let outputNode = masterGain;
    if (typeof this.audioContext.createDynamicsCompressor === 'function') {
        const compressor = this.audioContext.createDynamicsCompressor();
        compressor.threshold.value = -22;
        compressor.knee.value = 14;
        compressor.ratio.value = 9;
        compressor.attack.value = 0.003;
        compressor.release.value = 0.22;
        masterGain.connect(compressor);
        compressor.connect(this.audioContext.destination);
        outputNode = masterGain;
        this.compressorNode = compressor;
    } else {
        masterGain.connect(this.audioContext.destination);
    }
    this.masterGainNode = masterGain;
    this.masterOutputNode = outputNode;
} catch (error) {
    console.warn('Audio output chain init failed, using direct destination:', error);
    this.masterOutputNode = this.audioContext.destination;
    this.masterGainNode = null;
    this.compressorNode = null;
}
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
Object.keys(this.soundSamples).forEach((soundName) => {
    this.loadSampleBuffer(soundName).catch(() => {
        // Errors are surfaced in loadSampleBuffer; keep preloading fire-and-forget.
    });
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
            return buffer;
        }
        return null;
    })
    .catch((error) => {
        console.warn(`Failed to load sound sample '${soundName}' (${samplePath}):`, error);
        return null;
    })
    .finally(() => {
        this.sampleLoadPromises.delete(soundName);
    });

this.sampleLoadPromises.set(soundName, loadPromise);
return loadPromise;
    }

    tryPlaySample(soundName, volume, panValue, nowMs, prioritizeLocal = false) {
if (!this.audioContext) return false;
const sampleBuffer = this.sampleBuffers.get(soundName);
if (!sampleBuffer) return false;

const sampleDurationSec = Math.max(0.015, Number(sampleBuffer.duration) || 0.015);
const voice = this.acquireVoiceSlot(soundName, sampleDurationSec, nowMs, prioritizeLocal);
if (!voice) {
    return false;
}

const now = this.audioContext.currentTime;
const gainNode = voice.gainNode;
try {
    const attackSec = Math.max(0.002, Math.min(0.02, sampleDurationSec * 0.18));
    const releaseStartOffset = Math.max(attackSec, sampleDurationSec - 0.03);
    gainNode.gain.cancelScheduledValues(now);
    gainNode.gain.setValueAtTime(0.0001, now);
    gainNode.gain.exponentialRampToValueAtTime(Math.max(0.001, volume), now + attackSec);
    gainNode.gain.setValueAtTime(Math.max(0.001, volume), now + releaseStartOffset);
    gainNode.gain.exponentialRampToValueAtTime(0.001, now + sampleDurationSec);
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
sourceNode.connect(gainNode);
voice.sourceNode = sourceNode;
sourceNode.onended = () => {
    this.markVoiceReleased(voice, sourceNode);
};

try {
    sourceNode.start(now);
    sourceNode.stop(now + sampleDurationSec + 0.02);
    return true;
} catch (_) {
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
this.playSound(soundName, position, adjustedVolumeScale, {
    prioritizeLocal: !!isLocalPlayer,
    bypassLimiter: !!(options && options.bypassLimiter),
});
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
        const outputNode = this.masterOutputNode || this.audioContext.destination;
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
        console.warn('Audio voice pool init failed:', error);
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
if (this.audioContext.state === 'suspended') {
    if (!this.resumeInFlight) {
        this.resumeInFlight = true;
        this.audioContext.resume()
            .catch(e => console.warn("AudioContext resume failed:", e))
            .finally(() => {
                this.resumeInFlight = false;
            });
    }
    return;
}

const soundProfile = this.sounds[soundName];
const durationSec = Number(soundProfile.duration);
if (!Number.isFinite(durationSec) || durationSec <= 0) {
    console.warn(`Invalid duration for sound: ${soundName}`, soundProfile.duration);
    return;
}

const baseVolume = (soundProfile.vol !== undefined ? soundProfile.vol : 0.5) * this.globalVolume * volumeMultiplier;
if (baseVolume <= 0.001) return;

let finalVolume = baseVolume;
let panValue = 0;
if (position && localPlayerState && app && gameScene) {
    const viewCenter = { x: app.screen.width / 2, y: app.screen.height / 2 };
    const soundWorldPos = gameScene.toGlobal(position);
    const dx = soundWorldPos.x - viewCenter.x;
    const dy = soundWorldPos.y - viewCenter.y;
    const distance = Math.sqrt(dx * dx + dy * dy);
    const maxAudibleDistance = 800;

    if (distance > maxAudibleDistance) return;
    finalVolume *= Math.max(0, 1 - (distance / maxAudibleDistance));
    if (finalVolume <= 0.001) return;
    panValue = Math.max(-1, Math.min(1, dx / (app.screen.width / 2)));
}

const nowMs = (typeof performance !== 'undefined' && typeof performance.now === 'function')
    ? performance.now()
    : Date.now();
const prioritizeLocal = !!(options && options.prioritizeLocal);
const bypassLimiter = !!(options && options.bypassLimiter);
this.sweepExpiredVoices(nowMs);
if (!bypassLimiter && !this.shouldPlaySoundNow(soundName, nowMs)) {
    return;
}
if (this.tryPlaySample(soundName, finalVolume, panValue, nowMs, prioritizeLocal)) {
    return;
}
if (this.soundSamples[soundName]) {
    this.loadSampleBuffer(soundName).catch(() => {
        // loadSampleBuffer already logs detailed failures.
    });
}
this._playTone(
    soundName,
    soundProfile,
    finalVolume,
    panValue,
    durationSec,
    nowMs,
    prioritizeLocal
);
    }

    _playTone(soundName, profile, volume, panValue, durationSec, nowMs, prioritizeLocal = false) {
if (!this.audioContext) return;
const voice = this.acquireVoiceSlot(soundName, durationSec, nowMs, prioritizeLocal);
if (!voice) {
    return;
}

const now = this.audioContext.currentTime;
const gainNode = voice.gainNode;
try {
    gainNode.gain.cancelScheduledValues(now);
    const attackSec = Math.max(0.002, Math.min(0.01, durationSec * 0.22));
    gainNode.gain.setValueAtTime(0.0001, now);
    gainNode.gain.exponentialRampToValueAtTime(Math.max(0.001, volume), now + attackSec);
    gainNode.gain.exponentialRampToValueAtTime(0.001, now + durationSec);
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
if (profile.type === 'noise') {
    const buffer = this.getNoiseBuffer(durationSec);
    if (!buffer) {
        this.markVoiceReleased(voice);
        return;
    }
    sourceNode = this.audioContext.createBufferSource();
    sourceNode.buffer = buffer;
} else {
    const oscillator = this.audioContext.createOscillator();
    oscillator.type = profile.type || 'sine';
    if (Array.isArray(profile.freq)) {
        if (!Number.isFinite(profile.freq[0])) {
            console.warn("Invalid profile.freq[0]", profile);
            this.markVoiceReleased(voice);
            return;
        }
        oscillator.frequency.setValueAtTime(profile.freq[0], now);
        if (profile.freq.length > 1 && Number.isFinite(profile.freq[1])) {
            oscillator.frequency.linearRampToValueAtTime(profile.freq[1], now + durationSec * 0.8);
        }
        if (profile.freq.length > 2 && Number.isFinite(profile.freq[2])) {
            oscillator.frequency.linearRampToValueAtTime(profile.freq[2], now + durationSec);
        }
    } else if (Number.isFinite(profile.freq)) {
        oscillator.frequency.setValueAtTime(profile.freq, now);
    } else {
        console.warn("Invalid profile.freq", profile);
        this.markVoiceReleased(voice);
        return;
    }
    sourceNode = oscillator;
}

sourceNode.connect(gainNode);
voice.sourceNode = sourceNode;
sourceNode.onended = () => {
    this.markVoiceReleased(voice, sourceNode);
};

try {
    sourceNode.start(now);
    sourceNode.stop(now + durationSec);
} catch (_) {
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
