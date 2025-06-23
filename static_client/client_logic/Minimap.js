/**
 * Minimap - Displays a top-down miniaturized view of the game world
 * Dependencies: PIXI.js, GameProtocol
 */

export class Minimap {
    constructor(width = 150, height = 150, mapScale = 0.05) {
        this.app = new PIXI.Application({
            width,
            height,
            backgroundColor: 0x0F172A,
            antialias: true,
            resolution: 1
        });
        this.width = width;
        this.height = height;
        this.mapScale = mapScale;
        this.wallsNeedUpdate = true;
        this.objectivesNeedUpdate = true;

        // Create layered structure
        this.backgroundGraphics = new PIXI.Graphics();
        this.gridGraphics = new PIXI.Graphics();
        this.wallsGraphics = new PIXI.Graphics();
        this.objectivesContainer = new PIXI.Container();
        this.playersContainer = new PIXI.Container();
        this.overlayGraphics = new PIXI.Graphics();

        this.app.stage.addChild(this.backgroundGraphics);
        this.app.stage.addChild(this.gridGraphics);
        this.app.stage.addChild(this.wallsGraphics);
        this.app.stage.addChild(this.objectivesContainer);
        this.app.stage.addChild(this.playersContainer);
        this.app.stage.addChild(this.overlayGraphics);

        this.drawBackground();
        this.drawGrid();
        this.drawBorder();
    }

    drawBackground() {
        // Gradient background
        this.backgroundGraphics.beginFill(0x0F172A);
        this.backgroundGraphics.drawRect(0, 0, this.width, this.height);
        this.backgroundGraphics.endFill();
        
        // Subtle vignette effect
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

    clear() {
        this.wallsGraphics.clear();
        this.playersContainer.removeChildren();
        this.objectivesContainer.removeChildren();
        this.wallsNeedUpdate = true;
        this.objectivesNeedUpdate = true;
    }

    update(localPlayerData, allPlayersMap, allWallsArray, allFlagsArray) {
        if (!localPlayerData) return; // Don't update if local player data is missing

        if (this.wallsNeedUpdate && allWallsArray.length > 0) {
            this.drawWalls(allWallsArray);
            this.wallsNeedUpdate = false;
        }
        if (this.objectivesNeedUpdate && allFlagsArray && allFlagsArray.length > 0) {
            this.drawObjectives(allFlagsArray);
            this.objectivesNeedUpdate = false;
        }

        this.playersContainer.removeChildren();
        
        // Draw view cone for local player
        const localX = (localPlayerData.x * this.mapScale) + this.width / 2;
        const localY = (localPlayerData.y * this.mapScale) + this.height / 2;
        
        const viewCone = new PIXI.Graphics();
        viewCone.beginFill(0x00FF00, 0.1);
        viewCone.moveTo(0, 0);
        const viewAngle = Math.PI / 3; // 60 degree view cone
        const viewDistance = 50;
        for (let angle = -viewAngle/2; angle <= viewAngle/2; angle += viewAngle/10) {
            const x = Math.cos(localPlayerData.rotation + angle) * viewDistance;
            const y = Math.sin(localPlayerData.rotation + angle) * viewDistance;
            viewCone.lineTo(x, y);
        }
        viewCone.endFill();
        viewCone.position.set(localX, localY);
        this.playersContainer.addChild(viewCone);
        
        // Team colors from main client
        const teamColors = {
            0: 0xA0A0A0, // Neutral/FFA
            1: 0xFF6B6B, // Red
            2: 0x4ECDC4, // Blue
        };
        const defaultEnemyColor = 0xF87171;
        
        // Draw players
        allPlayersMap.forEach(player => {
            if (!player.alive) return;
            
            const dot = new PIXI.Graphics();
            let color = teamColors[player.team_id] || defaultEnemyColor;
            let shape = 'circle';
            
            if (player.id === localPlayerData.id) {
                color = 0x00FF00; // Bright green for local player
                shape = 'triangle';
            } else if (localPlayerData.team_id !== 0 && player.team_id === localPlayerData.team_id) {
                color = teamColors[player.team_id] || 0x60A5FA; // Team color for teammates
            }

            dot.beginFill(color, 0.9);
            
            if (shape === 'triangle') {
                // Draw directional arrow for local player
                const arrowPoints = [ 0, -4, 3, 3, 0, 1, -3, 3 ];
                dot.drawPolygon(arrowPoints);
                dot.rotation = player.rotation + (Math.PI / 2); // Adjust if sprite faces up
            } else {
                dot.drawCircle(0, 0, 3);
                // Add small direction indicator for other players
                const otherPlayerRotation = player.rotation + (Math.PI / 2); // Adjust if sprites face up
                dot.lineStyle(1, color, 0.6);
                dot.moveTo(0, 0);
                dot.lineTo(Math.cos(otherPlayerRotation) * 5, Math.sin(otherPlayerRotation) * 5);
            }
            
            dot.endFill();
            
            dot.x = (player.x * this.mapScale) + this.width / 2;
            dot.y = (player.y * this.mapScale) + this.height / 2;
            // Clamp to minimap bounds
            dot.x = Math.max(3, Math.min(this.width - 3, dot.x));
            dot.y = Math.max(3, Math.min(this.height - 3, dot.y));
            
            // Add pulse effect for local player
            if (player.id === localPlayerData.id) {
                const pulse = new PIXI.Graphics();
                pulse.lineStyle(1, 0x00FF00, 0.5);
                pulse.drawCircle(0, 0, 8 + Math.sin(Date.now() * 0.003) * 2);
                pulse.position.copyFrom(dot.position);
                this.playersContainer.addChildAt(pulse, 0); // Draw pulse behind the dot
            }
            
            this.playersContainer.addChild(dot);
        });
    }

    drawWalls(allWallsArray) {
        this.wallsGraphics.clear();
        
        // Draw wall shadows first
        this.wallsGraphics.beginFill(0x000000, 0.2);
        allWallsArray.forEach(wall => {
            if (wall.is_destructible && wall.current_health <= 0) return;
            const x = (wall.x * this.mapScale) + this.width / 2 + 1;
            const y = (wall.y * this.mapScale) + this.height / 2 + 1;
            const w = wall.width * this.mapScale;
            const h = wall.height * this.mapScale;
            this.wallsGraphics.drawRect(x, y, w, h);
        });
        this.wallsGraphics.endFill();
        
        // Draw walls
        allWallsArray.forEach(wall => {
            if (wall.is_destructible && wall.current_health <= 0) return;
            
            const x = (wall.x * this.mapScale) + this.width / 2;
            const y = (wall.y * this.mapScale) + this.height / 2;
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
        this.objectivesContainer.removeChildren();
        if (!allFlagsArray) return;
        
        const GP = window.GP;
        const teamColors = {
            0: 0xA0A0A0, // Neutral/FFA
            1: 0xFF6B6B, // Red
            2: 0x4ECDC4, // Blue
        };
        
        allFlagsArray.forEach(flag => {
            if (flag.status === GP.FlagStatus.Carried) return; // Don't draw carried flags on minimap directly
            
            const flagDot = new PIXI.Graphics();
            const color = teamColors[flag.team_id] || 0xFFFFFF;
            
            // Pulsing glow
            const glowSize = 8 + Math.sin(Date.now() * 0.004) * 2;
            flagDot.beginFill(color, 0.3);
            flagDot.drawCircle(0, 0, glowSize);
            flagDot.endFill();
            
            // Flag icon (simple rectangle for minimap)
            flagDot.beginFill(color);
            flagDot.drawRect(-2, -3, 4, 6); // Small rectangle for flag
            flagDot.endFill();
            
            flagDot.x = (flag.position.x * this.mapScale) + this.width / 2;
            flagDot.y = (flag.position.y * this.mapScale) + this.height / 2;
            // Clamp to minimap bounds
            flagDot.x = Math.max(5, Math.min(this.width - 5, flagDot.x));
            flagDot.y = Math.max(5, Math.min(this.height - 5, flagDot.y));
            
            this.objectivesContainer.addChild(flagDot);
        });
    }

    // Helper function to interpolate colors
    interpolateColor(color1, color2, factor) {
        const c1 = PIXI.Color.shared.setValue(color1).toRgbArray();
        const c2 = PIXI.Color.shared.setValue(color2).toRgbArray();
        const r = Math.floor(c1[0] * 255 * (1 - factor) + c2[0] * 255 * factor);
        const g = Math.floor(c1[1] * 255 * (1 - factor) + c2[1] * 255 * factor);
        const b = Math.floor(c1[2] * 255 * (1 - factor) + c2[2] * 255 * factor);
        return (r << 16) | (g << 8) | b;
    }
}
