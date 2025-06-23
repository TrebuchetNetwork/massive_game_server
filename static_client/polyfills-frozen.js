// Ultra-robust frozen polyfill system
(function(global) {
    'use strict';
    
    console.log('[Frozen Polyfills] Initializing frozen polyfill system...');
    
    // Polyfill implementations
    const arrayFromPolyfill = (function() {
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
            var C = this;
            var items = Object(arrayLike);
            if (arrayLike == null) {
                throw new TypeError('Array.from requires an array-like object - not null or undefined');
            }
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
    
    const arrayIsArrayPolyfill = function(arg) {
        return Object.prototype.toString.call(arg) === '[object Array]';
    };
    
    const objectAssignPolyfill = function(target) {
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
    
    // Install polyfills with maximum protection
    function installFrozenPolyfills() {
        console.log('[Frozen Polyfills] Installing frozen polyfills...');
        
        // Array.from
        if (!Array.from || typeof Array.from !== 'function') {
            console.log('[Frozen Polyfills] Installing Array.from');
            Array.from = arrayFromPolyfill;
        }
        
        // Array.isArray
        if (!Array.isArray || typeof Array.isArray !== 'function') {
            console.log('[Frozen Polyfills] Installing Array.isArray');
            Array.isArray = arrayIsArrayPolyfill;
        }
        
        // Object.assign
        if (!Object.assign || typeof Object.assign !== 'function') {
            console.log('[Frozen Polyfills] Installing Object.assign');
            Object.assign = objectAssignPolyfill;
        }
        
        // Freeze the methods to prevent modification
        try {
            Object.defineProperty(Array, 'from', {
                value: arrayFromPolyfill,
                writable: false,
                enumerable: false,
                configurable: false
            });
        } catch (e) {
            console.warn('[Frozen Polyfills] Could not freeze Array.from:', e);
        }
        
        try {
            Object.defineProperty(Array, 'isArray', {
                value: arrayIsArrayPolyfill,
                writable: false,
                enumerable: false,
                configurable: false
            });
        } catch (e) {
            console.warn('[Frozen Polyfills] Could not freeze Array.isArray:', e);
        }
        
        try {
            Object.defineProperty(Object, 'assign', {
                value: objectAssignPolyfill,
                writable: false,
                enumerable: false,
                configurable: true
            });
        } catch (e) {
            console.warn('[Frozen Polyfills] Could not freeze Object.assign:', e);
        }
        
        // Also add to window object as backup
        if (global.window) {
            global.window.Array = Array;
            global.window.Object = Object;
        }
        
        console.log('[Frozen Polyfills] Verification:', {
            'Array.from': typeof Array.from === 'function',
            'Array.isArray': typeof Array.isArray === 'function',
            'Object.assign': typeof Object.assign === 'function'
        });
    }
    
    // Install immediately
    installFrozenPolyfills();
    
    // Override PIXI to ensure polyfills persist
    var originalDefineProperty = Object.defineProperty;
    Object.defineProperty = function(obj, prop, descriptor) {
        // Prevent overwriting our polyfills
        if ((obj === Array && (prop === 'from' || prop === 'isArray')) ||
            (obj === Object && prop === 'assign')) {
            console.warn('[Frozen Polyfills] Blocked attempt to override ' + prop);
            return obj;
        }
        return originalDefineProperty.call(this, obj, prop, descriptor);
    };
    
    // Create a protective wrapper for PIXI loading
    var pixiLoadCallbacks = [];
    Object.defineProperty(global, 'PIXI', {
        get: function() { return global._PIXI; },
        set: function(value) {
            console.log('[Frozen Polyfills] PIXI being set, protecting polyfills...');
            global._PIXI = value;
            
            // Re-apply polyfills after PIXI loads
            installFrozenPolyfills();
            
            // Execute any callbacks
            pixiLoadCallbacks.forEach(function(cb) { cb(value); });
            
            return value;
        },
        configurable: true
    });
    
    // Expose for debugging
    global.__frozenPolyfills = {
        arrayFrom: arrayFromPolyfill,
        arrayIsArray: arrayIsArrayPolyfill,
        objectAssign: objectAssignPolyfill,
        reinstall: installFrozenPolyfills,
        onPixiLoad: function(callback) { pixiLoadCallbacks.push(callback); }
    };
    
})(typeof window !== 'undefined' ? window : this);

// Additional safety: Re-apply after document ready
if (typeof document !== 'undefined') {
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', function() {
            if (window.__frozenPolyfills) {
                window.__frozenPolyfills.reinstall();
            }
        });
    }
}
