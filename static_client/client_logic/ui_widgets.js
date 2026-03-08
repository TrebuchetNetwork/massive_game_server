const PIXI = globalThis.PIXI;

function currentNowMs(nowProvider) {
    try {
        const value = nowProvider();
        return Number.isFinite(value) ? value : Date.now();
    } catch (_) {
        return Date.now();
    }
}

export class Minimap {
    constructor(options = {}) {
        const {
            width = 150,
            height = 150,
            mapScale = 0.05,
            teamColors = {},
            defaultEnemyColor = 0xF87171,
            gameProtocol = null,
            interpolateColor = (start) => start,
            nowProvider = () => Date.now(),
        } = options;

        this.teamColors = teamColors;
        this.defaultEnemyColor = defaultEnemyColor;
        this.gameProtocol = gameProtocol;
        this.interpolateColor = interpolateColor;
        this.nowProvider = nowProvider;

        this.app = new PIXI.Application({
            width,
            height,
            backgroundColor: 0x0F172A,
            antialias: false,
            resolution: 1,
        });
        this.width = width;
        this.height = height;
        this.mapScale = mapScale;
        this.wallsNeedUpdate = true;
        this.objectivesNeedUpdate = true;
        this.performanceMode = "normal";
        this.maxPlayersRendered = 220;
        this.drawViewCone = true;
        this.drawDirectionLines = true;
        this.drawLocalPulse = true;
        this.enemyVisibilityRadius = 520;
        this.enemyMemoryMs = 3000;
        this.enemyLastSeen = new Map();

        this.backgroundGraphics = new PIXI.Graphics();
        this.gridGraphics = new PIXI.Graphics();
        this.wallsGraphics = new PIXI.Graphics();
        this.objectivesContainer = new PIXI.Container();
        this.contextObjectivesGraphics = new PIXI.Graphics();
        this.objectivesGraphics = new PIXI.Graphics();
        this.playersContainer = new PIXI.Container();
        this.playersGraphics = new PIXI.Graphics();
        this.pingsGraphics = new PIXI.Graphics();
        this.overlayGraphics = new PIXI.Graphics();
        this.objectivesContainer.addChild(this.contextObjectivesGraphics);
        this.objectivesContainer.addChild(this.objectivesGraphics);
        this.playersContainer.addChild(this.playersGraphics);

        this.app.stage.addChild(this.backgroundGraphics);
        this.app.stage.addChild(this.gridGraphics);
        this.app.stage.addChild(this.wallsGraphics);
        this.app.stage.addChild(this.objectivesContainer);
        this.app.stage.addChild(this.playersContainer);
        this.app.stage.addChild(this.pingsGraphics);
        this.app.stage.addChild(this.overlayGraphics);

        this.drawBackground();
        this.drawGrid();
        this.drawBorder();
    }

    drawBackground() {
        this.backgroundGraphics.beginFill(0x0F172A);
        this.backgroundGraphics.drawRect(0, 0, this.width, this.height);
        this.backgroundGraphics.endFill();

        const vignette = new PIXI.Graphics();
        vignette.beginFill(0x000000, 0.3);
        vignette.drawCircle(this.width / 2, this.height / 2, this.width * 0.7);
        vignette.endFill();
        vignette.filters = [new PIXI.BlurFilter(20)];
        this.backgroundGraphics.addChild(vignette);
    }

    drawGrid() {
        this.gridGraphics.lineStyle(0.5, 0x1E293B, 0.5);
        const gridSize = 30;

        for (let x = gridSize; x < this.width; x += gridSize) {
            this.gridGraphics.moveTo(x, 0);
            this.gridGraphics.lineTo(x, this.height);
        }

        for (let y = gridSize; y < this.height; y += gridSize) {
            this.gridGraphics.moveTo(0, y);
            this.gridGraphics.lineTo(this.width, y);
        }
    }

    drawBorder() {
        this.overlayGraphics.lineStyle(2, 0x334155, 0.8);
        this.overlayGraphics.drawRoundedRect(1, 1, this.width - 2, this.height - 2, 5);
    }

    setPerformanceMode(mode = "normal") {
        if (this.performanceMode === mode) return;
        this.performanceMode = mode;
        if (mode === "ultra") {
            this.maxPlayersRendered = 42;
            this.drawViewCone = false;
            this.drawDirectionLines = false;
            this.drawLocalPulse = false;
            return;
        }
        if (mode === "dense") {
            this.maxPlayersRendered = 80;
            this.drawViewCone = false;
            this.drawDirectionLines = false;
            this.drawLocalPulse = false;
            return;
        }
        this.maxPlayersRendered = 220;
        this.drawViewCone = true;
        this.drawDirectionLines = true;
        this.drawLocalPulse = true;
    }

    clear() {
        this.wallsGraphics.clear();
        this.playersGraphics.clear();
        this.contextObjectivesGraphics.clear();
        this.objectivesGraphics.clear();
        this.pingsGraphics.clear();
        this.wallsNeedUpdate = true;
        this.objectivesNeedUpdate = true;
        this.enemyLastSeen.clear();
    }

    destroy() {
        this.clear();
        if (this.app?.view?.parentNode) {
            this.app.view.parentNode.removeChild(this.app.view);
        }
        if (this.app && !this.app.destroyed) {
            this.app.destroy(true, { children: true, texture: true, baseTexture: true });
        }
        this.app = null;
    }

    update(
        localPlayerData,
        allPlayersMap,
        allWallsArray,
        allFlagsArray,
        activePings = [],
        contextualObjectives = null
    ) {
        if (!localPlayerData) return;

        if (this.wallsNeedUpdate && allWallsArray.length > 0) {
            this.drawWalls(allWallsArray);
            this.wallsNeedUpdate = false;
        }
        if (this.objectivesNeedUpdate && allFlagsArray && allFlagsArray.length > 0) {
            this.drawObjectives(allFlagsArray);
            this.objectivesNeedUpdate = false;
        }
        this.drawSupplementalObjectives(contextualObjectives);

        const playersGraphics = this.playersGraphics;
        playersGraphics.clear();
        const nowMs = currentNowMs(this.nowProvider);
        const localTeamId = Number(localPlayerData.team_id) || 0;
        const visibilityRadiusSq = this.enemyVisibilityRadius * this.enemyVisibilityRadius;

        const localX = localPlayerData.x * this.mapScale + this.width / 2;
        const localY = localPlayerData.y * this.mapScale + this.height / 2;
        if (this.drawViewCone) {
            playersGraphics.beginFill(0x00FF00, 0.1);
            playersGraphics.moveTo(localX, localY);
            const viewAngle = Math.PI / 3;
            const viewDistance = 50;
            for (let angle = -viewAngle / 2; angle <= viewAngle / 2; angle += viewAngle / 10) {
                const x = localX + Math.cos(localPlayerData.rotation + angle) * viewDistance;
                const y = localY + Math.sin(localPlayerData.rotation + angle) * viewDistance;
                playersGraphics.lineTo(x, y);
            }
            playersGraphics.lineTo(localX, localY);
            playersGraphics.endFill();
        }

        const stride =
            allPlayersMap.size > this.maxPlayersRendered
                ? Math.ceil(allPlayersMap.size / this.maxPlayersRendered)
                : 1;
        let sampledCount = 0;
        allPlayersMap.forEach((player) => {
            if (!player.alive) return;
            const isLocalPlayer = player.id === localPlayerData.id;
            if (!isLocalPlayer) {
                sampledCount += 1;
                if (sampledCount % stride !== 0) return;
            }

            let color = this.teamColors[player.team_id] || this.defaultEnemyColor;
            let shape = "circle";
            let alpha = 0.9;
            let renderX = Number(player.x);
            let renderY = Number(player.y);
            let renderRotation = Number(player.rotation) || 0;
            let renderAsLastKnown = false;

            if (isLocalPlayer) {
                color = 0x00FF00;
                shape = "triangle";
            } else if (localTeamId !== 0 && player.team_id === localTeamId) {
                color = this.teamColors[player.team_id] || 0x60A5FA;
            } else {
                const dx = Number(player.x) - Number(localPlayerData.x);
                const dy = Number(player.y) - Number(localPlayerData.y);
                const withinVision =
                    Number.isFinite(dx) &&
                    Number.isFinite(dy) &&
                    ((dx * dx) + (dy * dy)) <= visibilityRadiusSq;
                const enemyKey = String(player.id || "");
                if (withinVision) {
                    this.enemyLastSeen.set(enemyKey, {
                        x: Number(player.x),
                        y: Number(player.y),
                        rotation: Number(player.rotation) || 0,
                        seenAtMs: nowMs,
                    });
                } else {
                    const lastSeen = this.enemyLastSeen.get(enemyKey);
                    if (!lastSeen || (nowMs - Number(lastSeen.seenAtMs || 0)) > this.enemyMemoryMs) {
                        this.enemyLastSeen.delete(enemyKey);
                        return;
                    }
                    renderX = Number(lastSeen.x);
                    renderY = Number(lastSeen.y);
                    renderRotation = Number(lastSeen.rotation) || 0;
                    const fade = Math.max(0.16, 1 - ((nowMs - Number(lastSeen.seenAtMs || 0)) / this.enemyMemoryMs));
                    alpha = 0.16 + fade * 0.34;
                    color = 0xFCA5A5;
                    renderAsLastKnown = true;
                }
            }

            let dotX = renderX * this.mapScale + this.width / 2;
            let dotY = renderY * this.mapScale + this.height / 2;
            dotX = Math.max(3, Math.min(this.width - 3, dotX));
            dotY = Math.max(3, Math.min(this.height - 3, dotY));

            if (shape === "triangle") {
                const direction = renderRotation + Math.PI / 2;
                const tipLength = 4;
                const wingLength = 3;
                const wingOffset = 2.6;

                const tipX = dotX + Math.cos(direction) * tipLength;
                const tipY = dotY + Math.sin(direction) * tipLength;
                const leftX = dotX + Math.cos(direction + wingOffset) * wingLength;
                const leftY = dotY + Math.sin(direction + wingOffset) * wingLength;
                const rightX = dotX + Math.cos(direction - wingOffset) * wingLength;
                const rightY = dotY + Math.sin(direction - wingOffset) * wingLength;

                playersGraphics.beginFill(color, alpha);
                playersGraphics.moveTo(tipX, tipY);
                playersGraphics.lineTo(leftX, leftY);
                playersGraphics.lineTo(rightX, rightY);
                playersGraphics.lineTo(tipX, tipY);
                playersGraphics.endFill();
            } else {
                playersGraphics.beginFill(color, alpha);
                playersGraphics.drawCircle(dotX, dotY, renderAsLastKnown ? 2.25 : 3);
                playersGraphics.endFill();

                if (renderAsLastKnown && this.performanceMode === "normal") {
                    playersGraphics.lineStyle(1, color, Math.max(0.12, alpha * 0.8));
                    playersGraphics.drawCircle(dotX, dotY, 5.2);
                } else if (this.drawDirectionLines) {
                    const otherPlayerRotation = renderRotation + Math.PI / 2;
                    playersGraphics.lineStyle(1, color, alpha * 0.66);
                    playersGraphics.moveTo(dotX, dotY);
                    playersGraphics.lineTo(
                        dotX + Math.cos(otherPlayerRotation) * 5,
                        dotY + Math.sin(otherPlayerRotation) * 5
                    );
                }
            }

            if (isLocalPlayer && this.drawLocalPulse) {
                playersGraphics.lineStyle(1, 0x00FF00, 0.5);
                playersGraphics.drawCircle(dotX, dotY, 8 + Math.sin(nowMs * 0.003) * 2);
            }
        });

        this.pingsGraphics.clear();
        if (Array.isArray(activePings) && activePings.length > 0) {
            const pulse = Math.sin(nowMs * 0.01) * 0.16 + 0.84;
            for (let i = 0; i < activePings.length; i += 1) {
                const pingEntry = activePings[i];
                if (!pingEntry || !Number.isFinite(pingEntry.x) || !Number.isFinite(pingEntry.y)) {
                    continue;
                }
                let dotX = (pingEntry.x - localPlayerData.x) * this.mapScale + this.width / 2;
                let dotY = (pingEntry.y - localPlayerData.y) * this.mapScale + this.height / 2;
                if (dotX < -10 || dotX > this.width + 10 || dotY < -10 || dotY > this.height + 10) {
                    continue;
                }
                dotX = Math.max(4, Math.min(this.width - 4, dotX));
                dotY = Math.max(4, Math.min(this.height - 4, dotY));

                const color =
                    pingEntry.kind === "enemy"
                        ? 0xF87171
                        : pingEntry.kind === "defend"
                            ? 0xFBBF24
                            : 0x34D399;
                const rawStrength = Number(pingEntry.strength);
                let strength = Number.isFinite(rawStrength) ? rawStrength : 1;
                if (this.performanceMode === "dense") {
                    strength = Math.min(strength, 1.35);
                } else if (this.performanceMode === "ultra") {
                    strength = Math.min(strength, 1.2);
                }
                strength = Math.max(0.8, Math.min(2.1, strength));
                const radius = (5 + pulse * 2.8) * (0.9 + strength * 0.2);
                const ringAlpha = Math.min(0.98, 0.68 + strength * 0.14) * pulse;
                const coreAlpha = Math.min(0.92, 0.62 + strength * 0.16) * pulse;
                this.pingsGraphics.lineStyle(2 + (strength - 1) * 0.6, color, ringAlpha);
                this.pingsGraphics.drawCircle(dotX, dotY, radius);
                this.pingsGraphics.beginFill(color, coreAlpha);
                this.pingsGraphics.drawCircle(dotX, dotY, 2.4 + (strength - 1) * 0.9);
                this.pingsGraphics.endFill();
                if (strength >= 1.45 && this.performanceMode === "normal") {
                    this.pingsGraphics.lineStyle(1.2, color, Math.max(0.2, ringAlpha * 0.6));
                    this.pingsGraphics.drawCircle(dotX, dotY, radius + 3.6 + pulse * 2.1);
                }
            }
        }
    }

    drawWalls(allWallsArray) {
        this.wallsGraphics.clear();

        this.wallsGraphics.beginFill(0x000000, 0.2);
        allWallsArray.forEach((wall) => {
            if (wall.is_destructible && wall.current_health <= 0) return;
            const x = wall.x * this.mapScale + this.width / 2 + 1;
            const y = wall.y * this.mapScale + this.height / 2 + 1;
            const w = wall.width * this.mapScale;
            const h = wall.height * this.mapScale;
            this.wallsGraphics.drawRect(x, y, w, h);
        });
        this.wallsGraphics.endFill();

        allWallsArray.forEach((wall) => {
            if (wall.is_destructible && wall.current_health <= 0) return;

            const x = wall.x * this.mapScale + this.width / 2;
            const y = wall.y * this.mapScale + this.height / 2;
            const w = wall.width * this.mapScale;
            const h = wall.height * this.mapScale;

            if (wall.is_destructible) {
                const healthPercent = wall.current_health / wall.max_health;
                const color = this.interpolateColor(0xBF616A, 0x4A5568, healthPercent);
                this.wallsGraphics.beginFill(color, 0.7);
            } else {
                this.wallsGraphics.beginFill(0x4A5568, 0.8);
            }

            this.wallsGraphics.drawRect(x, y, w, h);
        });
        this.wallsGraphics.endFill();
    }

    drawObjectives(allFlagsArray) {
        this.objectivesGraphics.clear();
        if (!allFlagsArray) return;
        const nowMs = currentNowMs(this.nowProvider);
        const drawGlow = this.performanceMode === "normal";
        const carriedStatus = this.gameProtocol?.FlagStatus?.Carried;

        allFlagsArray.forEach((flag) => {
            if (carriedStatus !== undefined && flag.status === carriedStatus) return;
            const color = this.teamColors[flag.team_id] || 0xFFFFFF;
            let dotX = flag.position.x * this.mapScale + this.width / 2;
            let dotY = flag.position.y * this.mapScale + this.height / 2;
            dotX = Math.max(5, Math.min(this.width - 5, dotX));
            dotY = Math.max(5, Math.min(this.height - 5, dotY));

            if (drawGlow) {
                const glowSize = 8 + Math.sin(nowMs * 0.004) * 2;
                this.objectivesGraphics.beginFill(color, 0.3);
                this.objectivesGraphics.drawCircle(dotX, dotY, glowSize);
                this.objectivesGraphics.endFill();
            }

            this.objectivesGraphics.beginFill(color);
            this.objectivesGraphics.drawRect(dotX - 2, dotY - 3, 4, 6);
            this.objectivesGraphics.endFill();
        });
    }

    toArray(collection) {
        if (!collection) return [];
        if (Array.isArray(collection)) return collection;
        if (typeof collection.values === "function") return Array.from(collection.values());
        return [];
    }

    drawSupplementalObjectives(contextualObjectives) {
        this.contextObjectivesGraphics.clear();
        if (!contextualObjectives) return;

        const zoneType = this.gameProtocol?.ZoneType || {};
        const pickupType = this.gameProtocol?.PickupType || {};
        const zoneRows = this.toArray(contextualObjectives.zones);
        const pickupRows = this.toArray(contextualObjectives.pickups);
        const zoneLimit = this.performanceMode === "normal" ? 32 : 16;
        const pickupLimit = this.performanceMode === "normal" ? 48 : 20;

        let zonesDrawn = 0;
        for (let i = 0; i < zoneRows.length && zonesDrawn < zoneLimit; i += 1) {
            const zone = zoneRows[i];
            if (!zone) continue;
            const zx = Number(zone.x) * this.mapScale + this.width / 2;
            const zy = Number(zone.y) * this.mapScale + this.height / 2;
            const zw = Math.max(1.0, Number(zone.width) * this.mapScale);
            const zh = Math.max(1.0, Number(zone.height) * this.mapScale);
            if (
                !Number.isFinite(zx) || !Number.isFinite(zy) ||
                !Number.isFinite(zw) || !Number.isFinite(zh)
            ) {
                continue;
            }

            const zt = Number(zone.zone_type);
            let color = 0xFBBF24;
            if (zt === zoneType.DamageZone || zt === 1) color = 0xEF4444;
            if (zt === zoneType.BoostPad || zt === 2) color = 0x22D3EE;

            this.contextObjectivesGraphics.lineStyle(1, color, 0.5);
            this.contextObjectivesGraphics.beginFill(color, 0.08);
            this.contextObjectivesGraphics.drawRect(zx, zy, zw, zh);
            this.contextObjectivesGraphics.endFill();
            zonesDrawn += 1;
        }

        let pickupsDrawn = 0;
        for (let i = 0; i < pickupRows.length && pickupsDrawn < pickupLimit; i += 1) {
            const pickup = pickupRows[i];
            if (!pickup || pickup.is_active === false) continue;
            let px = Number(pickup.x) * this.mapScale + this.width / 2;
            let py = Number(pickup.y) * this.mapScale + this.height / 2;
            if (!Number.isFinite(px) || !Number.isFinite(py)) continue;
            if (px < -6 || px > this.width + 6 || py < -6 || py > this.height + 6) continue;
            px = Math.max(3, Math.min(this.width - 3, px));
            py = Math.max(3, Math.min(this.height - 3, py));

            const pt = Number(pickup.pickup_type);
            let color = 0xF59E0B;
            if (pt === pickupType.Health || pt === 0) color = 0x4ADE80;
            if (pt === pickupType.Ammo || pt === 1) color = 0xFCD34D;
            if (pt === pickupType.WeaponCrate || pt === 2) color = 0x93C5FD;
            if (pt === pickupType.SpeedBoost || pt === 3) color = 0x38BDF8;
            if (pt === pickupType.DamageBoost || pt === 4) color = 0xFB7185;
            if (pt === pickupType.Shield || pt === 5) color = 0xA78BFA;

            this.contextObjectivesGraphics.beginFill(color, 0.58);
            this.contextObjectivesGraphics.drawCircle(px, py, 1.8);
            this.contextObjectivesGraphics.endFill();
            pickupsDrawn += 1;
        }
    }
}

export class NetworkIndicator {
    constructor(options = {}) {
        const { nowProvider = () => Date.now() } = options;
        this.nowProvider = nowProvider;
        this.app = new PIXI.Application({
            width: 80,
            height: 20,
            backgroundAlpha: 0,
        });

        this.container = new PIXI.Container();
        this.app.stage.addChild(this.container);

        const bg = new PIXI.Graphics();
        bg.beginFill(0x1F2937, 0.8);
        bg.drawRoundedRect(0, 0, 80, 20, 5);
        bg.endFill();
        this.container.addChild(bg);

        this.pingText = new PIXI.Text("0ms", {
            fontSize: 11,
            fill: 0xE5E7EB,
            fontFamily: "monospace",
        });
        this.pingText.anchor.set(0, 0.5);
        this.pingText.position.set(30, 10);
        this.container.addChild(this.pingText);

        this.bars = [];
        for (let i = 0; i < 4; i += 1) {
            const bar = new PIXI.Graphics();
            bar.x = 5 + i * 5;
            bar.y = 15;
            this.bars.push(bar);
            this.container.addChild(bar);
        }

        this.statusDot = new PIXI.Graphics();
        this.statusDot.position.set(70, 10);
        this.container.addChild(this.statusDot);
    }

    update(currentPing) {
        this.pingText.text = `${Math.round(currentPing)}ms`;

        let quality = 4;
        let color = 0x00FF00;
        let statusColor = 0x00FF00;

        if (currentPing < 50) {
            quality = 4;
            color = 0x00FF00;
            this.pingText.style.fill = 0x00FF00;
        } else if (currentPing < 100) {
            quality = 3;
            color = 0xFFFF00;
            this.pingText.style.fill = 0xFFFF00;
        } else if (currentPing < 150) {
            quality = 2;
            color = 0xFF6600;
            this.pingText.style.fill = 0xFF6600;
        } else {
            quality = 1;
            color = 0xFF0000;
            this.pingText.style.fill = 0xFF0000;
            statusColor = 0xFF0000;
        }

        this.bars.forEach((bar, index) => {
            bar.clear();
            const height = (index + 1) * 3 + 3;
            const active = index < quality;

            if (active) {
                bar.beginFill(color, 0.9);
                bar.drawRect(0, -height, 3, height);
                bar.endFill();

                bar.beginFill(0xFFFFFF, 0.3);
                bar.drawRect(0, -height, 1, height);
                bar.endFill();
            } else {
                bar.beginFill(0x374151, 0.5);
                bar.drawRect(0, -height, 3, height);
                bar.endFill();
            }
        });

        this.statusDot.clear();
        const pulse = Math.sin(currentNowMs(this.nowProvider) * 0.005) * 0.2 + 0.8;
        this.statusDot.beginFill(statusColor, pulse);
        this.statusDot.drawCircle(0, 0, 3);
        this.statusDot.endFill();
    }

    destroy() {
        if (this.app?.view?.parentNode) {
            this.app.view.parentNode.removeChild(this.app.view);
        }
        if (this.app && !this.app.destroyed) {
            this.app.destroy(true, { children: true, texture: true, baseTexture: true });
        }
        this.app = null;
    }
}
