export function getDefaultWsUrl() {
    try {
        const wsProtocol = window.location.protocol === "https:" ? "wss:" : "ws:";
        return `${wsProtocol}//${window.location.host}/ws`;
    } catch (_) {
        return "ws://localhost:8080/ws";
    }
}

export function normalizeSignalingUrl(rawValue) {
    const raw = (rawValue || "").trim();
    let candidate = raw || getDefaultWsUrl();

    if (candidate.startsWith("/")) {
        candidate = `${window.location.origin}${candidate}`;
    } else if (!/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(candidate)) {
        const wsProtocol = window.location.protocol === "https:" ? "wss:" : "ws:";
        candidate = `${wsProtocol}//${candidate}`;
    }

    let parsed;
    try {
        parsed = new URL(candidate);
    } catch (_) {
        return {
            ok: false,
            error: `Invalid WebSocket URL: "${raw || candidate}"`,
        };
    }

    if (parsed.protocol === "http:") parsed.protocol = "ws:";
    if (parsed.protocol === "https:") parsed.protocol = "wss:";

    if (parsed.protocol !== "ws:" && parsed.protocol !== "wss:") {
        return {
            ok: false,
            error: `Unsupported protocol "${parsed.protocol}". Use ws:// or wss://.`,
        };
    }

    if (!parsed.pathname || parsed.pathname === "/") {
        parsed.pathname = "/ws";
    }

    if (window.location.protocol === "https:" && parsed.protocol === "ws:") {
        parsed.protocol = "wss:";
    }

    return { ok: true, url: parsed.toString() };
}

export function summarizeSignalingError(event, socket, resolvedUrl) {
    const readyStateNames = ["CONNECTING", "OPEN", "CLOSING", "CLOSED"];
    const readyState =
        socket && typeof socket.readyState === "number"
            ? `${socket.readyState} (${readyStateNames[socket.readyState] || "UNKNOWN"})`
            : "unknown";
    const browserOnline = navigator.onLine ? "online" : "offline";
    const details = [
        `Signaling error for ${resolvedUrl}`,
        `readyState=${readyState}`,
        `network=${browserOnline}`,
    ];

    if (window.location.protocol === "https:" && resolvedUrl.startsWith("ws://")) {
        details.push("hint=Mixed-content blocked. Use wss://");
    } else if (window.location.protocol === "http:" && resolvedUrl.startsWith("wss://")) {
        details.push("hint=Using TLS WebSocket from HTTP page is okay if endpoint supports TLS");
    }

    if (event && event.type) {
        details.push(`event=${event.type}`);
    }

    return details.join(" | ");
}

export function splitNonEmptyValues(rawValue, delimiter = ",") {
    if (!rawValue) return [];
    return String(rawValue)
        .split(delimiter)
        .map((value) => value.trim())
        .filter((value) => value.length > 0);
}

export function parseIceServerSpec(rawSpec, options = {}) {
    if (!rawSpec) return null;
    const parts = String(rawSpec)
        .split("|")
        .map((part) => part.trim());
    if (!parts[0]) return null;
    const urls = splitNonEmptyValues(parts[0], ",");
    if (!urls.length) return null;

    const server = {
        urls: urls.length === 1 ? urls[0] : urls,
    };
    if (options.includeCredentials === true) {
        if (parts[1]) server.username = parts[1];
        if (parts[2]) server.credential = parts[2];
    }
    return server;
}

export function resolveTurnCredentials() {
    const runtimeConfig = window.__MGS_TURN_CONFIG;
    if (runtimeConfig && typeof runtimeConfig === "object") {
        const username = String(runtimeConfig.username || runtimeConfig.user || "").trim();
        const credential = String(
            runtimeConfig.credential || runtimeConfig.password || runtimeConfig.pass || ""
        ).trim();
        if (username || credential) {
            return { username, credential };
        }
    }

    let username = "";
    let credential = "";
    try {
        username = String(sessionStorage.getItem("mgs_turn_username") || "").trim();
        credential = String(sessionStorage.getItem("mgs_turn_credential") || "").trim();
    } catch (_err) {
        // Ignore storage access errors (e.g., privacy mode restrictions).
    }
    return { username, credential };
}

export function buildPeerConnectionConfig(uiModeParams, logFn = () => {}) {
    const disableClientStun =
        uiModeParams.get("disable_stun") === "1" || uiModeParams.get("nostun") === "1";
    const servers = [];
    let ignoredUrlIceCredentials = false;

    const addServer = (server) => {
        if (!server || !server.urls) return;
        servers.push(server);
    };

    const stunParam = uiModeParams.get("stun");
    if (!disableClientStun) {
        const stunUrls = splitNonEmptyValues(stunParam, ",");
        if (stunUrls.length) {
            addServer({ urls: stunUrls.length === 1 ? stunUrls[0] : stunUrls });
        } else {
            addServer({ urls: "stun:stun.l.google.com:19302" });
        }
    }

    const genericIceSpecs = [];
    if (typeof uiModeParams.getAll === "function") {
        genericIceSpecs.push(...uiModeParams.getAll("ice"));
    } else if (uiModeParams.get("ice")) {
        genericIceSpecs.push(uiModeParams.get("ice"));
    }
    genericIceSpecs.forEach((rawSpec) => {
        splitNonEmptyValues(rawSpec, ";").forEach((entry) => {
            const parts = String(entry).split("|");
            const urlEmbeddedUser = String(parts[1] || "").trim();
            const urlEmbeddedCredential = String(parts[2] || "").trim();
            if (urlEmbeddedUser || urlEmbeddedCredential) {
                ignoredUrlIceCredentials = true;
            }
            addServer(parseIceServerSpec(entry));
        });
    });
    if (ignoredUrlIceCredentials) {
        logFn(
            "Ignoring ICE credentials in URL query params for security. Use window.__MGS_TURN_CONFIG or sessionStorage keys instead.",
            "warn"
        );
    }

    const turnUrls = splitNonEmptyValues(uiModeParams.get("turn"), ",");
    if (turnUrls.length) {
        const turnServer = {
            urls: turnUrls.length === 1 ? turnUrls[0] : turnUrls,
        };
        const hasLegacyTurnCredentialParams =
            uiModeParams.has("turn_user") ||
            uiModeParams.has("turn_username") ||
            uiModeParams.has("turn_pass") ||
            uiModeParams.has("turn_credential");
        if (hasLegacyTurnCredentialParams) {
            logFn(
                "TURN credentials in URL query params are disabled for security. Use window.__MGS_TURN_CONFIG or sessionStorage keys instead.",
                "warn"
            );
        }
        const turnCredentials = resolveTurnCredentials();
        if (turnCredentials.username) turnServer.username = turnCredentials.username;
        if (turnCredentials.credential) turnServer.credential = turnCredentials.credential;
        addServer(turnServer);
    }

    const deduped = [];
    const seen = new Set();
    servers.forEach((server) => {
        const key = JSON.stringify({
            urls: server.urls,
            username: server.username || "",
            credential: server.credential || "",
        });
        if (seen.has(key)) return;
        seen.add(key);
        deduped.push(server);
    });

    return { iceServers: deduped };
}
