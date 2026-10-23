<script>
    import * as api from '@/api.js';
    import Button from '@/components/Button/index.svelte';
    import NoteComponent from '@/components/NoteComponent/index.svelte';
    import OnboardingContainer from '@/containers/OnboardingContainer/index.svelte';
    import LockScreenContainer from '@/containers/LockScreenContainer/index.svelte';
    import { MAIN_VIEW } from '@/constants.js';

    let { vaults, initialView, onVaultsChanged } = $props();

    // Follows initialView, but can be overridden locally (assignable $derived, Svelte 5.25+).
    let view = $derived(initialView);
    let welcomeNote = $state('');

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
            welcomeNote = FS_WARNINGS[picked.warning] ?? '';
            await onVaultsChanged();
        }
    }

    function showCreate() {
        welcomeNote = '';
        view = MAIN_VIEW.CREATE;
    }

    function cancelCreate() {
        view = vaults.length > 0 ? MAIN_VIEW.LOCKED : MAIN_VIEW.WELCOME;
    }
</script>

{#if view === MAIN_VIEW.WELCOME}
    <div class="center-col">
        <p>An encrypted diary. It lives in a folder you choose; nothing leaves your machine.</p>
        <Button label="Create a new diary" handler={showCreate} />
        <Button label="Open an existing diary folder…" kind="default" handler={openExisting} />
        <NoteComponent text={welcomeNote} />
    </div>
{:else if view === MAIN_VIEW.CREATE}
    <OnboardingContainer onDone={onVaultsChanged} onCancel={cancelCreate} />
{:else if view === MAIN_VIEW.LOCKED}
    <LockScreenContainer {vaults} onUnlocked={onVaultsChanged} {onVaultsChanged} onCreate={showCreate} />
{/if}

<style src="./styles.css"></style>
