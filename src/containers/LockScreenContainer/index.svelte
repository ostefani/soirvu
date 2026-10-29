<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import * as api from '@/api.js';
    import LockScreen from '@/components/LockScreen/index.svelte';

    import {
        createModel,
        AUTHENTICATION_KIND_OPTIONS,
        FORK_SECRET_FIELD_CONFIGURATION,
        VAULT_ERROR_MESSAGES,
        FILESYSTEM_WARNING_MESSAGES,
    } from './constants.js';
    import { lockScreenText } from './data.js';

    let { vaults, onUnlocked, onVaultsChanged, onCreate } = $props();

    let model = $state(createModel());

    const selected = $derived(vaults.find((vault) => vault.id === (model.selectedId ?? vaults[0]?.id)) ?? null);

    // -------------------------------------------------------------------------
    // Error description formatter
    // -------------------------------------------------------------------------
    function describeError(error) {
        if (typeof error === 'string' && error.startsWith(lockScreenText.backoffPrefix)) {
            const seconds = error.slice(lockScreenText.backoffPrefix.length);
            return lockScreenText.backoffMessage(seconds);
        }
        return VAULT_ERROR_MESSAGES[error] ?? lockScreenText.unknownError(error);
    }

    // -------------------------------------------------------------------------
    // Unlock actions
    // -------------------------------------------------------------------------
    async function unlock() {
        if (!selected || !model.secret) return;
        model.busy = true;
        model.error = '';
        try {
            await api.vaultUnlock(selected.id, model.kind, model.secret);
            model.secret = '';
            onUnlocked(selected);
        } catch (error) {
            model.error = describeError(error);
        } finally {
            model.busy = false;
        }
    }

    $effect(() => {
        const vaultId = selected?.id;
        model.touchId = null;
        if (!vaultId) return;
        api.touchIdStatus(vaultId).then(
            (status) => {
                if (selected?.id === vaultId) {
                    model.touchId = status;
                }
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
        } catch (error) {
            if (error !== 'touch_id_cancelled') {
                model.error = describeError(error);
            }
        } finally {
            model.busy = false;
        }
    }

    // -------------------------------------------------------------------------
    // Vault lifecycle operations
    // -------------------------------------------------------------------------
    async function relocate(vault) {
        model.error = '';
        try {
            if (await api.vaultRelocate(vault.id)) {
                onVaultsChanged();
            }
        } catch (error) {
            model.error = describeError(error);
        }
    }

    async function forget(vault) {
        await api.vaultForget(vault.id);
        model.forgetArmed = null;
        onVaultsChanged();
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
                if (picked.warning) {
                    model.note = FILESYSTEM_WARNING_MESSAGES[picked.warning] ?? '';
                }
            } else if (picked) {
                model.selectedId = picked.vault_id;
                if (picked.warning) {
                    model.note = FILESYSTEM_WARNING_MESSAGES[picked.warning] ?? '';
                }
                onVaultsChanged();
            }
        } catch (error) {
            model.error = describeError(error);
        }
    }

    // -------------------------------------------------------------------------
    // Fork imported diary workflow
    // -------------------------------------------------------------------------
    async function doFork() {
        model.forkError = '';
        model.forkBusy = true;
        try {
            const forked = await api.vaultForkImport(model.forkSecret);
            model.forkSecret = '';
            model.forkNewId = forked.vault_id;
            model.forkKey = forked.recovery_key;
            onVaultsChanged();
        } catch (error) {
            model.forkError = describeError(error);
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

    // -------------------------------------------------------------------------
    // Derived UI configurations
    // -------------------------------------------------------------------------
    const secretField = $derived({
        name: 'secret',
        type: 'password',
        placeholder:
            model.kind === 'passphrase' ? lockScreenText.passphrasePlaceholder : lockScreenText.recoveryPlaceholder,
    });

    const kindOptions = AUTHENTICATION_KIND_OPTIONS.map((option) => ({
        ...option,
        handler: () => (model.kind = option.name),
    }));

    const unlockActions = $derived([
        {
            name: 'unlock',
            label: lockScreenText.unlockButton,
            handler: unlock,
            disabled: model.busy || !model.secret,
        },
        ...(model.touchId?.capable && model.touchId?.enrolled
            ? [
                  {
                      name: 'touch-id',
                      label: lockScreenText.touchIdButton,
                      handler: unlockTouchId,
                      disabled: model.busy,
                  },
              ]
            : []),
    ]);

    const forkActions = $derived([
        {
            name: 'fork',
            label: lockScreenText.forkButton,
            handler: doFork,
            disabled: model.forkBusy || !model.forkSecret,
        },
        {
            name: 'fork-cancel',
            label: lockScreenText.forkCancelButton,
            kind: 'quiet',
            handler: () => (model.forkDest = null),
        },
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
        setSecret: (value) => (model.secret = value),
        setForkSecret: (value) => (model.forkSecret = value),
    };
</script>

<LockScreen
    {vaults}
    {selected}
    bind:model
    {secretField}
    forkField={FORK_SECRET_FIELD_CONFIGURATION}
    {kindOptions}
    {unlockActions}
    {forkActions}
    {handlers}
    textData={lockScreenText} />
