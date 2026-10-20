<script>
    import { onMount } from 'svelte';
    import * as api from './api.js';
    import MainScreenContainer from './containers/MainScreenContainer/MainScreenContainer.svelte';
    import DiaryScreenContainer from './containers/DiaryScreenContainer/DiaryScreenContainer.svelte';
    import SettingsScreenContainer from './containers/SettingsScreenContainer/SettingsScreenContainer.svelte';

    import {
        APPLICATION_SCREEN,
        MAIN_VIEW,
        VAULT_STATUS,
        KEYBOARD_SHORTCUT,
    } from './constants.js';
    import { applicationText } from './data.js';

    let screen = $state(APPLICATION_SCREEN.BOOT);
    let mainView = $state(MAIN_VIEW.WELCOME);
    let vaults = $state([]);
    let currentVault = $state(null);
    let diaryElement = $state(null);

    async function refresh() {
        vaults = await api.vaultList();
        const openVault = vaults.find((vault) => vault.state === VAULT_STATUS.UNLOCKED);
        if (openVault) {
            currentVault = openVault;
            screen = APPLICATION_SCREEN.DIARY;
        } else if (vaults.length === 0) {
            mainView = MAIN_VIEW.WELCOME;
            currentVault = null;
            screen = APPLICATION_SCREEN.MAIN;
        } else {
            mainView = MAIN_VIEW.LOCKED;
            currentVault = null;
            screen = APPLICATION_SCREEN.MAIN;
        }
    }

    onMount(async () => {
        await api.onVaultStatus(async ({ status }) => {
            if (status !== VAULT_STATUS.UNLOCKED) {
                await refresh();
            }
        });

        // Clipboard clearing: report copies
        document.addEventListener('copy', () => {
            api.clipboardNoteCopy().catch(() => {});
        });

        // Manual lock: Cmd/Ctrl+Shift+L from anywhere in the diary
        window.addEventListener('keydown', (event) => {
            const hasModifier = event.metaKey || event.ctrlKey;
            const matchesKey = event.key.toLowerCase() === KEYBOARD_SHORTCUT.LOCK_KEY;
            const isInDiary = screen === APPLICATION_SCREEN.DIARY;

            if (hasModifier && event.shiftKey && matchesKey && isInDiary) {
                event.preventDefault();
                lock();
            }
        });

        await refresh();
    });

    async function lock() {
        await api.vaultLock();
        await refresh();
    }

    // Drop safety net: prevent accidental file navigation on unhandled drag & drop
    function swallowDrop(event) {
        event.preventDefault();
    }
</script>

<svelte:window ondragover={swallowDrop} ondrop={swallowDrop} />

{#if screen === APPLICATION_SCREEN.BOOT}
    <p class="note">{applicationText.bootMessage}</p>
{:else if screen === APPLICATION_SCREEN.MAIN}
    <MainScreenContainer {vaults} initialView={mainView} onVaultsChanged={refresh} />
{:else if screen === APPLICATION_SCREEN.DIARY || screen === APPLICATION_SCREEN.SETTINGS}
    <DiaryScreenContainer
        bind:this={diaryElement}
        vault={currentVault}
        onLock={lock}
        onSettings={() => (screen = APPLICATION_SCREEN.SETTINGS)}
    />
    {#if screen === APPLICATION_SCREEN.SETTINGS}
        <SettingsScreenContainer
            onBack={() => (screen = APPLICATION_SCREEN.DIARY)}
            onImported={() => diaryElement?.refreshNavigation()}
        />
    {/if}
{/if}
