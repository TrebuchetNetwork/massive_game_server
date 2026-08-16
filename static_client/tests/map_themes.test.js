import test from "node:test";
import assert from "node:assert/strict";

import {
    MAP_THEMES,
    MAP_THEME_DEFAULT_NAME,
    createMapThemes,
    resolveMapTheme,
} from "../client_logic/MapThemes.js";

function luminance(color) {
    const r = (color >> 16) & 0xFF;
    const g = (color >> 8) & 0xFF;
    const b = color & 0xFF;
    return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
}

test("map themes: factory exposes table, resolver and builders", () => {
    const mapThemes = createMapThemes();
    assert.equal(typeof mapThemes.resolveMapTheme, "function");
    assert.equal(typeof mapThemes.buildBackdropCanvas, "function");
    assert.equal(typeof mapThemes.buildNebulaCanvas, "function");
    assert.equal(mapThemes.defaultThemeName, MAP_THEME_DEFAULT_NAME);
    assert.ok(mapThemes.themes[MAP_THEME_DEFAULT_NAME]);
});

test("map themes: keyword matching is case-insensitive substring", () => {
    assert.equal(resolveMapTheme("Fortress").name, "ember");
    assert.equal(resolveMapTheme("EMBER RIDGE").name, "ember");
    assert.equal(resolveMapTheme("glacier fields").name, "frost");
    assert.equal(resolveMapTheme("Deep Void").name, "void");
});

test("map themes: unknown or empty names fall back to the default theme", () => {
    const fallback = MAP_THEMES[MAP_THEME_DEFAULT_NAME];
    assert.equal(resolveMapTheme("Massive Arena Dynamic 12p"), fallback);
    assert.equal(resolveMapTheme("Arena"), fallback);
    assert.equal(resolveMapTheme(""), fallback);
    assert.equal(resolveMapTheme(null), fallback);
    assert.equal(resolveMapTheme(undefined), fallback);
});

test("map themes: every theme has the full shape the appliers consume", () => {
    for (const theme of Object.values(MAP_THEMES)) {
        assert.equal(typeof theme.name, "string");
        assert.ok(Number.isInteger(theme.bgGradientStops.top));
        assert.ok(Number.isInteger(theme.bgGradientStops.bottom));
        assert.ok(Number.isInteger(theme.bgColor));
        assert.ok(theme.starDensity > 0);
        assert.ok(theme.starColors.length >= 1);
        assert.ok(theme.nebula.colors.length >= 1);
        assert.ok(theme.nebula.alpha > 0 && theme.nebula.alpha <= 1);
        assert.ok(theme.nebula.count >= 1);
        assert.ok(Number.isInteger(theme.vignetteTint));
        assert.ok(theme.wallTint === null || Number.isInteger(theme.wallTint));
    }
});

test("map themes: backgrounds stay dark enough for lime HUD and ship colors to pop", () => {
    for (const theme of Object.values(MAP_THEMES)) {
        assert.ok(luminance(theme.bgGradientStops.top) < 0.2, `${theme.name} top stop too bright`);
        assert.ok(luminance(theme.bgGradientStops.bottom) < 0.2, `${theme.name} bottom stop too bright`);
        assert.ok(luminance(theme.bgColor) < 0.2, `${theme.name} bgColor too bright`);
    }
});

test("map themes: themes are frozen against accidental mutation", () => {
    assert.ok(Object.isFrozen(MAP_THEMES.void));
    assert.ok(Object.isFrozen(MAP_THEMES.void.bgGradientStops));
    assert.ok(Object.isFrozen(MAP_THEMES.void.nebula));
});
