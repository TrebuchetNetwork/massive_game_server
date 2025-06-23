/**
 * NetworkIndicator - Shows network quality/ping status with visual indicators
 * Dependencies: PIXI.js
 */

export class NetworkIndicator {
    constructor() {
        this.app = new PIXI.Application({
            width: 80,
            height: 20,
            backgroundAlpha: 0
        });
        
        this.container = new PIXI.Container();
        this.app.stage.addChild(this.container);
        
        // Background
        const bg = new PIXI.Graphics();
        bg.beginFill(0x1F2937, 0.8);
        bg.drawRoundedRect(0, 0, 80, 20, 5);
        bg.endFill();
        this.container.addChild(bg);
        
        // Ping text
        this.pingText = new PIXI.Text('0ms', {
            fontSize: 11,
            fill: 0xE5E7EB,
            fontFamily: 'monospace'
        });
        this.pingText.anchor.set(0, 0.5);
        this.pingText.position.set(30, 10);
        this.container.addChild(this.pingText);
        
        // Connection bars
        this.bars = [];
        for (let i = 0; i < 4; i++) {
            const bar = new PIXI.Graphics();
            bar.x = 5 + i * 5;
            bar.y = 15;
            this.bars.push(bar);
            this.container.addChild(bar);
        }
        
        // Status dot
        this.statusDot = new PIXI.Graphics();
        this.statusDot.position.set(70, 10);
        this.container.addChild(this.statusDot);
    }

    update(currentPing) {
        // Update ping text
        this.pingText.text = Math.round(currentPing) + 'ms';
        
        // Determine connection quality
        let quality = 4;
        let color = 0x00FF00;
        let statusColor = 0x00FF00;
        
        if (currentPing < 50) {
            quality = 4;
            color = 0x00FF00; // Green
            this.pingText.style.fill = 0x00FF00;
        } else if (currentPing < 100) {
            quality = 3;
            color = 0xFFFF00; // Yellow
            this.pingText.style.fill = 0xFFFF00;
        } else if (currentPing < 150) {
            quality = 2;
            color = 0xFF6600; // Orange
            this.pingText.style.fill = 0xFF6600;
        } else {
            quality = 1;
            color = 0xFF0000; // Red
            this.pingText.style.fill = 0xFF0000;
            statusColor = 0xFF0000;
        }

        // Update bars with gradient effect
        this.bars.forEach((bar, index) => {
            bar.clear();
            const height = (index + 1) * 3 + 3;
            const active = index < quality;
            
            if (active) {
                // Active bar with gradient
                bar.beginFill(color, 0.9);
                bar.drawRect(0, -height, 3, height);
                bar.endFill();
                
                // Highlight
                bar.beginFill(0xFFFFFF, 0.3);
                bar.drawRect(0, -height, 1, height);
                bar.endFill();
            } else {
                // Inactive bar
                bar.beginFill(0x374151, 0.5);
                bar.drawRect(0, -height, 3, height);
                bar.endFill();
            }
        });
        
        // Update status dot with pulse
        this.statusDot.clear();
        const pulse = Math.sin(Date.now() * 0.005) * 0.2 + 0.8;
        this.statusDot.beginFill(statusColor, pulse);
        this.statusDot.drawCircle(0, 0, 3);
        this.statusDot.endFill();
    }
}
