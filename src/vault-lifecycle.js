// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import * as api from '@/api.js';
import { notify } from '@/components/Notification/store.svelte.js';
import { EVENT_TYPE, VAULT_STATUS } from '@/constants.js';
import { applicationText } from '@/data.js';
import { runFlushHooks } from '@/lib/lock-hooks.js';

export function startVaultLifecycle({ refresh }) {
    let unlisten;
    let unlistenLock;
    let destroyed = false;

    api.onVaultStatus(({ status }) => {
        if (status !== VAULT_STATUS.UNLOCKED) refresh();
    }).then((stop) => {
        if (destroyed) stop?.();
        else unlisten = stop;
    });

    api.onLockRequested(async ({ generation }) => {
        let ok = false;
        try {
            ok = await runFlushHooks();
        } finally {
            api.vaultLockReady(generation, ok).catch(() => {});
        }
    }).then((stop) => {
        if (destroyed) stop?.();
        else unlistenLock = stop;
    });

    api.lockNoticeTake()
        .then((dropped) => {
            if (dropped) notify(EVENT_TYPE.ERROR, applicationText.lockNotice);
        })
        .catch(() => {});

    refresh();

    return () => {
        destroyed = true;
        unlisten?.();
        unlistenLock?.();
    };
}
