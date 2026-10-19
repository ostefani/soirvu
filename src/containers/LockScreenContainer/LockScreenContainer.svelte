<script>
    import * as api from '../../api.js';
    import LockScreen from '../../components/LockScreen/LockScreen.svelte';

    let { vaults, onUnlocked, onVaultsChanged, onCreate } = $props();

    let model = $state({
        selectedId: null,
        kind: 'passphrase',
        secret: '',
        error: '',
        busy: false,
        forgetArmed: null,
        forkDest: null,
        forkSecret: '',
        forkKey: null,
        forkNewId: null,
        forkError: '',
        forkBusy: false,
        note: '',
        touchId: null,
    });

    const FS_WARNINGS = {
        cloud_sync:
            "That folder is in a cloud-synced location. Sync clients and the diary's database files do not mix — the vault is registered, but keeping it there risks corruption.",
        network_fs:
            "That folder is on a network share. The diary's database is unreliable over network filesystems — the vault is registered, but keeping it there is risky.",
    };

    const selected = $derived(vaults.find((v) => v.id === (model.selectedId ?? vaults[0]?.id)) ?? null);

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
        if (!selected || !model.secret) return;
        model.busy = true;
        model.error = '';
        try {
            await api.vaultUnlock(selected.id, model.kind, model.secret);
            model.secret = '';
            onUnlocked(selected);
        } catch (e) {
            model.error = describeError(e);
        } finally {
            model.busy = false;
        }
    }

    $effect(() => {
        const id = selected?.id;
        model.touchId = null;
        if (!id) return;
        api.touchIdStatus(id).then(
            (status) => {
                if (selected?.id === id) model.touchId = status;
            },
            () => {},
        );
    });

    async function unlockTouchId() {
        if (!selected) return;
        model.busy = true;
        model.error = '';
        try {
            await api.vaultUnlockTouchId(selected.id);
            onUnlocked(selected);
        } catch (e) {
            if (e !== 'touch_id_cancelled') model.error = describeError(e);
        } finally {
            model.busy = false;
        }
    }

    async function relocate(vault) {
        model.error = '';
        try {
            if (await api.vaultRelocate(vault.id)) onVaultsChanged();
        } catch (e) {
            model.error = describeError(e);
        }
    }

    async function forget(vault) {
        await api.vaultForget(vault.id);
        model.forgetArmed = null;
        onVaultsChanged();
    }

    async function doFork() {
        model.forkError = '';
        model.forkBusy = true;
        try {
            const forked = await api.vaultForkImport(model.forkSecret);
            model.forkSecret = '';
            model.forkNewId = forked.vault_id;
            model.forkKey = forked.recovery_key;
            onVaultsChanged();
        } catch (e) {
            model.forkError = describeError(e);
        } finally {
            model.forkBusy = false;
        }
    }

    function finishFork() {
        model.selectedId = model.forkNewId;
        model.forkKey = null;
        model.forkDest = null;
        model.forkNewId = null;
    }

    async function openAnother() {
        model.error = '';
        model.note = '';
        try {
            const picked = await api.vaultPickFolder('open');
            if (picked && picked.fork_required) {
                model.forkDest = picked.name;
                model.forkSecret = '';
                model.forkError = '';
                if (picked.warning) model.note = FS_WARNINGS[picked.warning] || '';
            } else if (picked) {
                model.selectedId = picked.vault_id;
                if (picked.warning) model.note = FS_WARNINGS[picked.warning] || '';
                onVaultsChanged();
            }
        } catch (e) {
            model.error = describeError(e);
        }
    }

    const secretField = $derived({
        name: 'secret',
        type: 'password',
        placeholder: model.kind === 'passphrase' ? 'Passphrase' : 'XXXX-XXXX-…',
    });

    const forkField = { name: 'forkSecret', type: 'password', placeholder: "The diary's passphrase" };

    const kindOptions = [
        { name: 'passphrase', label: 'Passphrase', handler: () => (model.kind = 'passphrase') },
        { name: 'recovery', label: 'Recovery key', handler: () => (model.kind = 'recovery') },
    ];

    const unlockActions = $derived([
        { name: 'unlock', label: 'Unlock', handler: unlock, disabled: model.busy || !model.secret },
        ...(model.touchId?.capable && model.touchId?.enrolled
            ? [{ name: 'touch-id', label: 'Unlock with Touch ID', handler: unlockTouchId, disabled: model.busy }]
            : []),
    ]);

    const forkActions = $derived([
        { name: 'fork', label: 'Make it a separate diary', handler: doFork, disabled: model.forkBusy || !model.forkSecret },
        { name: 'fork-cancel', label: 'Not now', kind: 'quiet', handler: () => (model.forkDest = null) },
    ]);

    const handlers = {
        select: (vault) => (model.selectedId = vault.id),
        refresh: () => onVaultsChanged(),
        relocate,
        forget,
        armForget: (vault) => (model.forgetArmed = vault.id),
        unlock,
        doFork,
        finishFork,
        openAnother,
        create: () => onCreate(),
    };
</script>

<LockScreen {vaults} {selected} bind:model {secretField} {forkField} {kindOptions} {unlockActions} {forkActions} {handlers} />
