/**
 * Space Shooter Game - UI Manager
 * Main controller for all UI components
 */

class UIManager {
  constructor() {
    this.currentScreen = 'main-menu';
    this.screens = new Map();
    this.isGameActive = false;
    this.isPaused = false;
    this.isBuyMenuOpen = false;
    this.isScoreboardOpen = false;
    
    // UI state
    this.state = {
      health: 100,
      maxHealth: 100,
      shield: 100,
      maxShield: 100,
      ammo: 30,
      maxAmmo: 30,
      credits: 0,
      teamScore: { blue: 0, red: 0 },
      timer: 0
    };
    
    this.init();
  }
  
  init() {
    this.registerScreens();
    this.bindEvents();
    this.bindKeyboardShortcuts();
    this.showScreen('main-menu');
    
    console.log('[UI] Manager initialized');
  }
  
  registerScreens() {
    const screenIds = [
      'main-menu',
      'hud',
      'buy-menu',
      'scoreboard',
      'pause-menu',
      'team-select',
      'round-end',
      'loading-screen'
    ];
    
    screenIds.forEach(id => {
      const element = document.getElementById(id);
      if (element) {
        this.screens.set(id, element);
      }
    });
    
    console.log(`[UI] Registered ${this.screens.size} screens`);
  }
  
  bindEvents() {
    // Window resize
    window.addEventListener('resize', () => this.handleResize());
    
    // Game state events
    document.addEventListener('game-start', () => this.onGameStart());
    document.addEventListener('game-end', () => this.onGameEnd());
    document.addEventListener('game-pause', () => this.onGamePause());
    document.addEventListener('game-resume', () => this.onGameResume());
    document.addEventListener('round-start', (e) => this.onRoundStart(e.detail));
    document.addEventListener('round-end', (e) => this.onRoundEnd(e.detail));
    
    // Menu navigation
    document.querySelectorAll('.nav-item').forEach(item => {
      item.addEventListener('click', (e) => {
        const section = e.currentTarget.dataset.section;
        this.switchMenuSection(section);
      });
    });
    
    // Pause menu buttons
    document.querySelectorAll('.pause-btn').forEach(btn => {
      btn.addEventListener('click', (e) => {
        const action = e.currentTarget.dataset.action;
        this.handlePauseAction(action);
      });
    });
    
    // Buy menu categories
    document.querySelectorAll('.category-btn').forEach(btn => {
      btn.addEventListener('click', (e) => {
        const category = e.currentTarget.dataset.category;
        this.switchBuyCategory(category);
      });
    });
    
    // Buy items
    document.querySelectorAll('.buy-item').forEach(item => {
      item.addEventListener('click', (e) => {
        const itemId = e.currentTarget.dataset.item;
        this.purchaseItem(itemId);
      });
    });
    
    // Initial resize
    this.handleResize();
  }
  
  bindKeyboardShortcuts() {
    document.addEventListener('keydown', (e) => {
      // Ignore if typing in input
      if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') {
        return;
      }
      
      switch(e.code) {
        case 'Tab':
          if (this.isGameActive && !this.isPaused) {
            e.preventDefault();
            this.toggleScoreboard(true);
          }
          break;
          
        case 'KeyB':
          if (this.isGameActive && !this.isPaused) {
            this.toggleBuyMenu();
          }
          break;
          
        case 'Escape':
          if (this.isGameActive) {
            this.togglePause();
          }
          break;
          
        case 'KeyY':
        case 'KeyT':
          if (this.isGameActive && !this.isPaused) {
            this.toggleChat(e.code === 'KeyY' ? 'team' : 'all');
          }
          break;
      }
    });
    
    document.addEventListener('keyup', (e) => {
      if (e.code === 'Tab' && this.isScoreboardOpen) {
        this.toggleScoreboard(false);
      }
    });
    
    // Quick buy keys
    document.addEventListener('keydown', (e) => {
      if (!this.isBuyMenuOpen) return;
      
      const key = parseInt(e.key);
      if (key >= 1 && key <= 9) {
        this.quickBuy(key);
      }
    });
  }
  
  showScreen(screenId) {
    // Hide all screens
    this.screens.forEach(screen => {
      if (screen) screen.classList.remove('active');
    });
    
    // Show target screen
    const target = this.screens.get(screenId);
    if (target) {
      target.classList.add('active');
      this.currentScreen = screenId;
      
      // Play transition sound
      this.playSound('screen-transition');
    }
  }
  
  switchMenuSection(section) {
    // Update nav items
    document.querySelectorAll('.nav-item').forEach(item => {
      item.classList.toggle('active', item.dataset.section === section);
    });
    
    // Update sections
    document.querySelectorAll('.menu-section').forEach(sec => {
      sec.classList.remove('active');
    });
    
    const targetSection = document.getElementById(`${section}-section`);
    if (targetSection) {
      targetSection.classList.add('active');
    }
    
    this.playSound('menu-click');
  }
  
  // Game State Handlers
  onGameStart() {
    this.isGameActive = true;
    this.isPaused = false;
    this.showScreen('hud');
    
    // Initialize HUD
    if (window.HUDController) {
      HUDController.init();
    }
    
    this.playSound('match-start');
    console.log('[UI] Game started');
  }
  
  onGameEnd() {
    this.isGameActive = false;
    this.isPaused = false;
    this.showScreen('main-menu');
    
    this.playSound('match-end');
    console.log('[UI] Game ended');
  }
  
  onGamePause() {
    this.isPaused = true;
  }
  
  onGameResume() {
    this.isPaused = false;
    this.isBuyMenuOpen = false;
    this.isScoreboardOpen = false;
    
    const buyMenu = this.screens.get('buy-menu');
    const scoreboard = this.screens.get('scoreboard');
    
    if (buyMenu) buyMenu.classList.remove('active');
    if (scoreboard) scoreboard.classList.remove('active');
  }
  
  onRoundStart(data) {
    this.showNotification(`Round ${data.round} Started`, 'objective');
  }
  
  onRoundEnd(data) {
    const roundEnd = this.screens.get('round-end');
    if (!roundEnd) return;
    
    const resultEl = roundEnd.querySelector('.round-result');
    const statsEl = roundEnd.querySelector('.round-stats');
    
    // Set result
    resultEl.className = `round-result ${data.result}`;
    resultEl.textContent = data.result.toUpperCase();
    
    // Set stats
    if (statsEl && data.stats) {
      statsEl.innerHTML = `
        <div class="round-stat">
          <div class="value">${data.stats.kills || 0}</div>
          <div class="label">Kills</div>
        </div>
        <div class="round-stat">
          <div class="value">${data.stats.deaths || 0}</div>
          <div class="label">Deaths</div>
        </div>
        <div class="round-stat">
          <div class="value">${data.stats.score || 0}</div>
          <div class="label">Score</div>
        </div>
      `;
    }
    
    roundEnd.classList.add('active');
    
    // Auto-hide after delay
    setTimeout(() => {
      roundEnd.classList.remove('active');
    }, 5000);
  }
  
  // Menu Toggles
  toggleScoreboard(show) {
    this.isScoreboardOpen = show;
    const scoreboard = this.screens.get('scoreboard');
    if (scoreboard) {
      scoreboard.classList.toggle('active', show);
    }
  }
  
  toggleBuyMenu() {
    this.isBuyMenuOpen = !this.isBuyMenuOpen;
    const buyMenu = this.screens.get('buy-menu');
    
    if (buyMenu) {
      buyMenu.classList.toggle('active', this.isBuyMenuOpen);
      
      // Dispatch game pause/resume
      document.dispatchEvent(new CustomEvent(
        this.isBuyMenuOpen ? 'game-pause' : 'game-resume'
      ));
    }
    
    this.playSound(this.isBuyMenuOpen ? 'menu-open' : 'menu-close');
  }
  
  togglePause() {
    if (!this.isGameActive) return;
    
    this.isPaused = !this.isPaused;
    const pauseMenu = this.screens.get('pause-menu');
    
    if (pauseMenu) {
      pauseMenu.classList.toggle('active', this.isPaused);
    }
    
    document.dispatchEvent(new CustomEvent(
      this.isPaused ? 'game-pause' : 'game-resume'
    ));
    
    this.playSound(this.isPaused ? 'menu-open' : 'menu-close');
  }
  
  toggleChat(type) {
    const chatInput = document.querySelector('.chat-input-container');
    if (chatInput) {
      chatInput.classList.add('active');
      chatInput.dataset.type = type;
      chatInput.querySelector('input')?.focus();
    }
  }
  
  // Pause Menu Actions
  handlePauseAction(action) {
    switch(action) {
      case 'resume':
        this.togglePause();
        break;
      case 'settings':
        this.openSettings();
        break;
      case 'surrender':
        this.confirmSurrender();
        break;
      case 'quit':
        this.confirmQuit();
        break;
    }
  }
  
  openSettings() {
    // TODO: Implement settings modal
    console.log('[UI] Open settings');
  }
  
  confirmSurrender() {
    if (confirm('Are you sure you want to surrender?')) {
      document.dispatchEvent(new CustomEvent('game-surrender'));
    }
  }
  
  confirmQuit() {
    if (confirm('Are you sure you want to quit?')) {
      this.onGameEnd();
    }
  }
  
  // Buy Menu
  switchBuyCategory(category) {
    document.querySelectorAll('.category-btn').forEach(btn => {
      btn.classList.toggle('active', btn.dataset.category === category);
    });
    
    // Filter items
    document.querySelectorAll('.buy-item').forEach(item => {
      const itemCategory = item.dataset.category;
      item.style.display = itemCategory === category ? 'flex' : 'none';
    });
    
    this.playSound('menu-click');
  }
  
  purchaseItem(itemId) {
    const item = document.querySelector(`.buy-item[data-item="${itemId}"]`);
    if (!item || item.classList.contains('owned')) return;
    
    const price = parseInt(item.querySelector('.price')?.textContent || 0);
    
    if (this.state.credits >= price) {
      document.dispatchEvent(new CustomEvent('item-purchase', {
        detail: { itemId, price }
      }));
      
      item.classList.add('owned');
      this.playSound('purchase');
    } else {
      this.playSound('error');
      this.showNotification('Not enough credits!', 'error');
    }
  }
  
  quickBuy(key) {
    const items = document.querySelectorAll('.buy-item:not(.owned)');
    const item = items[key - 1];
    if (item) {
      const itemId = item.dataset.item;
      this.purchaseItem(itemId);
    }
  }
  
  // Notifications
  showNotification(message, type = 'info', duration = 3000) {
    const container = document.querySelector('.hud-notifications') || 
                      document.createElement('div');
    
    if (!container.classList.contains('hud-notifications')) {
      container.className = 'hud-notifications';
      document.body.appendChild(container);
    }
    
    const notification = document.createElement('div');
    notification.className = `notification ${type}`;
    notification.textContent = message;
    
    container.appendChild(notification);
    
    setTimeout(() => {
      notification.classList.add('hiding');
      setTimeout(() => notification.remove(), 300);
    }, duration);
  }
  
  // Kill Feed
  addKillFeed(killer, victim, weapon, isHeadshot = false) {
    const killFeed = document.getElementById('kill-feed');
    if (!killFeed) return;
    
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
    appendSpan('victim', victim);
    if (isHeadshot) {
      appendSpan('headshot-icon', '🎯');
    }
    
    killFeed.appendChild(entry);
    
    // Remove after delay
    setTimeout(() => {
      entry.style.opacity = '0';
      setTimeout(() => entry.remove(), 300);
    }, 5000);
    
    // Keep only last 5 entries
    while (killFeed.children.length > 5) {
      killFeed.firstChild.remove();
    }
  }
  
  // Damage Numbers
  showDamageNumber(damage, x, y, isCritical = false) {
    const container = document.getElementById('damage-numbers');
    if (!container) return;
    
    const number = document.createElement('div');
    number.className = `damage-number ${isCritical ? 'critical' : ''}`;
    number.textContent = damage;
    number.style.left = `${x}px`;
    number.style.top = `${y}px`;
    
    container.appendChild(number);
    
    setTimeout(() => number.remove(), 1000);
  }
  
  // Hit Marker
  showHitMarker(isHeadshot = false) {
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
  
  // Combat Text
  showCombatText(text, type = 'kill') {
    const container = document.querySelector('.hud-center');
    if (!container) return;
    
    const combatText = document.createElement('div');
    combatText.className = `combat-text ${type}`;
    combatText.textContent = text;
    
    container.appendChild(combatText);
    
    setTimeout(() => combatText.remove(), 2000);
  }
  
  // Streak Indicator
  updateStreak(count) {
    const streakEl = document.querySelector('.streak-indicator');
    if (!streakEl) return;
    
    if (count > 1) {
      streakEl.classList.add('active');
      streakEl.querySelector('.streak-count').textContent = count;
    } else {
      streakEl.classList.remove('active');
    }
  }
  
  // Sound
  playSound(soundName) {
    // Dispatch sound event for audio manager
    document.dispatchEvent(new CustomEvent('play-sound', {
      detail: { sound: soundName }
    }));
  }
  
  // Resize Handler
  handleResize() {
    const isMobile = window.innerWidth <= 768;
    const isTablet = window.innerWidth <= 1024;
    
    document.body.classList.toggle('mobile', isMobile);
    document.body.classList.toggle('tablet', isTablet && !isMobile);
    
    // Update HUD scale if needed
    if (window.HUDController) {
      HUDController.handleResize();
    }
  }
  
  // State Updates
  updateState(updates) {
    Object.assign(this.state, updates);
    
    // Update HUD
    if (window.HUDController) {
      HUDController.updateFromState(this.state);
    }
  }
  
  // Loading Screen
  showLoading(text = 'Loading...') {
    const loading = this.screens.get('loading-screen');
    if (loading) {
      loading.querySelector('.loading-text').textContent = text;
      loading.classList.add('active');
    }
  }
  
  hideLoading() {
    const loading = this.screens.get('loading-screen');
    if (loading) {
      loading.classList.remove('active');
    }
  }
}

// Create global instance
window.uiManager = new UIManager();
