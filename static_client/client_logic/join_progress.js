/**
 * join_progress.js - staged join-flow feedback for the first-30-seconds UX.
 *
 * Pure/logic-only module (DOM updates are injected), so it is unit-testable
 * under node --test. Two concerns:
 *
 * 1. Staged join progress tracker: maps connection status transitions
 *    (connecting -> negotiating -> waiting -> playing | error) onto a
 *    3-step progress model (Connect -> Negotiate -> Spawn) rendered as an
 *    overlay while a join attempt is in flight, with an error + retry phase.
 *
 * 2. First-visit control hints: decides whether the hints overlay should be
 *    shown (localStorage flag) and which hint items apply for the platform.
 */

/** Ordered join stages surfaced to the player. */
export const JOIN_PROGRESS_STAGES = Object.freeze([
    Object.freeze({ key: 'connecting', label: 'Connect' }),
    Object.freeze({ key: 'negotiating', label: 'Negotiate' }),
    Object.freeze({ key: 'spawning', label: 'Spawn' }),
]);

/** Fallback detail lines per stage when the status detail is empty. */
const STAGE_FALLBACK_DETAIL = {
    connecting: 'Contacting signaling server...',
    negotiating: 'Establishing peer connection...',
    spawning: 'Waiting for match state...',
};

/** localStorage key recording that the control hints were dismissed/seen. */
export const CONTROL_HINTS_STORAGE_KEY = 'mgs_control_hints_seen_v1';

/**
 * Create a tracker that converts raw connection status transitions into a
 * staged join-progress snapshot.
 *
 * Lifecycle:
 * - 'connecting'/'negotiating' arms the tracker (join in flight).
 * - 'waiting' while armed advances to the spawn stage.
 * - 'playing'/'respawn' while armed completes the join (player spawned).
 * - 'error' while armed enters the error phase (detail + retry).
 * - 'idle' disarms without completing (e.g. manual reset before connecting).
 */
export function createJoinProgressTracker() {
    let armed = false;
    let phase = 'idle'; // idle | joining | error | done
    let currentStage = null;
    let detailText = '';

    function snapshot() {
        return { armed, phase, currentStage, detailText };
    }

    function handleStatus(statusKey, detail = '') {
        switch (statusKey) {
            case 'connecting':
                armed = true;
                phase = 'joining';
                currentStage = 'connecting';
                detailText = detail || STAGE_FALLBACK_DETAIL.connecting;
                break;
            case 'negotiating':
                armed = true;
                phase = 'joining';
                currentStage = 'negotiating';
                detailText = detail || STAGE_FALLBACK_DETAIL.negotiating;
                break;
            case 'waiting':
                if (!armed || phase !== 'joining') break;
                currentStage = 'spawning';
                detailText = detail || STAGE_FALLBACK_DETAIL.spawning;
                break;
            case 'playing':
            case 'respawn':
                if (!armed || phase !== 'joining') break;
                phase = 'done';
                currentStage = 'spawning';
                detailText = '';
                break;
            case 'error':
                if (!armed || phase !== 'joining') break;
                phase = 'error';
                detailText = detail || 'Connection failed.';
                break;
            case 'idle':
                armed = false;
                phase = 'idle';
                currentStage = null;
                detailText = '';
                break;
            default:
                break;
        }
        return snapshot();
    }

    function reset() {
        armed = false;
        phase = 'idle';
        currentStage = null;
        detailText = '';
        return snapshot();
    }

    return { handleStatus, snapshot, reset };
}

/**
 * Apply a join-progress snapshot to the overlay DOM. All elements are
 * injected; missing elements are tolerated. `stageElements` maps stage key
 * to the step element. `retryButton` is only visible in the error phase.
 */
export function applyJoinProgressUi({ snapshot: snap, overlay, stageElements, detailElement, retryButton }) {
    if (!overlay) return;
    const visible = snap.phase === 'joining' || snap.phase === 'error';
    overlay.classList.toggle('hidden', !visible);
    overlay.classList.toggle('join-progress--error', snap.phase === 'error');
    overlay.setAttribute('aria-hidden', String(!visible));

    if (detailElement) {
        detailElement.textContent = visible ? (snap.detailText || '') : '';
    }
    if (retryButton) {
        retryButton.classList.toggle('hidden', snap.phase !== 'error');
    }

    const stageOrder = JOIN_PROGRESS_STAGES.map((s) => s.key);
    const currentIndex = stageOrder.indexOf(snap.currentStage);
    for (const stage of JOIN_PROGRESS_STAGES) {
        const el = stageElements ? stageElements[stage.key] : null;
        if (!el) continue;
        const idx = stageOrder.indexOf(stage.key);
        const isDone = snap.phase === 'done' || (currentIndex >= 0 && idx < currentIndex);
        const isCurrent = snap.phase === 'joining' && idx === currentIndex;
        el.classList.toggle('join-progress__step--done', isDone);
        el.classList.toggle('join-progress__step--current', isCurrent);
        el.classList.toggle('join-progress__step--error', snap.phase === 'error' && idx === currentIndex);
    }
}

/**
 * Hint items for the first-visit control overlay. Mirrors the real bindings
 * in InputManager.js (WASD/arrows, mouse fire, 1/2 weapons, Q dash, E dodge,
 * R reload, V melee) and the mobile touch layout (left stick, right-side aim,
 * FIRE button).
 */
export function getControlHintItems(isMobile) {
    if (isMobile) {
        return [
            { key: '◀ stick', action: 'Move' },
            { key: 'drag right', action: 'Aim' },
            { key: 'FIRE', action: 'Hold to shoot' },
            { key: '1 / 2', action: 'Swap weapon' },
            { key: 'dash buttons', action: 'Dash / dodge' },
        ];
    }
    return [
        { key: 'W A S D', action: 'Move' },
        { key: 'mouse', action: 'Aim' },
        { key: 'click', action: 'Fire' },
        { key: '1 / 2', action: 'Swap weapon' },
        { key: 'Q / E', action: 'Dash / dodge' },
        { key: 'R · V', action: 'Reload · melee' },
    ];
}

/**
 * Whether the first-visit control hints should be shown. `storageLike` only
 * needs getItem (a localStorage or a plain test double). Storage failures
 * (private mode) fall back to showing the hints once for the session.
 */
export function shouldShowControlHints(storageLike, storageKey = CONTROL_HINTS_STORAGE_KEY) {
    try {
        return storageLike?.getItem?.(storageKey) !== '1';
    } catch (_error) {
        return true;
    }
}

/** Record that the control hints have been seen/dismissed. Never throws. */
export function markControlHintsSeen(storageLike, storageKey = CONTROL_HINTS_STORAGE_KEY) {
    try {
        storageLike?.setItem?.(storageKey, '1');
    } catch (_error) {
        // Storage unavailable (private mode) — hints may reappear next visit.
    }
}
