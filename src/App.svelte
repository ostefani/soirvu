<script>
    import { onMount } from 'svelte';
    import * as api from '@/api.js';
    import MainScreenContainer from '@/containers/MainScreenContainer/index.svelte';
    import DiaryScreenContainer from '@/containers/DiaryScreenContainer/index.svelte';
    import SettingsScreenContainer from '@/containers/SettingsScreenContainer/index.svelte';
    import NotificationStack from '@/containers/NotificationStackContainer/index.svelte';

    import { APPLICATION_SCREEN, MAIN_VIEW, VAULT_STATUS, KEYBOARD_SHORTCUT, EVENT_TYPE } from '@/constants.js';
    import { notify } from '@/components/Notification/store.svelte.js';
    import { runFlushHooks } from '@/lib/lock-hooks.js';
    import { applicationText } from '@/data.js';

    let screen = $state(APPLICATION_SCREEN.BOOT);
    let mainView = $state(MAIN_VIEW.WELCOME);
    let vaults = $state([]);
    let currentVault = $state(null);
    let importTick = $state(0);

    async function refresh() {
        vaults = await api.vaultList();
        const openVault = vaults.find((vault) => vault.state === VAULT_STATUS.UNLOCKED);
        if (openVault) {
            currentVault = openVault;
            screen = APPLICATION_SCREEN.DIARY;
        } else {
            currentVault = null;
            mainView = vaults.length === 0 ? MAIN_VIEW.WELCOME : MAIN_VIEW.LOCKED;
            screen = APPLICATION_SCREEN.MAIN;
        }
    }

    async function lock() {
        await api.vaultLock();
        await refresh();
    }

    const ACTIVITY_REPORT_INTERVAL = 5000;
    let lastActivitySent = 0;
    function noteActivity() {
        const now = Date.now();
        if (now - lastActivitySent < ACTIVITY_REPORT_INTERVAL) return;
        lastActivitySent = now;
        api.vaultActivity().catch(() => {});
    }

    // Manual lock: Cmd/Ctrl+Shift+L from anywhere in the diary
    function handleKeydown(event) {
        noteActivity();
        const isLockShortcut =
            (event.metaKey || event.ctrlKey) &&
            event.shiftKey &&
            event.key.toLowerCase() === KEYBOARD_SHORTCUT.LOCK_KEY;

        if (isLockShortcut && screen === APPLICATION_SCREEN.DIARY) {
            event.preventDefault();
            lock();
        }
    }

    // Clipboard clearing: report copies
    function handleCopy() {
        api.clipboardNoteCopy().catch(() => {});
    }

    // Drop safety net: prevent accidental file navigation on unhandled drag & drop
    function swallowDrop(event) {
        event.preventDefault();
    }

    onMount(() => {
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
                if (dropped) notify(EVENT_TYPE.ERROR, 'The diary locked before your latest edits could be saved.');
            })
            .catch(() => {});

        refresh();

        return () => {
            destroyed = true;
            unlisten?.();
            unlistenLock?.();
        };
    });
</script>

<svelte:window onkeydown={handleKeydown} onpointerdown={noteActivity} onwheel={noteActivity} ontouchstart={noteActivity} onmousemove={noteActivity} ondragover={swallowDrop} ondrop={swallowDrop} />
<svelte:document oncopy={handleCopy} />

{#if screen === APPLICATION_SCREEN.BOOT}
    <p class="note">{applicationText.bootMessage}</p>
{:else if screen === APPLICATION_SCREEN.MAIN}
    <MainScreenContainer {vaults} initialView={mainView} onVaultsChanged={refresh} />
{:else if screen === APPLICATION_SCREEN.DIARY || screen === APPLICATION_SCREEN.SETTINGS}
    <DiaryScreenContainer
        vault={currentVault}
        importTick={importTick}
        onLock={lock}
        onSettings={() => (screen = APPLICATION_SCREEN.SETTINGS)} />
    {#if screen === APPLICATION_SCREEN.SETTINGS}
        <SettingsScreenContainer
            onBack={() => (screen = APPLICATION_SCREEN.DIARY)}
            onImported={() => importTick++} />
    {/if}
{/if}

<NotificationStack />
