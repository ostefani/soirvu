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
    import { clearNotifications } from '@/components/Notification/store.svelte.js';

    let { onBack, onImported = null } = $props();

    let general = $state(createGeneralModel());
    let security = $state(createSecurityModel());
    let touchId = $state(createTouchIdModel());
    let exp = $state(createExportModel());
    let imp = $state(createImportModel());
    let sync = $state(createSyncModel());

    let timeoutMin = $state(null);

    $effect(() => {
        if (timeoutMin === null && general.settings) {
            timeoutMin = Math.max(1, Math.round(general.settings.lock_timeout_secs / 60));
        }
    });

    function secError(e) {
        return e === 'wrong_passphrase' ? 'Wrong current passphrase.' : `Something went wrong (${e}).`;
    }

    function dismissNotification(key, text) {
        switch (key) {
            case 'general': if (general.note === text) general.note = ''; break;
            case 'security': if (security.note === text) security.note = ''; break;
            case 'touchId': if (touchId.note === text) touchId.note = ''; break;
            case 'sync': if (sync.note === text) sync.note = ''; break;
            case 'restamp': if (sync.restampNote === text) sync.restampNote = ''; break;
            case 'export': if (exp.note === text) exp.note = ''; break;
            case 'import': if (imp.note === text) imp.note = ''; break;
        }
    }

    function clearNotificationsForTabChange() {
        clearNotifications();
        general.note = '';
        security.note = '';
        touchId.note = '';
        sync.note = '';
        sync.restampNote = '';
        exp.note = '';
        imp.note = '';
    }

    async function changePassphrase() {
        security.note = '';
        security.newRecoveryKey = '';
        security.busy = true;
        try {
            await api.vaultChangePassphrase(security.curPass, security.newPass);
            security.curPass = security.newPass;
            security.newPass = '';
            security.newPass2 = '';
            security.note = 'Passphrase changed. The diary stays open — the new passphrase applies at the next unlock.';
            security.noteEvent = EVENT_TYPE.UPDATED;
        } catch (e) {
            security.note = secError(e);
            security.noteEvent = EVENT_TYPE.ERROR;
        } finally {
            security.busy = false;
        }
    }

    async function rotateMasterKey() {
        security.note = '';
        security.busy = true;

        const unlisten = await listen('rotation-started', () => {
            security.rotating = true;
        });
        try {
            const key = await api.vaultRotateMasterKey(security.curPass);
            if (key === null) {
                security.note = 'Master key unchanged.';
                security.noteEvent = EVENT_TYPE.INFO;
            } else {
                security.newRecoveryKey = key;
                security.rotated = true;
                security.note =
                    'Master key changed — and the diary is now locked. Save the new recovery key, then unlock with your passphrase (it did not change).';
                security.noteEvent = EVENT_TYPE.UPDATED;
            }
        } catch (e) {
            security.note = secError(e);
            security.noteEvent = EVENT_TYPE.ERROR;
        } finally {
            unlisten();
            security.busy = false;
            security.rotating = false;
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
        exp.note = '';
        exp.busy = true;
        try {
            const outcome = which === 'json' ? await api.exportJson(exp.pass) : await api.exportMarkdown(exp.pass);
            if (outcome) {
                exp.pass = '';
                exp.note =
                    'Exported ' +
                    outcome.entries +
                    ' entries and ' +
                    outcome.attachments +
                    ' attachments to ' +
                    outcome.target +
                    '. These files are NOT encrypted.';
            }
        } catch (e) {
            exp.note = expError(e);
            exp.noteEvent = EVENT_TYPE.ERROR;
        } finally {
            exp.busy = false;
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
        sync.busy = true;
        try {
            sync.info = await api.syncDismissAckAhead();
        } catch (e) {
            sync.problems = [syncError(e)];
        } finally {
            sync.busy = false;
        }
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
            general.error = `Something went wrong (${e}).`;
        }
    }

    async function clearSync() {
        general.error = '';
        try {
            general.settings = await api.syncClearFolder();
            sync.pruneArm = false;
            sync.info = null;
            sync.note = '';
            sync.problems = [];
            sync.warnings = [];
            sync.ackAhead = null;
        } catch (e) {
            general.error = `Something went wrong (${e}).`;
        }
    }

    async function runSyncReemit() {
        sync.note = '';
        sync.problems = [];
        sync.warnings = [];
        sync.ackAhead = null;
        sync.busy = true;
        try {
            const bundle = await api.syncReemit();
            sync.note = bundle
                ? 'Everything is emitted as ' +
                  bundle.file_name +
                  ' — peers that lost bundles to a prune recover them on their next apply.'
                : 'Nothing to re-emit.';
            await loadSync();
        } catch (e) {
            sync.note = syncError(e);
            sync.noteEvent = EVENT_TYPE.ERROR;
        } finally {
            sync.busy = false;
        }
    }

    async function runSyncEmit() {
        sync.note = '';
        sync.problems = [];
        sync.warnings = [];
        sync.ackAhead = null;
        sync.busy = true;
        try {
            const bundle = await api.syncEmit();
            sync.note = bundle
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
            sync.note = syncError(e);
            sync.noteEvent = EVENT_TYPE.ERROR;
        } finally {
            sync.busy = false;
        }
    }

    async function runSyncApply() {
        sync.note = '';
        sync.problems = [];
        sync.warnings = [];
        sync.ackAhead = null;
        sync.busy = true;
        try {
            const report = await api.syncApply();
            sync.note =
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
            sync.problems = report.problems;
            sync.warnings = report.warnings;
            sync.ackAhead = report.ack_ahead;
            if (report.entries_applied > 0) {
                onImported?.();
            }
            await loadSync();
        } catch (e) {
            sync.note = syncError(e);
            sync.noteEvent = EVENT_TYPE.ERROR;
        } finally {
            sync.busy = false;
        }
    }

    async function runSyncPrune() {
        sync.note = '';
        sync.problems = [];
        sync.warnings = [];
        sync.ackAhead = null;
        sync.busy = true;
        try {
            const report = await api.syncPrune();
            sync.note =
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
            sync.note = syncError(e);
            sync.noteEvent = EVENT_TYPE.ERROR;
        } finally {
            sync.pruneArm = false;
            sync.busy = false;
        }
    }

    async function doRestamp() {
        sync.restampNote = '';
        sync.busy = true;
        try {
            const id = await api.restampDevice();
            sync.restampArm = false;
            sync.restampNote =
                'This copy now syncs as a new device (' +
                id.slice(-8) +
                '…). Its changes will be emitted again under the new identity.';
            await loadSync();
        } catch (e) {
            sync.restampNote = syncError(e);
            sync.restampNoteEvent = EVENT_TYPE.ERROR;
        } finally {
            sync.busy = false;
        }
    }

    async function runImport(which) {
        imp.note = '';
        imp.busy = true;
        try {
            const outcome = which === 'json' ? await api.importJson() : await api.importMarkdown();
            if (outcome) {
                imp.note = 'Imported ' + outcome.entries + ' entries and ' + outcome.attachments + ' attachments.';
                if (outcome.skippedEntries > 0 || outcome.skippedAttachments > 0) {
                    imp.note +=
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
            imp.note = String(e) === 'locked' ? 'The diary is locked.' : 'Something went wrong (' + String(e) + ').';
            imp.noteEvent = EVENT_TYPE.ERROR;
        } finally {
            imp.busy = false;
        }
    }

    async function refreshTouchId() {
        try {
            touchId.status = await api.touchIdStatus(null);
        } catch {
            touchId.status = null;
        }
    }

    async function enableTouchId() {
        touchId.note = '';
        touchId.busy = true;
        try {
            await api.touchIdEnroll();
            await refreshTouchId();
            touchId.note = 'Touch ID is on for this diary.';
        } catch (e) {
            touchId.note =
                e === 'touch_id_unavailable'
                    ? 'Touch ID is not available in this build.'
                    : `Something went wrong (${e}).`;
            touchId.noteEvent = e === 'touch_id_unavailable' ? EVENT_TYPE.INFO : EVENT_TYPE.ERROR;
        } finally {
            touchId.busy = false;
        }
    }

    async function disableTouchId() {
        touchId.note = '';
        touchId.busy = true;
        try {
            await api.touchIdDisable();
            await refreshTouchId();
            touchId.note = 'Touch ID is off for this diary.';
        } catch (e) {
            touchId.note = `Something went wrong (${e}).`;
            touchId.noteEvent = EVENT_TYPE.ERROR;
        } finally {
            touchId.busy = false;
        }
    }

    async function regenerateRecovery() {
        security.note = '';
        security.busy = true;
        try {
            security.newRecoveryKey = await api.vaultRegenerateRecovery(security.curPass);
            security.curPass = '';
        } catch (e) {
            security.note = secError(e);
            security.noteEvent = EVENT_TYPE.ERROR;
            security.newRecoveryKey = '';
        } finally {
            security.busy = false;
        }
    }

    onMount(async () => {
        try {
            general.settings = await api.settingsGet();
            refreshTouchId();
            if (general.settings.sync_location) {
                loadSync();
            }
        } catch (e) {
            general.error = `Something went wrong (${e}).`;
        }
    });

    async function saveSetting(patch, successMessage = 'Settings saved.') {
        general.error = '';
        general.note = '';
        try {
            await api.settingsSet({ ...general.settings, ...patch });
            general.settings = await api.settingsGet();
            general.note = successMessage;
        } catch (e) {
            general.error = `Something went wrong (${e}).`;
        }
    }

    const saveTimeout = () =>
        saveSetting({ lock_timeout_secs: Math.max(1, Math.round(timeoutMin)) * 60 }, 'Inactivity lock timeout saved.');
    const saveStrip = () =>
        saveSetting(
            { strip_exif: general.settings.strip_exif },
            general.settings.strip_exif ? 'Location metadata will be removed from imported photos.' : 'Location metadata will be kept in imported photos.',
        );
    const saveClipboard = () =>
        saveSetting(
            { clipboard_clear: general.settings.clipboard_clear },
            general.settings.clipboard_clear ? 'Clipboard clearing is enabled.' : 'Clipboard clearing is disabled.',
        );
    const saveSchedule = () => saveSetting({ backup_schedule: general.settings.backup_schedule }, 'Backup schedule saved.');

    async function backUpNow() {
        general.error = '';
        general.note = '';
        try {
            const name = await api.backupNow();
            general.note = `Backed up to ${name}.`;
        } catch (e) {
            general.error =
                e === 'backup_unavailable' ? 'The backup folder is not available.' : `Something went wrong (${e}).`;
        }
    }

    async function pickBackup() {
        general.error = '';
        general.note = '';
        try {
            const updated = await api.settingsPickBackupDir();
            if (updated) general.settings = updated;
        } catch (e) {
            general.error = `Something went wrong (${e}).`;
        }
    }

    async function clearBackup() {
        general.error = '';
        general.note = '';
        try {
            general.settings = await api.settingsClearBackupDir();
        } catch (e) {
            general.error = `Something went wrong (${e}).`;
        }
    }

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
            bind:general
            bind:security
            bind:exp
            bind:timeoutMin
            {touchId}
            {imp}
            {sync}
            {settingsText}
            {fields}
            {ACTIONS}
            {actions}
            {disabled}
            {handlers}
            onTabChange={clearNotificationsForTabChange}
            onDismissNotification={dismissNotification}
            {onBack} />
    </div>
</div>

<style src="./styles.css"></style>
