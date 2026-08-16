/**
 * MapThemes.js - Per-map visual themes ("better worlds")
 *
 * A theme restyles the world's backdrop for a given map: background
 * gradient, starfield density/tint, nebula layer, health-vignette tint and
 * an optional wall-panel tint. Themes are selected from the server-provided
 * map name (initial state `map_name`); unknown names fall back to the
 * default theme so a new map always gets a coherent look.
 *
 * All theme art is pre-rendered once per theme switch (canvas gradients),
 * never per frame. The pure table/resolver part of this module is
 * dependency-free so it can be unit-tested in Node; the canvas builders
 * only run in the browser and return null when no DOM is available.
 */

function makeTheme(def) {
    return Object.freeze({
        starDensity: 1,
        vignetteTint: 0xFF2233,
        wallTint: null,
        keywords: [],
        ...def,
        bgGradientStops: Object.freeze({ ...def.bgGradientStops }),
        starColors: Object.freeze([...(def.starColors || [0xFFFFFF])]),
        nebula: Object.freeze({ alpha: 0.14, count: 3, ...(def.nebula || {}) }),
    });
}

export const MAP_THEME_DEFAULT_NAME = 'void';

export const MAP_THEMES = Object.freeze({
    // Default: deep purple-black void, cyan-white stars, indigo nebulae.
    void: makeTheme({
        name: 'void',
        bgGradientStops: { top: 0x05040C, bottom: 0x160A30 },
        bgColor: 0x0A0716,
        starDensity: 1.0,
        starColors: [0xFFFFFF, 0x9FE8FF, 0xB8A6FF],
        nebula: { colors: [0x3B1F6E, 0x1B2F6B, 0x4C1D5E], alpha: 0.24, count: 3 },
        vignetteTint: 0xFF2233,
        wallTint: null,
        keywords: ['void', 'abyss', 'umbra', 'shadow', 'deep'],
    }),
    // Ember: scorched dark red-orange, warm nebulae, warm steel walls.
    ember: makeTheme({
        name: 'ember',
        bgGradientStops: { top: 0x0C0403, bottom: 0x2C0E06 },
        bgColor: 0x160804,
        starDensity: 0.85,
        starColors: [0xFFE9D6, 0xFFB27A, 0xFFF6E0],
        nebula: { colors: [0x7A2A10, 0x8C3A12, 0x5E1A2E], alpha: 0.22, count: 3 },
        vignetteTint: 0xFF2A18,
        wallTint: 0x4A3A34,
        keywords: ['ember', 'fortress', 'inferno', 'molten', 'volcan', 'ash', 'scorch'],
    }),
    // Frost: blue-teal dark, pale stars, teal nebulae, cool steel walls.
    frost: makeTheme({
        name: 'frost',
        bgGradientStops: { top: 0x02090F, bottom: 0x0B2436 },
        bgColor: 0x06131D,
        starDensity: 1.15,
        starColors: [0xEAFBFF, 0xBFEBFF, 0xD6F5EE],
        nebula: { colors: [0x14505E, 0x1B3A6B, 0x0F5E52], alpha: 0.22, count: 3 },
        vignetteTint: 0xFF2A3C,
        wallTint: 0x2E4456,
        keywords: ['frost', 'ice', 'glacier', 'arctic', 'tundra', 'frozen'],
    }),
});

/**
 * Resolve a map name to a theme. Keyword match is case-insensitive
 * substring against the (lowercased) map name; anything unknown falls back
 * to the default theme.
 */
export function resolveMapTheme(mapName) {
    const fallback = MAP_THEMES[MAP_THEME_DEFAULT_NAME];
    const normalized = String(mapName || '').trim().toLowerCase();
    if (!normalized) return fallback;
    for (const theme of Object.values(MAP_THEMES)) {
        if (theme.name === normalized) return theme;
    }
    for (const theme of Object.values(MAP_THEMES)) {
        for (const keyword of theme.keywords) {
            if (normalized.includes(keyword)) return theme;
        }
    }
    return fallback;
}

function toCssColor(color) {
    return `#${(color & 0xFFFFFF).toString(16).padStart(6, '0')}`;
}

/**
 * Vertical background gradient canvas for a theme. Narrow (16px wide) on
 * purpose: the sprite is scaled up to fill the screen, and horizontal
 * detail does not exist in a vertical gradient.
 */
export function buildBackdropCanvas(theme, height = 256) {
    if (typeof document === 'undefined') return null;
    const canvas = document.createElement('canvas');
    canvas.width = 16;
    canvas.height = Math.max(2, Math.floor(height));
    const g = canvas.getContext('2d');
    if (!g) return null;
    const gradient = g.createLinearGradient(0, 0, 0, canvas.height);
    gradient.addColorStop(0, toCssColor(theme.bgGradientStops.top));
    gradient.addColorStop(1, toCssColor(theme.bgGradientStops.bottom));
    g.fillStyle = gradient;
    g.fillRect(0, 0, canvas.width, canvas.height);
    return canvas;
}

/**
 * Soft radial glow canvas for one nebula sprite. Baked once per theme
 * switch; the sprite is then only moved via transforms (slow parallax).
 */
export function buildNebulaCanvas(color, size = 256) {
    if (typeof document === 'undefined') return null;
    const canvas = document.createElement('canvas');
    canvas.width = size;
    canvas.height = size;
    const g = canvas.getContext('2d');
    if (!g) return null;
    const center = size / 2;
    const r = (color >> 16) & 0xFF;
    const gg = (color >> 8) & 0xFF;
    const b = color & 0xFF;
    const gradient = g.createRadialGradient(center, center, 0, center, center, center);
    gradient.addColorStop(0, `rgba(${r}, ${gg}, ${b}, 0.85)`);
    gradient.addColorStop(0.45, `rgba(${r}, ${gg}, ${b}, 0.4)`);
    gradient.addColorStop(1, `rgba(${r}, ${gg}, ${b}, 0)`);
    g.fillStyle = gradient;
    g.fillRect(0, 0, size, size);
    return canvas;
}

export function createMapThemes() {
    return {
        themes: MAP_THEMES,
        defaultThemeName: MAP_THEME_DEFAULT_NAME,
        resolveMapTheme,
        buildBackdropCanvas,
        buildNebulaCanvas,
    };
}
