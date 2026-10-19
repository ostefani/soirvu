<script>
    import * as api from '../../api.js';
    import Button from '../../components/Button/Button.svelte';
    import NoteComponent from '../../components/NoteComponent/NoteComponent.svelte';
    import OnboardingContainer from '../OnboardingContainer/OnboardingContainer.svelte';
    import LockScreenContainer from '../LockScreenContainer/LockScreenContainer.svelte';

    let { vaults, initialView, onVaultsChanged } = $props();

    let view = $state('welcome');
    let welcomeNote = $state('');

    $effect(() => {
        view = initialView;
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
            await onVaultsChanged();
        }
    }

    const welcomeActions = [
        { name: 'create', label: 'Create a new diary', handler: () => (view = 'create') },
        { name: 'open', label: 'Open an existing diary folder…', kind: 'quiet', handler: openExisting },
    ];
</script>
<style src="./MainScreenContainer.css"></style>

{#if view === 'welcome'}
    <div class="center-col">
        <p>An encrypted diary. It lives in a folder you choose; nothing leaves your machine.</p>
        {#each welcomeActions as action (action.name)}
            <Button {...action} />
        {/each}
        <NoteComponent text={welcomeNote} />
    </div>
{:else if view === 'create'}
    <OnboardingContainer onDone={onVaultsChanged} onCancel={vaults.length > 0 ? onVaultsChanged : undefined} />
{:else}
    <LockScreenContainer
        {vaults}
        onUnlocked={onVaultsChanged}
        onVaultsChanged={onVaultsChanged}
        onCreate={() => (view = 'create')} />
{/if}
