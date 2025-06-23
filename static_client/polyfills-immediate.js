// Immediate Polyfill Application - Execute as early as possible
(function() {
    'use strict';
    
    console.log('[Immediate Polyfills] Starting immediate application...');
    
    // Helper to check if we're in a browser environment
    var isBrowser = typeof window !== 'undefined' && typeof document !== 'undefined';
    if (!isBrowser) {
        console.error('[Immediate Polyfills] Not in browser environment!');
        return;
    }
    
    // Get the true global object
    var global = window;
    
    // Store native implementations if they exist
    var natives = {
        ArrayFrom: Array.from,
        ArrayIsArray: Array.isArray,
        ObjectAssign: Object.assign,
        defineProperty: Object.defineProperty,
        getOwnPropertyDescriptor: Object.getOwnPropertyDescriptor
    };
    
    // Define robust polyfills
    var polyfills = {
        arrayFrom: (function() {
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
                if (this == null) {
                    throw new TypeError('Array.from called on null or undefined');
                }
                if (arrayLike == null) {
                    throw new TypeError('Array.from requires an array-like object - not null or undefined');
                }
                
                var C = this;
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
        })(),
        
        arrayIsArray: function isArray(arg) {
            return Object.prototype.toString.call(arg) === '[object Array]';
        },
        
        objectAssign: function assign(target, varArgs) {
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
        }
    };
    
    // Force application function
    function forceApplyPolyfills() {
        console.log('[Immediate Polyfills] Force applying polyfills...');
        
        // Array.from
        try {
            if (!Array.from || typeof Array.from !== 'function') {
                Array.from = polyfills.arrayFrom;
            }
            // Force it onto the constructor
            global.Array.from = polyfills.arrayFrom;
            
            // Also apply to any Array constructor we can find
            if (global.Array && global.Array.constructor) {
                global.Array.constructor.from = polyfills.arrayFrom;
            }
        } catch (e) {
            console.error('[Immediate Polyfills] Error applying Array.from:', e);
        }
        
        // Array.isArray
        try {
            if (!Array.isArray || typeof Array.isArray !== 'function') {
                Array.isArray = polyfills.arrayIsArray;
            }
            global.Array.isArray = polyfills.arrayIsArray;
            
            if (global.Array && global.Array.constructor) {
                global.Array.constructor.isArray = polyfills.arrayIsArray;
            }
        } catch (e) {
            console.error('[Immediate Polyfills] Error applying Array.isArray:', e);
        }
        
        // Object.assign
        try {
            if (!Object.assign || typeof Object.assign !== 'function') {
                Object.assign = polyfills.objectAssign;
            }
            global.Object.assign = polyfills.objectAssign;
            
            if (global.Object && global.Object.constructor) {
                global.Object.constructor.assign = polyfills.objectAssign;
            }
        } catch (e) {
            console.error('[Immediate Polyfills] Error applying Object.assign:', e);
        }
        
        // Make polyfills non-configurable
        try {
            Object.defineProperty(Array, 'from', {
                value: polyfills.arrayFrom,
                writable: true,
                enumerable: false,
                configurable: false
            });
            
            Object.defineProperty(Array, 'isArray', {
                value: polyfills.arrayIsArray,
                writable: true,
                enumerable: false,
                configurable: false
            });
            
            Object.defineProperty(Object, 'assign', {
                value: polyfills.objectAssign,
                writable: true,
                enumerable: false,
                configurable: false
            });
        } catch (e) {
            console.warn('[Immediate Polyfills] Could not make polyfills non-configurable:', e);
        }
    }
    
    // Apply immediately
    forceApplyPolyfills();
    
    // Override PIXI Color class if it exists
    function patchPixiColor() {
        if (global.PIXI && global.PIXI.Color) {
            console.log('[Immediate Polyfills] Patching PIXI.Color...');
            
            var originalNormalize = global.PIXI.Color.prototype.normalize;
            if (originalNormalize) {
                global.PIXI.Color.prototype.normalize = function(value) {
                    // Ensure Array.isArray is available
                    if (typeof Array.isArray !== 'function') {
                        Array.isArray = polyfills.arrayIsArray;
                    }
                    return originalNormalize.call(this, value);
                };
            }
        }
    }
    
    // Monitor for PIXI loading
    var pixiCheckCount = 0;
    var pixiInterval = setInterval(function() {
        pixiCheckCount++;
        if (global.PIXI) {
            console.log('[Immediate Polyfills] PIXI detected, patching...');
            patchPixiColor();
            forceApplyPolyfills(); // Reapply after PIXI loads
            clearInterval(pixiInterval);
        } else if (pixiCheckCount > 100) { // Stop after 10 seconds
            clearInterval(pixiInterval);
        }
    }, 100);
    
    // Continuous protection
    var protectionCount = 0;
    var protectionInterval = setInterval(function() {
        protectionCount++;
        
        var needsReapply = false;
        
        // Check each method
        if (typeof Array.from !== 'function') {
            console.warn('[Immediate Polyfills] Array.from was removed! Count:', protectionCount);
            needsReapply = true;
        }
        if (typeof Array.isArray !== 'function') {
            console.warn('[Immediate Polyfills] Array.isArray was removed! Count:', protectionCount);
            needsReapply = true;
        }
        if (typeof Object.assign !== 'function') {
            console.warn('[Immediate Polyfills] Object.assign was removed! Count:', protectionCount);
            needsReapply = true;
        }
        
        if (needsReapply) {
            forceApplyPolyfills();
        }
        
        // Stop after 30 seconds
        if (protectionCount > 300) {
            clearInterval(protectionInterval);
            console.log('[Immediate Polyfills] Protection stopped after 30 seconds');
        }
    }, 100);
    
    // Expose for debugging
    global.__immediatePolyfills = {
        polyfills: polyfills,
        forceApply: forceApplyPolyfills,
        verify: function() {
            return {
                'Array.from': typeof Array.from,
                'Array.isArray': typeof Array.isArray,
                'Object.assign': typeof Object.assign,
                'global.Array.from': typeof global.Array.from,
                'global.Array.isArray': typeof global.Array.isArray,
                'global.Object.assign': typeof global.Object.assign
            };
        },
        test: function() {
            var results = [];
            
            try {
                var arr = Array.from([1, 2, 3]);
                results.push('Array.from: SUCCESS - ' + JSON.stringify(arr));
            } catch (e) {
                results.push('Array.from: FAIL - ' + e.message);
            }
            
            try {
                var isArr = Array.isArray([1, 2, 3]);
                results.push('Array.isArray: SUCCESS - ' + isArr);
            } catch (e) {
                results.push('Array.isArray: FAIL - ' + e.message);
            }
            
            try {
                var obj = Object.assign({}, {a: 1}, {b: 2});
                results.push('Object.assign: SUCCESS - ' + JSON.stringify(obj));
            } catch (e) {
                results.push('Object.assign: FAIL - ' + e.message);
            }
            
            return results;
        }
    };
    
    // Final verification
    console.log('[Immediate Polyfills] Initial application complete:', global.__immediatePolyfills.verify());
    
    // Apply on DOMContentLoaded as well
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', forceApplyPolyfills);
    }
    
    // Apply on load as well
    window.addEventListener('load', forceApplyPolyfills);
    
})();
