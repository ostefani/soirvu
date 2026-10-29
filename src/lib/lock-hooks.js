// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
const hooks = new Set();

export function registerFlushHook(fn) {
    hooks.add(fn);
    return () => hooks.delete(fn);
}

export async function runFlushHooks() {
    const results = await Promise.allSettled([...hooks].map((hook) => hook()));
    return results.every((r) => r.status === 'fulfilled' && r.value !== false);
}
