const HINTS = {
    move: 'Move: left thumb / WASD',
    aim: 'Aim: drag right side / mouse',
    fire: 'Fire: tap FIRE / click',
    done: '',
};

const MAX_PROJECTILE_IDS = 512;

export function createJoinOnboarding({
    storage,
    storageKey = 'mgs_join_onboarding_v1',
    minMoveDistance = 12,
    minAimDelta = 0.08,
} = {}) {
    let step = 'move';
    let baseX = null;
    let baseY = null;
    let baseRotation = null;
    let aimGesture = false;
    let respawnRebasePending = false;
    let primeProjectiles = true;
    let projectileIdThreshold = -Infinity;
    const seenProjectileIds = new Set();

    try {
        if (storage?.getItem(storageKey) === 'done') step = 'done';
    } catch (_) { /* Storage can be unavailable in private browsing. */ }

    function rebase({ x, y, rotation }) {
        baseX = x;
        baseY = y;
        baseRotation = rotation;
    }

    function primeProjectileObservation() {
        seenProjectileIds.clear();
        primeProjectiles = true;
    }

    function rememberProjectile(id) {
        if (seenProjectileIds.has(id)) seenProjectileIds.delete(id);
        seenProjectileIds.add(id);
        if (seenProjectileIds.size > MAX_PROJECTILE_IDS) {
            seenProjectileIds.delete(seenProjectileIds.values().next().value);
        }
    }

    function persistDone() {
        try {
            storage?.setItem(storageKey, 'done');
        } catch (_) { /* Completion still applies when storage fails. */ }
    }

    return {
        view() {
            return {
                step,
                visible: step !== 'done',
                hint: HINTS[step],
            };
        },

        bind(state) {
            rebase(state);
            aimGesture = false;
            respawnRebasePending = state.alive === false;
            primeProjectileObservation();
        },

        onLocalState({ x, y, rotation, alive }) {
            if (alive === false) {
                respawnRebasePending = true;
                return;
            }
            if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(rotation)) return;
            if (respawnRebasePending) {
                rebase({ x, y, rotation });
                respawnRebasePending = false;
                aimGesture = false;
                primeProjectileObservation();
            }
            if (step === 'move') {
                if (!Number.isFinite(baseX) || !Number.isFinite(baseY)) return;
                const distance = Math.hypot(x - baseX, y - baseY);
                if (!(distance >= minMoveDistance)) return;
                step = 'aim';
                baseRotation = rotation;
                aimGesture = false;
                return;
            }
            if (step !== 'aim' || !aimGesture || !Number.isFinite(baseRotation)) return;
            const delta = rotation - baseRotation;
            const absoluteDelta = Math.abs(delta);
            const aimDelta = absoluteDelta <= Math.PI
                ? absoluteDelta
                : Math.abs(Math.atan2(Math.sin(delta), Math.cos(delta)));
            if (!(aimDelta >= minAimDelta)) return;
            step = 'fire';
            primeProjectileObservation();
        },

        notifyAimInput() {
            if (step === 'aim') aimGesture = true;
        },

        onProjectiles(projectiles, myPlayerId) {
            const priming = primeProjectiles;
            primeProjectiles = false;
            let completed = false;

            for (const projectile of projectiles) {
                rememberProjectile(String(projectile.id));
                const rawId = projectile.id;
                const isNumericId = typeof rawId === 'number'
                    || (typeof rawId === 'string' && rawId.trim() !== '');
                if (!isNumericId) continue;

                const numericId = Number(rawId);
                if (!Number.isFinite(numericId)) continue;

                const isAboveThreshold = numericId > projectileIdThreshold;
                if (isAboveThreshold) projectileIdThreshold = numericId;
                if (
                    !priming
                    && step === 'fire'
                    && isAboveThreshold
                    && String(projectile.owner_id) === String(myPlayerId)
                ) {
                    completed = true;
                }
            }

            if (completed) {
                step = 'done';
                persistDone();
            }
        },

        onReconnect(state) {
            rebase(state);
            aimGesture = false;
            respawnRebasePending = state.alive === false;
            primeProjectileObservation();
        },

        reset() {
            try {
                if (typeof storage?.removeItem === 'function') {
                    storage.removeItem(storageKey);
                } else {
                    storage?.setItem(storageKey, '');
                }
            } catch (_) { /* Reset still applies when storage fails. */ }

            step = 'move';
            baseX = null;
            baseY = null;
            baseRotation = null;
            aimGesture = false;
            respawnRebasePending = false;
            projectileIdThreshold = -Infinity;
            primeProjectileObservation();
        },
    };
}
