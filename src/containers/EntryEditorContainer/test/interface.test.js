import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');

test('the engine implements the methods used by the session and link controller', () => {
    const session = read('../session.svelte.js');
    const links = read('../links.svelte.js');
    const engineSource = read('../../../editor/index.js');
    const sessionMethods = session.matchAll(/\bengine(?:\?\.|\.)(\w+)/g);
    const linkMethods = links.matchAll(/\b(?:editor|getEditor\(\))(?:\?\.|\.)(\w+)/g);
    const used = new Set([...sessionMethods, ...linkMethods].map((match) => match[1]));
    assert.ok(used.size > 0);
    for (const name of used) {
        assert.match(engineSource, new RegExp(`^        ${name}\\(`, 'm'), `Missing editor method: ${name}`);
    }
});
