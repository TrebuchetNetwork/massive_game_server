/**
 * Space Shooter Game - HUD Controller
 * Manages all HUD updates and animations
 */

class HUDController {
  static instance = null;
  
  static init() {
    if (!HUDController.instance) {
      HUDController.instance = new HUDController();
    }
    return HUDController.instance;
  }
  
  constructor() {
    this.elements = {};
    this.minimapCanvas = null;
    this.minimapCtx = null;
    this.animationFrame = null;
    
    this.cacheElements();
    this.initMinimap();
    this.bindGameEvents();
    this.startUpdateLoop();
  }
  
  cacheElements() {
    // Health & Shield
    this.elements.healthBar = document.querySelector('.health-bar .bar-fill');
    this.elements.healthValue = document.querySelector('.health-bar .bar-value');
    this.elements.shieldBar = document.querySelector('.shield-bar .bar-fill');
    this.elements.shieldValue = document.querySelector('.shield-bar .bar-value');
    
    // Ammo
    this.elements.ammoCurrent = document.querySelector('.ammo-current');
    this.elements.ammoMax = document.querySelector('.ammo-max');
    
    // Credits
    this.elements.credits = document.querySelector('.credits-value');
    
    // Timer
    this.elements.timer = document.querySelector('.timer-value');
    this.elements.timerPhase = document.querySelector('.timer-phase');
    
    // Score
    this.elements.scoreBlue = document.querySelector('.score-team.blue .score-value');
    this.elements.scoreRed = document.querySelector('.score-team.red .score-value');
    
    // Kill Feed
    this.elements.killFeed = document.getElementById('kill-feed');
    
    // Notifications
    this.elements.notifications = document.getElementById('notifications');
    
    // Abilities
    this.elements.abilities = document.querySelectorAll('.ability-slot');
    
    // Crosshair
    this.elements.crosshair = document.getElementById('crosshair');
    
    console.log('[HUD] Elements cached');
  }
  
  initMinimap() {
    this.minimapCanvas = document.getElementById('minimap');
    if (!this.minimapCanvas) return;
    
    this.minimapCtx = this.minimapCanvas.getContext('2d');
    
    // Set canvas size
    const size = this.minimapCanvas.offsetWidth;
    this.minimapCanvas.width = size;
    this.minimapCanvas.height = size;
    
    // Initial render
    this.renderMinimap();
  }
  
  bindGameEvents() {
    // Player state changes
    document.addEventListener('player-health-change', (e) => this.updateHealth(e.detail));
    document.addEventListener('player-shield-change', (e) => this.updateShield(e.detail));
    document.addEventListener('player-ammo-change', (e) => this.updateAmmo(e.detail));
    document.addEventListener('player-credits-change', (e) => this.updateCredits(e.detail));
    
    // Match state
    document.addEventListener('match-timer-update', (e) => this.updateTimer(e.detail));
    document.addEventListener('score-update', (e) => this.updateScore(e.detail));
    document.addEventListener('phase-update', (e) => this.updatePhase(e.detail));
    
    // Combat events
    document.addEventListener('kill-event', (e) => this.addKillFeed(e.detail));
    document.addEventListener('hit-marker', (e) => this.showHitMarker(e.detail));
    document.addEventListener('damage-dealt', (e) => this.showDamageNumber(e.detail));
    
    // Ability events
    document.addEventListener('ability-cooldown', (e) => this.updateAbilityCooldown(e.detail));
    document.addEventListener('ultimate-charge', (e) => this.updateUltimateCharge(e.detail));
    
    // Minimap
    document.addEventListener('minimap-update', (e) => this.updateMinimap(e.detail));
    
    console.log('[HUD] Game events bound');
  }
  
  startUpdateLoop() {
    const update = () => {
      this.updateAnimations();
      this.animationFrame = requestAnimationFrame(update);
    };
    update();
  }
  
  // Health Update
  updateHealth({ current, max }) {
    if (!this.elements.healthBar || !this.elements.healthValue) return;
    
    const percentage = Math.max(0, Math.min(100, (current / max) * 100));
    
    this.elements.healthBar.style.width = `${percentage}%`;
    this.elements.healthValue.textContent = `${Math.floor(current)}/${max}`;
    
    // Visual feedback for low health
    this.elements.healthBar.classList.remove('low', 'critical');
    
    if (percentage <= 20) {
      this.elements.healthBar.classList.add('critical');
      this.shakeScreen();
    } else if (percentage <= 40) {
      this.elements.healthBar.classList.add('low');
    }
  }
  
  // Shield Update
  updateShield({ current, max }) {
    if (!this.elements.shieldBar || !this.elements.shieldValue) return;
    
    const percentage = Math.max(0, Math.min(100, (current / max) * 100));
    
    this.elements.shieldBar.style.width = `${percentage}%`;
    this.elements.shieldValue.textContent = `${Math.floor(current)}/${max}`;
  }
  
  // Ammo Update
  updateAmmo({ current, max, reserve }) {
    if (!this.elements.ammoCurrent || !this.elements.ammoMax) return;
    
    this.elements.ammoCurrent.textContent = current;
    this.elements.ammoMax.textContent = max;
    
    // Visual feedback
    this.elements.ammoCurrent.classList.remove('low', 'empty');
    
    if (current === 0) {
      this.elements.ammoCurrent.classList.add('empty');
    } else if (current <= max * 0.2) {
      this.elements.ammoCurrent.classList.add('low');
    }
    
    // Update reserve if element exists
    const reserveEl = document.querySelector('.ammo-reserve');
    if (reserveEl && reserve !== undefined) {
      reserveEl.textContent = `(${reserve})`;
    }
  }
  
  // Credits Update
  updateCredits(amount) {
    if (!this.elements.credits) return;
    
    // Animate number change
    const current = parseInt(this.elements.credits.textContent.replace(/,/g, '')) || 0;
    this.animateNumber(this.elements.credits, current, amount, 500);
  }
  
  // Timer Update
  updateTimer(seconds) {
    if (!this.elements.timer) return;
    
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    
    this.elements.timer.textContent = 
      `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
    
    // Visual feedback for low time
    this.elements.timer.classList.remove('warning', 'danger');
    
    if (seconds <= 10) {
      this.elements.timer.classList.add('danger');
    } else if (seconds <= 30) {
      this.elements.timer.classList.add('warning');
    }
  }
  
  // Phase Update
  updatePhase({ current, total }) {
    if (!this.elements.timerPhase) return;
    
    this.elements.timerPhase.textContent = `ROUND ${current}/${total}`;
  }
  
  // Score Update
  updateScore({ blue, red }) {
    if (this.elements.scoreBlue) {
      this.elements.scoreBlue.textContent = blue;
    }
    if (this.elements.scoreRed) {
      this.elements.scoreRed.textContent = red;
    }
  }
  
  // Kill Feed
  addKillFeed({ killer, victim, weapon, isHeadshot = false, assists = [] }) {
    if (!this.elements.killFeed) return;
    
    const entry = document.createElement('div');
    entry.className = `kill-entry ${isHeadshot ? 'headshot' : ''}`;
    const appendSpan = (className, value) => {
      const span = document.createElement('span');
      span.className = className;
      span.textContent = String(value ?? '');
      entry.appendChild(span);
    };

    appendSpan('killer', killer);
    appendSpan('weapon-icon', weapon);
    const assistCount = Array.isArray(assists) ? assists.length : 0;
    if (assistCount > 0) {
      appendSpan('assist-indicator', `+${assistCount}`);
    }
    appendSpan('victim', victim);
    
    this.elements.killFeed.appendChild(entry);
    
    // Remove after delay
    setTimeout(() => {
      entry.style.opacity = '0';
      entry.style.transform = 'translateX(20px)';
      setTimeout(() => entry.remove(), 300);
    }, 5000);
    
    // Keep only last 5 entries
    while (this.elements.killFeed.children.length > 5) {
      this.elements.killFeed.firstChild.remove();
    }
  }
  
  // Damage Number
  showDamageNumber({ damage, x, y, isCritical = false }) {
    const container = document.getElementById('damage-numbers');
    if (!container) return;
    
    const number = document.createElement('div');
    number.className = `damage-number ${isCritical ? 'critical' : ''}`;
    number.textContent = Math.floor(damage);
    
    // Random offset for visual variety
    const offsetX = (Math.random() - 0.5) * 40;
    const offsetY = (Math.random() - 0.5) * 20;
    
    number.style.left = `${x + offsetX}px`;
    number.style.top = `${y + offsetY}px`;
    
    container.appendChild(number);
    
    setTimeout(() => number.remove(), 1000);
  }
  
  // Hit Marker
  showHitMarker({ isHeadshot = false } = {}) {
    const hitMarker = document.getElementById('hit-marker');
    if (!hitMarker) return;
    
    hitMarker.classList.remove('active');
    void hitMarker.offsetWidth; // Force reflow
    hitMarker.classList.add('active');
    
    if (isHeadshot) {
      hitMarker.classList.add('headshot');
      setTimeout(() => hitMarker.classList.remove('headshot'), 200);
    }
  }
  
  // Ability Cooldown
  updateAbilityCooldown({ key, cooldown, maxCooldown }) {
    const slot = document.querySelector(`.ability-slot[data-key="${key}"]`);
    if (!slot) return;
    
    const percentage = (cooldown / maxCooldown) * 100;
    const cooldownEl = slot.querySelector('.ability-cooldown');
    const textEl = slot.querySelector('.ability-cooldown-text');
    
    if (cooldownEl) {
      cooldownEl.style.setProperty('--cooldown', `${percentage}`);
    }
    
    if (textEl) {
      textEl.textContent = cooldown > 0 ? Math.ceil(cooldown / 1000) : '';
    }
    
    slot.classList.toggle('active', cooldown <= 0);
  }
  
  // Ultimate Charge
  updateUltimateCharge({ charge, maxCharge }) {
    const ultimate = document.querySelector('.ability-slot.ultimate');
    if (!ultimate) return;
    
    const percentage = (charge / maxCharge) * 100;
    const fillEl = ultimate.querySelector('.charge-fill');
    
    if (fillEl) {
      fillEl.style.width = `${percentage}%`;
    }
    
    ultimate.classList.toggle('ready', percentage >= 100);
  }
  
  // Minimap
  renderMinimap() {
    if (!this.minimapCtx) return;
    
    const ctx = this.minimapCtx;
    const size = this.minimapCanvas.width;
    const center = size / 2;
    
    // Clear
    ctx.clearRect(0, 0, size, size);
    
    // Draw background grid
    ctx.strokeStyle = 'rgba(0, 212, 255, 0.1)';
    ctx.lineWidth = 1;
    
    const gridSize = 20;
    for (let i = 0; i < size; i += gridSize) {
      ctx.beginPath();
      ctx.moveTo(i, 0);
      ctx.lineTo(i, size);
      ctx.stroke();
      
      ctx.beginPath();
      ctx.moveTo(0, i);
      ctx.lineTo(size, i);
      ctx.stroke();
    }
    
    // Draw range circles
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.05)';
    for (let r = 40; r < size / 2; r += 40) {
      ctx.beginPath();
      ctx.arc(center, center, r, 0, Math.PI * 2);
      ctx.stroke();
    }
  }
  
  updateMinimap({ entities = [], objectives = [] }) {
    this.renderMinimap();
    
    if (!this.minimapCtx) return;
    
    const ctx = this.minimapCtx;
    const size = this.minimapCanvas.width;
    const center = size / 2;
    const scale = 0.1; // Scale factor for world to minimap
    
    // Draw objectives
    objectives.forEach(obj => {
      const x = center + obj.x * scale;
      const y = center + obj.y * scale;
      
      ctx.fillStyle = obj.team === 'blue' ? 'rgba(0, 212, 255, 0.6)' : 
                      obj.team === 'red' ? 'rgba(255, 0, 110, 0.6)' : 
                      'rgba(255, 255, 255, 0.4)';
      
      ctx.beginPath();
      ctx.arc(x, y, 8, 0, Math.PI * 2);
      ctx.fill();
      
      if (obj.contested) {
        ctx.strokeStyle = 'rgba(255, 183, 3, 0.8)';
        ctx.lineWidth = 2;
        ctx.stroke();
      }
    });
    
    // Draw entities
    entities.forEach(entity => {
      if (entity.isPlayer) return; // Skip player (drawn separately)
      
      const x = center + entity.x * scale;
      const y = center + entity.y * scale;
      
      ctx.fillStyle = entity.team === 'blue' ? '#00D4FF' : '#FF006E';
      
      ctx.beginPath();
      ctx.arc(x, y, 4, 0, Math.PI * 2);
      ctx.fill();
    });
  }
  
  // Crosshair
  setCrosshairState(state) {
    if (!this.elements.crosshair) return;
    
    this.elements.crosshair.classList.remove('firing', 'moving');
    
    if (state) {
      this.elements.crosshair.classList.add(state);
    }
  }
  
  // Screen Effects
  shakeScreen(intensity = 5, duration = 200) {
    const hud = document.querySelector('.hud-container');
    if (!hud) return;
    
    const startTime = Date.now();
    
    const shake = () => {
      const elapsed = Date.now() - startTime;
      
      if (elapsed < duration) {
        const x = (Math.random() - 0.5) * intensity;
        const y = (Math.random() - 0.5) * intensity;
        hud.style.transform = `translate(${x}px, ${y}px)`;
        requestAnimationFrame(shake);
      } else {
        hud.style.transform = '';
      }
    };
    
    shake();
  }
  
  // Number Animation
  animateNumber(element, start, end, duration) {
    const startTime = Date.now();
    
    const update = () => {
      const elapsed = Date.now() - startTime;
      const progress = Math.min(elapsed / duration, 1);
      
      // Ease out
      const eased = 1 - Math.pow(1 - progress, 3);
      
      const current = Math.floor(start + (end - start) * eased);
      element.textContent = current.toLocaleString();
      
      if (progress < 1) {
        requestAnimationFrame(update);
      }
    };
    
    update();
  }
  
  // Update from state object
  updateFromState(state) {
    if (state.health !== undefined && state.maxHealth !== undefined) {
      this.updateHealth({ current: state.health, max: state.maxHealth });
    }
    
    if (state.shield !== undefined && state.maxShield !== undefined) {
      this.updateShield({ current: state.shield, max: state.maxShield });
    }
    
    if (state.ammo !== undefined && state.maxAmmo !== undefined) {
      this.updateAmmo({ current: state.ammo, max: state.maxAmmo });
    }
    
    if (state.credits !== undefined) {
      this.updateCredits(state.credits);
    }
    
    if (state.timer !== undefined) {
      this.updateTimer(state.timer);
    }
    
    if (state.teamScore) {
      this.updateScore(state.teamScore);
    }
  }
  
  // Animation updates (called every frame)
  updateAnimations() {
    // Update ability cooldowns
    this.elements.abilities?.forEach(slot => {
      const cooldownEl = slot.querySelector('.ability-cooldown');
      if (cooldownEl) {
        const currentCooldown = parseFloat(cooldownEl.style.getPropertyValue('--cooldown') || 0);
        if (currentCooldown > 0) {
          // Decrease cooldown (would be synced with game)
        }
      }
    });
  }
  
  // Resize handler
  handleResize() {
    this.initMinimap();
  }
  
  // Cleanup
  destroy() {
    if (this.animationFrame) {
      cancelAnimationFrame(this.animationFrame);
    }
    HUDController.instance = null;
  }
}

// Export
window.HUDController = HUDController;
