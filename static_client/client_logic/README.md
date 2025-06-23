# Client Logic Modules

This folder contains modular JavaScript classes extracted from the advanced game client. These modules can be imported and used in any client implementation to add advanced features.

## Modules

### EffectsManager
Handles visual effects like explosions, impacts, muzzle flashes, and particle systems.

```javascript
import { EffectsManager } from './client_logic/EffectsManager.js';

const effectsManager = new EffectsManager(app, worldContainer, audioManager);
effectsManager.processGameEvent(event, GameProtocol);
effectsManager.update(deltaMs);
```

### AudioManager
Manages sound effects using Web Audio API with spatial audio support.

```javascript
import { AudioManager } from './client_logic/AudioManager.js';

const audioManager = new AudioManager();
audioManager.setGlobalVolume(0.5);
audioManager.playWeaponSound(weaponType, position, isLocalPlayer);
audioManager.playSound('explosion', position);
```

### Minimap
Displays a top-down miniaturized view of the game world with player positions and objectives.

```javascript
import { Minimap } from './client_logic/Minimap.js';

const minimap = new Minimap(150, 150, 0.05);
document.getElementById('minimapContainer').appendChild(minimap.app.view);
minimap.update(localPlayerData, players, walls, flags);
```

### NetworkIndicator
Shows network quality and ping status with visual bars and color coding.

```javascript
import { NetworkIndicator } from './client_logic/NetworkIndicator.js';

const networkIndicator = new NetworkIndicator();
document.getElementById('networkContainer').appendChild(networkIndicator.app.view);
networkIndicator.update(currentPing);
```

### Utilities
Shared utility functions and constants for colors, shapes, and visual effects.

```javascript
import { 
    teamColors, 
    weaponNames, 
    mixColors, 
    drawStar,
    createStarfield,
    createHealthVignette,
    applyScreenShake,
    initializeEnhancedGraphics
} from './client_logic/utils.js';
```

## Usage Example

```javascript
// Import all modules
import { 
    EffectsManager, 
    AudioManager, 
    Minimap, 
    NetworkIndicator,
    initializeEnhancedGraphics 
} from './client_logic/index.js';

// Initialize PIXI app
const app = new PIXI.Application({ /* ... */ });

// Create world container
const worldContainer = new PIXI.Container();
app.stage.addChild(worldContainer);

// Initialize enhanced graphics system
const { audioManager, effectsManager, starfield, healthVignette } = 
    initializeEnhancedGraphics(app, worldContainer, AudioManager, EffectsManager);

// Create minimap
const minimap = new Minimap();
document.getElementById('minimapContainer').appendChild(minimap.app.view);

// Create network indicator
const networkIndicator = new NetworkIndicator();
document.getElementById('networkContainer').appendChild(networkIndicator.app.view);

// In game loop
function gameLoop(delta) {
    effectsManager.update(delta);
    minimap.update(localPlayer, players, walls, flags);
    networkIndicator.update(ping);
    // ... rest of game logic
}
```

## Dependencies

All modules require:
- PIXI.js v7+
- GameProtocol (FlatBuffers generated code)

Some modules expect certain global variables to be available:
- `window.GP` - GameProtocol reference
- `window.app` - PIXI Application instance (for spatial audio)
- `window.gameScene` - Main game scene container (for spatial audio)
- `window.localPlayerState` - Current player state (for spatial audio)
- `window.myPlayerId` - Current player ID
- `window.playerContainer` - Container holding player sprites (for muzzle flash effects)

## Integration with Mobile Client

To add these features to the mobile client:

1. Import the desired modules
2. Initialize them after PIXI setup
3. Call update methods in the game loop
4. Handle game events through EffectsManager
5. Adjust visual settings for mobile performance if needed

```javascript
// In mobile client initialization
if (deviceSupportsAdvancedFeatures) {
    const { audioManager, effectsManager } = initializeEnhancedGraphics(app, worldContainer);
    
    // Reduce particle effects for mobile
    effectsManager.setParticlesEnabled(false);
    
    // Lower audio quality for mobile
    audioManager.setGlobalVolume(0.3);
}
