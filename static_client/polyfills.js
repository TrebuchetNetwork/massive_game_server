// Enhanced Polyfills with Protection Against Overwriting
(function(global) {
    'use strict';
    
    console.log('[Polyfills] Starting enhanced polyfill application...');
    
    // Store original implementations
    const polyfills = {};
    
    // Array.from polyfill
    polyfills.arrayFrom = (function() {
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
    
    // Array.isArray polyfill
    polyfills.arrayIsArray = function(arg) {
        return Object.prototype.toString.call(arg) === '[object Array]';
    };
    
    // Object.assign polyfill
    polyfills.objectAssign = function(target, firstSource) {
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
    
    // Function to apply polyfills with protection
    function applyPolyfills() {
        console.log('[Polyfills] Applying polyfills...');
        
        // Array.from
        if (!Array.from || typeof Array.from !== 'function') {
            console.log('[Polyfills] Installing Array.from');
            Array.from = polyfills.arrayFrom;
        }
        
        // Array.isArray
        if (!Array.isArray || typeof Array.isArray !== 'function') {
            console.log('[Polyfills] Installing Array.isArray');
            Array.isArray = polyfills.arrayIsArray;
        }
        
        // Object.assign
        if (!Object.assign || typeof Object.assign !== 'function') {
            console.log('[Polyfills] Installing Object.assign');
            try {
                Object.defineProperty(Object, 'assign', {
                    value: polyfills.objectAssign,
                    writable: true,
                    configurable: true
                });
            } catch (e) {
                Object.assign = polyfills.objectAssign;
            }
        }
        
        // Verify installation
        const status = {
            'Array.from': typeof Array.from === 'function',
            'Array.isArray': typeof Array.isArray === 'function',
            'Object.assign': typeof Object.assign === 'function'
        };
        
        console.log('[Polyfills] Status:', status);
        
        // Return false if any polyfill failed
        return status['Array.from'] && status['Array.isArray'] && status['Object.assign'];
    }
    
    // Apply polyfills immediately
    applyPolyfills();
    
    // Create a protective wrapper that checks and re-applies polyfills
    let checkInterval;
    function startPolyfillProtection() {
        checkInterval = setInterval(function() {
            let needsReapply = false;
            
            if (typeof Array.from !== 'function') {
                console.warn('[Polyfills] Array.from was removed! Reapplying...');
                needsReapply = true;
            }
            if (typeof Array.isArray !== 'function') {
                console.warn('[Polyfills] Array.isArray was removed! Reapplying...');
                needsReapply = true;
            }
            if (typeof Object.assign !== 'function') {
                console.warn('[Polyfills] Object.assign was removed! Reapplying...');
                needsReapply = true;
            }
            
            if (needsReapply) {
                applyPolyfills();
            }
        }, 100); // Check every 100ms
        
        // Stop checking after 10 seconds (when everything should be loaded)
        setTimeout(function() {
            clearInterval(checkInterval);
            console.log('[Polyfills] Protection check stopped after 10 seconds');
        }, 10000);
    }
    
    // Start protection
    startPolyfillProtection();
    
    // Export functions for manual re-application if needed
    global.__reapplyPolyfills = applyPolyfills;
    global.__polyfills = polyfills;
    
    // Store original descriptors
    global.__originalDescriptors = {
        arrayFrom: Object.getOwnPropertyDescriptor(Array, 'from'),
        arrayIsArray: Object.getOwnPropertyDescriptor(Array, 'isArray'),
        objectAssign: Object.getOwnPropertyDescriptor(Object, 'assign')
    };
    
    // Force polyfills to be writable first, then set them
    try {
        // Delete existing properties if they're non-configurable
        try { delete Array.from; } catch (e) {}
        try { delete Array.isArray; } catch (e) {}
        try { delete Object.assign; } catch (e) {}
        
        // Re-apply with our polyfills
        Object.defineProperty(Array, 'from', {
            value: polyfills.arrayFrom,
            writable: true,
            configurable: true,
            enumerable: false
        });
        Object.defineProperty(Array, 'isArray', {
            value: polyfills.arrayIsArray,
            writable: true,
            configurable: true,
            enumerable: false
        });
        Object.defineProperty(Object, 'assign', {
            value: polyfills.objectAssign,
            writable: true,
            configurable: true,
            enumerable: true
        });
    } catch (e) {
        console.log('[Polyfills] Error setting polyfills:', e.message);
        // Fallback: just assign directly
        Array.from = polyfills.arrayFrom;
        Array.isArray = polyfills.arrayIsArray;
        Object.assign = polyfills.objectAssign;
    }
    
})(typeof window !== 'undefined' ? window : global);
