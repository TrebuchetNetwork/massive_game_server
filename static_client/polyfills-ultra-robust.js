// Ultra-robust polyfill system with maximum compatibility
(function(global) {
    'use strict';
    
    console.log('[Ultra Polyfills] Starting polyfill installation...');
    
    // Helper to safely get the global object
    var globalObj = (function() {
        if (typeof globalThis !== 'undefined') return globalThis;
        if (typeof window !== 'undefined') return window;
        if (typeof global !== 'undefined') return global;
        if (typeof self !== 'undefined') return self;
        throw new Error('Unable to locate global object');
    })();
    
    // Store original methods to prevent loss
    var nativeArrayFrom = Array.from;
    var nativeArrayIsArray = Array.isArray;
    var nativeObjectAssign = Object.assign;
    
    // Flag to track if we're in a PIXI context
    var inPixiContext = false;
    
    // Polyfill implementations
    var arrayFromPolyfill = (function() {
        var toStr = Object.prototype.toString;
        var isCallable = function(fn) {
            return typeof fn === 'function' || toStr.call(fn) === '[object Function]';
        };
        var toInteger = function(value) {
            var number = Number(value);
            if (isNaN(number)) { return 0; }
            if (number === 0 || !isFinite(number)) { return number; }
            return (number > 0 ? 1 : -1) * Math.floor(Math.abs(number));
        };
        var maxSafeInteger = Math.pow(2, 53) - 1;
        var toLength = function(value) {
            var len = toInteger(value);
            return Math.min(Math.max(len, 0), maxSafeInteger);
        };
        
        return function from(arrayLike/*, mapFn, thisArg */) {
            // C can be this (if called as Array.from) or Array (if called statically)
            var C = this;
            
            // Handle null/undefined
            if (arrayLike == null) {
                throw new TypeError('Array.from requires an array-like object - not null or undefined');
            }
            
            var items = Object(arrayLike);
            var mapFn = arguments.length > 1 ? arguments[1] : void undefined;
            var T;
            
            if (typeof mapFn !== 'undefined') {
                if (!isCallable(mapFn)) {
                    throw new TypeError('Array.from: when provided, the second argument must be a function');
                }
                if (arguments.length > 2) {
                    T = arguments[2];
                }
            }
            
            var len = toLength(items.length);
            var A = isCallable(C) ? Object(new C(len)) : new Array(len);
            var k = 0;
            var kValue;
            
            while (k < len) {
                kValue = items[k];
                if (mapFn) {
                    A[k] = typeof T === 'undefined' ? mapFn(kValue, k) : mapFn.call(T, kValue, k);
                } else {
                    A[k] = kValue;
                }
                k += 1;
            }
            
            A.length = len;
            return A;
        };
    }());
    
    var arrayIsArrayPolyfill = function isArray(arg) {
        return Object.prototype.toString.call(arg) === '[object Array]';
    };
    
    var objectAssignPolyfill = function assign(target, varArgs) {
        'use strict';
        if (target === undefined || target === null) {
            throw new TypeError('Cannot convert first argument to object');
        }
        
        var to = Object(target);
        for (var i = 1; i < arguments.length; i++) {
            var nextSource = arguments[i];
            if (nextSource === undefined || nextSource === null) {
                continue;
            }
            
            var keysArray = Object.keys(Object(nextSource));
            for (var nextIndex = 0, len = keysArray.length; nextIndex < len; nextIndex++) {
                var nextKey = keysArray[nextIndex];
                var desc = Object.getOwnPropertyDescriptor(nextSource, nextKey);
                if (desc !== undefined && desc.enumerable) {
                    to[nextKey] = nextSource[nextKey];
                }
            }
        }
        return to;
    };
    
    // Install polyfills function
    function installPolyfills() {
        console.log('[Ultra Polyfills] Installing polyfills...');
        
        // Array.from
        if (!Array.from || typeof Array.from !== 'function') {
            console.log('[Ultra Polyfills] Installing Array.from polyfill');
            Array.from = arrayFromPolyfill;
        }
        
        // Array.isArray
        if (!Array.isArray || typeof Array.isArray !== 'function') {
            console.log('[Ultra Polyfills] Installing Array.isArray polyfill');
            Array.isArray = arrayIsArrayPolyfill;
        }
        
        // Object.assign
        if (!Object.assign || typeof Object.assign !== 'function') {
            console.log('[Ultra Polyfills] Installing Object.assign polyfill');
            Object.assign = objectAssignPolyfill;
        }
        
        // Additional safety: ensure methods exist on constructors
        if (typeof Array.from !== 'function') {
            Array.from = arrayFromPolyfill;
        }
        if (typeof Array.isArray !== 'function') {
            Array.isArray = arrayIsArrayPolyfill;
        }
        if (typeof Object.assign !== 'function') {
            Object.assign = objectAssignPolyfill;
        }
        
        // Make sure they're also available globally
        if (globalObj.Array && typeof globalObj.Array.from !== 'function') {
            globalObj.Array.from = arrayFromPolyfill;
        }
        if (globalObj.Array && typeof globalObj.Array.isArray !== 'function') {
            globalObj.Array.isArray = arrayIsArrayPolyfill;
        }
        if (globalObj.Object && typeof globalObj.Object.assign !== 'function') {
            globalObj.Object.assign = objectAssignPolyfill;
        }
        
        console.log('[Ultra Polyfills] Verification:', {
            'Array.from': typeof Array.from === 'function',
            'Array.isArray': typeof Array.isArray === 'function',
            'Object.assign': typeof Object.assign === 'function',
            'global.Array.from': globalObj.Array && typeof globalObj.Array.from === 'function',
            'global.Array.isArray': globalObj.Array && typeof globalObj.Array.isArray === 'function',
            'global.Object.assign': globalObj.Object && typeof globalObj.Object.assign === 'function'
        });
    }
    
    // Install immediately
    installPolyfills();
    
    // Protection system - continuously monitors and restores polyfills
    var protectionInterval = setInterval(function() {
        // Check if polyfills are still intact
        var needsReinstall = false;
        
        if (typeof Array.from !== 'function' || Array.from === null || Array.from === undefined) {
            console.warn('[Ultra Polyfills] Array.from was removed! Reinstalling...');
            needsReinstall = true;
        }
        if (typeof Array.isArray !== 'function' || Array.isArray === null || Array.isArray === undefined) {
            console.warn('[Ultra Polyfills] Array.isArray was removed! Reinstalling...');
            needsReinstall = true;
        }
        if (typeof Object.assign !== 'function' || Object.assign === null || Object.assign === undefined) {
            console.warn('[Ultra Polyfills] Object.assign was removed! Reinstalling...');
            needsReinstall = true;
        }
        
        if (needsReinstall) {
            installPolyfills();
        }
    }, 100); // Check every 100ms
    
    // Stop protection after 30 seconds (should be enough time for everything to load)
    setTimeout(function() {
        clearInterval(protectionInterval);
        console.log('[Ultra Polyfills] Protection monitoring stopped after 30 seconds');
    }, 30000);
    
    // Override defineProperty to prevent removal of our polyfills
    var originalDefineProperty = Object.defineProperty;
    if (originalDefineProperty) {
        Object.defineProperty = function(obj, prop, descriptor) {
            // Block attempts to redefine our polyfills to non-functions
            if (obj === Array && prop === 'from' && (!descriptor.value || typeof descriptor.value !== 'function')) {
                console.warn('[Ultra Polyfills] Blocked attempt to remove Array.from');
                return obj;
            }
            if (obj === Array && prop === 'isArray' && (!descriptor.value || typeof descriptor.value !== 'function')) {
                console.warn('[Ultra Polyfills] Blocked attempt to remove Array.isArray');
                return obj;
            }
            if (obj === Object && prop === 'assign' && (!descriptor.value || typeof descriptor.value !== 'function')) {
                console.warn('[Ultra Polyfills] Blocked attempt to remove Object.assign');
                return obj;
            }
            
            return originalDefineProperty.call(this, obj, prop, descriptor);
        };
    }
    
    // Monitor for PIXI loading
    var pixiCheckInterval = setInterval(function() {
        if (globalObj.PIXI && !inPixiContext) {
            inPixiContext = true;
            console.log('[Ultra Polyfills] PIXI detected! Ensuring polyfills are still active...');
            installPolyfills();
            
            // Double-check after a short delay
            setTimeout(function() {
                installPolyfills();
                console.log('[Ultra Polyfills] Post-PIXI verification complete');
            }, 100);
            
            clearInterval(pixiCheckInterval);
        }
    }, 50);
    
    // Stop checking for PIXI after 10 seconds
    setTimeout(function() {
        clearInterval(pixiCheckInterval);
    }, 10000);
    
    // Expose for debugging and manual reinstall
    globalObj.__ultraPolyfills = {
        arrayFrom: arrayFromPolyfill,
        arrayIsArray: arrayIsArrayPolyfill,
        objectAssign: objectAssignPolyfill,
        reinstall: installPolyfills,
        verify: function() {
            return {
                'Array.from': typeof Array.from === 'function',
                'Array.isArray': typeof Array.isArray === 'function',
                'Object.assign': typeof Object.assign === 'function',
                'Array.from.toString()': Array.from ? Array.from.toString().substring(0, 50) : 'missing',
                'Array.isArray.toString()': Array.isArray ? Array.isArray.toString().substring(0, 50) : 'missing',
                'Object.assign.toString()': Object.assign ? Object.assign.toString().substring(0, 50) : 'missing'
            };
        }
    };
    
    // Final verification
    console.log('[Ultra Polyfills] Initial installation complete. Status:', globalObj.__ultraPolyfills.verify());
    
})(typeof window !== 'undefined' ? window : typeof global !== 'undefined' ? global : this);

// Additional safety: Create a self-healing system
(function() {
    'use strict';
    
    // This runs after the main polyfill installation
    var healingCount = 0;
    var maxHealingAttempts = 50;
    
    function selfHeal() {
        healingCount++;
        
        // Check and heal if needed
        var healed = false;
        
        if (typeof Array.from !== 'function') {
            console.warn('[Self-Healing] Array.from is broken! Attempting to heal...');
            if (window.__ultraPolyfills && window.__ultraPolyfills.arrayFrom) {
                Array.from = window.__ultraPolyfills.arrayFrom;
                healed = true;
            }
        }
        
        if (typeof Array.isArray !== 'function') {
            console.warn('[Self-Healing] Array.isArray is broken! Attempting to heal...');
            if (window.__ultraPolyfills && window.__ultraPolyfills.arrayIsArray) {
                Array.isArray = window.__ultraPolyfills.arrayIsArray;
                healed = true;
            }
        }
        
        if (typeof Object.assign !== 'function') {
            console.warn('[Self-Healing] Object.assign is broken! Attempting to heal...');
            if (window.__ultraPolyfills && window.__ultraPolyfills.objectAssign) {
                Object.assign = window.__ultraPolyfills.objectAssign;
                healed = true;
            }
        }
        
        if (healed) {
            console.log('[Self-Healing] Healing complete. Verification:', window.__ultraPolyfills.verify());
        }
        
        // Continue healing if needed and under limit
        if (healingCount < maxHealingAttempts) {
            setTimeout(selfHeal, 200);
        }
    }
    
    // Start self-healing after a brief delay
    setTimeout(selfHeal, 50);
})();
