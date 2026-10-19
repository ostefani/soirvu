<script>
    import { onMount } from 'svelte';
    import { listen } from '@tauri-apps/api/event';
    import * as api from '../../api.js';
    import Settings from '../../components/Settings/Settings.svelte';

    let { onBack, onImported = null } = $props();

    let model = $state({
        settings: null,
        timeoutMin: 5,
        stripExif: true,
        clipboardClear: false,
        backupSchedule: 'off',
        error: '',
        curPass: '',
        newPass: '',
        newPass2: '',
        secNote: '',
        newRecoveryKey: '',
        secBusy: false,
        rotated: false,
        rotating: false,
        touchId: null,
        tidBusy: false,
        tidNote: '',
        expPass: '',
        expBusy: false,
        expNote: '',
        impBusy: false,
        impNote: '',
        syncInfo: null,
        syncBusy: false,
        pruneArm: false,
        syncNote: '',
        syncProblems: [],
        syncWarnings: [],
        syncAckAhead: null,
        restampArm: false,
        restampNote: '',
        note: '',
    });

                                                                    
    function secError(e) {
        return e === 'wrong_passphrase' ? 'Wrong current passphrase.' : `Something went wrong (${e}).`;
    }

    async function changePassphrase() {
        model.secNote = '';
        model.newRecoveryKey = '';
        model.secBusy = true;
        try {
            await api.vaultChangePassphrase(model.curPass, model.newPass);
            model.curPass = model.newPass;
            model.newPass = '';
            model.newPass2 = '';
            model.secNote = 'Passphrase changed. The diary stays open — the new passphrase applies at the next unlock.';
        } catch (e) {
            model.secNote = secError(e);
        } finally {
            model.secBusy = false;
        }
    }

    async function rotateMasterKey() {
        model.secNote = '';
        model.secBusy = true;

        const unlisten = await listen('rotation-started', () => {
            model.rotating = true;
        });
        try {
            const key = await api.vaultRotateMasterKey(model.curPass);
            if (key === null) {
                model.secNote = 'Master key unchanged.';
            } else {
                model.newRecoveryKey = key;
                model.rotated = true;
                model.secNote =
                    'Master key changed — and the diary is now locked. Save the new recovery key, then unlock with your passphrase (it did not change).';
            }
        } catch (e) {
            model.secNote = secError(e);
        } finally {
            unlisten();
            model.secBusy = false;
            model.rotating = false;
        }
    }

            
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
        model.expNote = '';
        model.expBusy = true;
        try {
            const outcome = which === 'json' ? await api.exportJson(model.expPass) : await api.exportMarkdown(model.expPass);
            if (outcome) {
                model.expPass = '';
                model.expNote =
                    'Exported ' +
                    outcome.entries +
                    ' entries and ' +
                    outcome.attachments +
                    ' attachments to ' +
                    outcome.target +
                    '. These files are NOT encrypted.';
            }
        } catch (e) {
            model.expNote = expError(e);
        } finally {
            model.expBusy = false;
        }
    }

        
                                    
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
        model.syncBusy = true;
        try {
            model.syncInfo = await api.syncDismissAckAhead();
        } catch (e) {
            model.syncProblems = [syncError(e)];
        } finally {
            model.syncBusy = false;
        }
    }

    async function loadSync() {
        try {
            model.syncInfo = await api.syncOverview();
        } catch {
            model.syncInfo = null;
        }
    }

    async function pickSync() {
        model.error = '';
        try {
            const updated = await api.syncPickFolder();
            if (updated) {
                model.settings = updated;
                await loadSync();
            }
        } catch (e) {
            model.error = `Something went wrong (${e}).`;
        }
    }

    async function clearSync() {
        model.error = '';
        try {
            model.settings = await api.syncClearFolder();
            model.pruneArm = false;
            model.syncInfo = null;
            model.syncNote = '';
            model.syncProblems = [];
            model.syncWarnings = [];
            model.syncAckAhead = null;
        } catch (e) {
            model.error = `Something went wrong (${e}).`;
        }
    }

    async function runSyncReemit() {
        model.syncNote = '';
        model.syncProblems = [];
        model.syncWarnings = [];
        model.syncAckAhead = null;
        model.syncBusy = true;
        try {
            const bundle = await api.syncReemit();
            model.syncNote = bundle
                ? 'Everything is emitted as ' +
                  bundle.file_name +
                  ' — peers that lost bundles to a prune recover them on their next apply.'
                : 'Nothing to re-emit.';
            await loadSync();
        } catch (e) {
            model.syncNote = syncError(e);
        } finally {
            model.syncBusy = false;
        }
    }

    async function runSyncEmit() {
        model.syncNote = '';
        model.syncProblems = [];
        model.syncWarnings = [];
        model.syncAckAhead = null;
        model.syncBusy = true;
        try {
            const bundle = await api.syncEmit();
            model.syncNote = bundle
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
            model.syncNote = syncError(e);
        } finally {
            model.syncBusy = false;
        }
    }

    async function runSyncApply() {
        model.syncNote = '';
        model.syncProblems = [];
        model.syncWarnings = [];
        model.syncAckAhead = null;
        model.syncBusy = true;
        try {
            const report = await api.syncApply();
            model.syncNote =
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
            model.syncProblems = report.problems;
            model.syncWarnings = report.warnings;
            model.syncAckAhead = report.ack_ahead;
            if (report.entries_applied > 0) {
                onImported?.();
            }
            await loadSync();
        } catch (e) {
            model.syncNote = syncError(e);
        } finally {
            model.syncBusy = false;
        }
    }

    async function runSyncPrune() {
        model.syncNote = '';
        model.syncProblems = [];
        model.syncWarnings = [];
        model.syncAckAhead = null;
        model.syncBusy = true;
        try {
            const report = await api.syncPrune();
            model.syncNote =
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
            model.syncNote = syncError(e);
        } finally {
            model.pruneArm = false;
            model.syncBusy = false;
        }
    }

    async function doRestamp() {
        model.restampNote = '';
        model.syncBusy = true;
        try {
            const id = await api.restampDevice();
            model.restampArm = false;
            model.restampNote =
                'This copy now syncs as a new device (' +
                id.slice(-8) +
                '…). Its changes will be emitted again under the new identity.';
            await loadSync();
        } catch (e) {
            model.restampNote = syncError(e);
        } finally {
            model.syncBusy = false;
        }
    }

    async function runImport(which) {
        model.impNote = '';
        model.impBusy = true;
        try {
            const outcome = which === 'json' ? await api.importJson() : await api.importMarkdown();
            if (outcome) {
                model.impNote = 'Imported ' + outcome.entries + ' entries and ' + outcome.attachments + ' attachments.';
                if (outcome.skippedEntries > 0 || outcome.skippedAttachments > 0) {
                    model.impNote +=
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
            model.impNote = String(e) === 'locked' ? 'The diary is locked.' : 'Something went wrong (' + String(e) + ').';
        } finally {
            model.impBusy = false;
        }
    }

    async function refreshTouchId() {
        try {
            model.touchId = await api.touchIdStatus(null);
        } catch {
            model.touchId = null;
        }
    }

    async function enableTouchId() {
        model.tidNote = '';
        model.tidBusy = true;
        try {
            await api.touchIdEnroll();
            await refreshTouchId();
            model.tidNote = 'Touch ID is on for this diary.';
        } catch (e) {
            model.tidNote =
                e === 'touch_id_unavailable'
                    ? 'Touch ID is not available in this build.'
                    : `Something went wrong (${e}).`;
        } finally {
            model.tidBusy = false;
        }
    }

    async function disableTouchId() {
        model.tidNote = '';
        model.tidBusy = true;
        try {
            await api.touchIdDisable();
            await refreshTouchId();
            model.tidNote = 'Touch ID is off for this diary.';
        } catch (e) {
            model.tidNote = `Something went wrong (${e}).`;
        } finally {
            model.tidBusy = false;
        }
    }

    async function regenerateRecovery() {
        model.secNote = '';
        model.secBusy = true;
        try {
            model.newRecoveryKey = await api.vaultRegenerateRecovery(model.curPass);
            model.curPass = '';
        } catch (e) {
            model.secNote = secError(e);
            model.newRecoveryKey = '';
        } finally {
            model.secBusy = false;
        }
    }
    
    onMount(async () => {
        try {
            model.settings = await api.settingsGet();
            model.timeoutMin = Math.max(1, Math.round(model.settings.lock_timeout_secs / 60));
            model.stripExif = model.settings.strip_exif;
            model.clipboardClear = model.settings.clipboard_clear;
            model.backupSchedule = model.settings.backup_schedule;
            refreshTouchId();
            if (model.settings.sync_location) {
                loadSync();
            }
        } catch (e) {
            model.error = `Something went wrong (${e}).`;
        }
    });

    async function saveTimeout() {
        model.error = '';
        model.note = '';
        try {
            const next = { ...model.settings, lock_timeout_secs: Math.max(1, Math.round(model.timeoutMin)) * 60 };
            await api.settingsSet(next);
            model.settings = await api.settingsGet();
            model.note = 'Saved.';
        } catch (e) {
            model.error = `Something went wrong (${e}).`;
        }
    }

    async function saveStrip() {
        model.error = '';
        model.note = '';
        try {
            await api.settingsSet({ ...model.settings, strip_exif: model.stripExif });
            model.settings = await api.settingsGet();
            model.note = 'Saved.';
        } catch (e) {
            model.error = `Something went wrong (${e}).`;
        }
    }

    async function saveClipboard() {
        model.error = '';
        model.note = '';
        try {
            await api.settingsSet({ ...model.settings, clipboard_clear: model.clipboardClear });
            model.settings = await api.settingsGet();
            model.note = 'Saved.';
        } catch (e) {
            model.error = `Something went wrong (${e}).`;
        }
    }

    async function saveSchedule() {
        model.error = '';
        model.note = '';
        try {
            await api.settingsSet({ ...model.settings, backup_schedule: model.backupSchedule });
            model.settings = await api.settingsGet();
            model.note = 'Saved.';
        } catch (e) {
            model.error = `Something went wrong (${e}).`;
        }
    }

    async function backUpNow() {
        model.error = '';
        model.note = '';
        try {
            const name = await api.backupNow();
            model.note = `Backed up to ${name}.`;
        } catch (e) {
            model.error = e === 'backup_unavailable' ? 'The backup folder is not available.' : `Something went wrong (${e}).`;
        }
    }

    async function pickBackup() {
        model.error = '';
        model.note = '';
        try {
            const updated = await api.settingsPickBackupDir();
            if (updated) model.settings = updated;
        } catch (e) {
            model.error = `Something went wrong (${e}).`;
        }
    }

    async function clearBackup() {
        model.error = '';
        model.note = '';
        try {
            model.settings = await api.settingsClearBackupDir();
        } catch (e) {
            model.error = `Something went wrong (${e}).`;
        }
    }

    const fields = {
        timeout: { name: 'timeoutMin', label: 'Lock after (minutes of inactivity)', type: 'number', min: 1 },
        security: [
            { name: 'curPass', label: 'Current passphrase', type: 'password' },
            { name: 'newPass', label: 'New passphrase', type: 'password' },
            { name: 'newPass2', label: 'Confirm new passphrase', type: 'password' },
        ],
        exportPass: { name: 'expPass', label: 'Passphrase', type: 'password', autocomplete: 'current-password' },
        schedule: {
            name: 'backupSchedule',
            label: 'Automatic backups',
            options: [
                { value: 'off', label: 'Off' },
                { value: 'daily', label: 'Daily' },
                { value: 'weekly', label: 'Weekly' },
            ],
        },
    };

    const actions = $derived({
        security: [
            {
                name: 'change-passphrase',
                label: 'Change passphrase',
                handler: changePassphrase,
                disabled: model.secBusy || !model.curPass || !model.newPass || model.newPass !== model.newPass2,
            },
            {
                name: 'regenerate-recovery',
                label: 'Regenerate recovery key',
                kind: 'quiet',
                handler: regenerateRecovery,
                disabled: model.secBusy || !model.curPass,
            },
        ],
        rotate: [
            {
                name: 'rotate',
                label: 'Change master key…',
                kind: 'quiet',
                handler: rotateMasterKey,
                disabled: model.secBusy || !model.curPass,
            },
            ...(model.rotated
                ? [{ name: 'rotate-done', label: 'Done — go to the lock screen', handler: () => api.vaultLock() }]
                : []),
        ],
        export: [
            {
                name: 'export-markdown',
                label: 'Export as Markdown…',
                handler: () => runExport('markdown'),
                disabled: model.expBusy || !model.expPass,
            },
            {
                name: 'export-json',
                label: 'Export as JSON…',
                handler: () => runExport('json'),
                disabled: model.expBusy || !model.expPass,
            },
        ],
        import: [
            { name: 'import-markdown', label: 'Import Markdown folder…', handler: () => runImport('markdown'), disabled: model.impBusy },
            { name: 'import-json', label: 'Import JSON file…', handler: () => runImport('json'), disabled: model.impBusy },
        ],
        syncFolder: [
            { name: 'sync-pick', label: 'Choose sync folder…', handler: pickSync },
            ...(model.settings?.sync_location
                ? [{ name: 'sync-clear', label: 'Clear', kind: 'quiet', handler: clearSync }]
                : []),
        ],
        syncOps: [
            { name: 'sync-emit', label: 'Emit changes', handler: runSyncEmit, disabled: model.syncBusy },
            { name: 'sync-reemit', label: 'Re-emit everything', kind: 'quiet', handler: runSyncReemit, disabled: model.syncBusy },
            { name: 'sync-apply', label: 'Apply', handler: runSyncApply, disabled: model.syncBusy },
            ...(model.pruneArm
                ? [
                      { name: 'sync-prune', label: 'Yes — prune', handler: runSyncPrune, disabled: model.syncBusy },
                      { name: 'sync-prune-cancel', label: 'Cancel', kind: 'quiet', handler: () => (model.pruneArm = false) },
                  ]
                : [{ name: 'sync-prune-arm', label: 'Prune old bundles', kind: 'quiet', handler: () => (model.pruneArm = true) }]),
        ],
        restamp: model.restampArm
            ? [
                  { name: 'restamp', label: 'Yes — give this copy a new identity', handler: doRestamp, disabled: model.syncBusy },
                  { name: 'restamp-cancel', label: 'Cancel', kind: 'quiet', handler: () => (model.restampArm = false) },
              ]
            : [{ name: 'restamp-arm', label: 'Restamp device identity…', kind: 'quiet', handler: () => (model.restampArm = true) }],
        backupFolder: [
            { name: 'backup-pick', label: 'Choose backup folder…', handler: pickBackup },
            ...(model.settings?.backup_location
                ? [{ name: 'backup-clear', label: 'Clear', kind: 'quiet', handler: clearBackup }]
                : []),
        ],
        backupNow: [
            { name: 'backup-now', label: 'Back up now', handler: backUpNow, disabled: !model.settings?.backup_location },
        ],
        timeout: [{ name: 'save-timeout', label: 'Save lock timeout', handler: saveTimeout }],
    });

    const handlers = {
        saveStrip,
        saveClipboard,
        saveSchedule,
        dismissAckAhead,
        touchIdEnable: enableTouchId,
        touchIdDisable: disableTouchId,
    };
</script>
<style src="./SettingsScreenContainer.css"></style>

<div class="settings-screen">
    <Settings bind:model {fields} {actions} {handlers} {onBack} />
</div>
