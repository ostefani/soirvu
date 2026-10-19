<script>
    import { onMount } from 'svelte';
    import * as api from './api.js';
    import Onboarding from './components/Onboarding/Onboarding.svelte';
    import LockScreen from './components/LockScreen/LockScreen.svelte';
    import Diary from './components/Diary/Diary.svelte';

    let screen = $state('boot'); // boot | welcome | create | locked | diary
    let vaults = $state([]);
    let currentVault = $state(null);
    let welcomeNote = $state('');

    async function refresh() {
        vaults = await api.vaultList();
        const open = vaults.find((v) => v.state === 'unlocked');
        if (open) {
            currentVault = open;
            screen = 'diary';
        } else if (vaults.length === 0) {
            screen = 'welcome';
        } else {
            currentVault = null;
            screen = 'locked';
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

    const FS_WARNINGS = {
        cloud_sync:
            "That folder is in a cloud-synced location. Sync clients and the diary's database files do not mix — the vault is registered, but keeping it there risks corruption.",
        network_fs:
            "That folder is on a network share. The diary's database is unreliable over network filesystems — the vault is registered, but keeping it there is risky.",
    };

    async function openExisting() {
        welcomeNote = '';
        const picked = await api.vaultPickFolder('open');
        if (picked) {
            if (picked.warning) welcomeNote = FS_WARNINGS[picked.warning] || '';
            await refresh();
        }
    }

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
{:else if screen === 'welcome'}
    <div class="center-col">
        <p>An encrypted diary. It lives in a folder you choose; nothing leaves your machine.</p>
        <button onclick={() => (screen = 'create')}>Create a new diary</button>
        <button class="quiet" onclick={openExisting}>Open an existing diary folder…</button>
        {#if welcomeNote}<p class="note">{welcomeNote}</p>{/if}
    </div>
{:else if screen === 'create'}
    <Onboarding onDone={refresh} onCancel={vaults.length > 0 ? refresh : undefined} />
{:else if screen === 'locked'}
    <LockScreen {vaults} onUnlocked={refresh} onVaultsChanged={refresh} onCreate={() => (screen = 'create')} />
{:else if screen === 'diary'}
    <Diary vault={currentVault} onLock={lock} />
{/if}
