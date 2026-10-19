<script>
    import * as api from '../../api.js';

    let { vaults, onUnlocked, onVaultsChanged, onCreate } = $props();

    let selectedId = $state(null);
    let kind = $state('passphrase');
    let secret = $state('');
    let error = $state('');
    let busy = $state(false);
    let forgetArmed = $state(null);
    let forkDest = $state(null);
    let forkSecret = $state('');
    let forkKey = $state(null);
    let forkNewId = $state(null);
    let forkError = $state('');
    let forkBusy = $state(false);
    let note = $state('');
    let touchId = $state(null);

    const FS_WARNINGS = {
        cloud_sync:
            "That folder is in a cloud-synced location. Sync clients and the diary's database files do not mix — the vault is registered, but keeping it there risks corruption.",
        network_fs:
            "That folder is on a network share. The diary's database is unreliable over network filesystems — the vault is registered, but keeping it there is risky.",
    };

    const selected = $derived(vaults.find((v) => v.id === (selectedId ?? vaults[0]?.id)) ?? null);

    function describeError(e) {
        if (typeof e === 'string' && e.startsWith('backoff:')) {
            return `Too many wrong attempts. Wait ${e.slice('backoff:'.length)} seconds and try again.`;
        }
        return (
            {
                wrong_passphrase: 'Wrong passphrase or recovery key.',
                invalid_recovery_key: 'That does not look like a recovery key.',
                storage: 'The vault folder is not available right now.',
                vault_not_found: 'This vault is no longer in the list.',
                vault_mismatch: 'That folder holds a different vault.',
                touch_id_unavailable: 'Touch ID is not available for this diary right now.',
                destination_not_empty: 'That folder is not empty. Pick an empty folder for the restored diary.',
                corrupt: 'The vault looks damaged. The CLI can check it: soirvu-cli verify.',
            }[e] || `Something went wrong (${e}).`
        );
    }

    async function unlock() {
        if (!selected || !secret) return;
        busy = true;
        error = '';
        try {
            await api.vaultUnlock(selected.id, kind, secret);
            secret = '';
            onUnlocked(selected);
        } catch (e) {
            error = describeError(e);
        } finally {
            busy = false;
        }
    }

    // Touch ID is asked about per selected diary: the button
    // exists only when this build can use it AND the diary is
    // enrolled (her rule: never show a dead option).
    $effect(() => {
        const id = selected?.id;
        touchId = null;
        if (!id) return;
        api.touchIdStatus(id).then(
            (status) => {
                if (selected?.id === id) touchId = status;
            },
            () => {},
        );
    });

    async function unlockTouchId() {
        if (!selected) return;
        busy = true;
        error = '';
        try {
            await api.vaultUnlockTouchId(selected.id);
            onUnlocked(selected);
        } catch (e) {
            // A dismissed prompt is not an error to report.
            if (e !== 'touch_id_cancelled') error = describeError(e);
        } finally {
            busy = false;
        }
    }

    async function relocate(vault) {
        error = '';
        try {
            if (await api.vaultRelocate(vault.id)) onVaultsChanged();
        } catch (e) {
            error = describeError(e);
        }
    }

    async function forget(vault) {
        await api.vaultForget(vault.id);
        forgetArmed = null;
        onVaultsChanged();
    }

    async function doFork() {
        forkError = '';
        forkBusy = true;
        try {
            const forked = await api.vaultForkImport(forkSecret);
            forkSecret = '';
            forkNewId = forked.vault_id;
            forkKey = forked.recovery_key;
            onVaultsChanged();
        } catch (e) {
            forkError = describeError(e);
        } finally {
            forkBusy = false;
        }
    }

    function finishFork() {
        selectedId = forkNewId;
        forkKey = null;
        forkDest = null;
        forkNewId = null;
    }

    async function openAnother() {
        error = '';
        note = '';
        try {
            const picked = await api.vaultPickFolder('open');
            if (picked && picked.fork_required) {
                forkDest = picked.name;
                forkSecret = '';
                forkError = '';
                if (picked.warning) note = FS_WARNINGS[picked.warning] || '';
            } else if (picked) {
                selectedId = picked.vault_id;
                if (picked.warning) note = FS_WARNINGS[picked.warning] || '';
                onVaultsChanged();
            }
        } catch (e) {
            error = describeError(e);
        }
    }
</script>
<style src="./LockScreen.css"></style>

<div class="center-col">
    <div class="vault-list">
        {#each vaults as vault (vault.id)}
            <div class="vault-row">
                <button
                    class="vault-name"
                    class:active={selected?.id === vault.id}
                    onclick={() => (selectedId = vault.id)}>
                    {vault.name}
                    {#if vault.state === 'not_found'}<span class="missing"> — folder not found</span>{/if}
                    {#if vault.state === 'volume_missing'}<span class="missing"> — volume not mounted</span>{/if}
                </button>
                {#if vault.state === 'not_found'}
                    <button onclick={() => relocate(vault)}>Locate…</button>
                {/if}
                {#if vault.state === 'volume_missing'}
                    <button onclick={() => onVaultsChanged()}>Retry</button>
                {/if}
                {#if forgetArmed === vault.id}
                    <button onclick={() => forget(vault)}>Really forget?</button>
                {:else}
                    <button class="quiet" onclick={() => (forgetArmed = vault.id)}>Forget</button>
                {/if}
            </div>
        {/each}
    </div>

    {#if selected && selected.state === 'available'}
        <div class="kind-toggle">
            <button class:active={kind === 'passphrase'} onclick={() => (kind = 'passphrase')}> Passphrase </button>
            <button class:active={kind === 'recovery'} onclick={() => (kind = 'recovery')}> Recovery key </button>
        </div>
        <input
            type="password"
            placeholder={kind === 'passphrase' ? 'Passphrase' : 'XXXX-XXXX-…'}
            bind:value={secret}
            onkeydown={(e) => e.key === 'Enter' && unlock()} />
        <button onclick={unlock} disabled={busy || !secret}>Unlock</button>
        {#if touchId?.capable && touchId?.enrolled}
            <button onclick={unlockTouchId} disabled={busy}>Unlock with Touch ID</button>
        {/if}
    {/if}

    {#if forkKey}
        <div class="fork-panel">
            <p>The copy is now a separate diary. Its new recovery key:</p>
            <p><code>{forkKey}</code></p>
            <p class="note">Save it now — it is shown once. The old recovery key opens only the original diary.</p>
            <button onclick={finishFork}>Done</button>
        </div>
    {:else if forkDest}
        <div class="fork-panel">
            <p>
                “{forkDest}” is a copy of a diary you already have. To keep both, the copy needs its own identity.
            </p>
            <input
                type="password"
                placeholder="The diary's passphrase"
                bind:value={forkSecret}
                onkeydown={(e) => e.key === 'Enter' && doFork()} />
            <button onclick={doFork} disabled={forkBusy || !forkSecret}>Make it a separate diary</button>
            <button class="quiet" onclick={() => (forkDest = null)}>Not now</button>
            {#if forkError}<p class="error">{forkError}</p>{/if}
        </div>
    {/if}

    <button class="quiet" onclick={openAnother}>Open another diary folder…</button>
    <button class="quiet" onclick={() => onCreate()}>Create a new diary…</button>

    {#if note}<p class="note">{note}</p>{/if}
    {#if error}<p class="error">{error}</p>{/if}
</div>
