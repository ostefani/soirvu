// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
import { getContext, setContext } from 'svelte';

/**
 * The three lookups every ActionRow needs. They are passed as getters so that
 * the values stay live when the parent's props change.
 *
 * @typedef {{
 *   readonly ACTIONS: Record<string, object>,
 *   readonly handlers: Record<string, (...args: any[]) => any>,
 *   readonly disabled: Record<string, boolean>,
 * }} ActionSource
 */

const KEY = Symbol('action-row');

/**
 * Called once by the screen that owns the actions (Settings).
 * @param {ActionSource} source
 */
export function provideActions(source) {
    setContext(KEY, source);
}

/** @returns {ActionSource} */
export function useActions() {
    const source = getContext(KEY);
    if (!source) {
        throw new Error('ActionRow must be rendered inside a screen that calls provideActions()');
    }
    return /** @type {ActionSource} */ (source);
}
