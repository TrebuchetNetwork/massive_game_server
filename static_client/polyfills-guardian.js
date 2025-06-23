// Polyfill Guardian - Continuous protection and restoration of critical polyfills
(function(global) {
    'use strict';
    
    console.log('[Polyfill Guardian] Initializing protection system...');
    
    // Store original implementations
    const implementations = {
        ArrayFrom: null,
        ArrayIsArray: null,
        ObjectAssign: null
    };
    
    // Polyfill implementations
    const polyfills = {
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
    
    // Store initial implementations
    function storeImplementations() {
        implementations.ArrayFrom = Array.from || polyfills.ArrayFrom;
        implementations.ArrayIsArray = Array.isArray || polyfills.ArrayIsArray;
        implementations.ObjectAssign = Object.assign || polyfills.ObjectAssign;
    }
    
    // Install polyfills with protection
    function installPolyfills() {
        try {
            // Array.from
            if (!Array.from || typeof Array.from !== 'function') {
                console.log('[Polyfill Guardian] Installing Array.from');
                Array.from = implementations.ArrayFrom || polyfills.ArrayFrom;
            }
            
            // Array.isArray
            if (!Array.isArray || typeof Array.isArray !== 'function') {
                console.log('[Polyfill Guardian] Installing Array.isArray');
                Array.isArray = implementations.ArrayIsArray || polyfills.ArrayIsArray;
            }
            
            // Object.assign
            if (!Object.assign || typeof Object.assign !== 'function') {
                console.log('[Polyfill Guardian] Installing Object.assign');
                if (Object.defineProperty) {
                    Object.defineProperty(Object, 'assign', {
                        enumerable: false,
                        configurable: true,
                        writable: true,
                        value: implementations.ObjectAssign || polyfills.ObjectAssign
                    });
                } else {
                    Object.assign = implementations.ObjectAssign || polyfills.ObjectAssign;
                }
            }
            
            // Additional ES6 polyfills
            if (!String.prototype.includes) {
                String.prototype.includes = function(search, start) {
                    'use strict';
                    if (typeof start !== 'number') {
                        start = 0;
                    }
                    if (start + search.length > this.length) {
                        return false;
                    } else {
                        return this.indexOf(search, start) !== -1;
                    }
                };
            }
            
            if (!Array.prototype.includes) {
                Array.prototype.includes = function(searchElement, fromIndex) {
                    var O = Object(this);
                    var len = parseInt(O.length) || 0;
                    if (len === 0) {
                        return false;
                    }
                    var n = parseInt(fromIndex) || 0;
                    var k;
                    if (n >= 0) {
                        k = n;
                    } else {
                        k = len + n;
                        if (k < 0) {k = 0;}
                    }
                    var currentElement;
                    while (k < len) {
                        currentElement = O[k];
                        if (searchElement === currentElement) {
                            return true;
                        }
                        k++;
                    }
                    return false;
                };
            }
            
            if (!Array.prototype.find) {
                Array.prototype.find = function(predicate) {
                    if (this == null) {
                        throw new TypeError('Array.prototype.find called on null or undefined');
                    }
                    if (typeof predicate !== 'function') {
                        throw new TypeError('predicate must be a function');
                    }
                    var list = Object(this);
                    var length = list.length >>> 0;
                    var thisArg = arguments[1];
                    var value;
                    for (var i = 0; i < length; i++) {
                        value = list[i];
                        if (predicate.call(thisArg, value, i, list)) {
                            return value;
                        }
                    }
                    return undefined;
                };
            }
            
            if (!Number.isFinite) {
                Number.isFinite = function(value) {
                    return typeof value === 'number' && isFinite(value);
                };
            }
            
            if (!Math.sign) {
                Math.sign = function(x) {
                    x = +x;
                    if (x === 0 || isNaN(x)) {
                        return x;
                    }
                    return x > 0 ? 1 : -1;
                };
            }
            
            if (!Object.keys) {
                Object.keys = function(obj) {
                    var keys = [];
                    for (var key in obj) {
                        if (Object.prototype.hasOwnProperty.call(obj, key)) {
                            keys.push(key);
                        }
                    }
                    return keys;
                };
            }
            
        } catch (e) {
            console.error('[Polyfill Guardian] Error installing polyfills:', e);
        }
    }
    
    // Check if polyfills are intact
    function checkPolyfills() {
        const issues = [];
        
        if (typeof Array.from !== 'function') {
            issues.push('Array.from');
        }
        if (typeof Array.isArray !== 'function') {
            issues.push('Array.isArray');
        }
        if (typeof Object.assign !== 'function') {
            issues.push('Object.assign');
        }
        
        return issues;
    }
    
    // Monitor and protect polyfills
    let monitoringActive = false;
    let checkInterval = null;
    
    function startMonitoring() {
        if (monitoringActive) return;
        
        monitoringActive = true;
        console.log('[Polyfill Guardian] Starting continuous monitoring...');
        
        // Check every 100ms for the first 10 seconds, then every 500ms
        let fastCheckDuration = 10000;
        let startTime = Date.now();
        
        checkInterval = setInterval(() => {
            const issues = checkPolyfills();
            
            if (issues.length > 0) {
                console.warn('[Polyfill Guardian] Detected missing polyfills:', issues);
                installPolyfills();
                console.log('[Polyfill Guardian] Polyfills restored');
            }
            
            // Switch to slower interval after initial period
            if (Date.now() - startTime > fastCheckDuration && checkInterval) {
                clearInterval(checkInterval);
                checkInterval = setInterval(() => {
                    const issues = checkPolyfills();
                    if (issues.length > 0) {
                        console.warn('[Polyfill Guardian] Detected missing polyfills:', issues);
                        installPolyfills();
                    }
                }, 500);
            }
        }, 100);
    }
    
    function stopMonitoring() {
        monitoringActive = false;
        if (checkInterval) {
            clearInterval(checkInterval);
            checkInterval = null;
        }
        console.log('[Polyfill Guardian] Monitoring stopped');
    }
    
    // Protect against common overwrite patterns
    function protectAgainstOverwrites() {
        // Intercept potential overwrite attempts
        if (Object.defineProperty) {
            try {
                // Monitor Array constructor modifications
                let arrayDescriptor = Object.getOwnPropertyDescriptor(global, 'Array');
                if (arrayDescriptor && arrayDescriptor.configurable) {
                    Object.defineProperty(global, 'Array', {
                        get: function() {
                            return arrayDescriptor.value;
                        },
                        set: function(newValue) {
                            console.warn('[Polyfill Guardian] Attempt to overwrite Array detected!');
                            // Allow the overwrite but immediately restore our polyfills
                            arrayDescriptor.value = newValue;
                            setTimeout(() => {
                                if (!Array.from || typeof Array.from !== 'function') {
                                    Array.from = implementations.ArrayFrom;
                                }
                                if (!Array.isArray || typeof Array.isArray !== 'function') {
                                    Array.isArray = implementations.ArrayIsArray;
                                }
                            }, 0);
                        },
                        configurable: true
                    });
                }
                
                // Monitor Object constructor modifications  
                let objectDescriptor = Object.getOwnPropertyDescriptor(global, 'Object');
                if (objectDescriptor && objectDescriptor.configurable) {
                    Object.defineProperty(global, 'Object', {
                        get: function() {
                            return objectDescriptor.value;
                        },
                        set: function(newValue) {
                            console.warn('[Polyfill Guardian] Attempt to overwrite Object detected!');
                            objectDescriptor.value = newValue;
                            setTimeout(() => {
                                if (!Object.assign || typeof Object.assign !== 'function') {
                                    if (Object.defineProperty) {
                                        Object.defineProperty(Object, 'assign', {
                                            enumerable: false,
                                            configurable: true,
                                            writable: true,
                                            value: implementations.ObjectAssign
                                        });
                                    }
                                }
                            }, 0);
                        },
                        configurable: true
                    });
                }
            } catch (e) {
                console.warn('[Polyfill Guardian] Could not set up overwrite protection:', e);
            }
        }
    }
    
    // Public API
    global.__polyfillGuardian = {
        install: installPolyfills,
        check: checkPolyfills,
        startMonitoring: startMonitoring,
        stopMonitoring: stopMonitoring,
        forceRestore: function() {
            console.log('[Polyfill Guardian] Force restoring all polyfills...');
            installPolyfills();
            const issues = checkPolyfills();
            if (issues.length === 0) {
                console.log('[Polyfill Guardian] All polyfills restored successfully');
                return true;
            } else {
                console.error('[Polyfill Guardian] Failed to restore:', issues);
                return false;
            }
        }
    };
    
    // Initialize
    storeImplementations();
    installPolyfills();
    protectAgainstOverwrites();
    
    // Auto-start monitoring after a short delay
    setTimeout(() => {
        startMonitoring();
    }, 100);
    
    // Verify initial installation
    const initialIssues = checkPolyfills();
    if (initialIssues.length === 0) {
        console.log('[Polyfill Guardian] All polyfills installed successfully');
    } else {
        console.error('[Polyfill Guardian] Initial installation failed for:', initialIssues);
    }
    
})(window);
