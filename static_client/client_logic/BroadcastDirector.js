export function createBroadcastDirector({
    dwellMs = 4000,
    switchMargin = 1.35,
    switchSustainMs = 1500,
    damageDecayMs = 5000,
    killBoostMs = 10000,
    killZoomMs = 1200,
    sweepMs = 60000,
} = {}) {
    const damage = new Map(); // playerId -> [{ value, atMs }]
    const kills = new Map(); // username -> [atMs]
    let targetId = null;
    let targetSince = 0;
    let challengerId = null;
    let challengerSince = 0;
    let zoomUntil = 0;
    let currentTargetName = null;
    let lastSweepAt = null;

    function recordDamage({ instigatorId, value, atMs }) {
        if (!instigatorId || !Number.isFinite(value)) return;
        const list = damage.get(instigatorId) || [];
        list.push({ value, atMs });
        damage.set(instigatorId, list);
    }

    function recordKill({ killerName, atMs }) {
        if (!killerName) return;
        const list = kills.get(killerName) || [];
        list.push(atMs);
        kills.set(killerName, list);
        if (currentTargetName && killerName === currentTargetName) zoomUntil = atMs + killZoomMs;
    }

    function recentDamage(id, atMs) {
        const list = (damage.get(id) || []).filter((d) => atMs - d.atMs <= damageDecayMs);
        if (list.length) damage.set(id, list);
        else damage.delete(id);
        return list.reduce((s, d) => s + d.value, 0);
    }

    function recentKills(name, atMs) {
        const list = (kills.get(name) || []).filter((t) => atMs - t <= killBoostMs);
        if (list.length) kills.set(name, list);
        else kills.delete(name);
        return list.length;
    }

    function sweepExpired(atMs) {
        if (lastSweepAt !== null && atMs - lastSweepAt < sweepMs) return;
        lastSweepAt = atMs;
        for (const id of damage.keys()) recentDamage(id, atMs);
        for (const name of kills.keys()) recentKills(name, atMs);
    }

    function scoreOf(p, players, leaderId, atMs) {
        let score = 3 * recentDamage(p.id, atMs) + 8 * recentKills(p.username, atMs);
        if (p.id === leaderId) score += 0.5;
        let nearestEnemy = Infinity;
        for (const q of players) {
            if (q.id === p.id || q.teamId === p.teamId) continue;
            if (!Number.isFinite(q.x) || !Number.isFinite(q.y)) continue;
            const dist = Math.hypot(q.x - p.x, q.y - p.y);
            if (dist < nearestEnemy) nearestEnemy = dist;
        }
        if (nearestEnemy < 400) score += 1.5;
        if (
            Number.isFinite(p.hp) &&
            Number.isFinite(p.maxHp) &&
            p.maxHp > 0 &&
            p.hp / p.maxHp < 0.3 &&
            nearestEnemy < 600
        ) {
            score += 2;
        }
        return score;
    }

    function selectTarget(players, leaderId, atMs) {
        if (!players.length) {
            targetId = null;
            currentTargetName = null;
            return null;
        }
        const scored = players.map((p) => ({ p, s: scoreOf(p, players, leaderId, atMs) }));
        scored.sort((a, b) => b.s - a.s || (a.p.id === b.p.id ? 0 : a.p.id < b.p.id ? -1 : 1));
        const best = scored[0];
        if (targetId === null || !players.some((p) => p.id === targetId)) {
            targetId = best.p.id;
            targetSince = atMs;
            challengerId = null;
        } else if (best.p.id !== targetId) {
            const current = scored.find((e) => e.p.id === targetId);
            const currentScore = current ? current.s : 0;
            const dwellOver = atMs - targetSince >= dwellMs;
            if (dwellOver && best.s > Math.max(currentScore, 0.01) * switchMargin) {
                if (challengerId !== best.p.id) {
                    challengerId = best.p.id;
                    challengerSince = atMs;
                } else if (atMs - challengerSince >= switchSustainMs) {
                    targetId = best.p.id;
                    targetSince = atMs;
                    challengerId = null;
                }
            } else {
                challengerId = null;
            }
        } else {
            challengerId = null;
        }
        const target = players.find((p) => p.id === targetId);
        currentTargetName = target ? target.username : null;
        return targetId;
    }

    function cameraFrame(players, leaderId, atMs) {
        sweepExpired(atMs);
        const visiblePlayers = players.filter((p) => Number.isFinite(p.x) && Number.isFinite(p.y));
        if (!visiblePlayers.length) return null;
        const id = selectTarget(visiblePlayers, leaderId, atMs);
        const target = visiblePlayers.find((p) => p.id === id);
        if (!target) return null;
        return { focusX: target.x, focusY: target.y, zoom: atMs < zoomUntil ? 1.15 : 1, targetId: id };
    }

    return { recordDamage, recordKill, selectTarget, cameraFrame };
}
