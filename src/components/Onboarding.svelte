<script>
    // Create-a-vault flow (§13 Phase 2 onboarding): pick an empty
    // folder, set a passphrase, then see the recovery key exactly
    // once and confirm it is saved before entering the diary.
    import * as api from '../api.js';

    let { onDone, onCancel = null } = $props();

    let step = $state(1); // 1 folder, 2 passphrase, 3 recovery key
    let folderName = $state('');
    let warning = $state('');

    const FS_WARNINGS = {
        cloud_sync:
            "This folder is in a cloud-synced location. Sync clients and the diary's database files do not mix — a synced copy can be taken mid-write and sync conflicts can corrupt the vault. You can continue, but a local folder is safer.",
        network_fs:
            "This folder is on a network share. The diary's database (SQLite) is unreliable over network filesystems. You can continue, but a local folder is safer.",
    };
    let passphrase = $state('');
    let confirm = $state('');
    let recoveryKey = $state('');
    let saved = $state(false);
    let error = $state('');
    let busy = $state(false);

    const ERRORS = {
        folder_not_empty:
            'That folder is not empty. A vault needs an empty folder — pick one, or create a new subfolder for it.',
        empty_passphrase: 'The passphrase must not be empty.',
        no_folder_picked: 'Pick a folder first.',
        storage: 'The vault could not be written there. Is the folder writable?',
    };

    async function pick() {
        error = '';
        const picked = await api.vaultPickFolder('create');
        if (picked) {
            folderName = picked.name;
            warning = picked.warning ? FS_WARNINGS[picked.warning] || '' : '';
            step = 2;
        }
    }

    async function create() {
        error = '';
        if (passphrase.length < 8) {
            error =
                'Use at least 8 characters — longer is better. This passphrase is the only protection the diary has.';
            return;
        }
        if (passphrase !== confirm) {
            error = 'The two passphrases do not match.';
            return;
        }
        busy = true;
        try {
            const created = await api.vaultCreate(passphrase);
            recoveryKey = created.recovery_key;
            passphrase = '';
            confirm = '';
            step = 3;
        } catch (e) {
            error = ERRORS[e] || `Something went wrong (${e}).`;
        } finally {
            busy = false;
        }
    }
</script>

<div class="center-col">
    {#if step === 1}
        <p>Create your diary in a folder you choose. Everything in it is encrypted.</p>
        <button onclick={pick}>Choose an empty folder…</button>
    {:else if step === 2}
        <p>Diary folder: <strong>{folderName}</strong></p>
        {#if warning}<p class="note">{warning}</p>{/if}
        <label>
            Passphrase
            <input type="password" bind:value={passphrase} autocomplete="new-password" />
        </label>
        <label>
            Repeat it
            <input type="password" bind:value={confirm} autocomplete="new-password" />
        </label>
        <button onclick={create} disabled={busy}>Create the diary</button>
    {:else}
        <p>
            This is your <strong>recovery key</strong>. If you forget the passphrase, it is the only way back in. The
            app does not keep a copy — write it down and keep it somewhere safe.
        </p>
        <p class="recovery-key">{recoveryKey}</p>
        <label class="check">
            <input type="checkbox" bind:checked={saved} />
            I have saved the recovery key
        </label>
        <button onclick={() => onDone()} disabled={!saved}>Open my diary</button>
    {/if}

    {#if onCancel && step < 3}
        <button class="quiet" onclick={() => onCancel()}>Back</button>
    {/if}

    {#if error}<p class="error">{error}</p>{/if}
</div>

<style>
    /* The recovery key is shown once, here, so its style lives here. */
    .recovery-key {
        font-family: ui-monospace, Menlo, monospace;
        font-size: 1.25em;
        letter-spacing: 0.08em;
        text-align: center;
        padding: 1rem;
        border: 1px dashed currentColor;
        border-radius: 8px;
        word-break: break-all;
    }
</style>
