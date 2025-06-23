/**
 * Runtime Polyfill Protector
 * Aggressively monitors and restores polyfills that get overwritten during runtime
 */

(function(window) {
    'use strict';
    
    console.log('[Runtime Polyfill Protector] Initializing...');
    
    // Store the correct polyfill implementations
    const correctPolyfills = {
        ArrayFrom: (function() {
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
        }()),
        
        ArrayIsArray: function(arg) {
            return Object.prototype.toString.call(arg) === '[object Array]';
        },
        
        ObjectAssign: function(target, firstSource) {
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
    
    // Check if a polyfill is correctly installed
    function isPolyfillValid(name, func) {
        if (typeof func !== 'function') return false;
        
        // Check if it's our polyfill by testing its behavior
        try {
            switch(name) {
                case 'Array.from':
                    const testResult = func([1, 2, 3]);
                    return testResult && testResult.length === 3;
                case 'Array.isArray':
                    return func([]) === true && func({}) === false;
                case 'Object.assign':
                    const testObj = func({}, {a: 1});
                    return testObj && testObj.a === 1;
                default:
                    return true;
            }
        } catch (e) {
            return false;
        }
    }
    
    // Force restore a polyfill
    function forceRestore(name, implementation) {
        console.warn(`[Runtime Polyfill Protector] Restoring ${name}`);
        
        switch(name) {
            case 'Array.from':
                Array.from = implementation;
                if (window.Array) window.Array.from = implementation;
                break;
            case 'Array.isArray':
                Array.isArray = implementation;
                if (window.Array) window.Array.isArray = implementation;
                break;
            case 'Object.assign':
                Object.assign = implementation;
                if (window.Object) window.Object.assign = implementation;
                break;
        }
    }
    
    // Check and restore all polyfills
    function checkAndRestore() {
        let restored = false;
        
        // Check Array.from
        if (!isPolyfillValid('Array.from', Array.from)) {
            forceRestore('Array.from', correctPolyfills.ArrayFrom);
            restored = true;
        }
        
        // Check Array.isArray
        if (!isPolyfillValid('Array.isArray', Array.isArray)) {
            forceRestore('Array.isArray', correctPolyfills.ArrayIsArray);
            restored = true;
        }
        
        // Check Object.assign
        if (!isPolyfillValid('Object.assign', Object.assign)) {
            forceRestore('Object.assign', correctPolyfills.ObjectAssign);
            restored = true;
        }
        
        return restored;
    }
    
    // Initial restoration
    checkAndRestore();
    
    // Monitor continuously - check every 100ms
    let checkInterval = setInterval(function() {
        checkAndRestore();
    }, 100);
    
    // Also check on specific events that might cause issues
    const criticalEvents = ['DOMContentLoaded', 'load', 'error'];
    criticalEvents.forEach(function(eventName) {
        window.addEventListener(eventName, function() {
            setTimeout(checkAndRestore, 0);
        }, true);
    });
    
    // Override defineProperty to catch attempts to redefine our polyfills
    const originalDefineProperty = Object.defineProperty;
    Object.defineProperty = function(obj, prop, descriptor) {
        // Check if someone is trying to redefine our polyfills
        if ((obj === Array && (prop === 'from' || prop === 'isArray')) ||
            (obj === Object && prop === 'assign')) {
            console.warn(`[Runtime Polyfill Protector] Blocked attempt to redefine ${obj.name}.${prop}`);
            return obj;
        }
        return originalDefineProperty.call(this, obj, prop, descriptor);
    };
    
    // Expose API for manual restoration
    window.__runtimePolyfillProtector = {
        checkAndRestore: checkAndRestore,
        isPolyfillValid: isPolyfillValid,
        forceRestoreAll: function() {
            console.log('[Runtime Polyfill Protector] Force restoring all polyfills');
            forceRestore('Array.from', correctPolyfills.ArrayFrom);
            forceRestore('Array.isArray', correctPolyfills.ArrayIsArray);
            forceRestore('Object.assign', correctPolyfills.ObjectAssign);
        },
        stopMonitoring: function() {
            if (checkInterval) {
                clearInterval(checkInterval);
                checkInterval = null;
                console.log('[Runtime Polyfill Protector] Monitoring stopped');
            }
        },
        startMonitoring: function() {
            if (!checkInterval) {
                checkInterval = setInterval(checkAndRestore, 100);
                console.log('[Runtime Polyfill Protector] Monitoring started');
            }
        }
    };
    
    console.log('[Runtime Polyfill Protector] Ready and monitoring');
    
})(window);
