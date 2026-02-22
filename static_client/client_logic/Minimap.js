/**
 * Legacy compatibility shim.
 *
 * Canonical implementation lives in `ui_widgets.js`.
 * This adapter keeps both constructor styles:
 * - `new Minimap(width, height, mapScale)`
 * - `new Minimap({ width, height, mapScale, ... })`
 */

import { Minimap as UiWidgetsMinimap } from "./ui_widgets.js";

export class Minimap extends UiWidgetsMinimap {
    constructor(widthOrOptions = 150, height = 150, mapScale = 0.05) {
        if (widthOrOptions && typeof widthOrOptions === "object") {
            super(widthOrOptions);
            return;
        }
        super({
            width: Number(widthOrOptions) || 150,
            height: Number(height) || 150,
            mapScale: Number(mapScale) || 0.05,
        });
    }
}
