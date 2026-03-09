/**
 * ErrorBoundary.js - Module initialization error boundary
 *
 * Wraps module factory calls in try-catch to prevent a single module
 * failure from crashing the entire client.  Provides a minimal fallback
 * UI overlay when a module fails and a full-screen critical error overlay
 * with a reconnect button.  Uses the same createXxxModule(getCtx)
 * factory pattern as other client_logic modules.
 */

import { emitClientLog } from './client_logger.js';

export function createErrorBoundary(getCtx) {

    /** @type {string[]} */
    const failedModules = [];

    /** @type {HTMLDivElement|null} */
    let recoveryOverlay = null;

    /** @type {HTMLDivElement|null} */
    let criticalOverlay = null;

    // ── Helpers ──────────────────────────────────────────────────────

    /**
     * Best-effort log: uses the live `log` function from the shared
     * context when available, otherwise falls back to emitClientLog
     * (which buffers until the logger is wired up).
     */
    function logMessage(message, level) {
        try {
            const ctx = typeof getCtx === 'function' ? getCtx() : null;
            if (ctx && typeof ctx.log === 'function') {
                ctx.log(message, level);
                return;
            }
        } catch (_) { /* getCtx may throw during early init */ }
        emitClientLog(message, level);
    }

    function ensureRecoveryOverlay() {
        if (recoveryOverlay) return recoveryOverlay;
        recoveryOverlay = document.createElement('div');
        Object.assign(recoveryOverlay.style, {
            position: 'fixed',
            top: '0',
            left: '0',
            width: '100%',
            zIndex: '9998',
            padding: '0',
            margin: '0',
            pointerEvents: 'none',
        });
        document.body.appendChild(recoveryOverlay);
        return recoveryOverlay;
    }

    function appendRecoveryBanner(name) {
        const container = ensureRecoveryOverlay();
        const banner = document.createElement('div');
        Object.assign(banner.style, {
            background: 'rgba(180, 80, 0, 0.92)',
            color: '#fff',
            fontFamily: 'monospace',
            fontSize: '13px',
            padding: '6px 14px',
            marginBottom: '2px',
            textAlign: 'center',
            pointerEvents: 'auto',
        });
        banner.textContent = `Module ${name} failed to load \u2014 attempting recovery`;
        container.appendChild(banner);

        // Auto-dismiss after 8 seconds so it does not permanently obstruct.
        setTimeout(() => {
            try { container.removeChild(banner); } catch (_) {}
        }, 8000);
    }

    // ── Public API ───────────────────────────────────────────────────

    /**
     * Wraps a module factory call in a try-catch.  On success the
     * factory return value is passed through.  On failure the error is
     * logged, a recovery banner is shown, and `null` is returned so the
     * caller can decide how to degrade.
     *
     * @param {string} name   Human-readable module name (e.g. "GameRenderer")
     * @param {() => T} initFn  Thunk that calls the real factory
     * @returns {T|null}
     * @template T
     */
    function wrapModuleInit(name, initFn) {
        try {
            return initFn();
        } catch (error) {
            const detail = error?.message || String(error);
            logMessage(`ErrorBoundary: ${name} init failed: ${detail}`, 'error');
            if (typeof console !== 'undefined') {
                console.error(`[ErrorBoundary] ${name} init failed`, error);
            }
            failedModules.push(name);
            appendRecoveryBanner(name);
            return null;
        }
    }

    /**
     * Returns a shallow copy of the failed module name list.
     * @returns {string[]}
     */
    function getFailedModules() {
        return failedModules.slice();
    }

    /**
     * Displays a full-screen critical error overlay with a reconnect
     * button.  Intended for unrecoverable failures where the game
     * cannot proceed.
     *
     * @param {string} message  Short description shown to the player
     */
    function showCriticalError(message) {
        // Prevent stacking multiple critical overlays.
        if (criticalOverlay) {
            try { document.body.removeChild(criticalOverlay); } catch (_) {}
            criticalOverlay = null;
        }

        criticalOverlay = document.createElement('div');
        Object.assign(criticalOverlay.style, {
            position: 'fixed',
            top: '0',
            left: '0',
            width: '100%',
            height: '100%',
            zIndex: '9999',
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            background: 'rgba(10, 10, 14, 0.95)',
            color: '#e0e0e0',
            fontFamily: 'monospace',
            textAlign: 'center',
            padding: '24px',
            boxSizing: 'border-box',
        });

        const heading = document.createElement('div');
        heading.textContent = 'Connection Error';
        Object.assign(heading.style, {
            fontSize: '22px',
            fontWeight: 'bold',
            marginBottom: '14px',
            color: '#ff6b6b',
        });

        const body = document.createElement('div');
        body.textContent = message || 'An unexpected error occurred.';
        Object.assign(body.style, {
            fontSize: '14px',
            maxWidth: '480px',
            lineHeight: '1.5',
            marginBottom: '24px',
            wordBreak: 'break-word',
        });

        const btn = document.createElement('button');
        btn.textContent = 'Reconnect';
        Object.assign(btn.style, {
            padding: '10px 32px',
            fontSize: '15px',
            fontFamily: 'monospace',
            fontWeight: 'bold',
            cursor: 'pointer',
            border: 'none',
            borderRadius: '4px',
            background: '#4a90d9',
            color: '#fff',
        });
        btn.addEventListener('click', () => {
            try { document.body.removeChild(criticalOverlay); } catch (_) {}
            criticalOverlay = null;
            window.location.reload();
        });

        criticalOverlay.appendChild(heading);
        criticalOverlay.appendChild(body);
        criticalOverlay.appendChild(btn);
        document.body.appendChild(criticalOverlay);

        logMessage(`ErrorBoundary critical: ${message}`, 'error');
    }

    return {
        wrapModuleInit,
        getFailedModules,
        showCriticalError,
    };
}
