<script>
    import { onMount } from 'svelte';
    import { listen } from '@tauri-apps/api/event';
    import * as api from '@/api.js';
    import Button from '@/components/Button/index.svelte';
    import Settings from '@/components/Settings/index.svelte';
    import {
        createExportModel,
        createGeneralModel,
        createImportModel,
        createSecurityModel,
        createSyncModel,
        createTouchIdModel,
    } from './settingsModel.js';
    import { ACTIONS, settingsText, fields } from './constants.js';
    import { EVENT_TYPE } from '@/constants.js';
    import { notify, clearNotifications } from '@/components/Notification/store.svelte.js';

    let { onBack, onImported = null } = $props();

    let general = $state(createGeneralModel());
    let security = $state(createSecurityModel());
    let touchId = $state(createTouchIdModel());
    let exp = $state(createExportModel());
    let imp = $state(createImportModel());
    let sync = $state(createSyncModel());

    // One notification slot per section: a new notify replaces the previous one in place.
    const toMinutes = (secs) => Math.max(1, Math.round(secs / 60));
    const somethingWrong = (e) => `Something went wrong (${e}).`;

    // ---------- errors ----------

    function secError(e) {
        return e === 'wrong_passphrase' ? 'Wrong current passphrase.' : somethingWrong(e);
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
                return somethingWrong(e);
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
                return somethingWrong(e);
        }
    }

    // ---------- security ----------

    async function changePassphrase() {
        security.newRecoveryKey = '';
        security.busy = true;
        try {
            await api.vaultChangePassphrase(security.curPass, security.newPass);
            security.curPass = security.newPass;
            security.newPass = '';
            security.newPass2 = '';
            notify(
                EVENT_TYPE.UPDATED,
                'Passphrase changed. The diary stays open — the new passphrase applies at the next unlock.',
            );
        } catch (e) {
            notify(EVENT_TYPE.ERROR, secError(e));
        } finally {
            security.busy = false;
        }
    }

    async function rotateMasterKey() {
        security.busy = true;
        let unlisten = () => {};
        try {
            unlisten = await listen('rotation-started', () => (security.rotating = true));
            const key = await api.vaultRotateMasterKey(security.curPass);
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
            security.newRecoveryKey = await api.vaultRegenerateRecovery(security.curPass);
            security.curPass = '';
        } catch (e) {
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
            if (outcome) {
                exp.pass = '';
                notify(
                    EVENT_TYPE.SAVED,
                    `Exported ${outcome.entries} entries and ${outcome.attachments} attachments to ${outcome.target}. These files are NOT encrypted.`,
                    { duration: 0 },
                );
            }
        } catch (e) {
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
                let message = `Imported ${outcome.entries} entries and ${outcome.attachments} attachments.`;
                if (outcome.skippedEntries > 0 || outcome.skippedAttachments > 0) {
                    message += ` Skipped ${outcome.skippedEntries} entries and ${outcome.skippedAttachments} attachments (already present or unreadable).`;
                }
                notify(EVENT_TYPE.SAVED, message);
                if (outcome.entries > 0) onImported?.();
            }
        } catch (e) {
            notify(EVENT_TYPE.ERROR, String(e) === 'locked' ? 'The diary is locked.' : somethingWrong(e));
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
            sync.info = await api.syncOverview();
        } catch {
            sync.info = null;
        }
    }

    async function pickSync() {
        general.error = '';
        try {
            const updated = await api.syncPickFolder();
            if (updated) {
                general.settings = updated;
                await loadSync();
            }
        } catch (e) {
            general.error = somethingWrong(e);
        }
    }

    async function clearSync() {
        general.error = '';
        try {
            general.settings = await api.syncClearFolder();
            sync.pruneArm = false;
            sync.info = null;
            resetSyncReport();
        } catch (e) {
            general.error = somethingWrong(e);
        }
    }

    async function dismissAckAhead() {
        sync.busy = true;
        try {
            sync.info = await api.syncDismissAckAhead();
        } catch (e) {
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
            notify(EVENT_TYPE.ERROR, syncError(e));
        } finally {
            sync.busy = false;
        }
    }

    const runSyncEmit = () =>
        runSync(async () => {
            const bundle = await api.syncEmit();
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
            notify(
                EVENT_TYPE.INFO,
                `Applied ${report.bundles_applied} of ${report.bundles_seen} bundles: ${report.entries_applied} entries, ${report.entries_conflicted} conflict copies, ${report.attachments_applied} attachments.`,
            );
            sync.problems = report.problems;
            sync.warnings = report.warnings;
            sync.ackAhead = report.ack_ahead;
            if (report.entries_applied > 0) onImported?.();
        });

    const runSyncPrune = () =>
        runSync(async () => {
            try {
                const report = await api.syncPrune();
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
            sync.restampArm = false;
            notify(
                EVENT_TYPE.UPDATED,
                `This copy now syncs as a new device (${id.slice(-8)}…). Its changes will be emitted again under the new identity.`,
            );
            await loadSync();
        } catch (e) {
            notify(EVENT_TYPE.ERROR, syncError(e));
        } finally {
            sync.busy = false;
        }
    }

    // ---------- Touch ID ----------

    async function refreshTouchId() {
        try {
            touchId.status = await api.touchIdStatus(null);
        } catch {
            touchId.status = null;
        }
    }

    async function enableTouchId() {
        touchId.busy = true;
        try {
            await api.touchIdEnroll();
            await refreshTouchId();
            notify(EVENT_TYPE.UPDATED, 'Touch ID is on for this diary.');
        } catch (e) {
            if (e === 'touch_id_unavailable') {
                notify(EVENT_TYPE.INFO, 'Touch ID is not available in this build.');
            } else {
                notify(EVENT_TYPE.ERROR, somethingWrong(e));
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
            notify(EVENT_TYPE.UPDATED, 'Touch ID is off for this diary.');
        } catch (e) {
            notify(EVENT_TYPE.ERROR, somethingWrong(e));
        } finally {
            touchId.busy = false;
        }
    }

    // ---------- general settings & backups ----------

    onMount(async () => {
        try {
            general.settings = await api.settingsGet();
            general.timeoutMin = toMinutes(general.settings.lock_timeout_secs);
            refreshTouchId();
            if (general.settings.sync_location) loadSync();
        } catch (e) {
            general.error = somethingWrong(e);
        }
    });

    async function saveSetting(patch, successMessage = 'Settings saved.') {
        general.error = '';
        try {
            await api.settingsSet({ ...general.settings, ...patch });
            general.settings = await api.settingsGet();
            notify(EVENT_TYPE.SAVED, successMessage);
        } catch (e) {
            general.error = somethingWrong(e);
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
            notify(EVENT_TYPE.SAVED, `Backed up to ${await api.backupNow()}.`);
        } catch (e) {
            general.error = e === 'backup_unavailable' ? 'The backup folder is not available.' : somethingWrong(e);
        }
    }

    async function pickBackup() {
        general.error = '';
        try {
            const updated = await api.settingsPickBackupDir();
            if (updated) general.settings = updated;
        } catch (e) {
            general.error = somethingWrong(e);
        }
    }

    async function clearBackup() {
        general.error = '';
        try {
            general.settings = await api.settingsClearBackupDir();
        } catch (e) {
            general.error = somethingWrong(e);
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
    });

    const handlers = {
        'save-timeout': saveTimeout,
        'change-passphrase': changePassphrase,
        'regenerate-recovery': regenerateRecovery,
        rotate: rotateMasterKey,
        'rotate-done': () => api.vaultLock(),
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
            <Button label={`‹ ${settingsText.back}`} kind="quiet" handler={onBack} />
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
