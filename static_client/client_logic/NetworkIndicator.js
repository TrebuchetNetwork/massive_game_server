/**
 * Legacy compatibility shim.
 *
 * Canonical implementation lives in `ui_widgets.js`.
 */

import { NetworkIndicator as UiWidgetsNetworkIndicator } from "./ui_widgets.js";

export class NetworkIndicator extends UiWidgetsNetworkIndicator {
    constructor(options = {}) {
        super(options && typeof options === "object" ? options : {});
    }
}
