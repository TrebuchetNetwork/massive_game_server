export function normalizeAngle(angle) {
    const value = Number(angle);
    if (!Number.isFinite(value)) {
        return 0;
    }
    const tau = Math.PI * 2;
    const wrapped = ((value + Math.PI) % tau + tau) % tau - Math.PI;
    return Object.is(wrapped, -0) ? 0 : wrapped;
}

export function clamp(value, min, max) {
    return Math.max(min, Math.min(max, value));
}

export function lerp(a, b, t) {
    return a + (b - a) * t;
}

export function smoothFollowGain(baseGain, deltaSeconds) {
    const frames = clamp((deltaSeconds || 0) * 60, 0.5, 3.5);
    return 1 - Math.pow(1 - baseGain, frames);
}
