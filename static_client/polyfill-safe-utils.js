// Polyfill-safe utility functions
// These utilities ensure that ES6 methods work even if polyfills are overwritten

window.polyfillSafeUtils = (function() {
    'use strict';
    
    // Store references to polyfilled methods at load time
    const safeArrayFrom = (function() {
        // Implementation of Array.from that doesn't rely on itself
        return function(arrayLike, mapFn, thisArg) {
            if (arrayLike == null) {
                throw new TypeError('Array.from requires an array-like object - not null or undefined');
            }
            
            var items = Object(arrayLike);
            var len = parseInt(items.length) || 0;
            var result = [];
            
            for (var i = 0; i < len; i++) {
                if (i in items) {
                    if (mapFn) {
                        result[i] = thisArg ? mapFn.call(thisArg, items[i], i) : mapFn(items[i], i);
                    } else {
                        result[i] = items[i];
                    }
                }
            }
            
            result.length = len;
            return result;
        };
    })();
    
    const safeArrayIsArray = (function() {
        return function(arg) {
            return Object.prototype.toString.call(arg) === '[object Array]';
        };
    })();
    
    const safeObjectAssign = (function() {
        return function(target) {
            'use strict';
            if (target == null) {
                throw new TypeError('Cannot convert undefined or null to object');
            }
            
            var to = Object(target);
            
            for (var index = 1; index < arguments.length; index++) {
                var nextSource = arguments[index];
                
                if (nextSource != null) {
                    for (var nextKey in nextSource) {
                        if (Object.prototype.hasOwnProperty.call(nextSource, nextKey)) {
                            to[nextKey] = nextSource[nextKey];
                        }
                    }
                }
            }
            return to;
        };
    })();
    
    // Export the safe functions
    return {
        arrayFrom: safeArrayFrom,
        isArray: safeArrayIsArray,
        assign: safeObjectAssign,
        
        // Helper to convert array-like objects to arrays safely
        toArray: function(arrayLike) {
            var result = [];
            for (var i = 0; i < arrayLike.length; i++) {
                result.push(arrayLike[i]);
            }
            return result;
        },
        
        // Helper for safe iteration
        forEach: function(collection, callback, thisArg) {
            if (collection && typeof collection.length === 'number') {
                for (var i = 0; i < collection.length; i++) {
                    if (i in collection) {
                        callback.call(thisArg, collection[i], i, collection);
                    }
                }
            } else if (collection && typeof collection.forEach === 'function') {
                collection.forEach(callback, thisArg);
            }
        },
        
        // Helper for safe mapping
        map: function(collection, callback, thisArg) {
            var result = [];
            if (collection && typeof collection.length === 'number') {
                for (var i = 0; i < collection.length; i++) {
                    if (i in collection) {
                        result[i] = callback.call(thisArg, collection[i], i, collection);
                    }
                }
                result.length = collection.length;
            }
            return result;
        },
        
        // Helper for safe filtering
        filter: function(collection, predicate, thisArg) {
            var result = [];
            if (collection && typeof collection.length === 'number') {
                for (var i = 0; i < collection.length; i++) {
                    if (i in collection) {
                        var value = collection[i];
                        if (predicate.call(thisArg, value, i, collection)) {
                            result.push(value);
                        }
                    }
                }
            }
            return result;
        }
    };
})();

// Make sure these are available globally as fallbacks
if (typeof window.arrayFromSafe === 'undefined') {
    window.arrayFromSafe = window.polyfillSafeUtils.arrayFrom;
}
if (typeof window.isArraySafe === 'undefined') {
    window.isArraySafe = window.polyfillSafeUtils.isArray;
}
if (typeof window.assignSafe === 'undefined') {
    window.assignSafe = window.polyfillSafeUtils.assign;
}
