import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const config = readFileSync(new URL('../../docker/nginx.conf', import.meta.url), 'utf8');

function exactLocation(path) {
    const marker = `        location = ${path} {`;
    const start = config.indexOf(marker);
    assert.notEqual(start, -1, `missing exact location ${path}`);
    const end = config.indexOf('\n        }', start);
    assert.notEqual(end, -1, `unterminated location ${path}`);
    return config.slice(start, end);
}

test('nginx landing paths allow same-origin child frames without hiding app headers', () => {
    for (const path of ['/', '/index.html']) {
        const block = exactLocation(path);
        assert.match(block, /frame-src 'self'/);
        assert.match(block, /frame-ancestors 'none'/);
        assert.doesNotMatch(block, /proxy_hide_header/);
    }
});

test('nginx client path allows same-origin framing without hiding app headers', () => {
    const block = exactLocation('/client.html');
    assert.match(block, /add_header X-Frame-Options "SAMEORIGIN" always;/);
    assert.match(block, /frame-ancestors 'self'/);
    assert.doesNotMatch(block, /X-Frame-Options "DENY"/);
    assert.doesNotMatch(block, /proxy_hide_header/);
});

test('nginx default security headers remain restrictive', () => {
    assert.match(config, /add_header X-Frame-Options "DENY" always;/);
    assert.match(config, /frame-src 'none'/);
    assert.match(config, /frame-ancestors 'none'/);
});
