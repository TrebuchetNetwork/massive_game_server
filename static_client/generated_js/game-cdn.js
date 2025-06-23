// Wrapper for game protocol with CDN-based flatbuffers
import * as flatbuffers from 'https://cdn.jsdelivr.net/npm/flatbuffers@23.5.26/+esm';

// Make flatbuffers available globally for the generated files
window.flatbuffers = flatbuffers;

// Re-export all game protocol types
export * from './game-protocol.js';

// Create a namespace object similar to the original
export const GameProtocol = {};

// Import and attach all exports to GameProtocol namespace
import * as AllExports from './game-protocol.js';
Object.keys(AllExports).forEach(key => {
  GameProtocol[key] = AllExports[key];
});
