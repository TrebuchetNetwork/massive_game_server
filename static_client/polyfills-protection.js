// Ultra-robust polyfill protection system
(function(global) {
    'use strict';
    
    console.log('[Polyfill Protection] Initializing ultra-robust polyfill system...');
    
    // Store our polyfill implementations in a closure
    const protectedPolyfills = {};
    
    // Array.from polyfill
    protectedPolyfills.arrayFrom = (function() {
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
    protectedPolyfills.arrayIsArray = function(arg) {
        return Object.prototype.toString.call(arg) === '[object Array]';
    };
    
    // Object.assign polyfill
    protectedPolyfills.objectAssign = function(target) {
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
    
    // Function to force-install polyfills with maximum protection
    function installProtectedPolyfills() {
        console.log('[Polyfill Protection] Installing protected polyfills...');
        
        // Helper to delete and redefine properties
        function forceDefine(obj, prop, value) {
            // Try to delete existing property
            try {
                delete obj[prop];
            } catch (e) {
                // If delete fails, try to make it configurable
                try {
                    Object.defineProperty(obj, prop, { configurable: true });
                    delete obj[prop];
                } catch (e2) {
                    console.warn('[Polyfill Protection] Could not delete ' + prop + ', will override');
                }
            }
            
            // Define with getter that always returns our polyfill
            try {
                Object.defineProperty(obj, prop, {
                    get: function() { return value; },
                    set: function() { 
                        console.warn('[Polyfill Protection] Attempt to override ' + prop + ' blocked!');
                        return false;
                    },
                    enumerable: false,
                    configurable: true
                });
            } catch (e) {
                // Fallback to direct assignment
                obj[prop] = value;
            }
        }
        
        // Install Array.from
        if (!Array.from || typeof Array.from !== 'function') {
            forceDefine(Array, 'from', protectedPolyfills.arrayFrom);
        }
        
        // Install Array.isArray
        if (!Array.isArray || typeof Array.isArray !== 'function') {
            forceDefine(Array, 'isArray', protectedPolyfills.arrayIsArray);
        }
        
        // Install Object.assign
        if (!Object.assign || typeof Object.assign !== 'function') {
            forceDefine(Object, 'assign', protectedPolyfills.objectAssign);
        }
        
        // Verify installation
        console.log('[Polyfill Protection] Verification:', {
            'Array.from': typeof Array.from === 'function',
            'Array.isArray': typeof Array.isArray === 'function',
            'Object.assign': typeof Object.assign === 'function'
        });
    }
    
    // Install immediately
    installProtectedPolyfills();
    
    // Create a protection system that checks for actual function removal
    var protectionInterval = setInterval(function() {
        // Check if any polyfill is missing (not just different reference)
        var needsReinstall = false;
        
        if (typeof Array.from !== 'function') {
            console.warn('[Polyfill Protection] Array.from is not a function! Reinstalling...');
            needsReinstall = true;
        }
        if (typeof Array.isArray !== 'function') {
            console.warn('[Polyfill Protection] Array.isArray is not a function! Reinstalling...');
            needsReinstall = true;
        }
        if (typeof Object.assign !== 'function') {
            console.warn('[Polyfill Protection] Object.assign is not a function! Reinstalling...');
            needsReinstall = true;
        }
        
        if (needsReinstall) {
            installProtectedPolyfills();
        }
    }, 100); // Check every 100ms
    
    // Also hook into PIXI initialization if possible
    if (global.PIXI) {
        console.log('[Polyfill Protection] PIXI detected, hooking into it...');
        var originalPIXI = global.PIXI;
        Object.defineProperty(global, 'PIXI', {
            get: function() { return originalPIXI; },
            set: function(newPIXI) {
                originalPIXI = newPIXI;
                // Reinstall polyfills after PIXI is set
                setTimeout(installProtectedPolyfills, 0);
            }
        });
    }
    
    // Expose for debugging
    global.__protectedPolyfills = protectedPolyfills;
    global.__reinstallPolyfills = installProtectedPolyfills;
    global.__stopPolyfillProtection = function() {
        clearInterval(protectionInterval);
        console.log('[Polyfill Protection] Protection stopped');
    };
    
    // Stop protection after 30 seconds (increased from 10)
    setTimeout(function() {
        clearInterval(protectionInterval);
        console.log('[Polyfill Protection] Protection interval stopped after 30 seconds');
        
        // Do one final check and install
        installProtectedPolyfills();
    }, 30000);
    
})(typeof window !== 'undefined' ? window : this);
