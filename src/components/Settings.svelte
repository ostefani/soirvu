<script>
    // App settings (Phase 3): the lock timeout and the backup
    // location. The backup folder is picked through Rust — the
    // UI only ever sees its display name, never the path (§3).
    import { onMount } from 'svelte';
    import { listen } from '@tauri-apps/api/event';
    import * as api from '../api.js';

    let { onBack, onImported } = $props();

    let settings = $state(null);
    let timeoutMin = $state(5);
    let stripExif = $state(true);
    let clipboardClear = $state(false);
    let backupSchedule = $state('off');
    let error = $state('');
    let curPass = $state('');
    let newPass = $state('');
    let newPass2 = $state('');
    let secNote = $state('');
    let newRecoveryKey = $state('');
    let secBusy = $state(false);
    let rotated = $state(false);
    let rotating = $state(false);
    let touchId = $state(null);
    let tidBusy = $state(false);
    let tidNote = $state('');

    function secError(e) {
        return e === 'wrong_passphrase' ? 'Wrong current passphrase.' : `Something went wrong (${e}).`;
    }

    async function changePassphrase() {
        secNote = '';
        newRecoveryKey = '';
        secBusy = true;
        try {
            await api.vaultChangePassphrase(curPass, newPass);
            curPass = newPass;
            newPass = '';
            newPass2 = '';
            secNote = 'Passphrase changed. The diary stays open — the new passphrase applies at the next unlock.';
        } catch (e) {
            secNote = secError(e);
        } finally {
            secBusy = false;
        }
    }

    async function rotateMasterKey() {
        secNote = '';
        secBusy = true;
        // The spinner starts on the bridge's rotation-started
        // event — after the native confirmation, when the work
        // actually begins — not on this click.
        const unlisten = await listen('rotation-started', () => {
            rotating = true;
        });
        try {
            const key = await api.vaultRotateMasterKey(curPass);
            if (key === null) {
                secNote = 'Master key unchanged.';
            } else {
                newRecoveryKey = key;
                rotated = true;
                secNote =
                    'Master key changed — and the diary is now locked. Save the new recovery key, then unlock with your passphrase (it did not change).';
            }
        } catch (e) {
            secNote = secError(e);
        } finally {
            unlisten();
            secBusy = false;
            rotating = false;
        }
    }

    let expPass = $state('');
    let expBusy = $state(false);
    let expNote = $state('');

    function expError(e) {
        switch (String(e)) {
            case 'wrong_passphrase':
                return 'Wrong passphrase.';
            case 'folder_not_empty':
                return 'That folder is not empty — pick an empty folder for the export.';
            case 'export_target_exists':
                return 'That folder already contains soirvu-export.json — pick another folder.';
            case 'locked':
                return 'The diary is locked.';
            default:
                return 'Something went wrong (' + String(e) + ').';
        }
    }

    async function runExport(which) {
        expNote = '';
        expBusy = true;
        try {
            const outcome = which === 'json' ? await api.exportJson(expPass) : await api.exportMarkdown(expPass);
            if (outcome) {
                expPass = '';
                expNote =
                    'Exported ' +
                    outcome.entries +
                    ' entries and ' +
                    outcome.attachments +
                    ' attachments to ' +
                    outcome.target +
                    '. These files are NOT encrypted.';
            }
        } catch (e) {
            expNote = expError(e);
        } finally {
            expBusy = false;
        }
    }

    let impBusy = $state(false);
    let impNote = $state('');

    let syncInfo = $state(null);
    let syncBusy = $state(false);
    let pruneArm = $state(false);
    let syncNote = $state('');
    let syncProblems = $state([]);
    let syncWarnings = $state([]);
    let syncAckAhead = $state(null);
    let restampArm = $state(false);
    let restampNote = $state('');

    function syncError(e) {
        switch (String(e)) {
            case 'sync_shared_identity':
                return 'Another copy of this diary is using this device identity — restamp this copy below to fix it.';
            case 'sync_unreadable':
                return 'A file in the sync folder sits at a bundle name and cannot be read — remove it there and try again.';
            case 'sync_folder_missing':
                return 'The sync folder is not there — is the drive mounted?';
            case 'sync_not_configured':
                return 'Choose a sync folder first.';
            case 'locked':
                return 'The diary is locked.';
            case 'sync':
                return 'Sync failed.';
            default:
                return 'Something went wrong (' + String(e) + ').';
        }
    }

    async function dismissAckAhead() {
        syncBusy = true;
        try {
            syncInfo = await api.syncDismissAckAhead();
        } catch (e) {
            syncProblems = [syncError(e)];
        } finally {
            syncBusy = false;
        }
    }

    async function loadSync() {
        try {
            syncInfo = await api.syncOverview();
        } catch {
            syncInfo = null;
        }
    }

    async function pickSync() {
        error = '';
        try {
            const updated = await api.syncPickFolder();
            if (updated) {
                settings = updated;
                await loadSync();
            }
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    }

    async function clearSync() {
        error = '';
        try {
            settings = await api.syncClearFolder();
            pruneArm = false;
            syncInfo = null;
            syncNote = '';
            syncProblems = [];
            syncWarnings = [];
            syncAckAhead = null;
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    }

    async function runSyncReemit() {
        syncNote = '';
        syncProblems = [];
        syncWarnings = [];
        syncAckAhead = null;
        syncBusy = true;
        try {
            const bundle = await api.syncReemit();
            syncNote = bundle
                ? 'Everything is emitted as ' + bundle.file_name + ' — peers that lost bundles to a prune recover them on their next apply.'
                : 'Nothing to re-emit.';
            await loadSync();
        } catch (e) {
            syncNote = syncError(e);
        } finally {
            syncBusy = false;
        }
    }

    async function runSyncEmit() {
        syncNote = '';
        syncProblems = [];
        syncWarnings = [];
        syncAckAhead = null;
        syncBusy = true;
        try {
            const bundle = await api.syncEmit();
            syncNote = bundle
                ? 'Wrote ' +
                  bundle.file_name +
                  ' — ' +
                  bundle.entries +
                  ' entries, ' +
                  bundle.attachments +
                  ' attachments.'
                : 'Nothing new to emit.';
            await loadSync();
        } catch (e) {
            syncNote = syncError(e);
        } finally {
            syncBusy = false;
        }
    }

    async function runSyncApply() {
        syncNote = '';
        syncProblems = [];
        syncWarnings = [];
        syncAckAhead = null;
        syncBusy = true;
        try {
            const report = await api.syncApply();
            syncNote =
                'Applied ' +
                report.bundles_applied +
                ' of ' +
                report.bundles_seen +
                ' bundles: ' +
                report.entries_applied +
                ' entries, ' +
                report.entries_conflicted +
                ' conflict copies, ' +
                report.attachments_applied +
                ' attachments.';
            syncProblems = report.problems;
            syncWarnings = report.warnings;
            syncAckAhead = report.ack_ahead;
            if (report.entries_applied > 0) {
                onImported?.();
            }
            await loadSync();
        } catch (e) {
            syncNote = syncError(e);
        } finally {
            syncBusy = false;
        }
    }

    async function runSyncPrune() {
        syncNote = '';
        syncProblems = [];
        syncWarnings = [];
        syncAckAhead = null;
        syncBusy = true;
        try {
            const report = await api.syncPrune();
            syncNote =
                report.covered_through > 0
                    ? 'Removed ' +
                      report.bundles_removed +
                      ' bundles and ' +
                      report.blobs_removed +
                      ' blobs — every peer has consumed this device through ' +
                      report.covered_through +
                      '.'
                    : 'Nothing pruned — no peer has acknowledged this device yet.';
            await loadSync();
        } catch (e) {
            syncNote = syncError(e);
        } finally {
            pruneArm = false;
            syncBusy = false;
        }
    }

    async function doRestamp() {
        restampNote = '';
        syncBusy = true;
        try {
            const id = await api.restampDevice();
            restampArm = false;
            restampNote =
                'This copy now syncs as a new device (' +
                id.slice(0, 8) +
                '…). Its changes will be emitted again under the new identity.';
            await loadSync();
        } catch (e) {
            restampNote = syncError(e);
        } finally {
            syncBusy = false;
        }
    }

    async function runImport(which) {
        impNote = '';
        impBusy = true;
        try {
            const outcome = which === 'json' ? await api.importJson() : await api.importMarkdown();
            if (outcome) {
                impNote = 'Imported ' + outcome.entries + ' entries and ' + outcome.attachments + ' attachments.';
                if (outcome.skippedEntries > 0 || outcome.skippedAttachments > 0) {
                    impNote +=
                        ' Skipped ' +
                        outcome.skippedEntries +
                        ' entries and ' +
                        outcome.skippedAttachments +
                        ' attachments (already present or unreadable).';
                }
                if (outcome.entries > 0) {
                    onImported?.();
                }
            }
        } catch (e) {
            impNote = String(e) === 'locked' ? 'The diary is locked.' : 'Something went wrong (' + String(e) + ').';
        } finally {
            impBusy = false;
        }
    }

    async function refreshTouchId() {
        try {
            touchId = await api.touchIdStatus(null);
        } catch {
            touchId = null;
        }
    }

    async function enableTouchId() {
        tidNote = '';
        tidBusy = true;
        try {
            await api.touchIdEnroll();
            await refreshTouchId();
            tidNote = 'Touch ID is on for this diary.';
        } catch (e) {
            tidNote =
                e === 'touch_id_unavailable'
                    ? 'Touch ID is not available in this build.'
                    : `Something went wrong (${e}).`;
        } finally {
            tidBusy = false;
        }
    }

    async function disableTouchId() {
        tidNote = '';
        tidBusy = true;
        try {
            await api.touchIdDisable();
            await refreshTouchId();
            tidNote = 'Touch ID is off for this diary.';
        } catch (e) {
            tidNote = `Something went wrong (${e}).`;
        } finally {
            tidBusy = false;
        }
    }

    async function regenerateRecovery() {
        secNote = '';
        secBusy = true;
        try {
            newRecoveryKey = await api.vaultRegenerateRecovery(curPass);
            curPass = '';
        } catch (e) {
            secNote = secError(e);
            newRecoveryKey = '';
        } finally {
            secBusy = false;
        }
    }
    let note = $state('');

    onMount(async () => {
        try {
            settings = await api.settingsGet();
            timeoutMin = Math.max(1, Math.round(settings.lock_timeout_secs / 60));
            stripExif = settings.strip_exif;
            clipboardClear = settings.clipboard_clear;
            backupSchedule = settings.backup_schedule;
            refreshTouchId();
            if (settings.sync_location) {
                loadSync();
            }
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    });

    async function saveTimeout() {
        error = '';
        note = '';
        try {
            const next = { ...settings, lock_timeout_secs: Math.max(1, Math.round(timeoutMin)) * 60 };
            await api.settingsSet(next);
            settings = await api.settingsGet();
            note = 'Saved.';
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    }

    async function saveStrip() {
        error = '';
        note = '';
        try {
            await api.settingsSet({ ...settings, strip_exif: stripExif });
            settings = await api.settingsGet();
            note = 'Saved.';
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    }

    async function saveClipboard() {
        error = '';
        note = '';
        try {
            await api.settingsSet({ ...settings, clipboard_clear: clipboardClear });
            settings = await api.settingsGet();
            note = 'Saved.';
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    }

    async function saveSchedule() {
        error = '';
        note = '';
        try {
            await api.settingsSet({ ...settings, backup_schedule: backupSchedule });
            settings = await api.settingsGet();
            note = 'Saved.';
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    }

    async function backUpNow() {
        error = '';
        note = '';
        try {
            const name = await api.backupNow();
            note = `Backed up to ${name}.`;
        } catch (e) {
            error = e === 'backup_unavailable' ? 'The backup folder is not available.' : `Something went wrong (${e}).`;
        }
    }

    async function pickBackup() {
        error = '';
        note = '';
        try {
            const updated = await api.settingsPickBackupDir();
            if (updated) settings = updated;
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    }

    async function clearBackup() {
        error = '';
        note = '';
        try {
            settings = await api.settingsClearBackupDir();
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    }
</script>

<div class="settings">
    <h2>Settings</h2>

    {#if settings}
        <p>
            This diary lives in:
            <strong>{settings.vault_location ?? '—'}</strong>
        </p>

        <label>
            Lock after (minutes of inactivity)
            <input type="number" min="1" bind:value={timeoutMin} />
        </label>
        <button onclick={saveTimeout}>Save lock timeout</button>

        <label>
            Strip location metadata (EXIF) from photos on import
            <input type="checkbox" bind:checked={stripExif} onchange={saveStrip} />
        </label>

        {#if settings?.platform === 'macos'}
            <label>
                Clear the clipboard 30 seconds after copying from the diary
                <input type="checkbox" bind:checked={clipboardClear} onchange={saveClipboard} />
            </label>
        {/if}

        <p>
            Backup location:
            {#if settings.backup_location}
                <strong>{settings.backup_location}</strong>
            {:else}
                <span class="note">not set</span>
            {/if}
        </p>
        <div class="row">
            <button onclick={pickBackup}>Choose backup folder…</button>
            {#if settings.backup_location}
                <button class="quiet" onclick={clearBackup}>Clear</button>
            {/if}
        </div>

        <label>
            Automatic backups
            <select bind:value={backupSchedule} onchange={saveSchedule}>
                <option value="off">Off</option>
                <option value="daily">Daily</option>
                <option value="weekly">Weekly</option>
            </select>
        </label>

        <div class="row">
            <button onclick={backUpNow} disabled={!settings.backup_location}>Back up now</button>
        </div>
        <p class="note">
            A backup is an encrypted copy of this vault (the newest 7 are kept). To restore one, choose its folder from
            the lock screen.
        </p>
        <h3>Passphrase and recovery key</h3>

        <label>
            Current passphrase
            <input type="password" bind:value={curPass} />
        </label>

        <label>
            New passphrase
            <input type="password" bind:value={newPass} />
        </label>

        <label>
            Confirm new passphrase
            <input type="password" bind:value={newPass2} />
        </label>

        <div class="row">
            <button onclick={changePassphrase} disabled={secBusy || !curPass || !newPass || newPass !== newPass2}>
                Change passphrase
            </button>
            <button class="quiet" onclick={regenerateRecovery} disabled={secBusy || !curPass}>
                Regenerate recovery key
            </button>
        </div>

        <div class="row">
            <button class="quiet" onclick={rotateMasterKey} disabled={secBusy || !curPass}> Change master key… </button>
            {#if rotated}
                <button onclick={() => api.vaultLock()}>Done — go to the lock screen</button>
            {/if}
        </div>

        {#if rotating}
            <p class="note progress">
                <span class="spinner"></span>
                <span>
                    Changing the master key… a fresh copy of the diary is being built and checked. This can take a while
                    for a large diary — don't close the app.
                </span>
            </p>
        {/if}

        {#if newRecoveryKey}
            <p>Your new recovery key: <code>{newRecoveryKey}</code></p>
            <p class="note">Save it now — it is shown once, and the old key no longer works.</p>
        {/if}
        {#if secNote}<p class="note">{secNote}</p>{/if}

        <p class="note">
            Only the diary's header changes — nothing is re-encrypted and the diary stays open. Backups made earlier
            keep the passphrase and recovery key they were made with.
        </p>
        {#if touchId?.capable || touchId?.enrolled}<h3>Touch ID</h3>{/if}
        {#if touchId?.enrolled && !touchId.capable}
            <p class="note">
                This diary has Touch ID enrolled, but this build cannot use it — it
                {touchId.built_in ? 'is not signed with the keychain profile' : 'was made without Touch ID'}. The
                passphrase and recovery key work as always. You can remove the enrollment here; removing it needs no
                keychain, only the diary's header changes.
            </p>
            <div class="row">
                <button class="quiet" onclick={disableTouchId} disabled={tidBusy}> Remove Touch ID enrollment </button>
            </div>
        {:else if touchId && !touchId.capable && touchId.built_in}
            <p class="note">
                Touch ID needs this app signed with a development profile that carries the keychain entitlement. This
                build is not signed that way, so Touch ID is unavailable — the passphrase and recovery key work as
                always.
            </p>
        {:else if touchId?.enrolled}
            <p class="note">This diary can be unlocked with Touch ID on this Mac.</p>
            <div class="row">
                <button class="quiet" onclick={disableTouchId} disabled={tidBusy}> Turn off Touch ID </button>
            </div>
        {:else if touchId?.capable}
            <p class="note">
                Unlock this diary with your fingerprint instead of typing the passphrase. The passphrase and recovery
                key keep working — Touch ID is one more key, never the only one.
            </p>
            <div class="row">
                <button onclick={enableTouchId} disabled={tidBusy}>Turn on Touch ID</button>
            </div>
        {/if}
        {#if tidNote}<p class="note">{tidNote}</p>{/if}

        <h3>Export</h3>
        <p class="note">
            Export writes the diary as plain, unencrypted files. You will be asked to confirm, to type your passphrase
            again, and to pick an empty folder.
        </p>
        <label>
            Passphrase
            <input type="password" bind:value={expPass} autocomplete="current-password" />
        </label>
        <div class="row">
            <button onclick={() => runExport('markdown')} disabled={expBusy || !expPass}> Export as Markdown… </button>
            <button onclick={() => runExport('json')} disabled={expBusy || !expPass}> Export as JSON… </button>
        </div>
        <p class="note">
            Markdown is for reading and moving to other apps. JSON keeps everything — ids, deleted entries — and is made
            for importing back into Soirvu.
        </p>
        {#if expNote}<p class="note">{expNote}</p>{/if}

        <h3>Import</h3>
        <p class="note">
            Import adds entries to this diary — nothing is overwritten or merged. A Markdown folder can be a Soirvu
            export or notes from another app. A JSON file must be a Soirvu export; entries whose id already exists are
            skipped.
        </p>
        <div class="row">
            <button onclick={() => runImport('markdown')} disabled={impBusy}> Import Markdown folder… </button>
            <button onclick={() => runImport('json')} disabled={impBusy}> Import JSON file… </button>
        </div>
        {#if impNote}<p class="note">{impNote}</p>{/if}

        <h3>Sync</h3>
        <p class="note">
            Sync writes this diary's changes as encrypted bundles into a folder. Another tool — Syncthing, a cloud
            folder, a USB stick — carries the folder between your devices; Soirvu itself never uses the network.
        </p>
        <p>
            Sync folder:
            {#if settings.sync_location}
                <strong>{settings.sync_location}</strong>
            {:else}
                <span class="note">not set</span>
            {/if}
        </p>
        <div class="row">
            <button onclick={pickSync}>Choose sync folder…</button>
            {#if settings.sync_location}
                <button class="quiet" onclick={clearSync}>Clear</button>
            {/if}
        </div>
        {#if settings.sync_location && syncInfo}
            {#if syncInfo.ack_ahead_seen}
                <p class="error">
                    An ack in this folder has claimed more than this vault holds — a copy may share its identity,
                    or this vault was restored. Entries a peer pruned on the strength of it return when that peer
                    re-emits everything. If THIS diary is the copy, restamp it below; if it is the original, the
                    copy is the one to restamp. Once the missing entries are back, dismiss this note.
                    <button class="quiet" onclick={dismissAckAhead} disabled={syncBusy}>Dismiss</button>
                </p>
                {#each syncInfo.peers as peer}
                    {#if peer.needs_reemit}
                        <p class="note">
                            Device <code>{peer.device_id.slice(0, 8)}…</code> needs to re-emit everything, or its earlier bundles are still on their way.
                        </p>
                    {/if}
                {/each}
            {/if}
            <p class="note">This device: <code>{syncInfo.device_id}</code></p>
            {#each syncInfo.peers as peer}
                <p class="note">
                    Peer <code>{peer.device_id.slice(0, 8)}…</code>: their changes incorporated through
                    {peer.applied_lseq}{#if peer.waiting > 0}
                        — {peer.waiting} bundle{peer.waiting === 1 ? '' : 's'} waiting{/if}
                </p>
            {/each}
            <div class="row">
                <button onclick={runSyncEmit} disabled={syncBusy}>Emit changes</button>
                <button class="quiet" onclick={runSyncReemit} disabled={syncBusy}>Re-emit everything</button>
                <button onclick={runSyncApply} disabled={syncBusy}>Apply</button>
                {#if pruneArm}
                    <button onclick={runSyncPrune} disabled={syncBusy}>Yes — prune</button>
                    <button class="quiet" onclick={() => (pruneArm = false)}>Cancel</button>
                {:else}
                    <button class="quiet" onclick={() => (pruneArm = true)}>Prune old bundles</button>
                {/if}
            </div>
            {#if pruneArm}
                <p class="note">
                    Prune removes the bundles and blobs every known peer has already consumed. A device that uses this
                    folder but has not synced yet — a fresh copy, for instance — can lose changes it never received.
                    Prune only once every device has synced.
                </p>
            {/if}
        {/if}
        {#if syncNote}<p class="note">{syncNote}</p>{/if}
        {#each syncProblems as problem}
            <p class="error">{problem}</p>
        {/each}
        {#each syncWarnings as warning}
            <p class="warning">{warning}</p>
        {/each}
        {#if syncAckAhead && !syncInfo?.ack_ahead_seen}
            <p class="error">{syncAckAhead}</p>
        {/if}
        <p class="note">
            If this diary is a copy of a folder from another device, both copies share one device identity and cannot
            sync with each other. Restamping gives THIS copy a new identity — the other device is untouched, and this
            copy's changes are emitted again under the new identity.
        </p>
        <div class="row">
            {#if restampArm}
                <button onclick={doRestamp} disabled={syncBusy}>Yes — give this copy a new identity</button>
                <button class="quiet" onclick={() => (restampArm = false)}>Cancel</button>
            {:else}
                <button class="quiet" onclick={() => (restampArm = true)}>Restamp device identity…</button>
            {/if}
        </div>
        {#if restampNote}<p class="note">{restampNote}</p>{/if}
    {:else if !error}
        <p class="note">Loading…</p>
    {/if}

    {#if note}<p class="note">{note}</p>{/if}
    {#if error}<p class="error">{error}</p>{/if}

    <button class="quiet" onclick={() => onBack()}>Back to the diary</button>
</div>

<style>
    .settings {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
        max-width: 26rem;
    }

    .settings label {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        font-size: 0.9em;
    }

    .settings .row {
        display: flex;
        gap: 0.4rem;
    }
    .progress {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .spinner {
        width: 0.9em;
        height: 0.9em;
        flex: none;
        border: 2px solid currentColor;
        border-top-color: transparent;
        border-radius: 50%;
        animation: spin 0.8s linear infinite;
    }

    @keyframes spin {
        to {
            transform: rotate(360deg);
        }
    }
    .warning {
        color: #8a5a00;
    }
</style>
