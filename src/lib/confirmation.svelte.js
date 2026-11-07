// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import { createRequestOwner } from './request-owner.js';

export function createConfirmation({ onConfirm, onConfirmed, describeError }) {
    let busy = $state(false);
    let error = $state('');
    const owner = createRequestOwner();

    async function confirm() {
        if (busy) return false;
        const token = owner.begin();
        if (!owner.current(token)) return false;
        busy = true;
        error = '';
        try {
            await onConfirm();
            if (!owner.current(token)) return false;
            await onConfirmed?.();
            return true;
        } catch (failure) {
            if (owner.current(token)) error = describeError(failure);
            return false;
        } finally {
            if (owner.current(token)) busy = false;
        }
    }

    return {
        get busy() { return busy; },
        get error() { return error; },
        confirm,
        dispose: () => owner.dispose(),
    };
}
