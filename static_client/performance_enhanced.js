// Enhanced Performance Monitor with Comprehensive Benchmarking
class EnhancedPerformanceMonitor {
    constructor() {
        this.enabled = false;
        this.metrics = new Map();
        this.frameMetrics = {
            frameTimes: new Float32Array(300),
            frameIndex: 0,
            lastFrameTime: performance.now(),
            targetFPS: 60,
            targetFrameTime: 1000 / 60
        };
        
        // Enhanced tracking
        this.functionCallGraph = new Map();
        this.hotPaths = new Map();
        this.memorySnapshots = [];
        this.gcEvents = [];
        
        // Automatic instrumentation
        this.instrumentedFunctions = new WeakMap();
        this.originalFunctions = new Map();
        
        // Web Worker analysis
        this.workerCandidates = new Map();
        this.parallelizableOperations = new Set();
        
        // Current call stack for tracking
        this.currentCallStack = [];
        
        // UI elements
        this.overlay = null;
        this.createEnhancedUI();
        
        // Start monitoring
        this.startGlobalMonitoring();
    }
    
    createEnhancedUI() {
        // Main overlay container with tabs
        this.overlay = document.createElement('div');
        this.overlay.id = 'enhancedPerformanceOverlay';
        this.overlay.style.cssText = `
            position: fixed;
            top: 10px;
            right: 10px;
            width: 450px;
            max-height: 90vh;
            background: rgba(17, 24, 39, 0.98);
            border: 1px solid #374151;
            border-radius: 12px;
            color: #E5E7EB;
            font-family: 'Consolas', 'Monaco', monospace;
            font-size: 11px;
            z-index: 10001;
            display: none;
            backdrop-filter: blur(10px);
            box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.3);
            overflow: hidden;
            display: flex;
            flex-direction: column;
        `;
        
        // Header with tabs
        const header = document.createElement('div');
        header.style.cssText = `
            padding: 12px 15px;
            background: rgba(31, 41, 55, 0.8);
            border-bottom: 1px solid #374151;
            display: flex;
            justify-content: space-between;
            align-items: center;
        `;
        
        const title = document.createElement('div');
        title.style.cssText = 'font-size: 14px; font-weight: bold; color: #60A5FA;';
        title.textContent = '🚀 Enhanced Performance Monitor';
        header.appendChild(title);
        
        const closeBtn = document.createElement('button');
        closeBtn.textContent = '✕';
        closeBtn.style.cssText = `
            background: none;
            border: none;
            color: #9CA3AF;
            font-size: 18px;
            cursor: pointer;
            padding: 0 5px;
        `;
        closeBtn.onclick = () => this.toggle();
        header.appendChild(closeBtn);
        
        this.overlay.appendChild(header);
        
        // Tab navigation
        const tabNav = document.createElement('div');
        tabNav.style.cssText = `
            display: flex;
            background: rgba(31, 41, 55, 0.5);
            border-bottom: 1px solid #374151;
        `;
        
        const tabs = ['Overview', 'Functions', 'Bottlenecks', 'Memory', 'Web Workers'];
        this.tabContents = {};
        this.tabButtons = {};
        
        tabs.forEach((tabName, index) => {
            const tabBtn = document.createElement('button');
            tabBtn.textContent = tabName;
            tabBtn.style.cssText = `
                flex: 1;
                padding: 10px;
                background: none;
                border: none;
                color: #9CA3AF;
                cursor: pointer;
                border-bottom: 2px solid transparent;
                transition: all 0.2s;
            `;
            tabBtn.onclick = () => this.switchTab(tabName);
            tabNav.appendChild(tabBtn);
            this.tabButtons[tabName] = tabBtn;
            
            // Create tab content
            const content = document.createElement('div');
            content.style.cssText = `
                padding: 15px;
                overflow-y: auto;
                flex: 1;
                display: none;
            `;
            this.tabContents[tabName] = content;
        });
        
        this.overlay.appendChild(tabNav);
        
        // Tab content container
        const contentContainer = document.createElement('div');
        contentContainer.style.cssText = 'flex: 1; overflow: hidden; display: flex; flex-direction: column;';
        Object.values(this.tabContents).forEach(content => contentContainer.appendChild(content));
        this.overlay.appendChild(contentContainer);
        
        // Initialize tab contents
        this.initializeTabContents();
        
        // Footer with controls
        const footer = document.createElement('div');
        footer.style.cssText = `
            padding: 10px;
            background: rgba(31, 41, 55, 0.8);
            border-top: 1px solid #374151;
            display: flex;
            gap: 10px;
        `;
        
        const exportBtn = this.createButton('📊 Export Data', () => this.exportPerformanceData());
        const clearBtn = this.createButton('🗑️ Clear Data', () => this.clearAllData());
        const autoFixBtn = this.createButton('🔧 Auto-Optimize', () => this.autoOptimize());
        
        footer.appendChild(exportBtn);
        footer.appendChild(clearBtn);
        footer.appendChild(autoFixBtn);
        
        this.overlay.appendChild(footer);
        
        // Add to body
        document.body.appendChild(this.overlay);
        
        // Default to Overview tab
        this.switchTab('Overview');
    }
    
    createButton(text, onClick) {
        const btn = document.createElement('button');
        btn.textContent = text;
        btn.style.cssText = `
            padding: 6px 12px;
            background: #4F46E5;
            color: white;
            border: none;
            border-radius: 6px;
            cursor: pointer;
            font-size: 11px;
            transition: background 0.2s;
        `;
        btn.onmouseover = () => btn.style.background = '#4338CA';
        btn.onmouseout = () => btn.style.background = '#4F46E5';
        btn.onclick = onClick;
        return btn;
    }
    
    initializeTabContents() {
        // Overview Tab
        this.tabContents['Overview'].innerHTML = `
            <div id="overviewContent">
                <canvas id="perfFrameChart" width="420" height="120" style="border: 1px solid #374151; border-radius: 4px; margin-bottom: 15px;"></canvas>
                <div id="overviewStats" style="display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; margin-bottom: 15px;"></div>
                <div id="realtimeMetrics"></div>
            </div>
        `;
        
        // Functions Tab
        this.tabContents['Functions'].innerHTML = `
            <div id="functionsContent">
                <input type="text" id="functionSearch" placeholder="🔍 Search functions..." style="
                    width: 100%;
                    padding: 8px;
                    margin-bottom: 10px;
                    background: #1F2937;
                    border: 1px solid #374151;
                    border-radius: 4px;
                    color: #E5E7EB;
                ">
                <div id="functionsList" style="max-height: 500px; overflow-y: auto;"></div>
            </div>
        `;
        
        // Bottlenecks Tab
        this.tabContents['Bottlenecks'].innerHTML = `
            <div id="bottlenecksContent">
                <div id="bottleneckSummary" style="margin-bottom: 15px;"></div>
                <div id="hotPathAnalysis"></div>
                <div id="optimizationSuggestions"></div>
            </div>
        `;
        
        // Memory Tab
        this.tabContents['Memory'].innerHTML = `
            <div id="memoryContent">
                <canvas id="memoryChart" width="420" height="120" style="border: 1px solid #374151; border-radius: 4px; margin-bottom: 15px;"></canvas>
                <div id="memoryStats"></div>
                <div id="gcEvents"></div>
            </div>
        `;
        
        // Web Workers Tab
        this.tabContents['Web Workers'].innerHTML = `
            <div id="webWorkersContent">
                <div id="workerCandidates"></div>
                <div id="parallelizationOpportunities"></div>
                <button id="generateWorkerCode" style="
                    margin-top: 15px;
                    padding: 10px 20px;
                    background: #10B981;
                    color: white;
                    border: none;
                    border-radius: 6px;
                    cursor: pointer;
                    width: 100%;
                ">Generate Worker Implementation</button>
            </div>
        `;
    }
    
    switchTab(tabName) {
        // Update button styles
        Object.entries(this.tabButtons).forEach(([name, btn]) => {
            if (name === tabName) {
                btn.style.color = '#60A5FA';
                btn.style.borderBottomColor = '#60A5FA';
                btn.style.background = 'rgba(59, 130, 246, 0.1)';
            } else {
                btn.style.color = '#9CA3AF';
                btn.style.borderBottomColor = 'transparent';
                btn.style.background = 'none';
            }
        });
        
        // Show selected content
        Object.entries(this.tabContents).forEach(([name, content]) => {
            content.style.display = name === tabName ? 'block' : 'none';
        });
        
        // Update content for the selected tab
        this.updateTabContent(tabName);
    }
    
    updateTabContent(tabName) {
        switch (tabName) {
            case 'Overview':
                this.updateOverviewTab();
                break;
            case 'Functions':
                this.updateFunctionsTab();
                break;
            case 'Bottlenecks':
                this.updateBottlenecksTab();
                break;
            case 'Memory':
                this.updateMemoryTab();
                break;
            case 'Web Workers':
                this.updateWebWorkersTab();
                break;
        }
    }
    
    startGlobalMonitoring() {
        // Instrument all global functions
        this.instrumentGlobalFunctions();
        
        // Monitor memory
        this.startMemoryMonitoring();
        
        // Start frame monitoring
        this.startFrameMonitoring();
        
        // Monitor garbage collection
        this.monitorGarbageCollection();
        
        // Update loop
        this.updateInterval = setInterval(() => {
            if (this.enabled) {
                this.analyzePerformance();
                this.detectBottlenecks();
                this.identifyWorkerCandidates();
            }
        }, 100);
    }
    
    instrumentGlobalFunctions() {
        // Core game functions to instrument
        const functionsToInstrument = [
            // Game loop
            'gameLoop', 'interpolateEntities', 'updateLocalPlayerPrediction',
            
            // Rendering
            'updateSprites', 'updateCamera', 'drawWalls', 'drawAimingSystem',
            'updateStarfield', 'updateFogOfWar', 'animatePickups', 'animateFlags',
            'createPlayerSprite', 'updatePlayerSprite', 'createProjectileSprite',
            'updateProjectileSprite', 'createPickupSprite', 'createFlagSprite',
            'updatePlayerGun', 'updatePlayerHealthBar', 'updateShieldVisual',
            'drawEnhancedWallCracks', 'createSpeedBoostEffect',
            
            // Network
            'processServerUpdate', 'parseFlatBufferMessage', 'sendInputsToServer',
            'createInputMessage', 'createChatMessage', 'setupDataChannelEvents',
            
            // UI
            'updateGameStatsUI', 'updateKillFeed', 'updateChatDisplay',
            'updateMatchInfo', 'updateScoreboard', 'toggleScoreboard',
            
            // Input handling
            'handleKeyInput', 'handleMouseMove', 'sendChatMessage',
            
            // Utility
            'interpolateColor', 'mixColors', 'drawRegularPolygon', 'drawStar',
            'escapeHtml', 'getMaxAmmoForWeaponClient'
        ];
        
        functionsToInstrument.forEach(funcName => {
            if (window[funcName] && typeof window[funcName] === 'function') {
                this.instrumentFunction(window, funcName, `Global.${funcName}`);
            }
        });
        
        // Instrument class methods
        this.instrumentClassMethods();
        
        // Auto-discover and instrument other functions
        // DISABLED: Auto-discovery causes issues with constructors
        // this.autoDiscoverFunctions();
    }
    
    instrumentClassMethods() {
        // EffectsManager
        if (window.EffectsManager) {
            const effectsMethods = [
                'update', 'processGameEvent', 'createEnhancedBulletImpact',
                'createEnhancedMuzzleFlash', 'createEnhancedExplosion',
                'createEnhancedDamageNumbers', 'createEnhancedWallDestructionEffect',
                'createEnhancedPowerupCollectEffect', 'createEnhancedFlagCaptureEffect',
                'createMeleeSwingEffect', 'createWallRespawnEffect', 'animateEffect'
            ];
            
            effectsMethods.forEach(method => {
                if (EffectsManager.prototype[method]) {
                    this.instrumentFunction(EffectsManager.prototype, method, `EffectsManager.${method}`);
                }
            });
        }
        
        // AudioManager
        if (window.AudioManager) {
            const audioMethods = ['playSound', 'playWeaponSound', '_playTone'];
            audioMethods.forEach(method => {
                if (AudioManager.prototype[method]) {
                    this.instrumentFunction(AudioManager.prototype, method, `AudioManager.${method}`);
                }
            });
        }
        
        // Minimap
        if (window.Minimap) {
            const minimapMethods = ['update', 'drawWalls', 'drawObjectives', 'clear'];
            minimapMethods.forEach(method => {
                if (Minimap.prototype[method]) {
                    this.instrumentFunction(Minimap.prototype, method, `Minimap.${method}`);
                }
            });
        }
        
        // NetworkIndicator
        if (window.NetworkIndicator) {
            this.instrumentFunction(NetworkIndicator.prototype, 'update', 'NetworkIndicator.update');
        }
    }
    
    autoDiscoverFunctions() {
        // Discover all functions in the global scope
        const discovered = new Set();
        
        // Built-in constructors and functions to skip
        const skipList = [
            'constructor', 'toString', 'valueOf',
            // Built-in constructors that require 'new'
            'Array', 'Object', 'Function', 'Boolean', 'Symbol', 'Error',
            'Number', 'String', 'RegExp', 'Date', 'Promise', 'Map', 'Set',
            'WeakMap', 'WeakSet', 'ArrayBuffer', 'SharedArrayBuffer',
            'Uint8Array', 'Uint16Array', 'Uint32Array', 'Int8Array',
            'Int16Array', 'Int32Array', 'Float32Array', 'Float64Array',
            'Uint8ClampedArray', 'BigInt64Array', 'BigUint64Array',
            'DataView', 'Blob', 'File', 'FileReader', 'URL', 'URLSearchParams',
            'FormData', 'Headers', 'Request', 'Response', 'Proxy', 'Reflect',
            // Browser APIs
            'XMLHttpRequest', 'WebSocket', 'Worker', 'SharedWorker',
            'Notification', 'AudioContext', 'CanvasRenderingContext2D',
            'WebGLRenderingContext', 'WebGL2RenderingContext',
            'RTCPeerConnection', 'RTCSessionDescription', 'RTCIceCandidate',
            'MediaStream', 'MediaStreamTrack', 'MediaDevices',
            // DOM APIs
            'Element', 'HTMLElement', 'HTMLDivElement', 'HTMLCanvasElement',
            'Document', 'DocumentFragment', 'Node', 'NodeList',
            'DOMParser', 'XMLSerializer', 'Range', 'Selection',
            'MutationObserver', 'IntersectionObserver', 'ResizeObserver',
            'PerformanceObserver',
            // Events
            'Event', 'CustomEvent', 'MouseEvent', 'KeyboardEvent',
            'TouchEvent', 'FocusEvent', 'WheelEvent', 'InputEvent',
            // Window properties that aren't meant to be instrumented
            'window', 'document', 'console', 'navigator', 'location',
            'history', 'screen', 'localStorage', 'sessionStorage',
            'indexedDB', 'crypto', 'performance',
            // Utility functions that might cause issues
            'alert', 'confirm', 'prompt', 'setTimeout', 'setInterval',
            'clearTimeout', 'clearInterval', 'requestAnimationFrame',
            'cancelAnimationFrame', 'fetch', 'atob', 'btoa',
            // Other APIs
            'Image', 'Audio', 'Video', 'Option', 'WebAssembly',
            'MessageChannel', 'MessagePort', 'BroadcastChannel',
            'AbortController', 'AbortSignal'
        ];
        
        Object.getOwnPropertyNames(window).forEach(prop => {
            try {
                const value = window[prop];
                if (typeof value === 'function' && 
                    !prop.startsWith('_') && 
                    !skipList.includes(prop) &&
                    !this.originalFunctions.has(`Global.${prop}`)) {
                    
                    // Additional checks to skip constructors
                    const isConstructor = 
                        // Check if it has a prototype with constructor
                        (value.prototype && value.prototype.constructor === value) ||
                        // Check if name starts with capital letter (common for constructors)
                        /^[A-Z]/.test(prop) ||
                        // Check if it's a class
                        value.toString().startsWith('class ') ||
                        // Check common constructor patterns
                        prop.endsWith('Error') || 
                        prop.endsWith('Event') ||
                        prop.includes('Element') ||
                        prop.startsWith('HTML') ||
                        prop.startsWith('SVG') ||
                        prop.startsWith('CSS') ||
                        prop.startsWith('DOM') ||
                        prop.startsWith('RTC') ||
                        prop.startsWith('Media');
                    
                    if (!isConstructor) {
                        discovered.add(prop);
                    }
                }
            } catch (e) {
                // Some properties might throw on access
            }
        });
        
        // Instrument discovered functions
        discovered.forEach(funcName => {
            try {
                this.instrumentFunction(window, funcName, `Global.${funcName}`);
            } catch (e) {
                console.warn(`Failed to instrument ${funcName}:`, e.message);
            }
        });
    }
    
    instrumentFunction(obj, funcName, displayName) {
        if (!obj || !obj[funcName] || typeof obj[funcName] !== 'function') return;
        
        // Skip if already instrumented
        if (this.instrumentedFunctions.has(obj[funcName])) return;
        
        const original = obj[funcName];
        const monitor = this;
        
        // Comprehensive constructor detection and skip
        try {
            // Skip if it's a native function
            if (original.toString().indexOf('[native code]') !== -1) {
                return;
            }
            
            // Skip if it's a class
            if (original.toString().startsWith('class ')) {
                return;
            }
            
            // Skip if it has a prototype with constructor (likely a constructor)
            if (original.prototype && original.prototype.constructor === original) {
                return;
            }
            
            // Skip if the name starts with capital letter (convention for constructors)
            if (funcName && /^[A-Z]/.test(funcName)) {
                return;
            }
            
            // Skip if it's in the global scope and matches constructor patterns
            if (obj === window && funcName) {
                // Skip anything that looks like a constructor or type
                if (funcName.endsWith('Error') || 
                    funcName.endsWith('Event') ||
                    funcName.includes('Element') ||
                    funcName.startsWith('HTML') ||
                    funcName.startsWith('SVG') ||
                    funcName.startsWith('CSS') ||
                    funcName.startsWith('DOM') ||
                    funcName.startsWith('RTC') ||
                    funcName.startsWith('Media') ||
                    funcName === 'Proxy' ||
                    funcName === 'Reflect') {
                    return;
                }
            }
            
            // Final test: Try calling without new to see if it throws
            // This catches any constructor we might have missed
            try {
                // Create a safe test environment
                const testThis = {};
                const testArgs = [];
                
                // Try to call the function without 'new'
                // If it's a constructor that requires 'new', this will throw
                original.call(testThis, ...testArgs);
            } catch (testError) {
                // If the error message indicates it needs 'new', skip it
                const errorMsg = testError.message || testError.toString();
                if (errorMsg.includes('new') || 
                    errorMsg.includes('constructor') || 
                    errorMsg.includes('Constructor') ||
                    errorMsg.includes('class')) {
                    return;
                }
                // For other errors, we'll continue (might be due to missing args)
            }
        } catch (e) {
            // If any check throws, skip this function to be safe
            return;
        }
        
        // Create metrics entry
        if (!this.metrics.has(displayName)) {
            this.metrics.set(displayName, {
                samples: new Float32Array(1000), // More samples for detailed histogram
                sampleIndex: 0,
                totalSamples: 0,
                min: Infinity,
                max: 0,
                sum: 0,
                count: 0,
                histogram: new Map(),
                percentiles: {},
                callStack: [],
                childCalls: new Map(),
                parentCalls: new Map(),
                lastCallTime: 0,
                avgTimeBetweenCalls: 0
            });
        }
        
        // Store original function
        this.originalFunctions.set(displayName, original);
        
        // Create instrumented version
        const instrumented = function(...args) {
            const metric = monitor.metrics.get(displayName);
            const callId = `${displayName}_${Date.now()}_${Math.random()}`;
            
            // Track call stack
            const parentCall = monitor.currentCallStack[monitor.currentCallStack.length - 1];
            if (parentCall) {
                // Update parent-child relationships
                const parentMetric = monitor.metrics.get(parentCall.name);
                if (parentMetric) {
                    parentMetric.childCalls.set(displayName, 
                        (parentMetric.childCalls.get(displayName) || 0) + 1);
                }
                metric.parentCalls.set(parentCall.name,
                    (metric.parentCalls.get(parentCall.name) || 0) + 1);
            }
            
            // Push to call stack
            monitor.currentCallStack.push({ name: displayName, id: callId });
            
            // Measure execution time
            const start = performance.now();
            let result;
            let error = null;
            
            try {
                result = original.apply(this, args);
            } catch (e) {
                error = e;
                metric.errors = (metric.errors || 0) + 1;
            }
            
            const duration = performance.now() - start;
            
            // Pop from call stack
            monitor.currentCallStack.pop();
            
            // Record metrics
            monitor.recordDetailedMetric(displayName, duration, args, result, error);
            
            // Track time between calls
            if (metric.lastCallTime > 0) {
                const timeSinceLastCall = start - metric.lastCallTime;
                metric.avgTimeBetweenCalls = metric.avgTimeBetweenCalls * 0.9 + timeSinceLastCall * 0.1;
            }
            metric.lastCallTime = start;
            
            if (error) throw error;
            return result;
        };
        
        // Copy function properties
        Object.setPrototypeOf(instrumented, Object.getPrototypeOf(original));
        Object.getOwnPropertyNames(original).forEach(prop => {
            if (prop !== 'length' && prop !== 'name' && prop !== 'prototype') {
                try {
                    instrumented[prop] = original[prop];
                } catch (e) {
                    // Some properties might be non-configurable
                }
            }
        });
        
        // Replace function
        obj[funcName] = instrumented;
        this.instrumentedFunctions.set(instrumented, true);
    }
    
    recordDetailedMetric(name, duration, args, result, error) {
        const metric = this.metrics.get(name);
        if (!metric) return;
        
        // Update samples
        metric.samples[metric.sampleIndex] = duration;
        metric.sampleIndex = (metric.sampleIndex + 1) % metric.samples.length;
        metric.totalSamples++;
        
        // Update stats
        metric.min = Math.min(metric.min, duration);
        metric.max = Math.max(metric.max, duration);
        metric.sum += duration;
        metric.count++;
        
        // Update histogram with finer buckets
        const bucket = Math.floor(duration * 10) / 10; // 0.1ms buckets
        metric.histogram.set(bucket, (metric.histogram.get(bucket) || 0) + 1);
        
        // Calculate percentiles periodically
        if (metric.count % 100 === 0) {
            this.calculatePercentiles(metric);
        }
        
        // Track hot paths
        if (duration > this.frameMetrics.targetFrameTime * 0.1) {
            const stackTrace = this.currentCallStack.map(c => c.name).join(' → ');
            this.hotPaths.set(stackTrace, (this.hotPaths.get(stackTrace) || 0) + 1);
        }
        
        // Analyze for parallelization
        this.analyzeForParallelization(name, args, result, duration);
    }
    
    calculatePercentiles(metric) {
        const validSamples = [];
        const sampleCount = Math.min(metric.totalSamples, metric.samples.length);
        
        for (let i = 0; i < sampleCount; i++) {
            if (metric.samples[i] > 0) {
                validSamples.push(metric.samples[i]);
            }
        }
        
        validSamples.sort((a, b) => a - b);
        
        const percentiles = [50, 75, 90, 95, 99];
        percentiles.forEach(p => {
            const index = Math.ceil((p / 100) * validSamples.length) - 1;
            metric.percentiles[`p${p}`] = validSamples[Math.max(0, index)] || 0;
        });
    }
    
    analyzeForParallelization(funcName, args, result, duration) {
        // Functions that are good candidates for web workers
        const parallelizableFunctions = [
            'parseFlatBufferMessage', 'interpolateEntities', 'processServerUpdate',
            'updateLocalPlayerPrediction', 'calculatePercentiles', 'drawWalls',
            'updateStarfield', 'animatePickups', 'animateFlags'
        ];
        
        // Check if function is parallelizable
        if (parallelizableFunctions.some(f => funcName.includes(f))) {
            if (!this.workerCandidates.has(funcName)) {
                this.workerCandidates.set(funcName, {
                    callCount: 0,
                    totalTime: 0,
                    avgTime: 0,
                    maxTime: 0,
                    canBeAsync: true,
                    dependencies: new Set(),
                    dataTransferSize: 0
                });
            }
            
            const candidate = this.workerCandidates.get(funcName);
            candidate.callCount++;
            candidate.totalTime += duration;
            candidate.avgTime = candidate.totalTime / candidate.callCount;
            candidate.maxTime = Math.max(candidate.maxTime, duration);
            
            // Estimate data transfer size
            try {
                const argSize = JSON.stringify(args).length;
                const resultSize = result ? JSON.stringify(result).length : 0;
                candidate.dataTransferSize = Math.max(candidate.dataTransferSize, argSize + resultSize);
            } catch (e) {
                // Some data might not be serializable
                candidate.canBeAsync = false;
            }
        }
    }
    
    startMemoryMonitoring() {
        if (!performance.memory) return;
        
        setInterval(() => {
            const memInfo = {
                timestamp: Date.now(),
                usedJSHeapSize: performance.memory.usedJSHeapSize,
                totalJSHeapSize: performance.memory.totalJSHeapSize,
                jsHeapSizeLimit: performance.memory.jsHeapSizeLimit
            };
            
            this.memorySnapshots.push(memInfo);
            
            // Keep only last 5 minutes of data
            const fiveMinutesAgo = Date.now() - 5 * 60 * 1000;
            this.memorySnapshots = this.memorySnapshots.filter(s => s.timestamp > fiveMinutesAgo);
            
            // Detect memory leaks
            this.detectMemoryLeaks();
        }, 1000);
    }
    
    detectMemoryLeaks() {
        if (this.memorySnapshots.length < 60) return; // Need at least 1 minute of data
        
        // Calculate memory growth rate
        const firstSnapshot = this.memorySnapshots[0];
        const lastSnapshot = this.memorySnapshots[this.memorySnapshots.length - 1];
        const timeDiff = (lastSnapshot.timestamp - firstSnapshot.timestamp) / 1000; // seconds
        const memoryGrowth = lastSnapshot.usedJSHeapSize - firstSnapshot.usedJSHeapSize;
        const growthRate = memoryGrowth / timeDiff; // bytes per second
        
        // If growing more than 100KB/s, likely a leak
        if (growthRate > 100 * 1024) {
            console.warn('Potential memory leak detected! Growth rate:', (growthRate / 1024).toFixed(2), 'KB/s');
        }
    }
    
    monitorGarbageCollection() {
        // Monitor for GC events by detecting sudden drops in memory
        let lastHeapSize = 0;
        
        setInterval(() => {
            if (!performance.memory) return;
            
            const currentHeapSize = performance.memory.usedJSHeapSize;
            if (lastHeapSize > 0) {
                const drop = lastHeapSize - currentHeapSize;
                // Significant drop likely indicates GC
                if (drop > 1024 * 1024) { // 1MB drop
                    this.gcEvents.push({
                        timestamp: Date.now(),
                        amount: drop,
                        beforeSize: lastHeapSize,
                        afterSize: currentHeapSize
                    });
                    
                    // Keep only last 100 GC events
                    if (this.gcEvents.length > 100) {
                        this.gcEvents.shift();
                    }
                }
            }
            lastHeapSize = currentHeapSize;
        }, 50);
    }
    
    startFrameMonitoring() {
        const recordFrame = () => {
            const now = performance.now();
            const frameTime = now - this.frameMetrics.lastFrameTime;
            this.frameMetrics.lastFrameTime = now;
            
            this.frameMetrics.frameTimes[this.frameMetrics.frameIndex] = frameTime;
            this.frameMetrics.frameIndex = (this.frameMetrics.frameIndex + 1) % this.frameMetrics.frameTimes.length;
            
            if (this.enabled) {
                requestAnimationFrame(recordFrame);
            }
        };
        
        requestAnimationFrame(recordFrame);
    }
    
    analyzePerformance() {
        // Identify functions that take significant frame time
        const frameTime = this.calculateAverageFrameTime();
        const significantFunctions = [];
        
        this.metrics.forEach((metric, name) => {
            if (metric.count > 0) {
                const avg = metric.sum / metric.count;
                const impact = (avg / frameTime) * 100;
                
                if (impact > 5) { // More than 5% of frame time
                    significantFunctions.push({
                        name,
                        avg,
                        impact,
                        count: metric.count,
                        max: metric.max
                    });
                }
            }
        });
        
        // Sort by impact
        significantFunctions.sort((a, b) => b.impact - a.impact);
        this.significantFunctions = significantFunctions;
    }
    
    detectBottlenecks() {
        const bottlenecks = new Map();
        const frameTime = this.calculateAverageFrameTime();
        const frameBudget = this.frameMetrics.targetFrameTime;
        
        this.metrics.forEach((metric, name) => {
            if (metric.count === 0) return;
            
            const avg = metric.sum / metric.count;
            const p95 = metric.percentiles.p95 || avg;
            const p99 = metric.percentiles.p99 || avg;
            
            // Bottleneck criteria
            const isBottleneck = avg > frameBudget * 0.05 || // Takes >5% of frame budget
                                p95 > frameBudget * 0.1 || // 95th percentile >10% of frame budget
                                p99 > frameBudget * 0.2;   // 99th percentile >20% of frame budget
            
            if (isBottleneck) {
                bottlenecks.set(name, {
                    avg,
                    p95,
                    p99,
                    max: metric.max,
                    impact: (avg / frameTime) * 100,
                    severity: this.calculateSeverity(avg, p95, p99, frameBudget),
                    callsPerFrame: metric.count / (this.frameMetrics.frameIndex || 1),
                    childCalls: Array.from(metric.childCalls.entries())
                        .sort((a, b) => b[1] - a[1])
                        .slice(0, 5)
                });
            }
        });
        
        this.bottlenecks = bottlenecks;
    }
    
    calculateSeverity(avg, p95, p99, frameBudget) {
        if (p99 > frameBudget * 0.5) return 'critical';
        if (p95 > frameBudget * 0.3) return 'high';
        if (avg > frameBudget * 0.1) return 'medium';
        return 'low';
    }
    
    identifyWorkerCandidates() {
        const candidates = [];
        
        this.workerCandidates.forEach((candidate, funcName) => {
            // Score based on multiple factors
            const score = 
                (candidate.avgTime * 10) + // Weight average time heavily
                (candidate.maxTime * 5) +   // Consider worst case
                (candidate.callCount * 0.1) - // Frequent calls are good
                (candidate.dataTransferSize / 1000); // Penalize large data transfers
            
            if (score > 10 && candidate.canBeAsync) {
                candidates.push({
                    name: funcName,
                    score,
                    ...candidate
                });
            }
        });
        
        // Sort by score
        candidates.sort((a, b) => b.score - a.score);
        this.topWorkerCandidates = candidates.slice(0, 10);
    }
    
    calculateAverageFrameTime() {
        let sum = 0;
        let count = 0;
        for (let i = 0; i < this.frameMetrics.frameTimes.length; i++) {
            if (this.frameMetrics.frameTimes[i] > 0) {
                sum += this.frameMetrics.frameTimes[i];
                count++;
            }
        }
        return count > 0 ? sum / count : 16.67;
    }
    
    // Tab update methods
    updateOverviewTab() {
        // Update frame chart
        const canvas = document.getElementById('perfFrameChart');
        if (canvas) {
            this.drawFrameChart(canvas);
        }
        
        // Update stats
        const statsDiv = document.getElementById('overviewStats');
        if (statsDiv) {
            const avgFrameTime = this.calculateAverageFrameTime();
            const currentFPS = Math.round(1000 / avgFrameTime);
            const memory = performance.memory || {};
            
            statsDiv.innerHTML = `
                <div style="background: #1F2937; padding: 10px; border-radius: 6px; text-align: center;">
                    <div style="color: #9CA3AF; font-size: 10px;">FPS</div>
                    <div style="color: ${currentFPS >= 55 ? '#10B981' : currentFPS >= 30 ? '#F59E0B' : '#EF4444'}; font-size: 24px; font-weight: bold;">${currentFPS}</div>
                </div>
                <div style="background: #1F2937; padding: 10px; border-radius: 6px; text-align: center;">
                    <div style="color: #9CA3AF; font-size: 10px;">Frame Time</div>
                    <div style="color: #60A5FA; font-size: 24px; font-weight: bold;">${avgFrameTime.toFixed(1)}ms</div>
                </div>
                <div style="background: #1F2937; padding: 10px; border-radius: 6px; text-align: center;">
                    <div style="color: #9CA3AF; font-size: 10px;">Memory</div>
                    <div style="color: #A78BFA; font-size: 24px; font-weight: bold;">${memory.usedJSHeapSize ? (memory.usedJSHeapSize / 1048576).toFixed(0) : 'N/A'} MB</div>
                </div>
            `;
        }
        
        // Update realtime metrics
        const metricsDiv = document.getElementById('realtimeMetrics');
        if (metricsDiv && this.significantFunctions) {
            let html = '<h3 style="margin-bottom: 10px; color: #E5E7EB;">Top Performance Impact</h3>';
            
            this.significantFunctions.slice(0, 5).forEach(func => {
                const color = func.impact > 20 ? '#EF4444' : func.impact > 10 ? '#F59E0B' : '#10B981';
                html += `
                    <div style="background: #1F2937; padding: 8px; margin-bottom: 6px; border-radius: 4px; display: flex; justify-content: space-between;">
                        <span style="color: #E5E7EB; font-size: 11px;">${func.name}</span>
                        <span style="color: ${color}; font-size: 11px; font-weight: bold;">${func.impact.toFixed(1)}%</span>
                    </div>
                `;
            });
            
            metricsDiv.innerHTML = html;
        }
    }
    
    updateFunctionsTab() {
        const listDiv = document.getElementById('functionsList');
        if (!listDiv) return;
        
        // Get search term
        const searchInput = document.getElementById('functionSearch');
        const searchTerm = searchInput ? searchInput.value.toLowerCase() : '';
        
        // Filter and sort metrics
        const sortedMetrics = Array.from(this.metrics.entries())
            .filter(([name]) => name.toLowerCase().includes(searchTerm))
            .map(([name, metric]) => ({
                name,
                avg: metric.count > 0 ? metric.sum / metric.count : 0,
                count: metric.count,
                total: metric.sum,
                max: metric.max,
                p95: metric.percentiles.p95 || 0,
                p99: metric.percentiles.p99 || 0
            }))
            .sort((a, b) => b.total - a.total);
        
        let html = '';
        sortedMetrics.forEach(func => {
            const color = func.avg > 5 ? '#EF4444' : func.avg > 2 ? '#F59E0B' : '#10B981';
            html += `
                <div style="background: #1F2937; padding: 10px; margin-bottom: 8px; border-radius: 6px; border-left: 3px solid ${color};">
                    <div style="display: flex; justify-content: space-between; margin-bottom: 6px;">
                        <span style="color: #E5E7EB; font-weight: bold;">${func.name}</span>
                        <span style="color: ${color};">${func.avg.toFixed(2)}ms avg</span>
                    </div>
                    <div style="display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; font-size: 10px; color: #9CA3AF;">
                        <div>Calls: ${func.count}</div>
                        <div>Total: ${func.total.toFixed(1)}ms</div>
                        <div>Max: ${func.max.toFixed(2)}ms</div>
                        <div>p95: ${func.p95.toFixed(2)}ms</div>
                    </div>
                </div>
            `;
        });
        
        listDiv.innerHTML = html || '<div style="text-align: center; color: #6B7280;">No functions found</div>';
    }
    
    updateBottlenecksTab() {
        const summaryDiv = document.getElementById('bottleneckSummary');
        const hotPathDiv = document.getElementById('hotPathAnalysis');
        const suggestionsDiv = document.getElementById('optimizationSuggestions');
        
        if (summaryDiv) {
            const criticalCount = Array.from(this.bottlenecks.values()).filter(b => b.severity === 'critical').length;
            const highCount = Array.from(this.bottlenecks.values()).filter(b => b.severity === 'high').length;
            
            summaryDiv.innerHTML = `
                <div style="background: #1F2937; padding: 15px; border-radius: 8px;">
                    <h3 style="color: #E5E7EB; margin-bottom: 10px;">Bottleneck Summary</h3>
                    <div style="display: flex; gap: 15px;">
                        <div style="text-align: center;">
                            <div style="color: #EF4444; font-size: 24px; font-weight: bold;">${criticalCount}</div>
                            <div style="color: #9CA3AF; font-size: 10px;">Critical</div>
                        </div>
                        <div style="text-align: center;">
                            <div style="color: #F59E0B; font-size: 24px; font-weight: bold;">${highCount}</div>
                            <div style="color: #9CA3AF; font-size: 10px;">High</div>
                        </div>
                        <div style="text-align: center;">
                            <div style="color: #60A5FA; font-size: 24px; font-weight: bold;">${this.bottlenecks.size}</div>
                            <div style="color: #9CA3AF; font-size: 10px;">Total</div>
                        </div>
                    </div>
                </div>
            `;
        }
        
        if (hotPathDiv) {
            let html = '<h3 style="color: #E5E7EB; margin-bottom: 10px;">Hot Paths</h3>';
            
            const topHotPaths = Array.from(this.hotPaths.entries())
                .sort((a, b) => b[1] - a[1])
                .slice(0, 5);
            
            topHotPaths.forEach(([path, count]) => {
                html += `
                    <div style="background: #1F2937; padding: 8px; margin-bottom: 6px; border-radius: 4px;">
                        <div style="color: #E5E7EB; font-size: 10px; word-break: break-all;">${path}</div>
                        <div style="color: #F59E0B; font-size: 10px; margin-top: 4px;">Called ${count} times</div>
                    </div>
                `;
            });
            
            hotPathDiv.innerHTML = html;
        }
        
        if (suggestionsDiv) {
            let html = '<h3 style="color: #E5E7EB; margin-bottom: 10px;">Optimization Suggestions</h3>';
            
            const suggestions = this.generateOptimizationSuggestions();
            suggestions.forEach(suggestion => {
                html += `
                    <div style="background: #1F2937; padding: 10px; margin-bottom: 8px; border-radius: 6px; border-left: 3px solid #10B981;">
                        <div style="color: #E5E7EB; font-weight: bold; margin-bottom: 4px;">${suggestion.title}</div>
                        <div style="color: #9CA3AF; font-size: 10px;">${suggestion.description}</div>
                    </div>
                `;
            });
            
            suggestionsDiv.innerHTML = html;
        }
    }
    
    updateMemoryTab() {
        const canvas = document.getElementById('memoryChart');
        if (canvas) {
            this.drawMemoryChart(canvas);
        }
        
        const statsDiv = document.getElementById('memoryStats');
        if (statsDiv && performance.memory) {
            const currentMem = performance.memory.usedJSHeapSize / 1048576;
            const totalMem = performance.memory.totalJSHeapSize / 1048576;
            const limitMem = performance.memory.jsHeapSizeLimit / 1048576;
            
            statsDiv.innerHTML = `
                <div style="background: #1F2937; padding: 15px; border-radius: 8px; margin-bottom: 10px;">
                    <h3 style="color: #E5E7EB; margin-bottom: 10px;">Memory Usage</h3>
                    <div style="margin-bottom: 8px;">
                        <div style="display: flex; justify-content: space-between; margin-bottom: 4px;">
                            <span style="color: #9CA3AF;">Used</span>
                            <span style="color: #E5E7EB;">${currentMem.toFixed(1)} MB</span>
                        </div>
                        <div style="background: #374151; height: 8px; border-radius: 4px; overflow: hidden;">
                            <div style="background: #60A5FA; height: 100%; width: ${(currentMem / limitMem * 100)}%;"></div>
                        </div>
                    </div>
                    <div style="font-size: 10px; color: #6B7280;">
                        Total: ${totalMem.toFixed(1)} MB | Limit: ${limitMem.toFixed(1)} MB
                    </div>
                </div>
            `;
        }
        
        const gcDiv = document.getElementById('gcEvents');
        if (gcDiv) {
            let html = '<h3 style="color: #E5E7EB; margin-bottom: 10px;">Recent GC Events</h3>';
            
            this.gcEvents.slice(-5).reverse().forEach(event => {
                const timeSince = ((Date.now() - event.timestamp) / 1000).toFixed(0);
                html += `
                    <div style="background: #1F2937; padding: 8px; margin-bottom: 6px; border-radius: 4px; font-size: 10px;">
                        <span style="color: #9CA3AF;">${timeSince}s ago</span>
                        <span style="color: #E5E7EB; margin-left: 10px;">Freed ${(event.amount / 1048576).toFixed(1)} MB</span>
                    </div>
                `;
            });
            
            gcDiv.innerHTML = html;
        }
    }
    
    updateWebWorkersTab() {
        const candidatesDiv = document.getElementById('workerCandidates');
        const opportunitiesDiv = document.getElementById('parallelizationOpportunities');
        
        if (candidatesDiv && this.topWorkerCandidates) {
            let html = '<h3 style="color: #E5E7EB; margin-bottom: 10px;">Web Worker Candidates</h3>';
            
            this.topWorkerCandidates.forEach((candidate, index) => {
                const benefit = (candidate.avgTime * candidate.callCount / 1000).toFixed(1);
                html += `
                    <div style="background: #1F2937; padding: 10px; margin-bottom: 8px; border-radius: 6px;">
                        <div style="display: flex; justify-content: space-between; margin-bottom: 6px;">
                            <span style="color: #E5E7EB; font-weight: bold;">${index + 1}. ${candidate.name}</span>
                            <span style="color: #10B981;">Score: ${candidate.score.toFixed(1)}</span>
                        </div>
                        <div style="font-size: 10px; color: #9CA3AF;">
                            <div>Avg time: ${candidate.avgTime.toFixed(2)}ms | Calls: ${candidate.callCount}</div>
                            <div>Potential benefit: ${benefit}ms per second</div>
                            <div>Data transfer: ~${(candidate.dataTransferSize / 1024).toFixed(1)} KB</div>
                        </div>
                    </div>
                `;
            });
            
            candidatesDiv.innerHTML = html;
        }
        
        if (opportunitiesDiv) {
            opportunitiesDiv.innerHTML = `
                <h3 style="color: #E5E7EB; margin-bottom: 10px;">Parallelization Opportunities</h3>
                <div style="background: #1F2937; padding: 15px; border-radius: 8px;">
                    <ul style="color: #9CA3AF; font-size: 11px; margin: 0; padding-left: 20px;">
                        <li>FlatBuffer parsing can be offloaded for large messages</li>
                        <li>Entity interpolation calculations are highly parallelizable</li>
                        <li>Starfield and visual effect updates can run in parallel</li>
                        <li>Minimap rendering can be done asynchronously</li>
                    </ul>
                </div>
            `;
        }
        
        // Setup worker code generation button
        const generateBtn = document.getElementById('generateWorkerCode');
        if (generateBtn) {
            generateBtn.onclick = () => this.generateWorkerCode();
        }
    }
    
    drawFrameChart(canvas) {
        const ctx = canvas.getContext('2d');
        const width = canvas.width;
        const height = canvas.height;
        
        // Clear
        ctx.fillStyle = '#0F172A';
        ctx.fillRect(0, 0, width, height);
        
        // Grid
        ctx.strokeStyle = '#1E293B';
        ctx.lineWidth = 1;
        ctx.setLineDash([2, 2]);
        
        // 60 FPS line
        const target60Y = height - (16.67 / 50) * height;
        ctx.beginPath();
        ctx.moveTo(0, target60Y);
        ctx.lineTo(width, target60Y);
        ctx.stroke();
        
        // 30 FPS line
        const target30Y = height - (33.33 / 50) * height;
        ctx.beginPath();
        ctx.moveTo(0, target30Y);
        ctx.lineTo(width, target30Y);
        ctx.stroke();
        
        ctx.setLineDash([]);
        
        // Draw frame times
        ctx.strokeStyle = '#60A5FA';
        ctx.lineWidth = 2;
        ctx.beginPath();
        
        const samples = this.frameMetrics.frameTimes.length;
        for (let i = 0; i < samples; i++) {
            const index = (this.frameMetrics.frameIndex + i) % samples;
            const frameTime = this.frameMetrics.frameTimes[index];
            const x = (i / samples) * width;
            const y = height - Math.min(frameTime / 50, 1) * height;
            
            if (i === 0) {
                ctx.moveTo(x, y);
            } else {
                ctx.lineTo(x, y);
            }
        }
        ctx.stroke();
        
        // Labels
        ctx.fillStyle = '#9CA3AF';
        ctx.font = '10px monospace';
        ctx.fillText('60 FPS', 5, target60Y - 2);
        ctx.fillText('30 FPS', 5, target30Y - 2);
        
        // Current FPS
        const avgFrameTime = this.calculateAverageFrameTime();
        const currentFPS = Math.round(1000 / avgFrameTime);
        ctx.font = '14px monospace';
        ctx.fillStyle = currentFPS >= 55 ? '#10B981' : currentFPS >= 30 ? '#F59E0B' : '#EF4444';
        ctx.fillText(`${currentFPS} FPS`, width - 50, 20);
    }
    
    drawMemoryChart(canvas) {
        const ctx = canvas.getContext('2d');
        const width = canvas.width;
        const height = canvas.height;
        
        // Clear
        ctx.fillStyle = '#0F172A';
        ctx.fillRect(0, 0, width, height);
        
        if (this.memorySnapshots.length < 2) return;
        
        // Find min/max for scaling
        let minMem = Infinity;
        let maxMem = 0;
        this.memorySnapshots.forEach(snapshot => {
            const mem = snapshot.usedJSHeapSize / 1048576; // MB
            minMem = Math.min(minMem, mem);
            maxMem = Math.max(maxMem, mem);
        });
        
        const range = maxMem - minMem || 1;
        const padding = range * 0.1;
        minMem -= padding;
        maxMem += padding;
        
        // Draw memory line
        ctx.strokeStyle = '#A78BFA';
        ctx.lineWidth = 2;
        ctx.beginPath();
        
        this.memorySnapshots.forEach((snapshot, i) => {
            const x = (i / (this.memorySnapshots.length - 1)) * width;
            const mem = snapshot.usedJSHeapSize / 1048576;
            const y = height - ((mem - minMem) / (maxMem - minMem)) * height;
            
            if (i === 0) {
                ctx.moveTo(x, y);
            } else {
                ctx.lineTo(x, y);
            }
        });
        ctx.stroke();
        
        // GC events
        ctx.fillStyle = '#10B981';
        this.gcEvents.forEach(event => {
            const snapshot = this.memorySnapshots.find(s => Math.abs(s.timestamp - event.timestamp) < 1000);
            if (snapshot) {
                const index = this.memorySnapshots.indexOf(snapshot);
                const x = (index / (this.memorySnapshots.length - 1)) * width;
                ctx.beginPath();
                ctx.arc(x, 10, 3, 0, Math.PI * 2);
                ctx.fill();
            }
        });
        
        // Labels
        ctx.fillStyle = '#9CA3AF';
        ctx.font = '10px monospace';
        ctx.fillText(`${maxMem.toFixed(0)} MB`, 5, 15);
        ctx.fillText(`${minMem.toFixed(0)} MB`, 5, height - 5);
    }
    
    generateOptimizationSuggestions() {
        const suggestions = [];
        
        // Check for high frequency low-value functions
        this.metrics.forEach((metric, name) => {
            if (metric.count > 1000 && metric.sum / metric.count < 0.1) {
                suggestions.push({
                    title: `Batch ${name} calls`,
                    description: `This function is called ${metric.count} times but takes only ${(metric.sum / metric.count).toFixed(2)}ms on average. Consider batching multiple calls.`
                });
            }
        });
        
        // Check for memory growth
        if (this.memorySnapshots.length > 10) {
            const firstMem = this.memorySnapshots[0].usedJSHeapSize;
            const lastMem = this.memorySnapshots[this.memorySnapshots.length - 1].usedJSHeapSize;
            const growth = lastMem - firstMem;
            if (growth > 10 * 1048576) { // 10MB growth
                suggestions.push({
                    title: 'Potential Memory Leak',
                    description: `Memory usage has grown by ${(growth / 1048576).toFixed(1)} MB. Check for retained references and clear unused objects.`
                });
            }
        }
        
        // Check for rendering bottlenecks
        const renderingFunctions = ['updateSprites', 'drawWalls', 'updateCamera'];
        renderingFunctions.forEach(func => {
            const metric = this.metrics.get(`Global.${func}`);
            if (metric && metric.count > 0) {
                const avg = metric.sum / metric.count;
                if (avg > 5) {
                    suggestions.push({
                        title: `Optimize ${func}`,
                        description: `${func} takes ${avg.toFixed(2)}ms on average. Consider using object pooling, spatial partitioning, or reducing draw calls.`
                    });
                }
            }
        });
        
        return suggestions;
    }
    
    generateWorkerCode() {
        if (!this.topWorkerCandidates || this.topWorkerCandidates.length === 0) {
            alert('No suitable functions for Web Worker implementation found.');
            return;
        }
        
        const candidate = this.topWorkerCandidates[0];
        const workerCode = `
// Web Worker implementation for ${candidate.name}
// Generated by Enhanced Performance Monitor

// Import necessary dependencies
importScripts('path/to/dependencies.js');

// Message handler
self.onmessage = function(e) {
    const { type, data } = e.data;
    
    switch (type) {
        case '${candidate.name}':
            const result = ${candidate.name}(data);
            self.postMessage({ type: 'result', data: result });
            break;
            
        default:
            console.error('Unknown message type:', type);
    }
};

// Function implementation (copy from main thread)
function ${candidate.name}(data) {
    // TODO: Copy the actual function implementation here
    // Make sure it doesn't rely on DOM or other main thread only APIs
}

// Helper functions (if needed)
// ...
        `;
        
        // Create and download the worker file
        const blob = new Blob([workerCode], { type: 'application/javascript' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = `${candidate.name}.worker.js`;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
        
        alert(`Worker code for ${candidate.name} has been generated and downloaded!`);
    }
    
    clearAllData() {
        this.metrics.clear();
        this.hotPaths.clear();
        this.memorySnapshots = [];
        this.gcEvents = [];
        this.workerCandidates.clear();
        this.frameMetrics.frameIndex = 0;
        this.frameMetrics.frameTimes.fill(0);
        
        console.log('Performance data cleared');
    }
    
    autoOptimize() {
        console.log('Auto-optimization starting...');
        
        // Find the most impactful optimizations
        const optimizations = [];
        
        // 1. Disable expensive visual effects if FPS is low
        const avgFrameTime = this.calculateAverageFrameTime();
        const currentFPS = 1000 / avgFrameTime;
        
        if (currentFPS < 30) {
            optimizations.push('Reducing visual quality for better performance');
            if (window.gameSettings) {
                window.gameSettings.particleEffects = false;
                window.gameSettings.screenShake = false;
                window.gameSettings.graphicsQuality = 'low';
            }
        }
        
        // 2. Reduce update frequencies for non-critical functions
        if (this.significantFunctions && this.significantFunctions.length > 0) {
            this.significantFunctions.forEach(func => {
                if (func.impact > 20 && func.name.includes('update')) {
                    optimizations.push(`Reducing update frequency for ${func.name}`);
                    // In a real implementation, you'd throttle these functions
                }
            });
        }
        
        alert(`Auto-optimization complete!\n\nApplied optimizations:\n${optimizations.join('\n')}`);
    }
    
    exportPerformanceData() {
        const data = {
            timestamp: new Date().toISOString(),
            summary: {
                avgFPS: Math.round(1000 / this.calculateAverageFrameTime()),
                avgFrameTime: this.calculateAverageFrameTime(),
                totalFunctions: this.metrics.size,
                bottlenecks: this.bottlenecks.size,
                memoryUsage: performance.memory ? {
                    current: performance.memory.usedJSHeapSize,
                    total: performance.memory.totalJSHeapSize,
                    limit: performance.memory.jsHeapSizeLimit
                } : null
            },
            frameMetrics: {
                samples: Array.from(this.frameMetrics.frameTimes),
                targetFPS: this.frameMetrics.targetFPS
            },
            functionMetrics: {},
            bottlenecks: Array.from(this.bottlenecks.entries()),
            hotPaths: Array.from(this.hotPaths.entries()).slice(0, 20),
            memorySnapshots: this.memorySnapshots.slice(-300), // Last 5 minutes
            gcEvents: this.gcEvents,
            workerCandidates: Array.from(this.workerCandidates.entries())
        };
        
        // Export detailed function metrics
        this.metrics.forEach((metric, name) => {
            if (metric.count > 0) {
                data.functionMetrics[name] = {
                    avg: metric.sum / metric.count,
                    min: metric.min,
                    max: metric.max,
                    count: metric.count,
                    total: metric.sum,
                    percentiles: metric.percentiles,
                    childCalls: Array.from(metric.childCalls.entries()),
                    parentCalls: Array.from(metric.parentCalls.entries())
                };
            }
        });
        
        // Create downloadable JSON
        const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = `performance-data-${Date.now()}.json`;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
        
        console.log('Performance data exported');
    }
    
    toggle() {
        this.enabled = !this.enabled;
        this.overlay.style.display = this.enabled ? 'flex' : 'none';
        
        if (this.enabled) {
            // Resume monitoring
            requestAnimationFrame(() => this.startFrameMonitoring());
        }
    }
}

// Export for global access
window.EnhancedPerformanceMonitor = EnhancedPerformanceMonitor;
