export function collectBroadcastPlayers(playerMap) {
    const players = [];
    let leaderId = null;
    playerMap.forEach((player, playerId) => {
        if (!player || !player.alive) return;
        const teamId = Number(player.teamId ?? player.team_id);
        const x = Number(player.x);
        const y = Number(player.y);
        if (!Number.isFinite(teamId) || teamId === 0 || !Number.isFinite(x) || !Number.isFinite(y)) return;
        const id = String(player.id ?? playerId);
        const username = String(player.username || '');
        if (username.startsWith('#1 ')) leaderId = id;
        players.push({
            id,
            x,
            y,
            hp: Number(player.hp ?? player.health),
            maxHp: Number(player.maxHp ?? player.max_health),
            teamId,
            username,
        });
    });
    return { players, leaderId };
}

export function createBroadcastLifecycle({ onStatus, idleMs = 10000, staleMs = 12000 } = {}) {
    let live = false;
    let lastSnapshotAt = null;
    let emptySince = null;
    let idleNotified = false;

    function emit(status, reason) {
        if (typeof onStatus === 'function') onStatus(status, reason);
    }

    function check(atMs) {
        if (emptySince !== null && !idleNotified && atMs - emptySince >= idleMs) {
            idleNotified = true;
            live = false;
            emit('fallback', 'exhibition_idle');
        }
        if (live && lastSnapshotAt !== null && atMs - lastSnapshotAt >= staleMs) {
            live = false;
            emit('fallback', 'stale');
        }
    }

    return {
        onSnapshot(hasFighters, atMs) {
            lastSnapshotAt = atMs;
            if (hasFighters) {
                emptySince = null;
                idleNotified = false;
                if (!live) {
                    live = true;
                    emit('live', null);
                }
                return;
            }
            if (emptySince === null) {
                emptySince = atMs;
                idleNotified = false;
            }
            check(atMs);
        },
        check,
        hasSnapshots() {
            return lastSnapshotAt !== null;
        },
        markFallback() {
            live = false;
        },
        reset() {
            live = false;
            lastSnapshotAt = null;
            emptySince = null;
            idleNotified = false;
        },
    };
}
