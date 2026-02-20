export function normalizeAngle(angle) {
    let wrapped = angle;
    while (wrapped > Math.PI) wrapped -= Math.PI * 2;
    while (wrapped < -Math.PI) wrapped += Math.PI * 2;
    return wrapped;
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
