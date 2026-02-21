export function removeSyntheticProjectiles(projectiles, projectileIdPrefix) {
    for (const projectileId of Array.from(projectiles.keys())) {
        if (typeof projectileId === 'string' && projectileId.startsWith(projectileIdPrefix)) {
            projectiles.delete(projectileId);
        }
    }
}

export function populateSyntheticProjectiles({
    rawCount,
    projectiles,
    projectileIdPrefix,
    weaponTypes,
    nowMs = performance.now(),
    maxProjectiles = 5000,
}) {
    const nextCount = Math.max(0, Math.min(maxProjectiles, Math.floor(Number(rawCount) || 0)));
    if (nextCount <= 0) {
        return 0;
    }

    const spawnWidth = 1400;
    const spawnHeight = 760;
    const columns = Math.max(1, Math.ceil(Math.sqrt(nextCount * (spawnWidth / spawnHeight))));
    const rows = Math.max(1, Math.ceil(nextCount / columns));
    const spacingX = spawnWidth / columns;
    const spacingY = spawnHeight / rows;

    for (let i = 0; i < nextCount; i += 1) {
        const ratio = i / Math.max(1, nextCount);
        const angle = ratio * Math.PI * 2;
        const col = i % columns;
        const row = Math.floor(i / columns);
        const x = -spawnWidth * 0.5 + (col + 0.5) * spacingX;
        const y = -spawnHeight * 0.5 + (row + 0.5) * spacingY;
        const speed = 180 + (i % 11) * 14;
        const id = `${projectileIdPrefix}${i}`;

        projectiles.set(id, {
            id,
            x,
            y,
            render_x: x,
            render_y: y,
            velocity_x: Math.cos(angle) * speed,
            velocity_y: Math.sin(angle) * speed,
            weapon_type: weaponTypes[i % weaponTypes.length],
            last_server_update_ms: nowMs,
        });
    }

    return nextCount;
}
