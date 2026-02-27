/**
 * Space Shooter Game - Touch Controls
 * Mobile touch input handling
 */

class TouchControls {
  constructor() {
    this.isMobile = window.matchMedia('(pointer: coarse)').matches || window.innerWidth <= 768;
    this.joystick = {
      active: false,
      origin: { x: 0, y: 0 },
      position: { x: 0, y: 0 },
      delta: { x: 0, y: 0 },
      maxDistance: 50
    };
    this.fireButton = {
      active: false
    };
    this.abilities = new Map();
    
    this.touchElements = {};
    
    if (this.isMobile) {
      this.init();
    }
  }
  
  init() {
    this.createTouchUI();
    this.cacheElements();
    this.bindEvents();
    
    console.log('[Touch] Controls initialized');
  }
  
  createTouchUI() {
    // Check if touch controls already exist
    if (document.querySelector('.touch-controls')) return;
    
    const touchControls = document.createElement('div');
    touchControls.className = 'touch-controls';
    touchControls.innerHTML = `
      <!-- Virtual Joystick -->
      <div class="virtual-joystick" id="virtual-joystick">
        <div class="joystick-base"></div>
        <div class="joystick-stick" id="joystick-stick"></div>
      </div>
      
      <!-- Fire Button -->
      <div class="fire-button" id="fire-button">
        <span class="fire-label">FIRE</span>
      </div>
      
      <!-- Mobile Abilities -->
      <div class="mobile-abilities" id="mobile-abilities">
        <div class="mobile-ability" data-key="Q" data-ability="boost">
          <span class="mobile-ability-icon" aria-hidden="true">Q</span>
        </div>
        <div class="mobile-ability" data-key="W" data-ability="shield">
          <span class="mobile-ability-icon" aria-hidden="true">W</span>
        </div>
        <div class="mobile-ability" data-key="E" data-ability="missile">
          <span class="mobile-ability-icon" aria-hidden="true">E</span>
        </div>
      </div>
      
      <!-- Weapon Switch -->
      <div class="weapon-switch" id="weapon-switch">
        <button class="weapon-btn" data-weapon="primary">1</button>
        <button class="weapon-btn" data-weapon="secondary">2</button>
      </div>
    `;
    
    document.body.appendChild(touchControls);
  }
  
  cacheElements() {
    this.touchElements.joystick = document.getElementById('virtual-joystick');
    this.touchElements.joystickStick = document.getElementById('joystick-stick');
    this.touchElements.fireButton = document.getElementById('fire-button');
    this.touchElements.abilities = document.querySelectorAll('.mobile-ability');
    this.touchElements.weaponSwitch = document.getElementById('weapon-switch');
  }
  
  bindEvents() {
    // Joystick
    if (this.touchElements.joystick) {
      this.touchElements.joystick.addEventListener('touchstart', (e) => this.onJoystickStart(e), { passive: false });
      this.touchElements.joystick.addEventListener('touchmove', (e) => this.onJoystickMove(e), { passive: false });
      this.touchElements.joystick.addEventListener('touchend', (e) => this.onJoystickEnd(e), { passive: false });
      this.touchElements.joystick.addEventListener('touchcancel', (e) => this.onJoystickEnd(e), { passive: false });
    }
    
    // Fire button
    if (this.touchElements.fireButton) {
      this.touchElements.fireButton.addEventListener('touchstart', (e) => this.onFireStart(e), { passive: false });
      this.touchElements.fireButton.addEventListener('touchend', (e) => this.onFireEnd(e), { passive: false });
    }
    
    // Abilities
    this.touchElements.abilities?.forEach(ability => {
      ability.addEventListener('touchstart', (e) => this.onAbilityStart(e), { passive: false });
      ability.addEventListener('touchend', (e) => this.onAbilityEnd(e), { passive: false });
    });
    
    // Weapon switch
    document.querySelectorAll('.weapon-btn')?.forEach(btn => {
      btn.addEventListener('click', (e) => this.onWeaponSwitch(e));
    });
    
    // Prevent default touch behaviors
    document.addEventListener('touchmove', (e) => {
      if (e.target.closest('.touch-controls')) {
        e.preventDefault();
      }
    }, { passive: false });
    
    // Handle resize
    window.addEventListener('resize', () => this.handleResize());
  }
  
  // Joystick Handlers
  onJoystickStart(e) {
    e.preventDefault();
    
    const touch = e.touches[0];
    const rect = this.touchElements.joystick.getBoundingClientRect();
    
    this.joystick.active = true;
    this.joystick.origin = {
      x: rect.left + rect.width / 2,
      y: rect.top + rect.height / 2
    };
    
    this.updateJoystickPosition(touch.clientX, touch.clientY);
    
    // Dispatch event
    this.dispatchInputEvent('joystick-start', this.joystick.delta);
  }
  
  onJoystickMove(e) {
    if (!this.joystick.active) return;
    e.preventDefault();
    
    const touch = e.touches[0];
    this.updateJoystickPosition(touch.clientX, touch.clientY);
    
    // Dispatch event
    this.dispatchInputEvent('joystick-move', this.joystick.delta);
  }
  
  onJoystickEnd(e) {
    if (!this.joystick.active) return;
    e.preventDefault();
    
    this.joystick.active = false;
    this.joystick.delta = { x: 0, y: 0 };
    
    // Reset stick position
    if (this.touchElements.joystickStick) {
      this.touchElements.joystickStick.style.transform = 'translate(-50%, -50%)';
    }
    
    // Dispatch event
    this.dispatchInputEvent('joystick-end', { x: 0, y: 0 });
  }
  
  updateJoystickPosition(clientX, clientY) {
    // Calculate delta from origin
    let deltaX = clientX - this.joystick.origin.x;
    let deltaY = clientY - this.joystick.origin.y;
    
    // Calculate distance
    const distance = Math.sqrt(deltaX * deltaX + deltaY * deltaY);
    
    // Clamp to max distance
    if (distance > this.joystick.maxDistance) {
      const ratio = this.joystick.maxDistance / distance;
      deltaX *= ratio;
      deltaY *= ratio;
    }
    
    this.joystick.delta = {
      x: deltaX / this.joystick.maxDistance,
      y: deltaY / this.joystick.maxDistance
    };
    
    // Update stick position
    if (this.touchElements.joystickStick) {
      this.touchElements.joystickStick.style.transform = 
        `translate(calc(-50% + ${deltaX}px), calc(-50% + ${deltaY}px))`;
    }
  }
  
  // Fire Button Handlers
  onFireStart(e) {
    e.preventDefault();
    
    this.fireButton.active = true;
    this.touchElements.fireButton?.classList.add('active');
    
    this.dispatchInputEvent('fire-start', {});
  }
  
  onFireEnd(e) {
    e.preventDefault();
    
    this.fireButton.active = false;
    this.touchElements.fireButton?.classList.remove('active');
    
    this.dispatchInputEvent('fire-end', {});
  }
  
  // Ability Handlers
  onAbilityStart(e) {
    e.preventDefault();
    
    const ability = e.currentTarget;
    const key = ability.dataset.key;
    const abilityName = ability.dataset.ability;
    
    ability.classList.add('active');
    
    this.dispatchInputEvent('ability-activate', { key, ability: abilityName });
  }
  
  onAbilityEnd(e) {
    e.preventDefault();
    
    const ability = e.currentTarget;
    ability.classList.remove('active');
  }
  
  // Weapon Switch Handler
  onWeaponSwitch(e) {
    const weapon = e.currentTarget.dataset.weapon;
    
    // Update active state
    document.querySelectorAll('.weapon-btn').forEach(btn => {
      btn.classList.toggle('active', btn === e.currentTarget);
    });
    
    this.dispatchInputEvent('weapon-switch', { weapon });
  }
  
  // Gesture Recognition
  initGestures() {
    let touchStartX = 0;
    let touchStartY = 0;
    let touchStartTime = 0;
    
    document.addEventListener('touchstart', (e) => {
      // Only track gestures on game area (not on controls)
      if (e.target.closest('.touch-controls')) return;
      
      touchStartX = e.touches[0].clientX;
      touchStartY = e.touches[0].clientY;
      touchStartTime = Date.now();
    }, { passive: true });
    
    document.addEventListener('touchend', (e) => {
      if (touchStartX === 0 && touchStartY === 0) return;
      
      const touchEndX = e.changedTouches[0].clientX;
      const touchEndY = e.changedTouches[0].clientY;
      const touchDuration = Date.now() - touchStartTime;
      
      const deltaX = touchEndX - touchStartX;
      const deltaY = touchEndY - touchStartY;
      const distance = Math.sqrt(deltaX * deltaX + deltaY * deltaY);
      
      // Swipe detection
      if (distance > 50 && touchDuration < 300) {
        const angle = Math.atan2(deltaY, deltaX) * 180 / Math.PI;
        
        // Determine swipe direction
        if (Math.abs(angle) < 45) {
          this.dispatchInputEvent('swipe-right', {});
        } else if (Math.abs(angle) > 135) {
          this.dispatchInputEvent('swipe-left', {});
        } else if (angle < 0) {
          this.dispatchInputEvent('swipe-up', {});
        } else {
          this.dispatchInputEvent('swipe-down', {});
        }
      }
      
      // Tap detection for boost
      if (distance < 10 && touchDuration < 200) {
        // Could be used for quick actions
      }
      
      touchStartX = 0;
      touchStartY = 0;
    }, { passive: true });
  }
  
  // Double tap for special actions
  initDoubleTap() {
    let lastTap = 0;
    let lastTapX = 0;
    let lastTapY = 0;
    
    document.addEventListener('touchend', (e) => {
      if (e.target.closest('.touch-controls')) return;
      
      const currentTime = Date.now();
      const tapX = e.changedTouches[0].clientX;
      const tapY = e.changedTouches[0].clientY;
      const tapLength = currentTime - lastTap;
      
      if (tapLength < 300 && tapLength > 0) {
        const distance = Math.sqrt(
          Math.pow(tapX - lastTapX, 2) + 
          Math.pow(tapY - lastTapY, 2)
        );
        
        if (distance < 30) {
          e.preventDefault();
          this.dispatchInputEvent('double-tap', { x: tapX, y: tapY });
        }
      }
      
      lastTap = currentTime;
      lastTapX = tapX;
      lastTapY = tapY;
    }, { passive: false });
  }
  
  // Pinch to zoom (for map)
  initPinchZoom() {
    let initialDistance = 0;
    let initialScale = 1;
    
    document.addEventListener('touchstart', (e) => {
      if (e.touches.length === 2) {
        initialDistance = Math.hypot(
          e.touches[0].pageX - e.touches[1].pageX,
          e.touches[0].pageY - e.touches[1].pageY
        );
      }
    }, { passive: true });
    
    document.addEventListener('touchmove', (e) => {
      if (e.touches.length === 2) {
        e.preventDefault();
        
        const currentDistance = Math.hypot(
          e.touches[0].pageX - e.touches[1].pageX,
          e.touches[0].pageY - e.touches[1].pageY
        );
        
        const scale = (currentDistance / initialDistance) * initialScale;
        
        this.dispatchInputEvent('pinch-zoom', { scale });
      }
    }, { passive: false });
  }
  
  // Dispatch input event
  dispatchInputEvent(type, data) {
    document.dispatchEvent(new CustomEvent('touch-input', {
      detail: { type, ...data }
    }));
  }
  
  // Handle resize
  handleResize() {
    const wasMobile = this.isMobile;
    this.isMobile = window.matchMedia('(pointer: coarse)').matches || window.innerWidth <= 768;
    
    if (this.isMobile && !wasMobile) {
      // Switched to mobile
      this.init();
    } else if (!this.isMobile && wasMobile) {
      // Switched to desktop
      this.destroy();
    }
  }
  
  // Update ability cooldown visual
  updateAbilityCooldown(key, percentage) {
    const ability = document.querySelector(`.mobile-ability[data-key="${key}"]`);
    if (ability) {
      ability.style.setProperty('--cooldown', `${percentage}%`);
      ability.classList.toggle('on-cooldown', percentage > 0);
    }
  }
  
  // Update ultimate charge
  updateUltimateCharge(percentage) {
    const ultimate = document.querySelector('.mobile-ability.ultimate');
    if (ultimate) {
      ultimate.classList.toggle('ready', percentage >= 100);
    }
  }
  
  // Show/hide touch controls
  show() {
    const controls = document.querySelector('.touch-controls');
    if (controls) {
      controls.style.opacity = '1';
      controls.style.pointerEvents = 'auto';
    }
  }
  
  hide() {
    const controls = document.querySelector('.touch-controls');
    if (controls) {
      controls.style.opacity = '0';
      controls.style.pointerEvents = 'none';
    }
  }
  
  // Destroy
  destroy() {
    const controls = document.querySelector('.touch-controls');
    if (controls) {
      controls.remove();
    }
  }
  
  // Get joystick input
  getJoystickInput() {
    return this.joystick.active ? this.joystick.delta : { x: 0, y: 0 };
  }
  
  // Check if firing
  isFiring() {
    return this.fireButton.active;
  }
}

// CSS for touch controls (injected)
const touchStyles = `
  .touch-controls {
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 150;
  }
  
  .virtual-joystick {
    position: absolute;
    bottom: 40px;
    left: 40px;
    width: 140px;
    height: 140px;
    pointer-events: auto;
  }
  
  .joystick-base {
    position: absolute;
    inset: 0;
    background: rgba(255, 255, 255, 0.08);
    border: 2px solid rgba(255, 255, 255, 0.2);
    border-radius: 50%;
  }
  
  .joystick-stick {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 60px;
    height: 60px;
    background: rgba(0, 212, 255, 0.4);
    border: 2px solid var(--color-primary);
    border-radius: 50%;
    box-shadow: var(--glow-primary);
    transition: transform 0.05s ease-out;
  }
  
  .fire-button {
    position: absolute;
    bottom: 60px;
    right: 40px;
    width: 100px;
    height: 100px;
    background: rgba(255, 0, 110, 0.3);
    border: 3px solid var(--color-danger);
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: var(--glow-danger);
    pointer-events: auto;
    touch-action: none;
    transition: all 0.1s ease;
  }
  
  .fire-button:active,
  .fire-button.active {
    background: rgba(255, 0, 110, 0.5);
    transform: scale(0.95);
  }
  
  .fire-label {
    font-family: var(--font-display);
    font-size: var(--text-sm);
    font-weight: var(--font-bold);
    color: var(--text-primary);
    letter-spacing: 2px;
  }
  
  .mobile-abilities {
    position: absolute;
    bottom: 30px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    gap: var(--space-3);
    pointer-events: auto;
  }
  
  .mobile-ability {
    width: 64px;
    height: 64px;
    background: var(--bg-panel);
    border: 2px solid rgba(255, 255, 255, 0.15);
    border-radius: var(--radius-md);
    display: flex;
    align-items: center;
    justify-content: center;
    touch-action: none;
    transition: all 0.15s ease;
    position: relative;
    overflow: hidden;
  }
  
  .mobile-ability::after {
    content: '';
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.8);
    clip-path: polygon(0 0, 100% 0, 100% var(--cooldown, 0%), 0 var(--cooldown, 0%));
  }
  
  .mobile-ability:active,
  .mobile-ability.active {
    border-color: var(--color-primary);
    background: rgba(0, 212, 255, 0.2);
    transform: scale(0.95);
  }
  
  .mobile-ability.ready {
    border-color: var(--color-tertiary);
    animation: pulse-glow 2s ease-in-out infinite;
  }
  
  .mobile-ability img {
    width: 60%;
    height: 60%;
    object-fit: contain;
  }
  
  .weapon-switch {
    position: absolute;
    top: 120px;
    right: 20px;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    pointer-events: auto;
  }
  
  .weapon-btn {
    width: 48px;
    height: 48px;
    background: var(--bg-panel);
    border: 2px solid rgba(255, 255, 255, 0.15);
    border-radius: var(--radius-md);
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: var(--font-bold);
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.15s ease;
  }
  
  .weapon-btn.active,
  .weapon-btn:active {
    border-color: var(--color-primary);
    color: var(--color-primary);
    background: rgba(0, 212, 255, 0.1);
  }
  
  @media (max-width: 480px) {
    .virtual-joystick {
      width: 120px;
      height: 120px;
      bottom: 30px;
      left: 20px;
    }
    
    .joystick-stick {
      width: 50px;
      height: 50px;
    }
    
    .fire-button {
      width: 80px;
      height: 80px;
      bottom: 40px;
      right: 20px;
    }
    
    .mobile-ability {
      width: 56px;
      height: 56px;
    }
  }
`;

// Inject styles
const styleEl = document.createElement('style');
styleEl.textContent = touchStyles;
document.head.appendChild(styleEl);

// Create global instance
window.touchControls = new TouchControls();
