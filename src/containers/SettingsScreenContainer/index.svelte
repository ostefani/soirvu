<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { onDestroy, onMount } from 'svelte';
    import * as api from '@/api.js';
    import { createSettingsCommit } from '@/lib/settings-commit.js';
    import Button from '@/components/Button/index.svelte';
    import Settings from '@/components/Settings/index.svelte';
    import IconLeft from '@/icons/arrow-left.svelte';
    import {
        createExportModel,
        createGeneralModel,
        createImportModel,
        createSecurityModel,
        createSyncModel,
        createTouchIdModel,
    } from './settingsModel.js';
    import { ACTIONS, settingsText, fields } from './constants.js';
    import { resolveErrorText, unknownErrorText } from '@/lib/errors.js';
    import {
        SETTINGS_BACKUP_ERRORS,
        SETTINGS_EXPORT_ERRORS,
        SETTINGS_GENERAL_ERRORS,
        SETTINGS_SECURITY_ERRORS,
        SETTINGS_SYNC_ERRORS,
    } from './errors.js';
    import { EVENT_TYPE } from '@/constants.js';
    import { notify, clearNotifications } from '@/components/Notification/store.svelte.js';

    let { onBack, onImported = null } = $props();

    // Completions must not repopulate abandoned models,
    // notifications or secret/result fields after disposal. The
    // persisting writes themselves always run to completion; only
    // the local model refresh and feedback are skipped.
    let alive = true;
    onDestroy(() => (alive = false));

    let general = $state(createGeneralModel());
    let security = $state(createSecurityModel());
    let touchId = $state(createTouchIdModel());
    let exp = $state(createExportModel());
    let imp = $state(createImportModel());
    let sync = $state(createSyncModel());

    // Serialized preference writes: every patch merges against the
    // latest acknowledged snapshot, so rapid toggles cannot revert
    // each other. Folder picker/clear results re-enter as one-field
    // patches through the same sequence instead of adopted
    // snapshots, which would have no ordering against queued writes.
    let settingsCommit = null;

    // One notification slot per section: a new notify replaces the previous one in place.
    const toMinutes = (secs) => Math.max(1, Math.round(secs / 60));

    // ---------- errors ----------

    // Area copy lives in ./errors.js tables; unknown codes use the
    // single shared rule instead of leaking the raw IPC code.
    function secError(e) {
        return resolveErrorText(SETTINGS_SECURITY_ERRORS, e).text;
    }

    function expError(e) {
        return resolveErrorText(SETTINGS_EXPORT_ERRORS, e).text;
    }

    function syncError(e) {
        return resolveErrorText(SETTINGS_SYNC_ERRORS, e).text;
    }

    // ---------- security ----------

    async function changePassphrase() {
        security.newRecoveryKey = '';
        security.busy = true;
        try {
            await api.vaultChangePassphrase(security.curPass, security.newPass);
            if (!alive) return;
            security.curPass = security.newPass;
            security.newPass = '';
            security.newPass2 = '';
            notify(
                EVENT_TYPE.UPDATED,
                'Passphrase changed. The diary stays open — the new passphrase applies at the next unlock.',
            );
        } catch (e) {
            if (!alive) return;
            notify(EVENT_TYPE.ERROR, secError(e));
        } finally {
            security.busy = false;
        }
    }

    async function rotateMasterKey() {
        security.busy = true;
        let unlisten = () => {};
        try {
            unlisten = await api.onRotationStarted(() => (security.rotating = true));
            const key = await api.vaultRotateMasterKey(security.curPass);
            if (!alive) return;
            if (key === null) {
                notify(EVENT_TYPE.INFO, 'Master key unchanged.');
            } else {
                security.newRecoveryKey = key;
                security.rotated = true;
                notify(
                    EVENT_TYPE.UPDATED,
                    'Master key changed — and the diary is now locked. Save the new recovery key, then unlock with your passphrase (it did not change).',
                    { duration: 0 },
                );
            }
        } catch (e) {
            if (!alive) return;
            notify(EVENT_TYPE.ERROR, secError(e));
        } finally {
            unlisten();
            security.busy = false;
            security.rotating = false;
        }
    }

    async function regenerateRecovery() {
        security.busy = true;
        try {
            const recovered = await api.vaultRegenerateRecovery(security.curPass);
            if (!alive) return;
            security.newRecoveryKey = recovered;
            security.curPass = '';
        } catch (e) {
            if (!alive) return;
            security.newRecoveryKey = '';
            notify(EVENT_TYPE.ERROR, secError(e));
        } finally {
            security.busy = false;
        }
    }

    // ---------- export / import ----------

    async function runExport(which) {
        exp.busy = true;
        try {
            const outcome = which === 'json' ? await api.exportJson(exp.pass) : await api.exportMarkdown(exp.pass);
            if (!alive) return;
            if (outcome) {
                exp.pass = '';
                notify(
                    EVENT_TYPE.SAVED,
                    `Exported ${outcome.entries} entries and ${outcome.attachments} attachments to ${outcome.target}. These files are NOT encrypted.`,
                    { duration: 0 },
                );
            }
        } catch (e) {
            if (!alive) return;
            notify(EVENT_TYPE.ERROR, expError(e));
        } finally {
            exp.busy = false;
        }
    }

    async function runImport(which) {
        imp.busy = true;
        try {
            const outcome = which === 'json' ? await api.importJson() : await api.importMarkdown();
            if (outcome) {
                // The workspace refresh is workspace truth: it runs
                // even after disposal so the diary never shows stale
                // data for an import that did happen.
                if (outcome.entries > 0 || outcome.folders > 0) onImported?.();
                if (!alive) return;
                let message = `Imported ${outcome.entries} entries and ${outcome.attachments} attachments.`;
                if (outcome.folders > 0) message += ` ${outcome.folders} folders.`;
                if (outcome.skippedEntries > 0 || outcome.skippedAttachments > 0) {
                    message += ` Skipped ${outcome.skippedEntries} entries and ${outcome.skippedAttachments} attachments (already present, invalid, unreadable or over the size limit).`;
                }
                if (outcome.exportOnlyAttachments > 0) {
                    message += ` Attachments available through export only: ${outcome.exportOnlyAttachments}.`;
                }
                notify(EVENT_TYPE.SAVED, message);
            }
        } catch (e) {
            if (!alive) return;
            notify(EVENT_TYPE.ERROR, resolveErrorText(SETTINGS_GENERAL_ERRORS, e).text);
        } finally {
            imp.busy = false;
        }
    }

    // ---------- sync ----------

    function resetSyncReport() {
        sync.problems = [];
        sync.warnings = [];
        sync.ackAhead = null;
    }

    async function loadSync() {
        try {
            const info = await api.syncOverview();
            if (!alive) return;
            sync.info = info;
        } catch {
            if (!alive) return;
            sync.info = null;
        }
    }

    // Folder picker/clear results re-enter as one-field patches
    // through the same sequence: merging over the latest
    // acknowledgement keeps a racing preference write from
    // reverting the picked folder, and vice versa.
    async function pickSync() {
        general.error = '';
        try {
            const updated = await api.syncPickFolder();
            if (updated) {
                const next = await settingsCommit.save({
                    sync_location: updated.sync_location,
                });
                if (!alive) return;
                general.settings = next;
                await loadSync();
            }
        } catch (e) {
            if (!alive) return;
            general.error = unknownErrorText(e);
        }
    }

    async function clearSync() {
        general.error = '';
        try {
            const cleared = await api.syncClearFolder();
            const next = await settingsCommit.save({
                sync_location: cleared.sync_location,
            });
            if (!alive) return;
            general.settings = next;
            sync.pruneArm = false;
            sync.info = null;
            resetSyncReport();
        } catch (e) {
            if (!alive) return;
            general.error = unknownErrorText(e);
        }
    }

    async function dismissAckAhead() {
        sync.busy = true;
        try {
            const info = await api.syncDismissAckAhead();
            if (!alive) return;
            sync.info = info;
        } catch (e) {
            if (!alive) return;
            sync.problems = [syncError(e)];
        } finally {
            sync.busy = false;
        }
    }

    async function runSync(task) {
        resetSyncReport();
        sync.busy = true;
        try {
            await task();
            await loadSync();
        } catch (e) {
            if (!alive) return;
            notify(EVENT_TYPE.ERROR, syncError(e));
        } finally {
            sync.busy = false;
        }
    }

    const runSyncEmit = () =>
        runSync(async () => {
            const bundle = await api.syncEmit();
            if (!alive) return;
            notify(
                EVENT_TYPE.INFO,
                bundle
                    ? `Wrote ${bundle.file_name} — ${bundle.entries} entries, ${bundle.attachments} attachments.`
                    : 'Nothing new to emit.',
            );
        });

    const runSyncReemit = () =>
        runSync(async () => {
            const bundle = await api.syncReemit();
            if (!alive) return;
            notify(
                EVENT_TYPE.INFO,
                bundle
                    ? `Everything is emitted as ${bundle.file_name} — peers that lost bundles to a prune recover them on their next apply.`
                    : 'Nothing to re-emit.',
            );
        });

    const runSyncApply = () =>
        runSync(async () => {
            const report = await api.syncApply();
            // Same workspace-truth rule as imports: the diary
            // refresh runs even after disposal.
            if (report.entries_applied > 0 || report.folders_applied > 0 || report.folders_conflicted > 0) {
                onImported?.();
            }
            if (!alive) return;
            let summary = `Applied ${report.bundles_applied} of ${report.bundles_seen} bundles: ${report.entries_applied} entries, ${report.entries_conflicted} conflict copies, ${report.attachments_applied} attachments.`;
            if (report.folders_applied > 0 || report.folders_conflicted > 0) {
                summary += ` ${report.folders_applied} folders applied.`;
            }
            notify(EVENT_TYPE.INFO, summary);
            sync.problems = report.problems;
            sync.warnings = report.warnings;
            sync.ackAhead = report.ack_ahead;
        });

    const runSyncPrune = () =>
        runSync(async () => {
            try {
                const report = await api.syncPrune();
                if (!alive) return;
                notify(
                    EVENT_TYPE.INFO,
                    report.covered_through > 0
                        ? `Removed ${report.bundles_removed} bundles and ${report.blobs_removed} blobs — every peer has consumed this device through ${report.covered_through}.`
                        : 'Nothing pruned — no peer has acknowledged this device yet.',
                );
            } finally {
                sync.pruneArm = false;
            }
        });

    async function doRestamp() {
        sync.busy = true;
        try {
            const id = await api.restampDevice();
            if (!alive) return;
            sync.restampArm = false;
            notify(
                EVENT_TYPE.UPDATED,
                `This copy now syncs as a new device (${id.slice(-8)}…). Its changes will be emitted again under the new identity.`,
            );
            await loadSync();
        } catch (e) {
            if (!alive) return;
            notify(EVENT_TYPE.ERROR, syncError(e));
        } finally {
            sync.busy = false;
        }
    }

    // ---------- Touch ID ----------

    async function refreshTouchId() {
        try {
            const status = await api.touchIdStatus(null);
            if (!alive) return;
            touchId.status = status;
        } catch {
            if (!alive) return;
            touchId.status = null;
        }
    }

    async function enableTouchId() {
        touchId.busy = true;
        try {
            await api.touchIdEnroll();
            await refreshTouchId();
            if (!alive) return;
            notify(EVENT_TYPE.UPDATED, 'Touch ID is on for this diary.');
        } catch (e) {
            if (!alive) return;
            if (e === 'touch_id_unavailable') {
                notify(EVENT_TYPE.INFO, 'Touch ID is not available in this build.');
            } else {
                notify(EVENT_TYPE.ERROR, unknownErrorText(e));
            }
        } finally {
            touchId.busy = false;
        }
    }

    async function disableTouchId() {
        touchId.busy = true;
        try {
            await api.touchIdDisable();
            await refreshTouchId();
            if (!alive) return;
            notify(EVENT_TYPE.UPDATED, 'Touch ID is off for this diary.');
        } catch (e) {
            if (!alive) return;
            notify(EVENT_TYPE.ERROR, unknownErrorText(e));
        } finally {
            touchId.busy = false;
        }
    }

    // ---------- general settings & backups ----------

    onMount(async () => {
        try {
            const settings = await api.settingsGet();
            if (!alive) return;
            general.settings = settings;
            settingsCommit = createSettingsCommit(general.settings, {
                set: (next) => api.settingsSet(next),
                get: () => api.settingsGet(),
            });
            general.timeoutMin = toMinutes(general.settings.lock_timeout_secs);
            refreshTouchId();
            if (general.settings.sync_location) loadSync();
        } catch (e) {
            if (!alive) return;
            general.error = unknownErrorText(e);
        }
    });

    // Outstanding preference writes: the controls' pending
    // state clears only when every queued write settles.
    let pendingSaves = 0;

    async function saveSetting(patch, successMessage = 'Settings saved.') {
        general.error = '';
        // The coordinator merges against the latest acknowledged
        // snapshot at execution time.
        pendingSaves++;
        general.prefsSaving = true;
        try {
            // The save itself always persists; only the local echo
            // and feedback are skipped after disposal.
            const next = await settingsCommit.save(patch);
            if (!alive) return;
            general.settings = next;
            notify(EVENT_TYPE.SAVED, successMessage);
        } catch (e) {
            if (!alive) return;
            general.error = unknownErrorText(e);
        } finally {
            general.prefsSaving = --pendingSaves > 0;
        }
    }

    const saveTimeout = () =>
        saveSetting({ lock_timeout_secs: toMinutes(general.timeoutMin * 60) * 60 }, 'Inactivity lock timeout saved.');

    const saveStrip = (strip) =>
        saveSetting(
            { strip_exif: strip },
            strip
                ? 'Location metadata will be removed from imported photos.'
                : 'Location metadata will be kept in imported photos.',
        );
    const saveClipboard = (clear) =>
        saveSetting(
            { clipboard_clear: clear },
            clear ? 'Clipboard clearing is enabled.' : 'Clipboard clearing is disabled.',
        );
    const saveSchedule = (schedule) => saveSetting({ backup_schedule: schedule }, 'Backup schedule saved.');

    const setSecurityField = (name, value) => {
        security[name] = value;
    };
    const setExportPass = (value) => {
        exp.pass = value;
    };
    const setTimeoutMin = (value) => {
        general.timeoutMin = value === '' ? null : Number(value);
    };

    async function backUpNow() {
        general.error = '';
        try {
            const target = await api.backupNow();
            if (!alive) return;
            notify(EVENT_TYPE.SAVED, `Backed up to ${target}.`);
        } catch (e) {
            if (!alive) return;
            general.error = resolveErrorText(SETTINGS_BACKUP_ERRORS, e).text;
        }
    }

    async function pickBackup() {
        general.error = '';
        try {
            const updated = await api.settingsPickBackupDir();
            if (updated) {
                const next = await settingsCommit.save({
                    backup_location: updated.backup_location,
                });
                if (!alive) return;
                general.settings = next;
            }
        } catch (e) {
            if (!alive) return;
            general.error = unknownErrorText(e);
        }
    }

    async function clearBackup() {
        general.error = '';
        try {
            const cleared = await api.settingsClearBackupDir();
            const next = await settingsCommit.save({
                backup_location: cleared.backup_location,
            });
            if (!alive) return;
            general.settings = next;
        } catch (e) {
            if (!alive) return;
            general.error = unknownErrorText(e);
        }
    }

    // ---------- view wiring ----------

    const actions = $derived({
        security: ['change-passphrase', 'regenerate-recovery'],
        rotate: ['rotate', ...(security.rotated ? ['rotate-done'] : [])],
        export: ['export-markdown', 'export-json'],
        import: ['import-markdown', 'import-json'],
        syncFolder: ['sync-pick', ...(general.settings?.sync_location ? ['sync-clear'] : [])],
        syncOps: [
            'sync-emit',
            'sync-reemit',
            'sync-apply',
            ...(sync.pruneArm ? ['sync-prune', 'sync-prune-cancel'] : ['sync-prune-arm']),
        ],
        restamp: sync.restampArm ? ['restamp', 'restamp-cancel'] : ['restamp-arm'],
        backupFolder: ['backup-pick', ...(general.settings?.backup_location ? ['backup-clear'] : [])],
        backupNow: ['backup-now'],
        timeout: ['save-timeout'],
    });

    const disabled = $derived({
        'change-passphrase':
            security.busy || !security.curPass || !security.newPass || security.newPass !== security.newPass2,
        'regenerate-recovery': security.busy || !security.curPass,
        rotate: security.busy || !security.curPass,
        'export-markdown': exp.busy || !exp.pass,
        'export-json': exp.busy || !exp.pass,
        'import-markdown': imp.busy,
        'import-json': imp.busy,
        'sync-emit': sync.busy,
        'sync-reemit': sync.busy,
        'sync-apply': sync.busy,
        'sync-prune': sync.busy,
        restamp: sync.busy,
        'backup-now': !general.settings?.backup_location,
        'save-timeout': general.prefsSaving,
    });

    const handlers = {
        'save-timeout': saveTimeout,
        'change-passphrase': changePassphrase,
        'regenerate-recovery': regenerateRecovery,
        rotate: rotateMasterKey,
        'rotate-done': async () => {
            try {
                await api.vaultLock();
            } catch (e) {
                if (!alive) return;
                notify(EVENT_TYPE.ERROR, secError(e));
            }
        },
        'export-markdown': () => runExport('markdown'),
        'export-json': () => runExport('json'),
        'import-markdown': () => runImport('markdown'),
        'import-json': () => runImport('json'),
        'sync-pick': pickSync,
        'sync-clear': clearSync,
        'sync-emit': runSyncEmit,
        'sync-reemit': runSyncReemit,
        'sync-apply': runSyncApply,
        'sync-prune': runSyncPrune,
        'sync-prune-cancel': () => (sync.pruneArm = false),
        'sync-prune-arm': () => (sync.pruneArm = true),
        restamp: doRestamp,
        'restamp-cancel': () => (sync.restampArm = false),
        'restamp-arm': () => (sync.restampArm = true),
        'backup-pick': pickBackup,
        'backup-clear': clearBackup,
        'backup-now': backUpNow,
        saveStrip,
        saveClipboard,
        saveSchedule,
        dismissAckAhead,
        touchIdEnable: enableTouchId,
        touchIdDisable: disableTouchId,
    };
</script>

<div class="settings-screen">
    <header class="settings-head">
        <div class="head-left">
            <Button kind="quiet-button" handler={onBack}><IconLeft />{settingsText.back}</Button>
        </div>
        <div class="head-title">
            <span>{settingsText.title}</span>
        </div>
        <div class="head-right"></div>
    </header>

    <div class="settings-scroll-content">
        <Settings
            {general}
            {security}
            {exp}
            {touchId}
            {imp}
            {sync}
            {settingsText}
            {fields}
            {ACTIONS}
            {actions}
            {disabled}
            {handlers}
            {setSecurityField}
            {setExportPass}
            {setTimeoutMin}
            onTabChange={clearNotifications}
            {onBack} />
    </div>
</div>

<style src="./styles.css"></style>
