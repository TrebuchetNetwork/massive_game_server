/**
 * AimingSystem.js - Aiming crosshair and trajectory rendering
 *
 * Extracts drawLiteAimingSystem and drawAimingSystem from client.html.
 * Uses getCtx callback pattern to access shared game state.
 */

export function createAimingSystem(getCtx) {

    function drawLiteAimingSystem(playerX, playerY, aimX, aimY, currentWeapon, crosshairColor, distance) {
        const ctx = getCtx();
        const { aimingGraphics, trajectoryGraphics, sniperRangeText, GP } = ctx;

        const crosshairRadius = currentWeapon === GP.WeaponType.Sniper ? 11 : 8;
        aimingGraphics.lineStyle(2, crosshairColor, 0.9);
        aimingGraphics.drawCircle(aimX, aimY, crosshairRadius);

        aimingGraphics.lineStyle(1.5, crosshairColor, 0.9);
        aimingGraphics.moveTo(aimX - crosshairRadius, aimY);
        aimingGraphics.lineTo(aimX + crosshairRadius, aimY);
        aimingGraphics.moveTo(aimX, aimY - crosshairRadius);
        aimingGraphics.lineTo(aimX, aimY + crosshairRadius);

        trajectoryGraphics.lineStyle(1, crosshairColor, 0.24);
        trajectoryGraphics.moveTo(playerX, playerY);
        trajectoryGraphics.lineTo(aimX, aimY);

        if (currentWeapon === GP.WeaponType.Sniper) {
            if (sniperRangeText) {
                sniperRangeText.text = Math.round(distance) + 'm';
                sniperRangeText.style.fill = crosshairColor;
                sniperRangeText.position.set(aimX + 20, aimY - 22);
                sniperRangeText.visible = true;
            }
        } else if (sniperRangeText) {
            sniperRangeText.visible = false;
        }
        aimingGraphics.position.set(0, 0);
    }

    function drawAimingSystem() {
        const ctx = getCtx();
        const {
            localPlayerState, mouseWorldPos, sniperRangeText, aimingGraphics,
            trajectoryGraphics, GP, weaponColors, weaponVelocities,
            ultraPerformanceMode, STABLE_MODE_FORCED, LOW_OVERHEAD_MODE,
            players, AIMING_LITE_PLAYER_THRESHOLD, smoothedFrameMs,
            TARGET_FRAME_MS_60FPS,
        } = ctx;

        if (!localPlayerState || !localPlayerState.alive || !mouseWorldPos) {
            if (sniperRangeText) sniperRangeText.visible = false;
            return;
        }

        aimingGraphics.clear();
        trajectoryGraphics.clear();

        const playerX = localPlayerState.render_x || localPlayerState.x;
        const playerY = localPlayerState.render_y || localPlayerState.y;
        const currentWeapon = localPlayerState.weapon;

        // Skip for melee weapons
        if (currentWeapon === GP.WeaponType.Melee) {
            if (sniperRangeText) sniperRangeText.visible = false;
            aimingGraphics.position.set(0, 0);
            return;
        }

        const dx = mouseWorldPos.x - playerX;
        const dy = mouseWorldPos.y - playerY;
        const distance = Math.sqrt(dx * dx + dy * dy);
        const crosshairColor = weaponColors[currentWeapon] || 0xFFFFFF;
        const useLiteAiming =
            ultraPerformanceMode ||
            STABLE_MODE_FORCED ||
            LOW_OVERHEAD_MODE ||
            players.size >= AIMING_LITE_PLAYER_THRESHOLD ||
            smoothedFrameMs > TARGET_FRAME_MS_60FPS;
        if (useLiteAiming) {
            drawLiteAimingSystem(
                playerX,
                playerY,
                mouseWorldPos.x,
                mouseWorldPos.y,
                currentWeapon,
                crosshairColor,
                distance
            );
            return;
        }

        // Get weapon velocity
        const weaponVel = weaponVelocities[currentWeapon] || 800;

        // Calculate player velocity effect on projectile
        const playerVelX = localPlayerState.velocity_x || 0;
        const playerVelY = localPlayerState.velocity_y || 0;

        // Calculate aim direction
        const aimAngle = Math.atan2(dy, dx);

        // Draw crosshair at mouse position
        const crosshairSize = 15;
        const crosshairThickness = 2;
        const crosshairGap = 5;

        // Outer circle
        aimingGraphics.lineStyle(crosshairThickness, crosshairColor, 0.8);
        aimingGraphics.drawCircle(mouseWorldPos.x, mouseWorldPos.y, crosshairSize);

        // Crosshair lines with gap
        aimingGraphics.lineStyle(crosshairThickness, crosshairColor, 1);
        // Top line
        aimingGraphics.moveTo(mouseWorldPos.x, mouseWorldPos.y - crosshairSize);
        aimingGraphics.lineTo(mouseWorldPos.x, mouseWorldPos.y - crosshairGap);
        // Bottom line
        aimingGraphics.moveTo(mouseWorldPos.x, mouseWorldPos.y + crosshairGap);
        aimingGraphics.lineTo(mouseWorldPos.x, mouseWorldPos.y + crosshairSize);
        // Left line
        aimingGraphics.moveTo(mouseWorldPos.x - crosshairSize, mouseWorldPos.y);
        aimingGraphics.lineTo(mouseWorldPos.x - crosshairGap, mouseWorldPos.y);
        // Right line
        aimingGraphics.moveTo(mouseWorldPos.x + crosshairGap, mouseWorldPos.y);
        aimingGraphics.lineTo(mouseWorldPos.x + crosshairSize, mouseWorldPos.y);

        // Draw weapon-specific aim indicators
        if (currentWeapon === GP.WeaponType.Shotgun) {
            // Shotgun spread cone
            const spreadAngle = Math.PI / 8; // 22.5 degrees spread
            const coneLength = Math.min(200, distance);

            trajectoryGraphics.lineStyle(1, crosshairColor, 0.3);
            trajectoryGraphics.moveTo(playerX, playerY);
            trajectoryGraphics.lineTo(
                playerX + Math.cos(aimAngle - spreadAngle/2) * coneLength,
                playerY + Math.sin(aimAngle - spreadAngle/2) * coneLength
            );
            trajectoryGraphics.moveTo(playerX, playerY);
            trajectoryGraphics.lineTo(
                playerX + Math.cos(aimAngle + spreadAngle/2) * coneLength,
                playerY + Math.sin(aimAngle + spreadAngle/2) * coneLength
            );

            // Arc at the end of cone
            trajectoryGraphics.arc(
                playerX, playerY,
                coneLength,
                aimAngle - spreadAngle/2,
                aimAngle + spreadAngle/2,
                false
            );
        }

        // Calculate trajectory with player movement compensation
        const projectileVelX = Math.cos(aimAngle) * weaponVel + playerVelX * 0.5;
        const projectileVelY = Math.sin(aimAngle) * weaponVel + playerVelY * 0.5;

        // Draw trajectory prediction line
        const trajectorySteps = 20;
        const timeStep = 0.05; // 50ms per step
        let prevX = playerX;
        let prevY = playerY;

        trajectoryGraphics.lineStyle(2, crosshairColor, 0.5);

        for (let i = 1; i <= trajectorySteps; i++) {
            const t = i * timeStep;
            const trajX = playerX + projectileVelX * t;
            const trajY = playerY + projectileVelY * t;

            // Draw trajectory segment
            if (i === 1) {
                trajectoryGraphics.moveTo(prevX, prevY);
            }

            // Fade out trajectory over distance
            const alpha = 0.5 * (1 - (i / trajectorySteps));
            trajectoryGraphics.lineStyle(2 - (i / trajectorySteps), crosshairColor, alpha);
            trajectoryGraphics.lineTo(trajX, trajY);

            // Draw dots along trajectory for better visibility
            if (i % 2 === 0) {
                trajectoryGraphics.beginFill(crosshairColor, alpha);
                trajectoryGraphics.drawCircle(trajX, trajY, 2);
                trajectoryGraphics.endFill();
            }

            prevX = trajX;
            prevY = trajY;

            // Stop if trajectory goes too far
            const trajDistance = Math.sqrt(
                (trajX - playerX) * (trajX - playerX) +
                (trajY - playerY) * (trajY - playerY)
            );
            if (trajDistance > 800) break;
        }

        // Calculate and show predicted impact point
        const projectileTime = distance / weaponVel;
        const impactX = playerX + projectileVelX * projectileTime;
        const impactY = playerY + projectileVelY * projectileTime;

        // Draw impact prediction marker
        trajectoryGraphics.lineStyle(3, crosshairColor, 0.8);
        trajectoryGraphics.drawCircle(impactX, impactY, 8);
        trajectoryGraphics.beginFill(crosshairColor, 0.4);
        trajectoryGraphics.drawCircle(impactX, impactY, 5);
        trajectoryGraphics.endFill();

        // Draw movement compensation indicator if player is moving
        const playerSpeed = Math.sqrt(playerVelX * playerVelX + playerVelY * playerVelY);
        if (playerSpeed > 10) {
            // Show how movement affects aim
            const moveCompX = playerVelX * projectileTime * 0.5;
            const moveCompY = playerVelY * projectileTime * 0.5;

            trajectoryGraphics.lineStyle(1, 0x00FF00, 0.5);
            trajectoryGraphics.moveTo(mouseWorldPos.x, mouseWorldPos.y);
            trajectoryGraphics.lineTo(mouseWorldPos.x + moveCompX, mouseWorldPos.y + moveCompY);

            // Arrow head
            const arrowAngle = Math.atan2(moveCompY, moveCompX);
            const arrowSize = 5;
            trajectoryGraphics.beginFill(0x00FF00, 0.5);
            trajectoryGraphics.moveTo(
                mouseWorldPos.x + moveCompX + Math.cos(arrowAngle) * arrowSize,
                mouseWorldPos.y + moveCompY + Math.sin(arrowAngle) * arrowSize
            );
            trajectoryGraphics.lineTo(
                mouseWorldPos.x + moveCompX + Math.cos(arrowAngle - 2.5) * arrowSize,
                mouseWorldPos.y + moveCompY + Math.sin(arrowAngle - 2.5) * arrowSize
            );
            trajectoryGraphics.lineTo(
                mouseWorldPos.x + moveCompX + Math.cos(arrowAngle + 2.5) * arrowSize,
                mouseWorldPos.y + moveCompY + Math.sin(arrowAngle + 2.5) * arrowSize
            );
            trajectoryGraphics.closePath();
            trajectoryGraphics.endFill();
        }

        // Weapon-specific indicators
        if (currentWeapon === GP.WeaponType.Sniper) {
            // Sniper scope lines
            const scopeSize = 30;
            aimingGraphics.lineStyle(1, crosshairColor, 0.6);
            // Horizontal line
            aimingGraphics.moveTo(mouseWorldPos.x - scopeSize, mouseWorldPos.y);
            aimingGraphics.lineTo(mouseWorldPos.x - crosshairGap, mouseWorldPos.y);
            aimingGraphics.moveTo(mouseWorldPos.x + crosshairGap, mouseWorldPos.y);
            aimingGraphics.lineTo(mouseWorldPos.x + scopeSize, mouseWorldPos.y);
            // Vertical line
            aimingGraphics.moveTo(mouseWorldPos.x, mouseWorldPos.y - scopeSize);
            aimingGraphics.lineTo(mouseWorldPos.x, mouseWorldPos.y - crosshairGap);
            aimingGraphics.moveTo(mouseWorldPos.x, mouseWorldPos.y + crosshairGap);
            aimingGraphics.lineTo(mouseWorldPos.x, mouseWorldPos.y + scopeSize);

            // Range finder
            if (sniperRangeText) {
                sniperRangeText.text = Math.round(distance) + 'm';
                sniperRangeText.style.fill = crosshairColor;
                sniperRangeText.position.set(mouseWorldPos.x + 25, mouseWorldPos.y - 25);
                sniperRangeText.visible = true;
            }
        } else if (sniperRangeText) {
            sniperRangeText.visible = false;
        }

        // Draw aim line from player to crosshair
        trajectoryGraphics.lineStyle(1, crosshairColor, 0.2);
        trajectoryGraphics.moveTo(playerX, playerY);
        trajectoryGraphics.lineTo(mouseWorldPos.x, mouseWorldPos.y);

        // Add breathing effect for sniper
        if (currentWeapon === GP.WeaponType.Sniper) {
            const breathAmount = Math.sin(Date.now() * 0.003) * 2;
            aimingGraphics.position.x = breathAmount;
            aimingGraphics.position.y = Math.cos(Date.now() * 0.003) * 1;
        } else {
            aimingGraphics.position.set(0, 0);
        }
    }

    return {
        drawLiteAimingSystem,
        drawAimingSystem,
    };
}
