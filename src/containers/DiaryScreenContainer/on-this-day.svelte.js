// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import * as api from '@/api.js';
import { createRequestOwner } from '@/lib/request-owner.js';
import { todayISO } from './today.js';

/**
 * Whole-vault "On this day" entries. Call from component
 * initialisation: the request owner is disposed with the component.
 *
 * Vault I/O arrives through `deps` (the `@/api.js` namespace unless a
 * caller passes fakes), so asynchronous behavior can be exercised
 * without native Tauri.
 */
export function createOnThisDay(deps = api) {
    const owner = createRequestOwner();
    let entries = $state.raw([]);

    async function load() {
        const token = owner.begin();
        try {
            const days = await deps.onThisDay(todayISO());
            if (owner.current(token)) entries = days;
        } catch {
            if (owner.current(token)) entries = [];
        }
    }

    $effect(() => () => owner.dispose());

    return {
        get entries() {
            return entries;
        },
        load,
    };
}
