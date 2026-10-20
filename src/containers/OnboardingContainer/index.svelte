<script>
    import * as api from '@/api.js';
    import Onboarding from '@/components/Onboarding/index.svelte';

    let { onDone, onCancel = null } = $props();

    let model = $state({
        step: 1,
        folderName: '',
        warning: '',
        passphrase: '',
        confirm: '',
        recoveryKey: '',
        saved: false,
        error: '',
        busy: false,
    });

    const FS_WARNINGS = {
        cloud_sync:
            "This folder is in a cloud-synced location. Sync clients and the diary's database files do not mix — a synced copy can be taken mid-write and sync conflicts can corrupt the vault. You can continue, but a local folder is safer.",
        network_fs:
            "This folder is on a network share. The diary's database (SQLite) is unreliable over network filesystems. You can continue, but a local folder is safer.",
    };

    const ERRORS = {
        folder_not_empty:
            'That folder is not empty. A vault needs an empty folder — pick one, or create a new subfolder for it.',
        empty_passphrase: 'The passphrase must not be empty.',
        no_folder_picked: 'Pick a folder first.',
        storage: 'The vault could not be written there. Is the folder writable?',
    };

    async function pick() {
        model.error = '';
        const picked = await api.vaultPickFolder('create');
        if (picked) {
            model.folderName = picked.name;
            model.warning = picked.warning ? FS_WARNINGS[picked.warning] || '' : '';
            model.step = 2;
        }
    }

    async function create() {
        model.error = '';
        if (model.passphrase.length < 8) {
            model.error =
                'Use at least 8 characters — longer is better. This passphrase is the only protection the diary has.';
            return;
        }
        if (model.passphrase !== model.confirm) {
            model.error = 'The two passphrases do not match.';
            return;
        }
        model.busy = true;
        try {
            const created = await api.vaultCreate(model.passphrase);
            model.recoveryKey = created.recovery_key;
            model.passphrase = '';
            model.confirm = '';
            model.step = 3;
        } catch (e) {
            model.error = ERRORS[e] || `Something went wrong (${e}).`;
        } finally {
            model.busy = false;
        }
    }

    const fields = [
        { name: 'passphrase', label: 'Passphrase', type: 'password', autocomplete: 'new-password' },
        { name: 'confirm', label: 'Repeat it', type: 'password', autocomplete: 'new-password' },
    ];

    const actions = $derived({
        pick: [{ name: 'pick', label: 'Choose an empty folder…', handler: pick }],
        create: [{ name: 'create', label: 'Create the diary', handler: create, disabled: model.busy }],
        done: [{ name: 'done', label: 'Open my diary', handler: () => onDone(), disabled: !model.saved }],
        cancel: onCancel ? [{ name: 'cancel', label: 'Back', kind: 'quiet', handler: () => onCancel() }] : [],
    });
</script>

<Onboarding bind:model {fields} {actions} />
