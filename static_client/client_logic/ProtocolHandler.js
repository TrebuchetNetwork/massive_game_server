/**
 * ProtocolHandler.js - FlatBuffers protocol parsing extracted from client.html
 *
 * Exports a factory that returns all protocol-related functions.
 * These functions are pure parsers that transform FlatBuffer binary data
 * into plain JS objects consumed by the game loop.
 */

export function createProtocolHandler({
    flatbuffers,
    GameProtocol,
    GP,
    log,
    isWallDebugEnabled,
    logWallDebug,
}) {
    const InitialStateType = GP.InitialStateMessage || GP.InitialState;
    const DeltaStateType = GP.DeltaStateMessage || GP.DeltaState;

    // ── Feature-detection constants ──────────────────────────────────
    const DELTA_SUPPORTS_REMOVED_PLAYER_IDS =
        typeof DeltaStateType?.prototype?.removedPlayerIdsLength === 'function';
    const DELTA_SUPPORTS_CHANGED_PLAYER_FIELDS =
        typeof DeltaStateType?.prototype?.changedPlayerFieldsLength === 'function';
    const DELTA_SUPPORTS_UPDATED_WALLS =
        typeof DeltaStateType?.prototype?.updatedWallsLength === 'function';
    const DELTA_SUPPORTS_FULL_WALLS =
        typeof DeltaStateType?.prototype?.wallsLength === 'function';

    // ── Delta field bitmask constants ────────────────────────────────
    const PLAYER_DELTA_FIELD_POSITION_ROTATION = 1;
    const PLAYER_DELTA_FIELD_HEALTH_ALIVE      = 2;
    const PLAYER_DELTA_FIELD_WEAPON_AMMO       = 4;
    const PLAYER_DELTA_FIELD_SCORE_STATS       = 8;
    const PLAYER_DELTA_FIELD_POWERUPS          = 16;
    const PLAYER_DELTA_FIELD_SHIELD            = 32;
    const PLAYER_DELTA_FIELD_FLAG              = 64;
    const PLAYER_DELTA_FULL_MASK               = 127;

    // ── Scratch / reusable objects for zero-alloc parsing ────────────
    const flatbufferByteBufferScratch = new flatbuffers.ByteBuffer(new Uint8Array(0));
    const flatbufferParseScratch = {
        gameMessage:          new GP.GameMessage(),
        welcomeMessage:       new GP.WelcomeMessage(),
        initialStateMessage:  new InitialStateType(),
        deltaStateMessage:    new DeltaStateType(),
        chatMessage:          new GP.ChatMessage(),
        matchInfo:            new GP.MatchInfo(),
        playerState:          new GP.PlayerState(),
        projectileState:      new GP.ProjectileState(),
        pickup:               new GP.Pickup(),
        wall:                 new GP.Wall(),
        flagState:            new GP.FlagState(),
        killFeedEntry:        new GP.KillFeedEntry(),
        gameEvent:            new GP.GameEvent(),
        vec2:                 new GP.Vec2(),
    };

    let fastDeltaPathEnabled = true;
    let fastDeltaPathErrorCount = 0;

    // ── Coalesced packet constants ───────────────────────────────────
    const COALESCED_PACKET_MAGIC_0 = 0x4d; // M
    const COALESCED_PACKET_MAGIC_1 = 0x47; // G
    const COALESCED_PACKET_MAGIC_2 = 0x53; // S
    const COALESCED_PACKET_MAGIC_3 = 0x42; // B
    const COALESCED_PACKET_VERSION = 1;
    const COALESCED_PACKET_HEADER_BYTES = 7;
    const COALESCED_PACKET_ENTRY_HEADER_BYTES = 4;

    // ── Binary helpers ───────────────────────────────────────────────

    function toBinaryView(data) {
        if (data instanceof Uint8Array) return data;
        if (data instanceof ArrayBuffer) return new Uint8Array(data);
        if (ArrayBuffer.isView(data) && data.buffer instanceof ArrayBuffer) {
            return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
        }
        return null;
    }

    function bindFlatBufferData(data) {
        const bytes = toBinaryView(data);
        if (!bytes) return null;
        flatbufferByteBufferScratch.bytes_ = bytes;
        flatbufferByteBufferScratch.position_ = 0;
        return flatbufferByteBufferScratch;
    }

    function unpackCoalescedPackets(data) {
        const bytes = toBinaryView(data);
        if (!bytes) return null;
        if (bytes.byteLength < COALESCED_PACKET_HEADER_BYTES) return [bytes];
        if (
            bytes[0] !== COALESCED_PACKET_MAGIC_0 ||
            bytes[1] !== COALESCED_PACKET_MAGIC_1 ||
            bytes[2] !== COALESCED_PACKET_MAGIC_2 ||
            bytes[3] !== COALESCED_PACKET_MAGIC_3
        ) {
            return [bytes];
        }
        if (bytes[4] !== COALESCED_PACKET_VERSION) return [bytes];

        const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
        const packetCount = view.getUint16(5, true);
        if (packetCount <= 0) return [bytes];

        const packets = new Array(packetCount);
        let cursor = COALESCED_PACKET_HEADER_BYTES;
        for (let i = 0; i < packetCount; i += 1) {
            if (cursor + COALESCED_PACKET_ENTRY_HEADER_BYTES > bytes.byteLength) return [bytes];
            const packetLength = view.getUint32(cursor, true);
            cursor += COALESCED_PACKET_ENTRY_HEADER_BYTES;
            if (packetLength <= 0 || cursor + packetLength > bytes.byteLength) return [bytes];
            packets[i] = bytes.subarray(cursor, cursor + packetLength);
            cursor += packetLength;
        }
        if (cursor !== bytes.byteLength) return [bytes];
        return packets;
    }

    // ── State assignment helpers ─────────────────────────────────────

    function assignWallStateFromTable(target, wall) {
        target.id = wall.id();
        target.x = wall.x();
        target.y = wall.y();
        target.width = wall.width();
        target.height = wall.height();
        target.is_destructible = wall.isDestructible();
        target.current_health = wall.currentHealth();
        target.max_health = wall.maxHealth();
        return target;
    }

    function normalizePlayerDeltaMask(rawMask, forceFullState = false) {
        if (forceFullState) return PLAYER_DELTA_FULL_MASK;
        const parsedMask = Number(rawMask);
        if (!Number.isFinite(parsedMask)) return PLAYER_DELTA_FULL_MASK;
        const mask = parsedMask & PLAYER_DELTA_FULL_MASK;
        return mask === 0 ? PLAYER_DELTA_FULL_MASK : mask;
    }

    function assignPlayerStateFromTable(target, player, resolvedUsername, rawChangedMask, forceFullState = false) {
        const changedMask = normalizePlayerDeltaMask(rawChangedMask, forceFullState);
        const hasPositionDelta = (changedMask & PLAYER_DELTA_FIELD_POSITION_ROTATION) !== 0;
        const hasHealthDelta   = (changedMask & PLAYER_DELTA_FIELD_HEALTH_ALIVE) !== 0;
        const hasWeaponDelta   = (changedMask & PLAYER_DELTA_FIELD_WEAPON_AMMO) !== 0;
        const hasScoreDelta    = (changedMask & PLAYER_DELTA_FIELD_SCORE_STATS) !== 0;
        const hasPowerupDelta  = (changedMask & PLAYER_DELTA_FIELD_POWERUPS) !== 0;
        const hasShieldDelta   = (changedMask & PLAYER_DELTA_FIELD_SHIELD) !== 0;
        const hasFlagDelta     = (changedMask & PLAYER_DELTA_FIELD_FLAG) !== 0;

        target.id = player.id();
        if (hasScoreDelta || !target.username) {
            target.username = resolvedUsername || target.username || '';
        }
        if (hasPositionDelta) {
            target.x = player.x();
            target.y = player.y();
            target.rotation = player.rotation();
            target.velocity_x = player.velocityX();
            target.velocity_y = player.velocityY();
        }
        if (hasHealthDelta) {
            target.health = player.health();
            target.max_health = player.maxHealth();
            target.alive = player.alive();
            target.respawn_timer = player.respawnTimer();
        }
        if (hasWeaponDelta) {
            target.weapon = player.weapon();
            target.ammo = player.ammo();
            target.reload_progress = player.reloadProgress();
            target.weapon_swap_progress = typeof player.weaponSwapProgress === 'function'
                ? player.weaponSwapProgress()
                : (Number(target.weapon_swap_progress) || 0);
        }
        if (hasScoreDelta) {
            target.score = player.score();
            target.kills = player.kills();
            target.deaths = player.deaths();
            target.team_id = player.teamId();
        }
        if (hasPowerupDelta) {
            target.speed_boost_remaining = player.speedBoostRemaining();
            target.damage_boost_remaining = player.damageBoostRemaining();
            target.invulnerable_remaining = typeof player.invulnerableRemaining === 'function'
                ? player.invulnerableRemaining()
                : (Number(target.invulnerable_remaining) || 0);
        }
        if (hasShieldDelta) {
            target.shield_current = player.shieldCurrent();
            target.shield_max = player.shieldMax();
        }
        if (hasFlagDelta) {
            target.is_carrying_flag_team_id = player.isCarryingFlagTeamId();
        }
        target.__changed_fields = changedMask;
        return target;
    }

    function assignPlayerStateFromObject(target, player, resolvedUsername, rawChangedMask, forceFullState = false) {
        const changedMask = normalizePlayerDeltaMask(rawChangedMask, forceFullState);
        const hasPositionDelta = (changedMask & PLAYER_DELTA_FIELD_POSITION_ROTATION) !== 0;
        const hasHealthDelta   = (changedMask & PLAYER_DELTA_FIELD_HEALTH_ALIVE) !== 0;
        const hasWeaponDelta   = (changedMask & PLAYER_DELTA_FIELD_WEAPON_AMMO) !== 0;
        const hasScoreDelta    = (changedMask & PLAYER_DELTA_FIELD_SCORE_STATS) !== 0;
        const hasPowerupDelta  = (changedMask & PLAYER_DELTA_FIELD_POWERUPS) !== 0;
        const hasShieldDelta   = (changedMask & PLAYER_DELTA_FIELD_SHIELD) !== 0;
        const hasFlagDelta     = (changedMask & PLAYER_DELTA_FIELD_FLAG) !== 0;

        target.id = player.id;
        if (hasScoreDelta || !target.username) {
            target.username = resolvedUsername || target.username || '';
        }
        if (hasPositionDelta) {
            target.x = player.x;
            target.y = player.y;
            target.rotation = player.rotation;
            target.velocity_x = player.velocity_x;
            target.velocity_y = player.velocity_y;
        }
        if (hasHealthDelta) {
            target.health = player.health;
            target.max_health = player.max_health;
            target.alive = player.alive;
            target.respawn_timer = player.respawn_timer;
        }
        if (hasWeaponDelta) {
            target.weapon = player.weapon;
            target.ammo = player.ammo;
            target.reload_progress = player.reload_progress;
            target.weapon_swap_progress = player.weapon_swap_progress;
        }
        if (hasScoreDelta) {
            target.score = player.score;
            target.kills = player.kills;
            target.deaths = player.deaths;
            target.team_id = player.team_id;
        }
        if (hasPowerupDelta) {
            target.speed_boost_remaining = player.speed_boost_remaining;
            target.damage_boost_remaining = player.damage_boost_remaining;
            target.invulnerable_remaining = player.invulnerable_remaining;
        }
        if (hasShieldDelta) {
            target.shield_current = player.shield_current;
            target.shield_max = player.shield_max;
        }
        if (hasFlagDelta) {
            target.is_carrying_flag_team_id = player.is_carrying_flag_team_id;
        }
        target.__changed_fields = changedMask;
        return target;
    }

    function markProjectileServerUpdate(projectileState, receivedAtMs = performance.now()) {
        projectileState.last_server_update_ms = receivedAtMs;
        if (!Number.isFinite(projectileState.render_x)) {
            projectileState.render_x = Number(projectileState.x) || 0;
        }
        if (!Number.isFinite(projectileState.render_y)) {
            projectileState.render_y = Number(projectileState.y) || 0;
        }
        return projectileState;
    }

    function assignProjectileStateFromTable(target, projectile, receivedAtMs = performance.now()) {
        target.id = projectile.id();
        target.x = projectile.x();
        target.y = projectile.y();
        target.owner_id = typeof projectile.ownerId === 'function' ? (projectile.ownerId() || '') : '';
        target.weapon_type = projectile.weaponType();
        target.velocity_x = projectile.velocityX();
        target.velocity_y = projectile.velocityY();
        return markProjectileServerUpdate(target, receivedAtMs);
    }

    function assignPickupStateFromTable(target, pickup) {
        target.id = pickup.id();
        target.x = pickup.x();
        target.y = pickup.y();
        target.pickup_type = pickup.pickupType();
        target.weapon_type = pickup.weaponType();
        target.is_active = pickup.isActive();
        return target;
    }

    // ── Match info parsing ───────────────────────────────────────────

    function parseTeamScores(matchInfoTable) {
        if (!matchInfoTable) return [];
        const teamScoresLength = matchInfoTable.teamScoresLength();
        if (teamScoresLength <= 0) return [];
        const scores = [];
        for (let i = 0; i < teamScoresLength; i += 1) {
            const ts = matchInfoTable.teamScores(i);
            if (!ts) continue;
            scores.push({ team_id: ts.teamId(), score: ts.score() });
        }
        return scores;
    }

    function parseMatchInfo(matchInfoTable) {
        if (!matchInfoTable) return null;

        const result = {
            match_state: matchInfoTable.matchState(),
            game_mode: matchInfoTable.gameMode(),
            time_remaining: matchInfoTable.timeRemaining(),
            team_scores: parseTeamScores(matchInfoTable),
        };

        if (typeof matchInfoTable.winnerId === 'function') {
            result.winner_id = matchInfoTable.winnerId();
        }
        if (typeof matchInfoTable.winnerName === 'function') {
            result.winner_name = matchInfoTable.winnerName();
        }
        if (typeof matchInfoTable.team1CommanderId === 'function') {
            result.team1_commander_id = matchInfoTable.team1CommanderId() || '';
        }
        if (typeof matchInfoTable.team2CommanderId === 'function') {
            result.team2_commander_id = matchInfoTable.team2CommanderId() || '';
        }
        if (typeof matchInfoTable.team1CommanderWaypoint === 'function') {
            const wp = matchInfoTable.team1CommanderWaypoint();
            result.team1_commander_waypoint = wp ? { x: wp.x(), y: wp.y() } : null;
        }
        if (typeof matchInfoTable.team2CommanderWaypoint === 'function') {
            const wp = matchInfoTable.team2CommanderWaypoint();
            result.team2_commander_waypoint = wp ? { x: wp.x(), y: wp.y() } : null;
        }
        if (typeof matchInfoTable.team1CommanderAttackBias === 'function') {
            result.team1_commander_attack_bias = matchInfoTable.team1CommanderAttackBias();
        }
        if (typeof matchInfoTable.team2CommanderAttackBias === 'function') {
            result.team2_commander_attack_bias = matchInfoTable.team2CommanderAttackBias();
        }
        if (typeof matchInfoTable.matchType === 'function') {
            result.match_type = matchInfoTable.matchType();
        }
        if (typeof matchInfoTable.matchSummary === 'function') {
            const summaryTable = matchInfoTable.matchSummary();
            if (summaryTable) {
                result.match_summary = {};
                if (typeof summaryTable.totalKills === 'function') result.match_summary.total_kills = summaryTable.totalKills();
                if (typeof summaryTable.totalDamageDealt === 'function') result.match_summary.total_damage_dealt = summaryTable.totalDamageDealt();
                if (typeof summaryTable.matchDurationSec === 'function') result.match_summary.match_duration_sec = summaryTable.matchDurationSec();
                if (typeof summaryTable.mvpPlayerId === 'function') result.match_summary.mvp_player_id = summaryTable.mvpPlayerId();
                if (typeof summaryTable.mvpPlayerName === 'function') result.match_summary.mvp_player_name = summaryTable.mvpPlayerName();
                if (typeof summaryTable.mvpKills === 'function') result.match_summary.mvp_kills = summaryTable.mvpKills();
                if (typeof summaryTable.mvpDeaths === 'function') result.match_summary.mvp_deaths = summaryTable.mvpDeaths();
                if (typeof summaryTable.mvpScore === 'function') result.match_summary.mvp_score = summaryTable.mvpScore();
            }
        }

        return result;
    }

    // ── Main FlatBuffer parser ───────────────────────────────────────

    function parseFlatBufferMessage(data) {
        try {
            const buf = bindFlatBufferData(data);
            if (!buf) {
                log('Received unsupported data payload on DataChannel.', 'error');
                return null;
            }
            const gameMsg = GameProtocol.GameMessage.getRootAsGameMessage(buf, flatbufferParseScratch.gameMessage);
            const msgType = gameMsg.msgType();

            switch (msgType) {
                case GameProtocol.MessageType.Welcome: {
                    const welcome = gameMsg.actualMessage(flatbufferParseScratch.welcomeMessage);
                    if (!welcome) {
                        log('Failed to get WelcomeMessage from union', 'error');
                        return null;
                    }
                    return {
                        type: 'welcome',
                        playerId: welcome.playerId(),
                        message: welcome.message(),
                        serverTickRate: welcome.serverTickRate()
                    };
                }

                case GP.MessageType.InitialState: {
                    const initial = gameMsg.actualMessage(flatbufferParseScratch.initialStateMessage);
                    if (!initial) {
                        log(`No InitialState payload for type ${msgType}`, 'error');
                        return null;
                    }

                    const initialStateData = {
                        player_id: initial.playerId(),
                        timestamp: Number(initial.timestamp()),
                        map_name: initial.mapName(),
                        match_info: parseMatchInfo(initial.matchInfo(flatbufferParseScratch.matchInfo)),
                    };

                    const wallTable = flatbufferParseScratch.wall;
                    const wallLength = initial.wallsLength();
                    if (wallLength > 0) {
                        const rows = new Array(wallLength);
                        let writeIdx = 0;
                        for (let i = 0; i < wallLength; i += 1) {
                            const wall = initial.walls(i, wallTable);
                            if (!wall) continue;
                            rows[writeIdx++] = {
                                id: wall.id(), x: wall.x(), y: wall.y(),
                                width: wall.width(), height: wall.height(),
                                is_destructible: wall.isDestructible(),
                                current_health: wall.currentHealth(),
                                max_health: wall.maxHealth()
                            };
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; initialStateData.walls = rows; }
                    }

                    const playerTable = flatbufferParseScratch.playerState;
                    const playerLength = initial.playersLength();
                    if (playerLength > 0) {
                        const rows = new Array(playerLength);
                        let writeIdx = 0;
                        for (let i = 0; i < playerLength; i += 1) {
                            const player = initial.players(i, playerTable);
                            if (!player) continue;
                            rows[writeIdx++] = {
                                id: player.id(), username: player.username(),
                                x: player.x(), y: player.y(), rotation: player.rotation(),
                                velocity_x: player.velocityX(), velocity_y: player.velocityY(),
                                health: player.health(), max_health: player.maxHealth(),
                                alive: player.alive(), respawn_timer: player.respawnTimer(),
                                weapon: player.weapon(), ammo: player.ammo(),
                                reload_progress: player.reloadProgress(),
                                score: player.score(), kills: player.kills(), deaths: player.deaths(),
                                team_id: player.teamId(),
                                speed_boost_remaining: player.speedBoostRemaining(),
                                damage_boost_remaining: player.damageBoostRemaining(),
                                shield_current: player.shieldCurrent(), shield_max: player.shieldMax(),
                                is_carrying_flag_team_id: player.isCarryingFlagTeamId(),
                                ability_1_cooldown_remaining: typeof player.ability1CooldownRemaining === 'function'
                                    ? player.ability1CooldownRemaining()
                                    : 0,
                                ability_2_cooldown_remaining: typeof player.ability2CooldownRemaining === 'function'
                                    ? player.ability2CooldownRemaining()
                                    : 0,
                                invulnerable_remaining: typeof player.invulnerableRemaining === 'function'
                                    ? player.invulnerableRemaining()
                                    : 0,
                                secondary_weapon: typeof player.secondaryWeapon === 'function'
                                    ? player.secondaryWeapon()
                                    : player.weapon(),
                                weapon_swap_progress: typeof player.weaponSwapProgress === 'function'
                                    ? player.weaponSwapProgress()
                                    : 0,
                                current_streak: typeof player.currentStreak === 'function'
                                    ? player.currentStreak()
                                    : 0,
                                primary_weapon: typeof player.primaryWeapon === 'function'
                                    ? player.primaryWeapon()
                                    : player.weapon(),
                            };
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; initialStateData.players = rows; }
                    }

                    const projectileTable = flatbufferParseScratch.projectileState;
                    const projectileLength = initial.projectilesLength();
                    if (projectileLength > 0) {
                        const rows = new Array(projectileLength);
                        let writeIdx = 0;
                        for (let i = 0; i < projectileLength; i += 1) {
                            const projectile = initial.projectiles(i, projectileTable);
                            if (!projectile) continue;
                            rows[writeIdx++] = {
                                id: projectile.id(), x: projectile.x(), y: projectile.y(),
                                owner_id: typeof projectile.ownerId === 'function' ? (projectile.ownerId() || '') : '',
                                weapon_type: projectile.weaponType(),
                                velocity_x: projectile.velocityX(), velocity_y: projectile.velocityY()
                            };
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; initialStateData.projectiles = rows; }
                    }

                    const pickupTable = flatbufferParseScratch.pickup;
                    const pickupLength = initial.pickupsLength();
                    if (pickupLength > 0) {
                        const rows = new Array(pickupLength);
                        let writeIdx = 0;
                        for (let i = 0; i < pickupLength; i += 1) {
                            const pickup = initial.pickups(i, pickupTable);
                            if (!pickup) continue;
                            rows[writeIdx++] = {
                                id: pickup.id(), x: pickup.x(), y: pickup.y(),
                                pickup_type: pickup.pickupType(), weapon_type: pickup.weaponType(),
                                is_active: pickup.isActive()
                            };
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; initialStateData.pickups = rows; }
                    }

                    const flagTable = flatbufferParseScratch.flagState;
                    const vecTable = flatbufferParseScratch.vec2;
                    const flagLength = initial.flagStatesLength();
                    if (flagLength > 0) {
                        const rows = new Array(flagLength);
                        let writeIdx = 0;
                        for (let i = 0; i < flagLength; i += 1) {
                            const flagState = initial.flagStates(i, flagTable);
                            if (!flagState) continue;
                            const position = flagState.position(vecTable);
                            rows[writeIdx++] = {
                                team_id: flagState.teamId(), status: flagState.status(),
                                position: position ? { x: position.x(), y: position.y() } : { x: 0, y: 0 },
                                carrier_id: flagState.carrierId(), respawn_timer: flagState.respawnTimer()
                            };
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; initialStateData.flag_states = rows; }
                    }

                    const zoneLength = typeof initial.zonesLength === 'function'
                        ? initial.zonesLength()
                        : 0;
                    if (zoneLength > 0 && typeof initial.zones === 'function') {
                        const rows = new Array(zoneLength);
                        let writeIdx = 0;
                        for (let i = 0; i < zoneLength; i += 1) {
                            const zone = initial.zones(i);
                            if (!zone) continue;
                            rows[writeIdx++] = {
                                id: zone.id(), x: zone.x(), y: zone.y(),
                                width: zone.width(), height: zone.height(),
                                zone_type: zone.zoneType(), direction: zone.direction()
                            };
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; initialStateData.zones = rows; }
                    }

                    return { type: 'initial', data: initialStateData };
                }

                case GP.MessageType.DeltaState: {
                    const delta = gameMsg.actualMessage(flatbufferParseScratch.deltaStateMessage);
                    if (!delta) {
                        log(`No DeltaState payload for type ${msgType}`, 'error');
                        return null;
                    }

                    const deltaStateData = {
                        timestamp: Number(delta.timestamp()),
                        last_processed_input_sequence: delta.lastProcessedInputSequence(),
                        match_info: parseMatchInfo(delta.matchInfo(flatbufferParseScratch.matchInfo)),
                    };

                    if (DELTA_SUPPORTS_REMOVED_PLAYER_IDS) {
                        const removedPlayerLength = delta.removedPlayerIdsLength();
                        if (removedPlayerLength > 0) {
                            const rows = new Array(removedPlayerLength);
                            let writeIdx = 0;
                            for (let i = 0; i < removedPlayerLength; i += 1) {
                                const removedPlayerId = delta.removedPlayerIds(i);
                                if (!removedPlayerId) continue;
                                rows[writeIdx++] = removedPlayerId;
                            }
                            if (writeIdx > 0) { rows.length = writeIdx; deltaStateData.removed_player_ids = rows; }
                        }
                    }

                    const playerTable = flatbufferParseScratch.playerState;
                    const playerLength = delta.playersLength();
                    if (playerLength > 0) {
                        const changedPlayerFieldLength = DELTA_SUPPORTS_CHANGED_PLAYER_FIELDS
                            ? delta.changedPlayerFieldsLength() : 0;
                        const rows = new Array(playerLength);
                        let writeIdx = 0;
                        for (let i = 0; i < playerLength; i += 1) {
                            const player = delta.players(i, playerTable);
                            if (!player) continue;
                            rows[writeIdx++] = {
                                id: player.id(),
                                changed_fields: i < changedPlayerFieldLength
                                    ? delta.changedPlayerFields(i) : PLAYER_DELTA_FULL_MASK,
                                username: player.username() || '',
                                x: player.x(), y: player.y(), rotation: player.rotation(),
                                velocity_x: player.velocityX(), velocity_y: player.velocityY(),
                                health: player.health(), max_health: player.maxHealth(),
                                alive: player.alive(), respawn_timer: player.respawnTimer(),
                                weapon: player.weapon(), ammo: player.ammo(),
                                reload_progress: player.reloadProgress(),
                                score: player.score(), kills: player.kills(), deaths: player.deaths(),
                                team_id: player.teamId(),
                                speed_boost_remaining: player.speedBoostRemaining(),
                                damage_boost_remaining: player.damageBoostRemaining(),
                                shield_current: player.shieldCurrent(), shield_max: player.shieldMax(),
                                is_carrying_flag_team_id: player.isCarryingFlagTeamId(),
                                invulnerable_remaining: typeof player.invulnerableRemaining === 'function'
                                    ? player.invulnerableRemaining()
                                    : 0,
                                weapon_swap_progress: typeof player.weaponSwapProgress === 'function'
                                    ? player.weaponSwapProgress()
                                    : 0,
                            };
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; deltaStateData.players = rows; }
                    }

                    const projectileTable = flatbufferParseScratch.projectileState;
                    const projectileLength = delta.projectilesLength();
                    if (projectileLength > 0) {
                        const rows = new Array(projectileLength);
                        let writeIdx = 0;
                        for (let i = 0; i < projectileLength; i += 1) {
                            const projectile = delta.projectiles(i, projectileTable);
                            if (!projectile) continue;
                            rows[writeIdx++] = {
                                id: projectile.id(), x: projectile.x(), y: projectile.y(),
                                owner_id: typeof projectile.ownerId === 'function' ? (projectile.ownerId() || '') : '',
                                weapon_type: projectile.weaponType(),
                                velocity_x: projectile.velocityX(), velocity_y: projectile.velocityY()
                            };
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; deltaStateData.projectiles = rows; }
                    }

                    const removedProjectileLength = delta.removedProjectilesLength();
                    if (removedProjectileLength > 0) {
                        const rows = new Array(removedProjectileLength);
                        let writeIdx = 0;
                        for (let i = 0; i < removedProjectileLength; i += 1) {
                            const removedProjectileId = delta.removedProjectiles(i);
                            if (!removedProjectileId) continue;
                            rows[writeIdx++] = removedProjectileId;
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; deltaStateData.removed_projectiles = rows; }
                    }

                    const pickupTable = flatbufferParseScratch.pickup;
                    const pickupLength = delta.pickupsLength();
                    if (pickupLength > 0) {
                        const rows = new Array(pickupLength);
                        let writeIdx = 0;
                        for (let i = 0; i < pickupLength; i += 1) {
                            const pickup = delta.pickups(i, pickupTable);
                            if (!pickup) continue;
                            rows[writeIdx++] = {
                                id: pickup.id(), x: pickup.x(), y: pickup.y(),
                                pickup_type: pickup.pickupType(), weapon_type: pickup.weaponType(),
                                is_active: pickup.isActive()
                            };
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; deltaStateData.pickups = rows; }
                    }

                    const destroyedWallLength = delta.destroyedWallIdsLength();
                    if (destroyedWallLength > 0) {
                        const rows = new Array(destroyedWallLength);
                        let writeIdx = 0;
                        for (let i = 0; i < destroyedWallLength; i += 1) {
                            const wallId = delta.destroyedWallIds(i);
                            if (!wallId) continue;
                            rows[writeIdx++] = wallId;
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; deltaStateData.destroyed_wall_ids = rows; }
                    }

                    const deactivatedPickupLength = delta.deactivatedPickupIdsLength();
                    if (deactivatedPickupLength > 0) {
                        const rows = new Array(deactivatedPickupLength);
                        let writeIdx = 0;
                        for (let i = 0; i < deactivatedPickupLength; i += 1) {
                            const pickupId = delta.deactivatedPickupIds(i);
                            if (!pickupId) continue;
                            rows[writeIdx++] = pickupId;
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; deltaStateData.deactivated_pickup_ids = rows; }
                    }

                    const killFeedEntry = flatbufferParseScratch.killFeedEntry;
                    const vecTable = flatbufferParseScratch.vec2;
                    const killFeedLength = delta.killFeedLength();
                    if (killFeedLength > 0) {
                        const rows = new Array(killFeedLength);
                        let writeIdx = 0;
                        for (let i = 0; i < killFeedLength; i += 1) {
                            const kf = delta.killFeed(i, killFeedEntry);
                            if (!kf) continue;
                            const killerPosition = kf.killerPosition(vecTable);
                            const victimPosition = kf.victimPosition(vecTable);
                            rows[writeIdx++] = {
                                killer_id: typeof kf.killerId === 'function' ? kf.killerId() : null,
                                victim_id: typeof kf.victimId === 'function' ? kf.victimId() : null,
                                killer_name: kf.killerName(), victim_name: kf.victimName(),
                                weapon: kf.weapon(), timestamp: kf.timestamp(),
                                killer_position: killerPosition ? { x: killerPosition.x(), y: killerPosition.y() } : null,
                                victim_position: victimPosition ? { x: victimPosition.x(), y: victimPosition.y() } : null,
                                is_headshot: kf.isHeadshot(),
                                kill_context: typeof kf.killContext === 'function' ? kf.killContext() : 0,
                            };
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; deltaStateData.kill_feed = rows; }
                    }

                    const flagTable = flatbufferParseScratch.flagState;
                    const flagLength = delta.flagStatesLength();
                    if (flagLength > 0) {
                        const rows = new Array(flagLength);
                        let writeIdx = 0;
                        for (let i = 0; i < flagLength; i += 1) {
                            const flagState = delta.flagStates(i, flagTable);
                            if (!flagState) continue;
                            const position = flagState.position(vecTable);
                            rows[writeIdx++] = {
                                team_id: flagState.teamId(), status: flagState.status(),
                                position: position ? { x: position.x(), y: position.y() } : { x: 0, y: 0 },
                                carrier_id: flagState.carrierId(), respawn_timer: flagState.respawnTimer()
                            };
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; deltaStateData.flag_states = rows; }
                    }

                    const gameEventTable = flatbufferParseScratch.gameEvent;
                    const gameEventLength = delta.gameEventsLength();
                    if (gameEventLength > 0) {
                        const rows = new Array(gameEventLength);
                        let writeIdx = 0;
                        for (let i = 0; i < gameEventLength; i += 1) {
                            const gameEvent = delta.gameEvents(i, gameEventTable);
                            if (!gameEvent) continue;
                            const position = gameEvent.position(vecTable);
                            rows[writeIdx++] = {
                                event_type: gameEvent.eventType(),
                                position: position ? { x: position.x(), y: position.y() } : { x: 0, y: 0 },
                                instigator_id: gameEvent.instigatorId(), target_id: gameEvent.targetId(),
                                weapon_type: gameEvent.weaponType(), value: gameEvent.value(),
                                falloff_multiplier: typeof gameEvent.falloffMultiplier === 'function'
                                    ? gameEvent.falloffMultiplier()
                                    : 1.0,
                            };
                        }
                        if (writeIdx > 0) { rows.length = writeIdx; deltaStateData.game_events = rows; }
                    }

                    if (DELTA_SUPPORTS_UPDATED_WALLS) {
                        const updatedWallLength = delta.updatedWallsLength();
                        if (updatedWallLength > 0) {
                            if (isWallDebugEnabled()) {
                                logWallDebug(`[DELTA DEBUG] updated wall count: ${updatedWallLength}`, 'info');
                            }
                            const wallTable = flatbufferParseScratch.wall;
                            const rows = new Array(updatedWallLength);
                            let writeIdx = 0;
                            for (let i = 0; i < updatedWallLength; i += 1) {
                                const wall = delta.updatedWalls(i, wallTable);
                                if (!wall) continue;
                                rows[writeIdx++] = {
                                    id: wall.id(), x: wall.x(), y: wall.y(),
                                    width: wall.width(), height: wall.height(),
                                    is_destructible: wall.isDestructible(),
                                    current_health: wall.currentHealth(), max_health: wall.maxHealth()
                                };
                            }
                            if (writeIdx > 0) { rows.length = writeIdx; deltaStateData.updated_walls = rows; }
                        }
                    }

                    if (DELTA_SUPPORTS_FULL_WALLS) {
                        const wallLength = delta.wallsLength();
                        if (wallLength > 0) {
                            if (isWallDebugEnabled()) {
                                logWallDebug(`[DELTA DEBUG] full wall list count: ${wallLength}`, 'info');
                            }
                            const wallTable = flatbufferParseScratch.wall;
                            const rows = new Array(wallLength);
                            let writeIdx = 0;
                            for (let i = 0; i < wallLength; i += 1) {
                                const wall = delta.walls(i, wallTable);
                                if (!wall) continue;
                                rows[writeIdx++] = {
                                    id: wall.id(), x: wall.x(), y: wall.y(),
                                    width: wall.width(), height: wall.height(),
                                    is_destructible: wall.isDestructible(),
                                    current_health: wall.currentHealth(), max_health: wall.maxHealth()
                                };
                            }
                            if (writeIdx > 0) { rows.length = writeIdx; deltaStateData.walls = rows; }
                        }
                    }

                    if (isWallDebugEnabled()) {
                        const debugInfo = {
                            hasPlayers: !!(deltaStateData.players && deltaStateData.players.length > 0),
                            playerCount: deltaStateData.players ? deltaStateData.players.length : 0,
                            hasDestroyedWalls: !!(deltaStateData.destroyed_wall_ids && deltaStateData.destroyed_wall_ids.length > 0),
                            destroyedWallCount: deltaStateData.destroyed_wall_ids ? deltaStateData.destroyed_wall_ids.length : 0,
                            hasUpdatedWalls: !!(deltaStateData.updated_walls && deltaStateData.updated_walls.length > 0),
                            updatedWallCount: deltaStateData.updated_walls ? deltaStateData.updated_walls.length : 0,
                            hasFullWallsList: !!(deltaStateData.walls && deltaStateData.walls.length > 0),
                            fullWallsCount: deltaStateData.walls ? deltaStateData.walls.length : 0
                        };
                        if (debugInfo.hasDestroyedWalls || debugInfo.hasUpdatedWalls || debugInfo.hasFullWallsList) {
                            logWallDebug(`[DELTA DEBUG] Wall data summary: ${JSON.stringify(debugInfo)}`, 'info');
                        }
                    }

                    return { type: 'delta', data: deltaStateData };
                }

                case GP.MessageType.Chat: {
                    const chat = gameMsg.actualMessage(flatbufferParseScratch.chatMessage);
                    if (!chat) {
                        log(`No Chat payload for type ${msgType}`, 'error');
                        return null;
                    }
                    return {
                        type: 'chat',
                        data: {
                            seq: Number(chat.seq()),
                            player_id: chat.playerId(),
                            username: chat.username(),
                            message: chat.message(),
                            timestamp: Number(chat.timestamp())
                        }
                    };
                }

                case GP.MessageType.MatchUpdate: {
                    const matchUpdateMsg = gameMsg.actualMessage(flatbufferParseScratch.matchInfo);
                    if (!matchUpdateMsg) {
                        log(`No MatchInfo payload for type MatchUpdate`, 'error');
                        return null;
                    }
                    return {
                        type: 'match_update',
                        data: parseMatchInfo(matchUpdateMsg)
                    };
                }

                default:
                    log(`Received unknown or unhandled message type: ${msgType}`, 'error');
                    return null;
            }
        } catch (e) {
            console.error('Error parsing FlatBuffer:', e, data);
            log(`Error parsing FlatBuffer: ${e.message}`, 'error');
            return null;
        }
    }

    // ── Message construction ─────────────────────────────────────────

    function createInputMessage(currentInputState) {
        const builder = new flatbuffers.Builder(128);
        GP.PlayerInput.startPlayerInput(builder);
        GP.PlayerInput.addTimestamp(builder, BigInt(Date.now()));
        GP.PlayerInput.addSequence(builder, currentInputState.sequence);
        GP.PlayerInput.addMoveForward(builder, currentInputState.move_forward);
        GP.PlayerInput.addMoveBackward(builder, currentInputState.move_backward);
        GP.PlayerInput.addMoveLeft(builder, currentInputState.move_left);
        GP.PlayerInput.addMoveRight(builder, currentInputState.move_right);
        GP.PlayerInput.addShooting(builder, currentInputState.shooting);
        GP.PlayerInput.addReload(builder, currentInputState.reload);
        GP.PlayerInput.addRotation(builder, currentInputState.rotation);
        GP.PlayerInput.addMeleeAttack(builder, currentInputState.melee_attack);
        GP.PlayerInput.addChangeWeaponSlot(builder, currentInputState.change_weapon_slot);
        GP.PlayerInput.addUseAbilitySlot(builder, currentInputState.use_ability_slot);
        if (typeof GP.PlayerInput.addPingX === 'function') {
            GP.PlayerInput.addPingX(builder, Number(currentInputState.ping_x) || 0);
        }
        if (typeof GP.PlayerInput.addPingY === 'function') {
            GP.PlayerInput.addPingY(builder, Number(currentInputState.ping_y) || 0);
        }
        const playerInputOffset = GP.PlayerInput.endPlayerInput(builder);

        GP.GameMessage.startGameMessage(builder);
        GP.GameMessage.addMsgType(builder, GP.MessageType.Input);
        GP.GameMessage.addActualMessageType(builder, GP.MessagePayload.PlayerInput);
        GP.GameMessage.addActualMessage(builder, playerInputOffset);
        const gameMessageOffset = GP.GameMessage.endGameMessage(builder);
        builder.finish(gameMessageOffset);
        return builder.asUint8Array();
    }

    function createChatMessage(text, myPlayerId, localPlayerState) {
        const builder = new flatbuffers.Builder(256);
        const messageStr = builder.createString(text);
        const playerIdStr = builder.createString(myPlayerId || 'unknown');
        const usernameStr = builder.createString(localPlayerState?.username || 'Player');

        GP.ChatMessage.startChatMessage(builder);
        GP.ChatMessage.addSeq(builder, BigInt(0));
        GP.ChatMessage.addPlayerId(builder, playerIdStr);
        GP.ChatMessage.addUsername(builder, usernameStr);
        GP.ChatMessage.addMessage(builder, messageStr);
        GP.ChatMessage.addTimestamp(builder, BigInt(Date.now()));
        const chatMessageOffset = GP.ChatMessage.endChatMessage(builder);

        GP.GameMessage.startGameMessage(builder);
        GP.GameMessage.addMsgType(builder, GP.MessageType.Chat);
        GP.GameMessage.addActualMessageType(builder, GP.MessagePayload.ChatMessage);
        GP.GameMessage.addActualMessage(builder, chatMessageOffset);
        const gameMessageOffset = GP.GameMessage.endGameMessage(builder);
        builder.finish(gameMessageOffset);
        return builder.asUint8Array();
    }

    return {
        // Constants
        PLAYER_DELTA_FULL_MASK,
        PLAYER_DELTA_FIELD_POSITION_ROTATION,
        PLAYER_DELTA_FIELD_HEALTH_ALIVE,
        PLAYER_DELTA_FIELD_WEAPON_AMMO,
        PLAYER_DELTA_FIELD_SCORE_STATS,
        PLAYER_DELTA_FIELD_POWERUPS,
        PLAYER_DELTA_FIELD_SHIELD,
        PLAYER_DELTA_FIELD_FLAG,
        DELTA_SUPPORTS_REMOVED_PLAYER_IDS,
        DELTA_SUPPORTS_CHANGED_PLAYER_FIELDS,
        DELTA_SUPPORTS_UPDATED_WALLS,
        DELTA_SUPPORTS_FULL_WALLS,

        // Scratch references (needed by fast-delta path in client.html)
        flatbufferByteBufferScratch,
        flatbufferParseScratch,

        // Getter/setter for fast delta toggle
        get fastDeltaPathEnabled() { return fastDeltaPathEnabled; },
        set fastDeltaPathEnabled(v) { fastDeltaPathEnabled = v; },
        get fastDeltaPathErrorCount() { return fastDeltaPathErrorCount; },
        set fastDeltaPathErrorCount(v) { fastDeltaPathErrorCount = v; },

        // Functions
        toBinaryView,
        bindFlatBufferData,
        unpackCoalescedPackets,
        assignWallStateFromTable,
        normalizePlayerDeltaMask,
        assignPlayerStateFromTable,
        assignPlayerStateFromObject,
        markProjectileServerUpdate,
        assignProjectileStateFromTable,
        assignPickupStateFromTable,
        parseTeamScores,
        parseMatchInfo,
        parseFlatBufferMessage,
        createInputMessage,
        createChatMessage,
    };
}
