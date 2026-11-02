import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const containerDir = join(dirname(fileURLToPath(import.meta.url)), '..');
const sessionSource = readFileSync(join(containerDir, 'session.svelte.js'), 'utf8');
const viewSource = readFileSync(
    join(containerDir, '..', '..', 'components', 'EntryEditor', 'index.svelte'),
    'utf8',
);

test('every engine method the session calls is exported by the view', () => {
    const used = new Set(
        [...sessionSource.matchAll(/viewApi\?\.(\w+)|viewApi\.(\w+)/g)].map((m) => m[1] ?? m[2]),
    );
    const exported = new Set([...viewSource.matchAll(/export function (\w+)/g)].map((m) => m[1]));
    assert.ok(used.size > 0);
    for (const name of [...used].sort()) {
        assert.ok(
            exported.has(name),
            `viewApi.${name} is called by the session but not exported by EntryEditor`,
        );
    }
});
