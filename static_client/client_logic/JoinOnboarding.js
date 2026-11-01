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
    let primeProjectiles = true;
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
            primeProjectileObservation();
        },

        onLocalState({ x, y, rotation }) {
            if (step === 'move') {
                if (baseX === null || baseY === null) return;
                if (Math.hypot(x - baseX, y - baseY) < minMoveDistance) return;
                step = 'aim';
                baseRotation = rotation;
                aimGesture = false;
                return;
            }
            if (step !== 'aim' || !aimGesture || baseRotation === null) return;
            if (Math.abs(rotation - baseRotation) < minAimDelta) return;
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
                const id = String(projectile.id);
                const isNew = !seenProjectileIds.has(id);
                rememberProjectile(id);
                if (
                    !priming
                    && step === 'fire'
                    && isNew
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
            primeProjectileObservation();
        },
    };
}
