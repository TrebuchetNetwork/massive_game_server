// Preload flatbuffers globally to fix import issues
import * as flatbuffers from 'https://cdn.jsdelivr.net/npm/flatbuffers@23.5.26/+esm';

// Make flatbuffers available globally
window.flatbuffers = flatbuffers;

// Also make it available as a module for dynamic imports
window.flatbuffersModule = flatbuffers;

// Override the import resolution for flatbuffers
const originalDynamicImport = window.eval('import');
if (originalDynamicImport) {
    window.eval(`
        const _originalImport = import;
        window.import = function(specifier) {
            if (specifier === 'flatbuffers') {
                return Promise.resolve(window.flatbuffersModule);
            }
            return _originalImport(specifier);
        };
    `);
}

console.log('Flatbuffers preloaded successfully');
