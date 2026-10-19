<script>
    import { onMount } from 'svelte';
    import * as api from './api.js';
    import MainScreenContainer from './containers/MainScreenContainer/MainScreenContainer.svelte';
    import DiaryScreenContainer from './containers/DiaryScreenContainer/DiaryScreenContainer.svelte';
    import SettingsScreenContainer from './containers/SettingsScreenContainer/SettingsScreenContainer.svelte';

    let screen = $state('boot'); // boot | main | diary | settings
    let mainView = $state('welcome'); // welcome | locked
    let vaults = $state([]);
    let currentVault = $state(null);

    async function refresh() {
        vaults = await api.vaultList();
        const open = vaults.find((v) => v.state === 'unlocked');
        if (open) {
            currentVault = open;
            screen = 'diary';
        } else if (vaults.length === 0) {
            mainView = 'welcome';
            currentVault = null;
            screen = 'main';
        } else {
            mainView = 'locked';
            currentVault = null;
            screen = 'main';
        }
    }

    onMount(async () => {
        await api.onVaultStatus(async ({ status }) => {
            if (status !== 'unlocked') {
                await refresh();
            }
        });
        // clipboard clearing: report copies;
        document.addEventListener('copy', () => {
            api.clipboardNoteCopy().catch(() => {});
        });
        // Manual lock: Cmd/Ctrl+Shift+L from anywhere in
        // the diary.
        window.addEventListener('keydown', (e) => {
            if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === 'l' && screen === 'diary') {
                e.preventDefault();
                lock();
            }
        });
        await refresh();
    });

    async function lock() {
        await api.vaultLock();
        await refresh();
    }
    // The drop safety net: a file dropped anywhere no
    // component claims (the lock screen, the list,
    // settings) must not navigate the webview to the
    // file. The editor claims its own drops and
    // prevents the default itself; this catches the
    // rest. Dragging over the window gets the same
    // treatment, or the drop never fires cleanly.
    function swallowDrop(event) {
        event.preventDefault();
    }
</script>

<svelte:window ondragover={swallowDrop} ondrop={swallowDrop} />

{#if screen === 'boot'}
    <p class="note">Starting…</p>
{:else if screen === 'main'}
    <MainScreenContainer {vaults} initialView={mainView} onVaultsChanged={refresh} />
{:else if screen === 'diary'}
    <DiaryScreenContainer vault={currentVault} onLock={lock} onSettings={() => (screen = 'settings')} />
{:else if screen === 'settings'}
    <SettingsScreenContainer onBack={() => (screen = 'diary')} />
{/if}
