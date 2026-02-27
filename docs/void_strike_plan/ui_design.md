# Space Shooter Game - Modern UI/UX Design

## Table of Contents
1. [Design Philosophy](#design-philosophy)
2. [Visual Style Guide](#visual-style-guide)
3. [Main Menu System](#main-menu-system)
4. [HUD (Heads-Up Display)](#hud-heads-up-display)
5. [In-Game Menus](#in-game-menus)
6. [Career Mode UI](#career-mode-ui)
7. [Mobile Adaptations](#mobile-adaptations)
8. [Implementation Guide](#implementation-guide)
9. [Code Reference](#code-reference)

---

## Design Philosophy

### Core Principles
- **Clarity First**: Information must be instantly readable during intense combat
- **Sci-Fi Aesthetic**: Clean, futuristic design with subtle glows and gradients
- **Responsive**: Seamless experience across desktop and mobile
- **Performance**: Lightweight UI that doesn't impact game performance
- **Accessibility**: High contrast, scalable fonts, colorblind-friendly options

### Design Language
- **Glassmorphism**: Translucent panels with backdrop blur
- **Neon Accents**: Cyan (#00D4FF) and Magenta (#FF006E) as primary action colors
- **Geometric Shapes**: Sharp angles, hexagons, and clean lines
- **Animated Transitions**: Smooth 200-300ms transitions for all interactions

---

## Visual Style Guide

### Color Palette

```css
:root {
  /* Primary Colors */
  --color-primary: #00D4FF;        /* Cyan - Main accent */
  --color-secondary: #FF006E;      /* Magenta - Danger/Enemy */
  --color-tertiary: #9D4EDD;       /* Purple - Special/Rare */
  
  /* Background Colors */
  --bg-dark: #0A0A0F;              /* Deep space black */
  --bg-panel: rgba(15, 15, 25, 0.85);  /* Translucent panels */
  --bg-card: rgba(25, 25, 40, 0.7);    /* Card backgrounds */
  
  /* UI Colors */
  --color-health: #00F5D4;         /* Health bar */
  --color-shield: #00BBF9;         /* Shield bar */
  --color-energy: #FEF440;         /* Energy/ammo */
  --color-credits: #FFD700;        /* Gold/Currency */
  --color-exp: #9D4EDD;            /* Experience */
  
  /* Status Colors */
  --color-success: #06FFA5;
  --color-warning: #FFB703;
  --color-danger: #FF006E;
  --color-info: #00D4FF;
  
  /* Text Colors */
  --text-primary: #FFFFFF;
  --text-secondary: rgba(255, 255, 255, 0.7);
  --text-muted: rgba(255, 255, 255, 0.5);
  
  /* Glow Effects */
  --glow-primary: 0 0 20px rgba(0, 212, 255, 0.5);
  --glow-danger: 0 0 20px rgba(255, 0, 110, 0.5);
  --glow-success: 0 0 20px rgba(6, 255, 165, 0.5);
}
```

### Typography

```css
@import url('https://fonts.googleapis.com/css2?family=Orbitron:wght@400;500;600;700;800;900&family=Rajdhani:wght@300;400;500;600;700&display=swap');

:root {
  /* Font Families */
  --font-display: 'Orbitron', sans-serif;    /* Headers, titles */
  --font-body: 'Rajdhani', sans-serif;       /* Body text, UI */
  
  /* Font Sizes */
  --text-xs: 0.75rem;      /* 12px */
  --text-sm: 0.875rem;     /* 14px */
  --text-base: 1rem;       /* 16px */
  --text-lg: 1.125rem;     /* 18px */
  --text-xl: 1.25rem;      /* 20px */
  --text-2xl: 1.5rem;      /* 24px */
  --text-3xl: 1.875rem;    /* 30px */
  --text-4xl: 2.25rem;     /* 36px */
  --text-5xl: 3rem;        /* 48px */
  
  /* Font Weights */
  --font-light: 300;
  --font-regular: 400;
  --font-medium: 500;
  --font-semibold: 600;
  --font-bold: 700;
}
```

### Spacing & Layout

```css
:root {
  /* Spacing Scale */
  --space-1: 0.25rem;   /* 4px */
  --space-2: 0.5rem;    /* 8px */
  --space-3: 0.75rem;   /* 12px */
  --space-4: 1rem;      /* 16px */
  --space-5: 1.25rem;   /* 20px */
  --space-6: 1.5rem;    /* 24px */
  --space-8: 2rem;      /* 32px */
  --space-10: 2.5rem;   /* 40px */
  --space-12: 3rem;     /* 48px */
  
  /* Border Radius */
  --radius-sm: 4px;
  --radius-md: 8px;
  --radius-lg: 12px;
  --radius-xl: 16px;
  --radius-full: 9999px;
  
  /* Z-Index Scale */
  --z-background: 0;
  --z-game: 10;
  --z-ui: 100;
  --z-overlay: 200;
  --z-modal: 300;
  --z-tooltip: 400;
  --z-toast: 500;
}
```

---

## Main Menu System

### Menu Structure

```
Main Menu
├── Play
│   ├── Quick Match
│   ├── Ranked Match
│   ├── Custom Game
│   └── Training
├── Career
│   ├── Profile
│   ├── Stats
│   ├── Progression
│   └── Match History
├── Shop
│   ├── Ships
│   ├── Weapons
│   ├── Skins
│   └── Bundles
├── Social
│   ├── Friends
│   ├── Clan
│   ├── Leaderboard
│   └── Chat
└── Settings
    ├── Graphics
    ├── Audio
    ├── Controls
    └── Gameplay
```

### Main Menu HTML Structure

```html
<!-- Main Menu Container -->
<div id="main-menu" class="menu-container active">
  <!-- Background Video/Image -->
  <div class="menu-background">
    <div class="stars-bg"></div>
    <div class="nebula-overlay"></div>
  </div>
  
  <!-- Header -->
  <header class="menu-header">
    <div class="logo">
      <span class="logo-icon">◈</span>
      <span class="logo-text">NEBULA STRIKE</span>
    </div>
    <div class="player-preview">
      <div class="player-avatar">
        <img src="avatar.png" alt="Player">
        <div class="rank-badge">A</div>
      </div>
      <div class="player-info">
        <span class="player-name">Commander</span>
        <span class="player-level">Level 42</span>
      </div>
      <div class="player-currency">
        <span class="credits">
          <i class="icon-credits"></i>
          <span>12,450</span>
        </span>
        <span class="premium">
          <i class="icon-premium"></i>
          <span>250</span>
        </span>
      </div>
    </div>
  </header>
  
  <!-- Main Navigation -->
  <nav class="main-nav">
    <button class="nav-item active" data-section="play">
      <span class="nav-icon">▶</span>
      <span class="nav-label">PLAY</span>
      <div class="nav-glow"></div>
    </button>
    <button class="nav-item" data-section="career">
      <span class="nav-icon">★</span>
      <span class="nav-label">CAREER</span>
      <div class="nav-glow"></div>
    </button>
    <button class="nav-item" data-section="shop">
      <span class="nav-icon">◈</span>
      <span class="nav-label">SHOP</span>
      <div class="nav-glow"></div>
    </button>
    <button class="nav-item" data-section="social">
      <span class="nav-icon">◉</span>
      <span class="nav-label">SOCIAL</span>
      <div class="nav-glow"></div>
    </button>
    <button class="nav-item" data-section="settings">
      <span class="nav-icon">⚙</span>
      <span class="nav-label">SETTINGS</span>
      <div class="nav-glow"></div>
    </button>
  </nav>
  
  <!-- Content Area -->
  <main class="menu-content">
    <!-- Play Section -->
    <section id="play-section" class="menu-section active">
      <div class="game-modes">
        <div class="mode-card featured">
          <div class="mode-bg"></div>
          <div class="mode-content">
            <h2>QUICK MATCH</h2>
            <p>Jump into action immediately</p>
            <div class="mode-stats">
              <span>⏱ ~5 min</span>
              <span>👥 5v5</span>
            </div>
          </div>
          <button class="btn-play">PLAY NOW</button>
        </div>
        
        <div class="mode-grid">
          <div class="mode-card">
            <div class="mode-icon">🏆</div>
            <h3>RANKED</h3>
            <p>Competitive matches</p>
          </div>
          <div class="mode-card">
            <div class="mode-icon">⚔</div>
            <h3>CUSTOM</h3>
            <p>Create your game</p>
          </div>
          <div class="mode-card">
            <div class="mode-icon">🎯</div>
            <h3>TRAINING</h3>
            <p>Practice your skills</p>
          </div>
        </div>
      </div>
    </section>
    
    <!-- Career Section -->
    <section id="career-section" class="menu-section">
      <div class="career-overview">
        <div class="rank-display">
          <div class="rank-icon">
            <svg viewBox="0 0 100 100">
              <polygon points="50,5 95,27.5 95,72.5 50,95 5,72.5 5,27.5" 
                       fill="none" stroke="var(--color-primary)" stroke-width="2"/>
              <text x="50" y="60" text-anchor="middle" fill="var(--color-primary)" 
                    font-size="30" font-family="Orbitron">A</text>
            </svg>
          </div>
          <div class="rank-info">
            <h2>ASTRAL COMMANDER</h2>
            <div class="rank-progress">
              <div class="progress-bar">
                <div class="progress-fill" style="width: 73%"></div>
              </div>
              <span>7,340 / 10,000 XP</span>
            </div>
          </div>
        </div>
        
        <div class="stats-grid">
          <div class="stat-card">
            <span class="stat-value">247</span>
            <span class="stat-label">Matches</span>
          </div>
          <div class="stat-card">
            <span class="stat-value">62%</span>
            <span class="stat-label">Win Rate</span>
          </div>
          <div class="stat-card">
            <span class="stat-value">1,847</span>
            <span class="stat-label">Kills</span>
          </div>
          <div class="stat-card highlight">
            <span class="stat-value">3.2</span>
            <span class="stat-label">K/D Ratio</span>
          </div>
        </div>
      </div>
    </section>
    
    <!-- Other sections... -->
  </main>
  
  <!-- Footer -->
  <footer class="menu-footer">
    <div class="news-ticker">
      <span class="ticker-label">NEWS:</span>
      <div class="ticker-content">
        <span>New Season Starting Soon! | Double XP Weekend | New Ship: Void Stalker Available</span>
      </div>
    </div>
    <div class="version">v2.4.1</div>
  </footer>
</div>
```

### Main Menu CSS

```css
/* Menu Container */
.menu-container {
  position: fixed;
  inset: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-dark);
  z-index: var(--z-ui);
  opacity: 0;
  visibility: hidden;
  transition: opacity 0.3s ease, visibility 0.3s ease;
}

.menu-container.active {
  opacity: 1;
  visibility: visible;
}

/* Background */
.menu-background {
  position: absolute;
  inset: 0;
  overflow: hidden;
  z-index: -1;
}

.stars-bg {
  position: absolute;
  inset: 0;
  background: 
    radial-gradient(2px 2px at 20px 30px, #eee, transparent),
    radial-gradient(2px 2px at 40px 70px, #fff, transparent),
    radial-gradient(1px 1px at 90px 40px, #fff, transparent),
    radial-gradient(2px 2px at 160px 120px, #ddd, transparent),
    radial-gradient(1px 1px at 230px 80px, #fff, transparent);
  background-size: 250px 250px;
  animation: stars-move 100s linear infinite;
}

@keyframes stars-move {
  from { transform: translateY(0); }
  to { transform: translateY(-250px); }
}

.nebula-overlay {
  position: absolute;
  inset: 0;
  background: 
    radial-gradient(ellipse at 20% 80%, rgba(157, 78, 221, 0.2) 0%, transparent 50%),
    radial-gradient(ellipse at 80% 20%, rgba(0, 212, 255, 0.15) 0%, transparent 50%);
}

/* Header */
.menu-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--space-4) var(--space-8);
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
}

.logo {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.logo-icon {
  font-size: var(--text-3xl);
  color: var(--color-primary);
  text-shadow: var(--glow-primary);
  animation: pulse 2s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.7; }
}

.logo-text {
  font-family: var(--font-display);
  font-size: var(--text-2xl);
  font-weight: var(--font-bold);
  letter-spacing: 4px;
  background: linear-gradient(90deg, var(--color-primary), var(--color-tertiary));
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
}

/* Player Preview */
.player-preview {
  display: flex;
  align-items: center;
  gap: var(--space-4);
}

.player-avatar {
  position: relative;
  width: 48px;
  height: 48px;
  border-radius: var(--radius-full);
  border: 2px solid var(--color-primary);
  overflow: hidden;
}

.player-avatar img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.rank-badge {
  position: absolute;
  bottom: -4px;
  right: -4px;
  width: 20px;
  height: 20px;
  background: var(--color-primary);
  border-radius: var(--radius-full);
  display: flex;
  align-items: center;
  justify-content: center;
  font-family: var(--font-display);
  font-size: var(--text-xs);
  font-weight: var(--font-bold);
  color: var(--bg-dark);
}

.player-info {
  display: flex;
  flex-direction: column;
}

.player-name {
  font-family: var(--font-display);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.player-level {
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.player-currency {
  display: flex;
  gap: var(--space-4);
  padding-left: var(--space-4);
  border-left: 1px solid rgba(255, 255, 255, 0.2);
}

.player-currency span {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  font-family: var(--font-display);
  font-weight: var(--font-semibold);
}

.credits { color: var(--color-credits); }
.premium { color: var(--color-tertiary); }

/* Main Navigation */
.main-nav {
  display: flex;
  justify-content: center;
  gap: var(--space-2);
  padding: var(--space-4) var(--space-8);
}

.nav-item {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-3) var(--space-6);
  background: transparent;
  border: none;
  cursor: pointer;
  transition: all 0.2s ease;
}

.nav-item::before {
  content: '';
  position: absolute;
  inset: 0;
  background: linear-gradient(180deg, rgba(0, 212, 255, 0.1), transparent);
  border-radius: var(--radius-md);
  opacity: 0;
  transition: opacity 0.2s ease;
}

.nav-item:hover::before,
.nav-item.active::before {
  opacity: 1;
}

.nav-icon {
  font-size: var(--text-xl);
  color: var(--text-secondary);
  transition: color 0.2s ease;
}

.nav-item:hover .nav-icon,
.nav-item.active .nav-icon {
  color: var(--color-primary);
}

.nav-label {
  font-family: var(--font-display);
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  letter-spacing: 2px;
  color: var(--text-secondary);
  transition: color 0.2s ease;
}

.nav-item:hover .nav-label,
.nav-item.active .nav-label {
  color: var(--text-primary);
}

.nav-glow {
  position: absolute;
  bottom: 0;
  left: 50%;
  transform: translateX(-50%);
  width: 0;
  height: 2px;
  background: var(--color-primary);
  box-shadow: var(--glow-primary);
  transition: width 0.2s ease;
}

.nav-item.active .nav-glow {
  width: 60%;
}

/* Menu Content */
.menu-content {
  flex: 1;
  padding: var(--space-6) var(--space-8);
  overflow-y: auto;
}

.menu-section {
  display: none;
  animation: fadeIn 0.3s ease;
}

.menu-section.active {
  display: block;
}

@keyframes fadeIn {
  from { opacity: 0; transform: translateY(10px); }
  to { opacity: 1; transform: translateY(0); }
}

/* Game Mode Cards */
.game-modes {
  display: flex;
  flex-direction: column;
  gap: var(--space-6);
  max-width: 900px;
  margin: 0 auto;
}

.mode-card {
  position: relative;
  background: var(--bg-card);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: var(--radius-lg);
  overflow: hidden;
  transition: all 0.3s ease;
  cursor: pointer;
}

.mode-card:hover {
  border-color: var(--color-primary);
  transform: translateY(-2px);
  box-shadow: var(--glow-primary);
}

.mode-card.featured {
  padding: var(--space-8);
  min-height: 200px;
}

.mode-card.featured .mode-bg {
  position: absolute;
  inset: 0;
  background: linear-gradient(135deg, 
    rgba(0, 212, 255, 0.1) 0%, 
    transparent 50%,
    rgba(157, 78, 221, 0.1) 100%);
}

.mode-card.featured .mode-content {
  position: relative;
  z-index: 1;
}

.mode-card.featured h2 {
  font-family: var(--font-display);
  font-size: var(--text-4xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  margin-bottom: var(--space-2);
}

.mode-card.featured p {
  color: var(--text-secondary);
  margin-bottom: var(--space-4);
}

.mode-stats {
  display: flex;
  gap: var(--space-4);
}

.mode-stats span {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  font-size: var(--text-sm);
  color: var(--text-muted);
}

.btn-play {
  position: absolute;
  right: var(--space-8);
  top: 50%;
  transform: translateY(-50%);
  padding: var(--space-4) var(--space-8);
  background: linear-gradient(90deg, var(--color-primary), var(--color-tertiary));
  border: none;
  border-radius: var(--radius-md);
  font-family: var(--font-display);
  font-size: var(--text-lg);
  font-weight: var(--font-bold);
  color: var(--bg-dark);
  cursor: pointer;
  transition: all 0.2s ease;
}

.btn-play:hover {
  transform: translateY(-50%) scale(1.05);
  box-shadow: var(--glow-primary);
}

.mode-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--space-4);
}

.mode-grid .mode-card {
  padding: var(--space-6);
  text-align: center;
}

.mode-grid .mode-icon {
  font-size: var(--text-3xl);
  margin-bottom: var(--space-3);
}

.mode-grid h3 {
  font-family: var(--font-display);
  font-size: var(--text-lg);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  margin-bottom: var(--space-1);
}

.mode-grid p {
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

/* Career Section */
.career-overview {
  max-width: 800px;
  margin: 0 auto;
}

.rank-display {
  display: flex;
  align-items: center;
  gap: var(--space-6);
  padding: var(--space-6);
  background: var(--bg-card);
  border-radius: var(--radius-lg);
  margin-bottom: var(--space-6);
}

.rank-icon {
  width: 100px;
  height: 100px;
}

.rank-icon svg {
  width: 100%;
  height: 100%;
  filter: drop-shadow(var(--glow-primary));
}

.rank-info h2 {
  font-family: var(--font-display);
  font-size: var(--text-2xl);
  font-weight: var(--font-bold);
  color: var(--color-primary);
  margin-bottom: var(--space-3);
}

.rank-progress {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.progress-bar {
  width: 300px;
  height: 8px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: var(--radius-full);
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: linear-gradient(90deg, var(--color-primary), var(--color-tertiary));
  border-radius: var(--radius-full);
  transition: width 0.5s ease;
}

.rank-progress span {
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.stats-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--space-4);
}

.stat-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: var(--space-6);
  background: var(--bg-card);
  border-radius: var(--radius-lg);
  border: 1px solid rgba(255, 255, 255, 0.1);
  transition: all 0.2s ease;
}

.stat-card:hover {
  border-color: var(--color-primary);
}

.stat-card.highlight {
  border-color: var(--color-primary);
  background: linear-gradient(180deg, rgba(0, 212, 255, 0.1), var(--bg-card));
}

.stat-value {
  font-family: var(--font-display);
  font-size: var(--text-3xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
}

.stat-label {
  font-size: var(--text-sm);
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 1px;
}

/* Footer */
.menu-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--space-3) var(--space-8);
  border-top: 1px solid rgba(255, 255, 255, 0.1);
  background: rgba(0, 0, 0, 0.3);
}

.news-ticker {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  overflow: hidden;
}

.ticker-label {
  font-family: var(--font-display);
  font-size: var(--text-xs);
  font-weight: var(--font-bold);
  color: var(--color-primary);
  letter-spacing: 2px;
}

.ticker-content {
  overflow: hidden;
  white-space: nowrap;
}

.ticker-content span {
  display: inline-block;
  animation: ticker 20s linear infinite;
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

@keyframes ticker {
  0% { transform: translateX(100%); }
  100% { transform: translateX(-100%); }
}

.version {
  font-size: var(--text-xs);
  color: var(--text-muted);
}
```

---

## HUD (Heads-Up Display)

### HUD Layout Overview

```
┌─────────────────────────────────────────────────────────────┐
│  [Health/Shield]                    [Team Score]  [Timer]  │
│  ═══════════════                    [Blue: 8]     [3:42]   │
│                                      [Red: 6]               │
│                                                             │
│                                                             │
│                                                             │
│                        [CROSSHAIR]                          │
│                                                             │
│                                                             │
│                                                             │
│  [Weapon]  [Ammo]        [Minimap]        [Abilities]       │
│  [Icon]    [∞/150]       [Radar]          [Q][W][E][R]      │
│  [Credits: 4,250]                                           │
└─────────────────────────────────────────────────────────────┘
```

### HUD HTML Structure

```html
<!-- HUD Container -->
<div id="hud" class="hud-container">
  <!-- Top Bar -->
  <div class="hud-top">
    <!-- Health & Shield (Left) -->
    <div class="hud-health-section">
      <div class="ship-status">
        <div class="ship-icon">
          <svg viewBox="0 0 40 40">
            <path d="M20 5 L30 35 L20 30 L10 35 Z" fill="currentColor"/>
          </svg>
        </div>
        <div class="status-bars">
          <div class="bar-container shield-bar">
            <div class="bar-label">
              <span>SHIELD</span>
              <span class="bar-value">850/1000</span>
            </div>
            <div class="bar-track">
              <div class="bar-fill" style="width: 85%"></div>
              <div class="bar-segments"></div>
            </div>
          </div>
          <div class="bar-container health-bar">
            <div class="bar-label">
              <span>HULL</span>
              <span class="bar-value">420/500</span>
            </div>
            <div class="bar-track">
              <div class="bar-fill" style="width: 84%"></div>
              <div class="bar-segments"></div>
            </div>
          </div>
        </div>
      </div>
    </div>
    
    <!-- Score & Timer (Center/Right) -->
    <div class="hud-score-section">
      <div class="match-timer">
        <div class="timer-display">
          <span class="timer-value">03:42</span>
          <span class="timer-phase">ROUND 1/12</span>
        </div>
      </div>
      <div class="team-score">
        <div class="score-team blue">
          <span class="team-name">BLUE</span>
          <span class="score-value">8</span>
        </div>
        <div class="score-divider">:</div>
        <div class="score-team red">
          <span class="score-value">6</span>
          <span class="team-name">RED</span>
        </div>
      </div>
    </div>
  </div>
  
  <!-- Center Crosshair -->
  <div class="hud-center">
    <div class="crosshair" id="crosshair">
      <div class="crosshair-dot"></div>
      <div class="crosshair-lines">
        <span class="line top"></span>
        <span class="line right"></span>
        <span class="line bottom"></span>
        <span class="line left"></span>
      </div>
    </div>
    
    <!-- Hit Marker -->
    <div class="hit-marker" id="hit-marker">
      <span class="hit-line tl"></span>
      <span class="hit-line tr"></span>
      <span class="hit-line bl"></span>
      <span class="hit-line br"></span>
    </div>
    
    <!-- Damage Numbers -->
    <div class="damage-numbers" id="damage-numbers"></div>
  </div>
  
  <!-- Bottom Bar -->
  <div class="hud-bottom">
    <!-- Weapon & Ammo (Left) -->
    <div class="hud-weapon-section">
      <div class="weapon-display">
        <div class="weapon-icon">
          <img src="weapon-plasma.png" alt="Plasma Cannon">
        </div>
        <div class="weapon-info">
          <span class="weapon-name">PLASMA CANNON</span>
          <div class="ammo-display">
            <span class="ammo-current">∞</span>
            <span class="ammo-divider">/</span>
            <span class="ammo-max">150</span>
          </div>
        </div>
      </div>
      <div class="credits-display">
        <i class="icon-credits"></i>
        <span class="credits-value">4,250</span>
      </div>
    </div>
    
    <!-- Minimap (Center) -->
    <div class="hud-minimap-section">
      <div class="minimap-container">
        <canvas id="minimap" width="180" height="180"></canvas>
        <div class="minimap-overlay">
          <div class="compass">
            <span class="compass-n">N</span>
            <span class="compass-e">E</span>
            <span class="compass-s">S</span>
            <span class="compass-w">W</span>
          </div>
        </div>
      </div>
    </div>
    
    <!-- Abilities (Right) -->
    <div class="hud-abilities-section">
      <div class="abilities-row">
        <div class="ability-slot" data-key="Q">
          <div class="ability-icon">
            <img src="ability-boost.png" alt="Boost">
          </div>
          <div class="ability-cooldown" style="--cooldown: 0%"></div>
          <span class="ability-key">Q</span>
        </div>
        <div class="ability-slot" data-key="W">
          <div class="ability-icon">
            <img src="ability-shield.png" alt="Shield">
          </div>
          <div class="ability-cooldown" style="--cooldown: 45%"></div>
          <span class="ability-key">W</span>
        </div>
        <div class="ability-slot" data-key="E">
          <div class="ability-icon">
            <img src="ability-missile.png" alt="Missile">
          </div>
          <div class="ability-cooldown" style="--cooldown: 0%"></div>
          <span class="ability-key">E</span>
        </div>
        <div class="ability-slot ultimate" data-key="R">
          <div class="ability-icon">
            <img src="ability-ultimate.png" alt="Ultimate">
          </div>
          <div class="ability-cooldown" style="--cooldown: 78%"></div>
          <span class="ability-key">R</span>
          <div class="ultimate-charge">
            <div class="charge-fill" style="width: 22%"></div>
          </div>
        </div>
      </div>
    </div>
  </div>
  
  <!-- Kill Feed -->
  <div class="kill-feed" id="kill-feed">
    <div class="kill-entry">
      <span class="killer">Player1</span>
      <span class="weapon-icon">⚔</span>
      <span class="victim">Enemy2</span>
    </div>
  </div>
  
  <!-- Notifications -->
  <div class="hud-notifications" id="notifications"></div>
</div>
```

### HUD CSS

```css
/* HUD Container */
.hud-container {
  position: fixed;
  inset: 0;
  pointer-events: none;
  z-index: var(--z-ui);
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  padding: var(--space-4);
}

/* Enable pointer events for interactive HUD elements */
.hud-container .interactive {
  pointer-events: auto;
}

/* Top Section */
.hud-top {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
}

/* Health Section */
.hud-health-section {
  display: flex;
  align-items: center;
}

.ship-status {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  background: var(--bg-panel);
  backdrop-filter: blur(10px);
  border-radius: var(--radius-lg);
  border: 1px solid rgba(255, 255, 255, 0.1);
}

.ship-icon {
  width: 40px;
  height: 40px;
  color: var(--color-primary);
}

.ship-icon svg {
  width: 100%;
  height: 100%;
  filter: drop-shadow(var(--glow-primary));
}

.status-bars {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  min-width: 200px;
}

.bar-container {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.bar-label {
  display: flex;
  justify-content: space-between;
  font-family: var(--font-display);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
}

.bar-label span:first-child {
  color: var(--text-secondary);
  letter-spacing: 1px;
}

.bar-value {
  color: var(--text-primary);
}

.bar-track {
  position: relative;
  height: 8px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: var(--radius-full);
  overflow: hidden;
}

.bar-fill {
  height: 100%;
  border-radius: var(--radius-full);
  transition: width 0.2s ease;
}

.shield-bar .bar-fill {
  background: linear-gradient(90deg, var(--color-shield), var(--color-primary));
  box-shadow: 0 0 10px rgba(0, 187, 249, 0.5);
}

.health-bar .bar-fill {
  background: linear-gradient(90deg, var(--color-health), var(--color-success));
  box-shadow: 0 0 10px rgba(0, 245, 212, 0.5);
}

.bar-segments {
  position: absolute;
  inset: 0;
  background: repeating-linear-gradient(
    90deg,
    transparent,
    transparent calc(25% - 1px),
    rgba(0, 0, 0, 0.3) calc(25% - 1px),
    rgba(0, 0, 0, 0.3) 25%
  );
}

/* Score Section */
.hud-score-section {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: var(--space-2);
}

.match-timer {
  padding: var(--space-2) var(--space-4);
  background: var(--bg-panel);
  backdrop-filter: blur(10px);
  border-radius: var(--radius-md);
  border: 1px solid rgba(255, 255, 255, 0.1);
}

.timer-display {
  display: flex;
  flex-direction: column;
  align-items: center;
}

.timer-value {
  font-family: var(--font-display);
  font-size: var(--text-2xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
}

.timer-phase {
  font-size: var(--text-xs);
  color: var(--text-secondary);
  letter-spacing: 2px;
}

.team-score {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-4);
  background: var(--bg-panel);
  backdrop-filter: blur(10px);
  border-radius: var(--radius-md);
  border: 1px solid rgba(255, 255, 255, 0.1);
}

.score-team {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.score-team.blue .score-value {
  color: var(--color-primary);
}

.score-team.red .score-value {
  color: var(--color-danger);
}

.team-name {
  font-family: var(--font-display);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-secondary);
  letter-spacing: 1px;
}

.score-value {
  font-family: var(--font-display);
  font-size: var(--text-2xl);
  font-weight: var(--font-bold);
}

.score-divider {
  font-family: var(--font-display);
  font-size: var(--text-xl);
  color: var(--text-muted);
}

/* Center Crosshair */
.hud-center {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
}

.crosshair {
  position: relative;
  width: 40px;
  height: 40px;
}

.crosshair-dot {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  width: 4px;
  height: 4px;
  background: var(--color-primary);
  border-radius: var(--radius-full);
  box-shadow: var(--glow-primary);
}

.crosshair-lines {
  position: absolute;
  inset: 0;
}

.crosshair-lines .line {
  position: absolute;
  background: rgba(255, 255, 255, 0.8);
  transition: all 0.1s ease;
}

.crosshair-lines .line.top {
  top: 0;
  left: 50%;
  transform: translateX(-50%);
  width: 2px;
  height: 12px;
}

.crosshair-lines .line.bottom {
  bottom: 0;
  left: 50%;
  transform: translateX(-50%);
  width: 2px;
  height: 12px;
}

.crosshair-lines .line.left {
  left: 0;
  top: 50%;
  transform: translateY(-50%);
  width: 12px;
  height: 2px;
}

.crosshair-lines .line.right {
  right: 0;
  top: 50%;
  transform: translateY(-50%);
  width: 12px;
  height: 2px;
}

/* Hit Marker */
.hit-marker {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  width: 60px;
  height: 60px;
  opacity: 0;
  pointer-events: none;
}

.hit-marker.active {
  animation: hit-marker-anim 0.2s ease-out;
}

@keyframes hit-marker-anim {
  0% { opacity: 1; transform: translate(-50%, -50%) scale(0.8); }
  50% { opacity: 1; transform: translate(-50%, -50%) scale(1.1); }
  100% { opacity: 0; transform: translate(-50%, -50%) scale(1); }
}

.hit-line {
  position: absolute;
  width: 12px;
  height: 2px;
  background: var(--color-danger);
  box-shadow: var(--glow-danger);
}

.hit-line.tl {
  top: 10px;
  left: 10px;
  transform: rotate(45deg);
}

.hit-line.tr {
  top: 10px;
  right: 10px;
  transform: rotate(-45deg);
}

.hit-line.bl {
  bottom: 10px;
  left: 10px;
  transform: rotate(-45deg);
}

.hit-line.br {
  bottom: 10px;
  right: 10px;
  transform: rotate(45deg);
}

/* Bottom Section */
.hud-bottom {
  display: flex;
  justify-content: space-between;
  align-items: flex-end;
}

/* Weapon Section */
.hud-weapon-section {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.weapon-display {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  background: var(--bg-panel);
  backdrop-filter: blur(10px);
  border-radius: var(--radius-lg);
  border: 1px solid rgba(255, 255, 255, 0.1);
}

.weapon-icon {
  width: 48px;
  height: 48px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.weapon-icon img {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}

.weapon-info {
  display: flex;
  flex-direction: column;
}

.weapon-name {
  font-family: var(--font-display);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-secondary);
  letter-spacing: 1px;
}

.ammo-display {
  display: flex;
  align-items: baseline;
  gap: var(--space-1);
}

.ammo-current {
  font-family: var(--font-display);
  font-size: var(--text-3xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
}

.ammo-divider {
  font-size: var(--text-xl);
  color: var(--text-muted);
}

.ammo-max {
  font-family: var(--font-display);
  font-size: var(--text-lg);
  font-weight: var(--font-semibold);
  color: var(--text-secondary);
}

.credits-display {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-4);
  background: rgba(255, 215, 0, 0.1);
  border-radius: var(--radius-md);
  border: 1px solid rgba(255, 215, 0, 0.3);
}

.credits-display i {
  color: var(--color-credits);
}

.credits-value {
  font-family: var(--font-display);
  font-weight: var(--font-semibold);
  color: var(--color-credits);
}

/* Minimap Section */
.hud-minimap-section {
  position: absolute;
  left: 50%;
  bottom: var(--space-4);
  transform: translateX(-50%);
}

.minimap-container {
  position: relative;
  width: 180px;
  height: 180px;
  background: var(--bg-panel);
  backdrop-filter: blur(10px);
  border-radius: var(--radius-full);
  border: 2px solid rgba(255, 255, 255, 0.2);
  overflow: hidden;
}

.minimap-container::before {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: var(--radius-full);
  background: 
    radial-gradient(circle at center, transparent 30%, rgba(0, 0, 0, 0.5) 70%);
  pointer-events: none;
}

#minimap {
  width: 100%;
  height: 100%;
}

.minimap-overlay {
  position: absolute;
  inset: 0;
  pointer-events: none;
}

.compass {
  position: absolute;
  inset: 0;
}

.compass span {
  position: absolute;
  font-family: var(--font-display);
  font-size: var(--text-xs);
  font-weight: var(--font-bold);
  color: var(--text-muted);
}

.compass-n { top: 8px; left: 50%; transform: translateX(-50%); }
.compass-e { right: 8px; top: 50%; transform: translateY(-50%); }
.compass-s { bottom: 8px; left: 50%; transform: translateX(-50%); }
.compass-w { left: 8px; top: 50%; transform: translateY(-50%); }

/* Abilities Section */
.hud-abilities-section {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
}

.abilities-row {
  display: flex;
  gap: var(--space-2);
  padding: var(--space-3);
  background: var(--bg-panel);
  backdrop-filter: blur(10px);
  border-radius: var(--radius-lg);
  border: 1px solid rgba(255, 255, 255, 0.1);
}

.ability-slot {
  position: relative;
  width: 56px;
  height: 56px;
  border-radius: var(--radius-md);
  overflow: hidden;
  border: 2px solid rgba(255, 255, 255, 0.2);
  transition: all 0.2s ease;
}

.ability-slot:hover {
  border-color: var(--color-primary);
  transform: scale(1.05);
}

.ability-slot.ultimate {
  border-color: var(--color-tertiary);
}

.ability-icon {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.5);
}

.ability-icon img {
  width: 70%;
  height: 70%;
  object-fit: contain;
}

.ability-cooldown {
  position: absolute;
  inset: 0;
  background: rgba(0, 0, 0, 0.8);
  clip-path: polygon(
    50% 50%,
    50% 0%,
    calc(50% + 50% * sin(var(--cooldown) * 3.6deg)) calc(50% - 50% * cos(var(--cooldown) * 3.6deg))
  );
}

.ability-key {
  position: absolute;
  bottom: 2px;
  right: 4px;
  font-family: var(--font-display);
  font-size: var(--text-xs);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  text-shadow: 0 0 4px rgba(0, 0, 0, 0.8);
}

.ultimate-charge {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  height: 4px;
  background: rgba(0, 0, 0, 0.5);
}

.charge-fill {
  height: 100%;
  background: linear-gradient(90deg, var(--color-tertiary), var(--color-primary));
  transition: width 0.3s ease;
}

/* Kill Feed */
.kill-feed {
  position: absolute;
  top: 100px;
  right: var(--space-4);
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  max-width: 300px;
}

.kill-entry {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--bg-panel);
  backdrop-filter: blur(10px);
  border-radius: var(--radius-md);
  border-left: 3px solid var(--color-primary);
  animation: kill-entry-in 0.3s ease;
}

@keyframes kill-entry-in {
  from { opacity: 0; transform: translateX(20px); }
  to { opacity: 1; transform: translateX(0); }
}

.kill-entry .killer {
  font-weight: var(--font-semibold);
  color: var(--color-primary);
}

.kill-entry .victim {
  font-weight: var(--font-semibold);
  color: var(--color-danger);
}

.kill-entry .weapon-icon {
  color: var(--text-muted);
}

/* Notifications */
.hud-notifications {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  pointer-events: none;
}

.notification {
  padding: var(--space-3) var(--space-6);
  background: var(--bg-panel);
  backdrop-filter: blur(10px);
  border-radius: var(--radius-md);
  font-family: var(--font-display);
  font-weight: var(--font-semibold);
  animation: notification-in 0.3s ease, notification-out 0.3s ease 2.7s;
}

.notification.kill {
  border: 2px solid var(--color-primary);
  color: var(--color-primary);
}

.notification.death {
  border: 2px solid var(--color-danger);
  color: var(--color-danger);
}

@keyframes notification-in {
  from { opacity: 0; transform: scale(0.8); }
  to { opacity: 1; transform: scale(1); }
}

@keyframes notification-out {
  to { opacity: 0; transform: scale(0.8); }
}
```

---

## In-Game Menus

### Buy Menu (CS2-Style)

```html
<!-- Buy Menu -->
<div id="buy-menu" class="buy-menu">
  <div class="buy-menu-header">
    <h2>ARSENAL</h2>
    <div class="buy-credits">
      <span>CREDITS:</span>
      <span class="credits-amount">4,250</span>
    </div>
  </div>
  
  <div class="buy-menu-content">
    <!-- Categories -->
    <div class="buy-categories">
      <button class="category-btn active" data-category="primary">
        <span class="cat-icon">🔫</span>
        <span>PRIMARY</span>
      </button>
      <button class="category-btn" data-category="secondary">
        <span class="cat-icon">🔫</span>
        <span>SECONDARY</span>
      </button>
      <button class="category-btn" data-category="abilities">
        <span class="cat-icon">⚡</span>
        <span>ABILITIES</span>
      </button>
      <button class="category-btn" data-category="upgrades">
        <span class="cat-icon">⬆</span>
        <span>UPGRADES</span>
      </button>
    </div>
    
    <!-- Items Grid -->
    <div class="buy-items">
      <div class="buy-item" data-item="plasma-rifle">
        <div class="item-image">
          <img src="plasma-rifle.png" alt="Plasma Rifle">
        </div>
        <div class="item-info">
          <span class="item-name">PLASMA RIFLE</span>
          <div class="item-stats">
            <div class="stat">
              <span class="stat-bar"><span style="width: 70%"></span></span>
              <span>DAMAGE</span>
            </div>
            <div class="stat">
              <span class="stat-bar"><span style="width: 60%"></span></span>
              <span>FIRE RATE</span>
            </div>
            <div class="stat">
              <span class="stat-bar"><span style="width: 80%"></span></span>
              <span>RANGE</span>
            </div>
          </div>
        </div>
        <div class="item-price">
          <span class="price">2,500</span>
        </div>
        <div class="item-key">1</div>
      </div>
      
      <!-- More items... -->
    </div>
  </div>
  
  <div class="buy-menu-footer">
    <div class="buy-hints">
      <span>[CLICK] Purchase</span>
      <span>[1-9] Quick Buy</span>
      <span>[B] Close</span>
    </div>
  </div>
</div>
```

### Scoreboard (TAB Menu)

```html
<!-- Scoreboard -->
<div id="scoreboard" class="scoreboard">
  <div class="scoreboard-header">
    <div class="match-info">
      <span class="map-name">NEBULA STATION</span>
      <span class="match-mode">Team Deathmatch</span>
    </div>
    <div class="match-score">
      <span class="team-blue">8</span>
      <span class="score-divider">:</span>
      <span class="team-red">6</span>
    </div>
    <div class="match-timer">03:42</div>
  </div>
  
  <div class="scoreboard-teams">
    <!-- Blue Team -->
    <div class="team-table blue-team">
      <div class="table-header">
        <span class="col-rank">#</span>
        <span class="col-player">PLAYER</span>
        <span class="col-score">SCORE</span>
        <span class="col-kills">K</span>
        <span class="col-deaths">D</span>
        <span class="col-assists">A</span>
        <span class="col-ping">PING</span>
      </div>
      <div class="table-body">
        <div class="player-row local-player">
          <span class="col-rank">1</span>
          <span class="col-player">
            <img src="avatar.png" class="player-avatar-small">
            <span class="player-name">You</span>
          </span>
          <span class="col-score">2,450</span>
          <span class="col-kills">12</span>
          <span class="col-deaths">4</span>
          <span class="col-assists">6</span>
          <span class="col-ping">24</span>
        </div>
        <!-- More players... -->
      </div>
    </div>
    
    <!-- Red Team -->
    <div class="team-table red-team">
      <!-- Same structure... -->
    </div>
  </div>
</div>
```

### Pause Menu

```html
<!-- Pause Menu -->
<div id="pause-menu" class="pause-menu">
  <div class="pause-overlay"></div>
  <div class="pause-content">
    <h2>GAME PAUSED</h2>
    <nav class="pause-nav">
      <button class="pause-btn" data-action="resume">
        <span>RESUME</span>
      </button>
      <button class="pause-btn" data-action="settings">
        <span>SETTINGS</span>
      </button>
      <button class="pause-btn" data-action="surrender">
        <span>SURRENDER</span>
      </button>
      <button class="pause-btn danger" data-action="quit">
        <span>QUIT TO MENU</span>
      </button>
    </nav>
  </div>
</div>
```

### In-Game Menu CSS

```css
/* Buy Menu */
.buy-menu {
  position: fixed;
  inset: 0;
  display: flex;
  flex-direction: column;
  background: rgba(10, 10, 15, 0.95);
  backdrop-filter: blur(20px);
  z-index: var(--z-overlay);
  opacity: 0;
  visibility: hidden;
  transition: all 0.3s ease;
}

.buy-menu.active {
  opacity: 1;
  visibility: visible;
}

.buy-menu-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--space-4) var(--space-8);
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
}

.buy-menu-header h2 {
  font-family: var(--font-display);
  font-size: var(--text-2xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  letter-spacing: 4px;
}

.buy-credits {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-4);
  background: rgba(255, 215, 0, 0.1);
  border-radius: var(--radius-md);
  border: 1px solid rgba(255, 215, 0, 0.3);
}

.buy-credits span:first-child {
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.credits-amount {
  font-family: var(--font-display);
  font-size: var(--text-xl);
  font-weight: var(--font-bold);
  color: var(--color-credits);
}

.buy-menu-content {
  flex: 1;
  display: flex;
  padding: var(--space-6);
  gap: var(--space-6);
}

.buy-categories {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  width: 180px;
}

.category-btn {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-4);
  background: var(--bg-card);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: var(--radius-md);
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.2s ease;
}

.category-btn:hover,
.category-btn.active {
  background: rgba(0, 212, 255, 0.1);
  border-color: var(--color-primary);
  color: var(--text-primary);
}

.category-btn .cat-icon {
  font-size: var(--text-xl);
}

.category-btn span:last-child {
  font-family: var(--font-display);
  font-weight: var(--font-semibold);
  letter-spacing: 1px;
}

.buy-items {
  flex: 1;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: var(--space-4);
  overflow-y: auto;
}

.buy-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-4);
  background: var(--bg-card);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: var(--radius-lg);
  cursor: pointer;
  transition: all 0.2s ease;
}

.buy-item:hover {
  border-color: var(--color-primary);
  transform: translateY(-2px);
  box-shadow: var(--glow-primary);
}

.buy-item.owned {
  border-color: var(--color-success);
  opacity: 0.7;
}

.buy-item .item-image {
  width: 80px;
  height: 80px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.3);
  border-radius: var(--radius-md);
}

.buy-item .item-image img {
  max-width: 90%;
  max-height: 90%;
  object-fit: contain;
}

.buy-item .item-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.item-name {
  font-family: var(--font-display);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.item-stats {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.item-stats .stat {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.item-stats .stat span:last-child {
  font-size: var(--text-xs);
  color: var(--text-muted);
  min-width: 60px;
}

.stat-bar {
  flex: 1;
  height: 4px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: var(--radius-full);
  overflow: hidden;
}

.stat-bar span {
  display: block;
  height: 100%;
  background: linear-gradient(90deg, var(--color-primary), var(--color-tertiary));
  border-radius: var(--radius-full);
}

.item-price {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
}

.item-price .price {
  font-family: var(--font-display);
  font-size: var(--text-xl);
  font-weight: var(--font-bold);
  color: var(--color-credits);
}

.item-key {
  position: absolute;
  top: var(--space-2);
  right: var(--space-2);
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(255, 255, 255, 0.1);
  border-radius: var(--radius-sm);
  font-family: var(--font-display);
  font-size: var(--text-xs);
  font-weight: var(--font-bold);
  color: var(--text-secondary);
}

.buy-menu-footer {
  padding: var(--space-4) var(--space-8);
  border-top: 1px solid rgba(255, 255, 255, 0.1);
}

.buy-hints {
  display: flex;
  gap: var(--space-6);
}

.buy-hints span {
  font-size: var(--text-sm);
  color: var(--text-muted);
}

/* Scoreboard */
.scoreboard {
  position: fixed;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  width: 90%;
  max-width: 1000px;
  background: var(--bg-panel);
  backdrop-filter: blur(20px);
  border-radius: var(--radius-xl);
  border: 1px solid rgba(255, 255, 255, 0.1);
  z-index: var(--z-overlay);
  opacity: 0;
  visibility: hidden;
  transition: all 0.2s ease;
}

.scoreboard.active {
  opacity: 1;
  visibility: visible;
}

.scoreboard-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--space-4) var(--space-6);
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
}

.match-info {
  display: flex;
  flex-direction: column;
}

.map-name {
  font-family: var(--font-display);
  font-weight: var(--font-bold);
  color: var(--text-primary);
}

.match-mode {
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.match-score {
  display: flex;
  align-items: center;
  gap: var(--space-4);
  font-family: var(--font-display);
  font-size: var(--text-4xl);
  font-weight: var(--font-bold);
}

.match-score .team-blue {
  color: var(--color-primary);
}

.match-score .team-red {
  color: var(--color-danger);
}

.score-divider {
  color: var(--text-muted);
}

.match-timer {
  font-family: var(--font-display);
  font-size: var(--text-xl);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.scoreboard-teams {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  padding: var(--space-4) var(--space-6);
}

.team-table {
  border-radius: var(--radius-md);
  overflow: hidden;
}

.team-table.blue-team {
  border-left: 4px solid var(--color-primary);
}

.team-table.red-team {
  border-left: 4px solid var(--color-danger);
}

.table-header {
  display: grid;
  grid-template-columns: 40px 1fr 80px 50px 50px 50px 60px;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: rgba(0, 0, 0, 0.3);
}

.table-header span {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-muted);
  letter-spacing: 1px;
}

.table-body {
  display: flex;
  flex-direction: column;
}

.player-row {
  display: grid;
  grid-template-columns: 40px 1fr 80px 50px 50px 50px 60px;
  gap: var(--space-2);
  align-items: center;
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid rgba(255, 255, 255, 0.05);
  transition: background 0.2s ease;
}

.player-row:hover {
  background: rgba(255, 255, 255, 0.05);
}

.player-row.local-player {
  background: rgba(0, 212, 255, 0.1);
}

.col-rank {
  font-family: var(--font-display);
  font-weight: var(--font-bold);
  color: var(--text-muted);
}

.col-player {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.player-avatar-small {
  width: 24px;
  height: 24px;
  border-radius: var(--radius-full);
}

.player-name {
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.col-score {
  font-family: var(--font-display);
  font-weight: var(--font-bold);
  color: var(--color-primary);
}

.col-kills,
.col-deaths,
.col-assists {
  text-align: center;
  color: var(--text-secondary);
}

.col-ping {
  text-align: center;
  font-size: var(--text-sm);
  color: var(--text-muted);
}

/* Pause Menu */
.pause-menu {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: var(--z-modal);
  opacity: 0;
  visibility: hidden;
  transition: all 0.3s ease;
}

.pause-menu.active {
  opacity: 1;
  visibility: visible;
}

.pause-overlay {
  position: absolute;
  inset: 0;
  background: rgba(0, 0, 0, 0.7);
  backdrop-filter: blur(5px);
}

.pause-content {
  position: relative;
  padding: var(--space-8) var(--space-12);
  background: var(--bg-panel);
  backdrop-filter: blur(20px);
  border-radius: var(--radius-xl);
  border: 1px solid rgba(255, 255, 255, 0.1);
  text-align: center;
}

.pause-content h2 {
  font-family: var(--font-display);
  font-size: var(--text-3xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  margin-bottom: var(--space-6);
  letter-spacing: 4px;
}

.pause-nav {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.pause-btn {
  padding: var(--space-4) var(--space-12);
  background: var(--bg-card);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: var(--radius-md);
  font-family: var(--font-display);
  font-size: var(--text-lg);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  letter-spacing: 2px;
  cursor: pointer;
  transition: all 0.2s ease;
}

.pause-btn:hover {
  background: rgba(0, 212, 255, 0.1);
  border-color: var(--color-primary);
  transform: translateX(5px);
}

.pause-btn.danger:hover {
  background: rgba(255, 0, 110, 0.1);
  border-color: var(--color-danger);
}
```

---

## Career Mode UI

### Profile Page

```html
<!-- Career Profile -->
<div id="career-profile" class="career-profile">
  <div class="profile-header">
    <div class="profile-avatar-large">
      <img src="avatar-large.png" alt="Player">
      <div class="level-badge">42</div>
    </div>
    <div class="profile-info">
      <h1>CommanderX</h1>
      <div class="profile-tags">
        <span class="tag rank">Astral Commander</span>
        <span class="tag clan">[NEBULA]</span>
      </div>
      <div class="profile-badges">
        <div class="badge" title="First Blood Master">
          <img src="badge-firstblood.png">
        </div>
        <div class="badge" title="Ace Pilot">
          <img src="badge-ace.png">
        </div>
        <div class="badge" title="Team Player">
          <img src="badge-team.png">
        </div>
      </div>
    </div>
    <div class="profile-stats-summary">
      <div class="summary-item">
        <span class="value">247</span>
        <span class="label">Matches</span>
      </div>
      <div class="summary-item">
        <span class="value">62%</span>
        <span class="label">Win Rate</span>
      </div>
      <div class="summary-item">
        <span class="value">3.2</span>
        <span class="label">K/D</span>
      </div>
    </div>
  </div>
  
  <div class="profile-content">
    <!-- Stats Tabs -->
    <div class="stats-tabs">
      <button class="tab-btn active" data-tab="overview">Overview</button>
      <button class="tab-btn" data-tab="weapons">Weapons</button>
      <button class="tab-btn" data-tab="ships">Ships</button>
      <button class="tab-btn" data-tab="history">History</button>
    </div>
    
    <!-- Overview Tab -->
    <div class="tab-content active" id="overview-tab">
      <div class="stats-grid-detailed">
        <div class="stat-card-detailed">
          <div class="stat-header">
            <span class="stat-title">Combat</span>
          </div>
          <div class="stat-body">
            <div class="stat-row">
              <span>Kills</span>
              <span class="value">1,847</span>
            </div>
            <div class="stat-row">
              <span>Deaths</span>
              <span class="value">577</span>
            </div>
            <div class="stat-row">
              <span>Assists</span>
              <span class="value">892</span>
            </div>
            <div class="stat-row highlight">
              <span>K/D Ratio</span>
              <span class="value">3.2</span>
            </div>
          </div>
        </div>
        
        <div class="stat-card-detailed">
          <div class="stat-header">
            <span class="stat-title">Performance</span>
          </div>
          <div class="stat-body">
            <div class="stat-row">
              <span>Accuracy</span>
              <span class="value">68.4%</span>
            </div>
            <div class="stat-row">
              <span>Headshots</span>
              <span class="value">423</span>
            </div>
            <div class="stat-row">
              <span>Damage Dealt</span>
              <span class="value">2.4M</span>
            </div>
            <div class="stat-row">
              <span>Best Streak</span>
              <span class="value">18</span>
            </div>
          </div>
        </div>
        
        <div class="stat-card-detailed wide">
          <div class="stat-header">
            <span class="stat-title">Match History</span>
          </div>
          <div class="match-history-chart">
            <!-- Chart visualization -->
          </div>
        </div>
      </div>
    </div>
  </div>
</div>
```

### Progression Tree

```html
<!-- Progression Tree -->
<div id="progression-tree" class="progression-tree">
  <div class="tree-header">
    <h2>SHIP MASTERY</h2>
    <div class="mastery-progress">
      <span>Overall Progress: 47%</span>
      <div class="progress-bar-large">
        <div class="progress-fill" style="width: 47%"></div>
      </div>
    </div>
  </div>
  
  <div class="tree-content">
    <div class="mastery-branch">
      <div class="branch-header">
        <img src="ship-interceptor.png" alt="Interceptor">
        <h3>INTERCEPTOR</h3>
        <span class="mastery-level">Level 8/10</span>
      </div>
      <div class="skill-tree">
        <div class="skill-node unlocked" data-skill="speed-boost">
          <div class="node-icon">⚡</div>
          <span class="node-name">Speed Boost</span>
        </div>
        <div class="skill-node unlocked" data-skill="rapid-fire">
          <div class="node-icon">🔫</div>
          <span class="node-name">Rapid Fire</span>
        </div>
        <div class="skill-node locked" data-skill="stealth">
          <div class="node-icon">👻</div>
          <span class="node-name">Stealth</span>
          <div class="lock-overlay">
            <span>Unlock at Level 9</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</div>
```

---

## Mobile Adaptations

### Mobile HUD Layout

```
┌─────────────────────────────────────┐
│  [Health]              [Timer]     │
│  ════════               [2:34]     │
│                                     │
│                                     │
│                                     │
│        [Simplified Crosshair]       │
│                                     │
│                                     │
│                                     │
│  [Joystick]              [Fire]    │
│  ○                       [🔴]      │
│  [Ability1] [Ability2] [Ability3]  │
└─────────────────────────────────────┘
```

### Mobile Touch Controls CSS

```css
/* Mobile HUD */
@media (max-width: 768px) {
  .hud-container {
    padding: var(--space-2);
  }
  
  /* Simplified top section */
  .hud-top {
    flex-direction: row;
    gap: var(--space-2);
  }
  
  .hud-health-section {
    flex: 1;
  }
  
  .ship-status {
    padding: var(--space-2);
    min-width: auto;
  }
  
  .ship-icon {
    width: 32px;
    height: 32px;
  }
  
  .status-bars {
    min-width: 120px;
  }
  
  .bar-label span:first-child {
    display: none;
  }
  
  .hud-score-section {
    flex-direction: row;
    gap: var(--space-2);
  }
  
  .match-timer {
    padding: var(--space-1) var(--space-2);
  }
  
  .timer-value {
    font-size: var(--text-lg);
  }
  
  .timer-phase {
    display: none;
  }
  
  .team-score {
    padding: var(--space-1) var(--space-2);
  }
  
  .team-name {
    display: none;
  }
  
  .score-value {
    font-size: var(--text-xl);
  }
  
  /* Simplified crosshair */
  .crosshair {
    width: 30px;
    height: 30px;
  }
  
  .crosshair-lines .line {
    display: none;
  }
  
  /* Bottom section - touch controls */
  .hud-bottom {
    position: relative;
    height: 200px;
  }
  
  /* Hide desktop elements */
  .hud-weapon-section,
  .hud-minimap-section,
  .hud-abilities-section {
    display: none;
  }
  
  /* Show touch controls */
  .touch-controls {
    display: flex;
    position: absolute;
    inset: 0;
  }
  
  /* Virtual Joystick */
  .virtual-joystick {
    position: absolute;
    bottom: 20px;
    left: 20px;
    width: 120px;
    height: 120px;
  }
  
  .joystick-base {
    position: absolute;
    inset: 0;
    background: rgba(255, 255, 255, 0.1);
    border: 2px solid rgba(255, 255, 255, 0.3);
    border-radius: var(--radius-full);
  }
  
  .joystick-stick {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 50px;
    height: 50px;
    background: rgba(0, 212, 255, 0.5);
    border: 2px solid var(--color-primary);
    border-radius: var(--radius-full);
    box-shadow: var(--glow-primary);
  }
  
  /* Fire Button */
  .fire-button {
    position: absolute;
    bottom: 40px;
    right: 30px;
    width: 80px;
    height: 80px;
    background: rgba(255, 0, 110, 0.3);
    border: 3px solid var(--color-danger);
    border-radius: var(--radius-full);
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: var(--glow-danger);
    touch-action: none;
  }
  
  .fire-button:active {
    background: rgba(255, 0, 110, 0.5);
    transform: scale(0.95);
  }
  
  .fire-button::after {
    content: '';
    width: 30px;
    height: 30px;
    background: var(--color-danger);
    border-radius: var(--radius-full);
  }
  
  /* Mobile Ability Buttons */
  .mobile-abilities {
    position: absolute;
    bottom: 20px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    gap: var(--space-3);
  }
  
  .mobile-ability {
    width: 56px;
    height: 56px;
    background: var(--bg-panel);
    border: 2px solid rgba(255, 255, 255, 0.2);
    border-radius: var(--radius-md);
    display: flex;
    align-items: center;
    justify-content: center;
    touch-action: none;
  }
  
  .mobile-ability:active {
    border-color: var(--color-primary);
    background: rgba(0, 212, 255, 0.2);
  }
  
  .mobile-ability img {
    width: 70%;
    height: 70%;
    object-fit: contain;
  }
  
  /* Kill feed - smaller */
  .kill-feed {
    top: 60px;
    right: var(--space-2);
    max-width: 200px;
  }
  
  .kill-entry {
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-xs);
  }
}

/* Tablet adjustments */
@media (min-width: 769px) and (max-width: 1024px) {
  .hud-container {
    padding: var(--space-3);
  }
  
  .status-bars {
    min-width: 160px;
  }
  
  .minimap-container {
    width: 140px;
    height: 140px;
  }
  
  .ability-slot {
    width: 48px;
    height: 48px;
  }
}
```

---

## Implementation Guide

### File Structure

```
/ui
├── css/
│   ├── variables.css      # CSS custom properties
│   ├── main-menu.css      # Main menu styles
│   ├── hud.css            # HUD styles
│   ├── ingame-menus.css   # Buy menu, scoreboard, pause
│   ├── career.css         # Career mode styles
│   ├── mobile.css         # Mobile adaptations
│   └── animations.css     # Shared animations
├── js/
│   ├── ui-manager.js      # Main UI controller
│   ├── menu-system.js     # Menu navigation
│   ├── hud-controller.js  # HUD updates
│   ├── buy-menu.js        # Buy menu logic
│   ├── touch-controls.js  # Mobile touch handling
│   └── notifications.js   # Toast notifications
├── components/
│   ├── main-menu.html
│   ├── hud.html
│   ├── buy-menu.html
│   ├── scoreboard.html
│   └── pause-menu.html
└── assets/
    ├── icons/
    ├── fonts/
    └── sounds/
```

### JavaScript UI Manager

```javascript
// ui-manager.js - Main UI Controller
class UIManager {
  constructor() {
    this.currentScreen = 'main-menu';
    this.screens = new Map();
    this.isGameActive = false;
    this.isPaused = false;
    
    this.init();
  }
  
  init() {
    this.registerScreens();
    this.bindEvents();
    this.showScreen('main-menu');
  }
  
  registerScreens() {
    this.screens.set('main-menu', document.getElementById('main-menu'));
    this.screens.set('hud', document.getElementById('hud'));
    this.screens.set('buy-menu', document.getElementById('buy-menu'));
    this.screens.set('scoreboard', document.getElementById('scoreboard'));
    this.screens.set('pause-menu', document.getElementById('pause-menu'));
  }
  
  bindEvents() {
    // Keyboard shortcuts
    document.addEventListener('keydown', (e) => this.handleKeydown(e));
    
    // Window resize
    window.addEventListener('resize', () => this.handleResize());
    
    // Game state changes
    document.addEventListener('game-start', () => this.onGameStart());
    document.addEventListener('game-end', () => this.onGameEnd());
  }
  
  handleKeydown(e) {
    if (!this.isGameActive) return;
    
    switch(e.code) {
      case 'Tab':
        e.preventDefault();
        this.toggleScoreboard();
        break;
      case 'KeyB':
        this.toggleBuyMenu();
        break;
      case 'Escape':
        this.togglePause();
        break;
    }
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
    }
  }
  
  onGameStart() {
    this.isGameActive = true;
    this.showScreen('hud');
    HUDController.init();
  }
  
  onGameEnd() {
    this.isGameActive = false;
    this.showScreen('main-menu');
  }
  
  toggleScoreboard() {
    const scoreboard = this.screens.get('scoreboard');
    if (scoreboard) {
      scoreboard.classList.toggle('active');
    }
  }
  
  toggleBuyMenu() {
    const buyMenu = this.screens.get('buy-menu');
    if (buyMenu) {
      buyMenu.classList.toggle('active');
      // Pause game when buy menu is open
      document.dispatchEvent(new CustomEvent(
        buyMenu.classList.contains('active') ? 'game-pause' : 'game-resume'
      ));
    }
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
  }
  
  handleResize() {
    // Update responsive elements
    const isMobile = window.innerWidth <= 768;
    document.body.classList.toggle('mobile', isMobile);
  }
}

// HUD Controller
class HUDController {
  static instance = null;
  
  static init() {
    if (!HUDController.instance) {
      HUDController.instance = new HUDController();
    }
    return HUDController.instance;
  }
  
  constructor() {
    this.elements = {
      healthBar: document.querySelector('.health-bar .bar-fill'),
      healthValue: document.querySelector('.health-bar .bar-value'),
      shieldBar: document.querySelector('.shield-bar .bar-fill'),
      shieldValue: document.querySelector('.shield-bar .bar-value'),
      ammoCurrent: document.querySelector('.ammo-current'),
      ammoMax: document.querySelector('.ammo-max'),
      credits: document.querySelector('.credits-value'),
      timer: document.querySelector('.timer-value'),
      scoreBlue: document.querySelector('.team-blue .score-value'),
      scoreRed: document.querySelector('.team-red .score-value'),
      killFeed: document.getElementById('kill-feed'),
      notifications: document.getElementById('notifications')
    };
    
    this.bindGameEvents();
  }
  
  bindGameEvents() {
    document.addEventListener('player-health-change', (e) => this.updateHealth(e.detail));
    document.addEventListener('player-shield-change', (e) => this.updateShield(e.detail));
    document.addEventListener('player-ammo-change', (e) => this.updateAmmo(e.detail));
    document.addEventListener('player-credits-change', (e) => this.updateCredits(e.detail));
    document.addEventListener('match-timer-update', (e) => this.updateTimer(e.detail));
    document.addEventListener('score-update', (e) => this.updateScore(e.detail));
    document.addEventListener('kill-event', (e) => this.addKillFeed(e.detail));
  }
  
  updateHealth({ current, max }) {
    const percentage = (current / max) * 100;
    this.elements.healthBar.style.width = `${percentage}%`;
    this.elements.healthValue.textContent = `${current}/${max}`;
    
    // Warning color when low
    if (percentage < 25) {
      this.elements.healthBar.style.background = 'var(--color-danger)';
    }
  }
  
  updateShield({ current, max }) {
    const percentage = (current / max) * 100;
    this.elements.shieldBar.style.width = `${percentage}%`;
    this.elements.shieldValue.textContent = `${current}/${max}`;
  }
  
  updateAmmo({ current, max }) {
    this.elements.ammoCurrent.textContent = current;
    this.elements.ammoMax.textContent = max;
  }
  
  updateCredits(amount) {
    this.elements.credits.textContent = amount.toLocaleString();
  }
  
  updateTimer(seconds) {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    this.elements.timer.textContent = 
      `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
  }
  
  updateScore({ blue, red }) {
    this.elements.scoreBlue.textContent = blue;
    this.elements.scoreRed.textContent = red;
  }
  
  addKillFeed({ killer, victim, weapon }) {
    const entry = document.createElement('div');
    entry.className = 'kill-entry';
    entry.innerHTML = `
      <span class="killer">${killer}</span>
      <span class="weapon-icon">${weapon}</span>
      <span class="victim">${victim}</span>
    `;
    
    this.elements.killFeed.appendChild(entry);
    
    // Remove after 5 seconds
    setTimeout(() => entry.remove(), 5000);
    
    // Keep only last 5 entries
    while (this.elements.killFeed.children.length > 5) {
      this.elements.killFeed.firstChild.remove();
    }
  }
  
  showNotification(text, type = 'info') {
    const notification = document.createElement('div');
    notification.className = `notification ${type}`;
    notification.textContent = text;
    
    this.elements.notifications.appendChild(notification);
    
    setTimeout(() => notification.remove(), 3000);
  }
}

// Initialize
const uiManager = new UIManager();
```

### Pixi.js UI Overlay Integration

```javascript
// pixi-ui-overlay.js - Pixi.js UI Elements
class PixiUIOverlay {
  constructor(app) {
    this.app = app;
    this.container = new PIXI.Container();
    this.container.zIndex = 100; // Above game world
    
    this.init();
  }
  
  init() {
    // Add container to stage
    this.app.stage.addChild(this.container);
    
    // Create UI layers
    this.createDamageIndicators();
    this.createObjectiveMarkers();
    this.createScreenEffects();
  }
  
  createDamageIndicators() {
    this.damageIndicators = new PIXI.Container();
    this.container.addChild(this.damageIndicators);
  }
  
  showDamageIndicator(angle, damage) {
    // Show directional damage indicator
    const indicator = new PIXI.Graphics();
    indicator.beginFill(0xFF006E, 0.8);
    
    // Draw arrow pointing to damage source
    const arrowSize = 40;
    const x = this.app.screen.width / 2;
    const y = this.app.screen.height / 2;
    const distance = Math.min(this.app.screen.width, this.app.screen.height) / 3;
    
    const rad = (angle - 90) * Math.PI / 180;
    const ax = x + Math.cos(rad) * distance;
    const ay = y + Math.sin(rad) * distance;
    
    // Draw arrow
    indicator.moveTo(ax, ay - arrowSize);
    indicator.lineTo(ax - arrowSize/2, ay + arrowSize/2);
    indicator.lineTo(ax + arrowSize/2, ay + arrowSize/2);
    indicator.closePath();
    indicator.endFill();
    
    indicator.rotation = rad;
    
    this.damageIndicators.addChild(indicator);
    
    // Fade out
    let alpha = 1;
    const fade = () => {
      alpha -= 0.05;
      indicator.alpha = alpha;
      if (alpha > 0) {
        requestAnimationFrame(fade);
      } else {
        indicator.destroy();
      }
    };
    fade();
  }
  
  createObjectiveMarkers() {
    this.objectiveMarkers = new PIXI.Container();
    this.container.addChild(this.objectiveMarkers);
  }
  
  showObjectiveMarker(x, y, type = 'capture') {
    const marker = new PIXI.Container();
    
    // Draw marker based on type
    const graphics = new PIXI.Graphics();
    
    if (type === 'capture') {
      graphics.lineStyle(2, 0x00D4FF);
      graphics.drawCircle(0, 0, 20);
      graphics.beginFill(0x00D4FF, 0.3);
      graphics.drawCircle(0, 0, 15);
    }
    
    marker.addChild(graphics);
    marker.x = x;
    marker.y = y;
    
    // Pulse animation
    let scale = 1;
    let growing = true;
    const pulse = () => {
      if (growing) {
        scale += 0.01;
        if (scale >= 1.2) growing = false;
      } else {
        scale -= 0.01;
        if (scale <= 1) growing = true;
      }
      marker.scale.set(scale);
      requestAnimationFrame(pulse);
    };
    pulse();
    
    this.objectiveMarkers.addChild(marker);
    return marker;
  }
  
  createScreenEffects() {
    this.screenEffects = new PIXI.Graphics();
    this.container.addChild(this.screenEffects);
  }
  
  showDamageFlash() {
    // Red flash when taking damage
    this.screenEffects.clear();
    this.screenEffects.beginFill(0xFF006E, 0.3);
    this.screenEffects.drawRect(0, 0, this.app.screen.width, this.app.screen.height);
    this.screenEffects.endFill();
    
    let alpha = 0.3;
    const fade = () => {
      alpha -= 0.03;
      this.screenEffects.alpha = alpha;
      if (alpha > 0) {
        requestAnimationFrame(fade);
      } else {
        this.screenEffects.clear();
      }
    };
    fade();
  }
  
  showLevelUpEffect() {
    // Gold glow effect for level up
    this.screenEffects.clear();
    this.screenEffects.beginFill(0xFFD700, 0.2);
    this.screenEffects.drawRect(0, 0, this.app.screen.width, this.app.screen.height);
    this.screenEffects.endFill();
    
    // Animate expanding ring
    const ring = new PIXI.Graphics();
    this.container.addChild(ring);
    
    let radius = 0;
    const maxRadius = Math.max(this.app.screen.width, this.app.screen.height);
    
    const expand = () => {
      radius += 15;
      ring.clear();
      ring.lineStyle(4, 0xFFD700, 1 - radius / maxRadius);
      ring.drawCircle(this.app.screen.width / 2, this.app.screen.height / 2, radius);
      
      if (radius < maxRadius) {
        requestAnimationFrame(expand);
      } else {
        ring.destroy();
        this.screenEffects.clear();
      }
    };
    expand();
  }
}

// Export for use
window.PixiUIOverlay = PixiUIOverlay;
```

---

## Sound Design Cues

### UI Sound Events

```javascript
// ui-sounds.js
const UISounds = {
  // Initialize audio context
  init() {
    this.audioContext = new (window.AudioContext || window.webkitAudioContext)();
    this.sounds = new Map();
  },
  
  // Load sound
  async load(name, url) {
    const response = await fetch(url);
    const arrayBuffer = await response.arrayBuffer();
    const audioBuffer = await this.audioContext.decodeAudioData(arrayBuffer);
    this.sounds.set(name, audioBuffer);
  },
  
  // Play sound
  play(name, options = {}) {
    const buffer = this.sounds.get(name);
    if (!buffer) return;
    
    const source = this.audioContext.createBufferSource();
    source.buffer = buffer;
    
    // Volume control
    const gainNode = this.audioContext.createGain();
    gainNode.gain.value = options.volume || 1;
    
    source.connect(gainNode);
    gainNode.connect(this.audioContext.destination);
    
    source.start();
  },
  
  // Predefined sound events
  playHover() {
    this.play('ui_hover', { volume: 0.3 });
  },
  
  playClick() {
    this.play('ui_click', { volume: 0.5 });
  },
  
  playConfirm() {
    this.play('ui_confirm', { volume: 0.6 });
  },
  
  playBack() {
    this.play('ui_back', { volume: 0.5 });
  },
  
  playError() {
    this.play('ui_error', { volume: 0.5 });
  },
  
  playPurchase() {
    this.play('ui_purchase', { volume: 0.7 });
  },
  
  playLevelUp() {
    this.play('ui_levelup', { volume: 0.8 });
  },
  
  playMatchStart() {
    this.play('match_start', { volume: 0.8 });
  },
  
  playMatchEnd() {
    this.play('match_end', { volume: 0.8 });
  }
};

// Bind to UI events
document.addEventListener('DOMContentLoaded', () => {
  UISounds.init();
  
  // Add hover sounds to buttons
  document.querySelectorAll('button, .nav-item, .mode-card').forEach(el => {
    el.addEventListener('mouseenter', () => UISounds.playHover());
    el.addEventListener('click', () => UISounds.playClick());
  });
});
```

---

## Performance Optimizations

### CSS Performance

```css
/* Use transform instead of position changes */
.animated-element {
  will-change: transform;
  transform: translateZ(0); /* Force GPU acceleration */
}

/* Contain paint for HUD elements */
.hud-container {
  contain: layout style paint;
}

/* Use CSS containment for sections */
.menu-section {
  contain: layout style;
}

/* Reduce motion for accessibility */
@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
  }
}
```

### JavaScript Performance

```javascript
// Use requestAnimationFrame for smooth animations
class SmoothUI {
  constructor() {
    this.animations = new Map();
  }
  
  animate(element, property, target, duration = 300) {
    const start = performance.now();
    const startValue = parseFloat(getComputedStyle(element)[property]);
    
    const animate = (now) => {
      const elapsed = now - start;
      const progress = Math.min(elapsed / duration, 1);
      
      // Easing function (ease-out)
      const eased = 1 - Math.pow(1 - progress, 3);
      
      const current = startValue + (target - startValue) * eased;
      element.style[property] = `${current}px`;
      
      if (progress < 1) {
        requestAnimationFrame(animate);
      }
    };
    
    requestAnimationFrame(animate);
  }
  
  // Throttle HUD updates
  throttle(callback, limit) {
    let waiting = false;
    return function(...args) {
      if (!waiting) {
        callback.apply(this, args);
        waiting = true;
        setTimeout(() => waiting = false, limit);
      }
    };
  }
}
```

---

## Summary

This comprehensive UI design provides:

1. **Modern Sci-Fi Aesthetic**: Clean, futuristic design with neon accents and glassmorphism
2. **Complete Menu System**: Main menu with play, career, shop, social, and settings sections
3. **Full HUD**: Health/shield bars, ammo display, minimap, abilities, and kill feed
4. **In-Game Menus**: Buy menu, scoreboard, and pause menu
5. **Career Mode**: Profile, stats, progression tree, and leaderboards
6. **Mobile Support**: Touch controls and responsive adaptations
7. **Performance**: GPU-accelerated animations and optimized rendering
8. **Accessibility**: Reduced motion support and high contrast options

### Key Files to Create:
- `/ui/css/variables.css` - Design tokens
- `/ui/css/main-menu.css` - Menu styles
- `/ui/css/hud.css` - HUD styles
- `/ui/css/ingame-menus.css` - Buy menu, scoreboard
- `/ui/css/mobile.css` - Mobile adaptations
- `/ui/js/ui-manager.js` - Main controller
- `/ui/js/hud-controller.js` - HUD updates
- `/ui/js/touch-controls.js` - Mobile input

### Integration with Existing Client:
The UI system is designed to integrate seamlessly with the existing Pixi.js game client. HTML/CSS overlays sit above the game canvas, with JavaScript event dispatching for game state synchronization.
