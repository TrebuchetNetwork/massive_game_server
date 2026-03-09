const PENDING_LOGS_KEY = "__mgsPendingClientLogs";
const ACTIVE_LOGGER_KEY = "__mgsClientLog";
const MAX_PENDING_LOGS = 100;

function getGlobalScope() {
    if (typeof window !== "undefined") return window;
    if (typeof globalThis !== "undefined") return globalThis;
    return null;
}

function normalizeDetail(detail) {
    if (detail instanceof Error) {
        return detail.message || String(detail);
    }
    if (typeof detail === "string") return detail;
    if (typeof detail === "undefined" || detail === null) return "";
    try {
        return JSON.stringify(detail);
    } catch (_) {
        return String(detail);
    }
}

export function emitClientLog(message, level = "info", detail) {
    const scope = getGlobalScope();
    const suffix = normalizeDetail(detail);
    const finalMessage = suffix ? `${message}: ${suffix}` : message;
    if (!scope) return;

    if (typeof scope[ACTIVE_LOGGER_KEY] === "function") {
        scope[ACTIVE_LOGGER_KEY](finalMessage, level);
        return;
    }

    const pending = Array.isArray(scope[PENDING_LOGS_KEY]) ? scope[PENDING_LOGS_KEY] : [];
    pending.push({ message: finalMessage, level });
    if (pending.length > MAX_PENDING_LOGS) {
        pending.splice(0, pending.length - MAX_PENDING_LOGS);
    }
    scope[PENDING_LOGS_KEY] = pending;
}

export function flushPendingClientLogs() {
    const scope = getGlobalScope();
    if (!scope || typeof scope[ACTIVE_LOGGER_KEY] !== "function") return;

    const pending = Array.isArray(scope[PENDING_LOGS_KEY]) ? scope[PENDING_LOGS_KEY] : [];
    for (const entry of pending) {
        scope[ACTIVE_LOGGER_KEY](entry.message, entry.level || "info");
    }
    scope[PENDING_LOGS_KEY] = [];
}
