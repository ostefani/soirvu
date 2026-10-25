<script>
    import Button from '@/components/Button/index.svelte';
    import Input from '@/components/Input/index.svelte';
    import Select from '@/components/Select/index.svelte';
    import ErrorComponent from '@/components/ErrorComponent/index.svelte';
    import NoteComponent from '@/components/NoteComponent/index.svelte';
    import WarningComponent from '@/components/WarningComponent/index.svelte';
    import Tabs from '@/components/Tabs/index.svelte';
    import Checkbox from '@/components/Checkbox/index.svelte';
    import Description from '@/components/Description/index.svelte';

    let {
        general,
        security,
        exp,
        touchId,
        imp,
        sync,
        settingsText,
        fields,
        ACTIONS,
        actions,
        disabled,
        handlers,
        setSecurityField,
        setExportPass,
        setTimeoutMin,
        onTabChange,
    } = $props();

    let activeTab = $state('general');

    function changeTab(id) {
        onTabChange?.(id);
        activeTab = id;
    }

    const tabs = [
        { id: 'general', label: 'General' },
        { id: 'security', label: 'Security' },
        { id: 'backup', label: 'Backups' },
        { id: 'sync', label: 'Sync' },
        { id: 'transfer', label: 'Import & Export' },
    ];
</script>

<div class="settings-wrapper">
    <Tabs {tabs} {activeTab} onChange={changeTab} />

    {#if general.settings}
        <!-- ================================================================= -->
        <!-- TAB 1: GENERAL                                                    -->
        <!-- ================================================================= -->
        {#if activeTab === 'general'}
            <div class="tab-panel" role="tabpanel" id="panel-general" aria-labelledby="tab-general">
                <div class="settings-card settings-storage">
                    <div class="card-header">
                        <h3 class="card-title">Diary Storage & Inactivity Lock</h3>
                        <Description text="View storage location and configure automatic locking." />
                    </div>

                    <div class="setting-item">
                        <span class="setting-label">{settingsText.vaultLocation}</span>
                        <code class="location-value">{general.settings.vault_location ?? '—'}</code>
                    </div>

                    <hr class="settings-divider" />

                    <div class="setting-item timeout-item">
                        {#each actions.timeout as action (action)}
                            <Input
                                field={fields.timeout}
                                value={general.timeoutMin}
                                oninput={(e) => setTimeoutMin(e.currentTarget.value)} />
                            <Button
                                {...ACTIONS[action]}
                                kind="texture-button"
                                handler={handlers[action]}
                                disabled={disabled[action]} />
                        {/each}
                    </div>
                </div>

                <div class="settings-card settings-clipboard">
                    <div class="card-header">
                        <h3 class="card-title">Privacy & Clipboard</h3>
                        <Description text="Control location metadata handling and clipboard auto-clearing." />
                    </div>

                    <div class="checkbox-group">
                        <Checkbox
                            checked={general.settings.strip_exif}
                            onchange={(e) => handlers.saveStrip(e.currentTarget.checked)}
                            label={settingsText.stripExif} />

                        {#if general.settings?.platform === 'macos'}
                            <Checkbox
                                checked={general.settings.clipboard_clear}
                                onchange={(e) => handlers.saveClipboard(e.currentTarget.checked)}
                                label={settingsText.clipboardClear} />
                        {/if}
                    </div>
                </div>

                {#if general.error}
                    <ErrorComponent text={general.error} />
                {/if}
            </div>

            <!-- ================================================================= -->
            <!-- TAB 2: SECURITY                                                   -->
            <!-- ================================================================= -->
        {:else if activeTab === 'security'}
            <div class="tab-panel" role="tabpanel" id="panel-security" aria-labelledby="tab-security">
                <div class="settings-card">
                    <div class="card-header">
                        <h3 class="card-title">{settingsText.securityTitle}</h3>
                        {#each settingsText.securityNotes as note}
                            <Description text={note} />
                        {/each}
                    </div>
                    <form class="security-form">
                        {#each fields.security as field, i (field.name)}
                            <div class="field" class:field-second={i === 2}>
                                <Input
                                    {field}
                                    value={security[field.name]}
                                    oninput={(e) => setSecurityField(field.name, e.currentTarget.value)} />
                            </div>
                        {/each}

                        <div class="form-actions">
                            {#each actions.security as action (action)}
                                <Button
                                    {...ACTIONS[action]}
                                    kind={action === 'change-passphrase' ? 'primary-button' : 'texture-button'}
                                    handler={handlers[action]}
                                    disabled={disabled[action]} />
                            {/each}
                        </div>

                        <div class="form-actions form-actions-secondary">
                            {#each actions.rotate as action (action)}
                                <Button
                                    {...ACTIONS[action]}
                                    kind="texture-button"
                                    handler={handlers[action]}
                                    disabled={disabled[action]} />
                            {/each}
                        </div>

                        {#if security.rotating}
                            <div class="progress form-full">
                                <span class="spinner"></span>
                                <span>{settingsText.rotating}</span>
                            </div>
                        {/if}

                        {#if security.newRecoveryKey}
                            <div class="form-full">
                                <NoteComponent
                                    variant="warning"
                                    title={settingsText.newKeyLead}
                                    text={settingsText.newKeyNote}>
                                    <p><code>{security.newRecoveryKey}</code></p>
                                </NoteComponent>
                            </div>
                        {/if}
                    </form>
                </div>

                {#if touchId.status?.capable || touchId.status?.enrolled}
                    <div class="settings-card">
                        <div class="card-header">
                            <h3 class="card-title">{settingsText.touchIdTitle}</h3>
                        </div>

                        {#if touchId.status?.enrolled && !touchId.status.capable}
                            <NoteComponent
                                variant="warning"
                                text={`${settingsText.touchIdEnrolledNotCapableLead} ${touchId.status.built_in ? settingsText.touchIdNotSignedProfile : settingsText.touchIdNotBuilt}${settingsText.touchIdEnrolledNotCapableTail}`} />
                            <div class="setting-row">
                                <Button
                                    label={settingsText.touchIdRemoveEnrollment}
                                    kind="quiet"
                                    handler={handlers.touchIdDisable}
                                    disabled={touchId.busy} />
                            </div>
                        {:else if touchId.status && !touchId.status.capable && touchId.status.built_in}
                            <Description text={settingsText.touchIdNotSigned} />
                        {:else if touchId.status?.enrolled}
                            <Description text={settingsText.touchIdEnrolled} />
                            <div class="setting-row">
                                <Button
                                    label={settingsText.touchIdTurnOff}
                                    kind="quiet"
                                    handler={handlers.touchIdDisable}
                                    disabled={touchId.busy} />
                            </div>
                        {:else if touchId.status?.capable}
                            <Description text={settingsText.touchIdCapable} />
                            <div class="setting-row">
                                <Button
                                    label={settingsText.touchIdTurnOn}
                                    handler={handlers.touchIdEnable}
                                    disabled={touchId.busy} />
                            </div>
                        {/if}
                    </div>
                {/if}
            </div>

            <!-- ================================================================= -->
            <!-- TAB 3: BACKUPS                                                    -->
            <!-- ================================================================= -->
        {:else if activeTab === 'backup'}
            <div class="tab-panel" role="tabpanel" id="panel-backup" aria-labelledby="tab-backup">
                <div class="settings-card">
                    <div class="card-header">
                        <h3 class="card-title">Automated & Manual Backups</h3>
                        {#each settingsText.backupNotes as note}
                            <Description text={note} />
                        {/each}
                    </div>

                    <div class="setting-item">
                        <span class="setting-label">{settingsText.backupLocation}</span>
                        <span class="setting-value">
                            {#if general.settings.backup_location}
                                <strong>{general.settings.backup_location}</strong>
                            {:else}
                                <span class="muted">{settingsText.notSet}</span>
                            {/if}
                        </span>
                        <div class="setting-row">
                            {#each actions.backupFolder as action (action)}
                                <Button
                                    {...ACTIONS[action]}
                                    kind="texture-button"
                                    handler={handlers[action]}
                                    disabled={disabled[action]} />
                            {/each}
                        </div>
                    </div>

                    <hr class="settings-divider" />

                    <div class="setting-item">
                        <Select
                            field={fields.schedule}
                            value={general.settings.backup_schedule}
                            onchange={(e) => handlers.saveSchedule(e.currentTarget.value)} />
                        <div class="setting-row">
                            {#each actions.backupNow as action (action)}
                                <Button
                                    {...ACTIONS[action]}
                                    kind="texture-button"
                                    handler={handlers[action]}
                                    disabled={disabled[action]} />
                            {/each}
                        </div>
                    </div>
                </div>
            </div>

            <!-- ================================================================= -->
            <!-- TAB 4: SYNC                                                       -->
            <!-- ================================================================= -->
        {:else if activeTab === 'sync'}
            <div class="tab-panel" role="tabpanel" id="panel-sync" aria-labelledby="tab-sync">
                <div class="settings-card">
                    <div class="card-header">
                        <h3 class="card-title">{settingsText.syncTitle}</h3>
                        <Description text={settingsText.syncIntro} />
                    </div>

                    <div class="setting-item">
                        <span class="setting-label">{settingsText.syncFolder}</span>
                        <span class="setting-value">
                            {#if general.settings.sync_location}
                                <strong>{general.settings.sync_location}</strong>
                            {:else}
                                <span class="muted">{settingsText.notSet}</span>
                            {/if}
                        </span>
                        <div class="setting-row">
                            {#each actions.syncFolder as action (action)}
                                <Button
                                    {...ACTIONS[action]}
                                    kind="texture-button"
                                    handler={handlers[action]}
                                    disabled={disabled[action]} />
                            {/each}
                        </div>
                    </div>

                    {#if general.settings.sync_location && sync.info}
                        <hr class="settings-divider" />

                        {#if sync.info.ack_ahead_seen}
                            <ErrorComponent text={settingsText.ackAhead}>
                                <Button
                                    label={settingsText.dismiss}
                                    kind="texture-button"
                                    handler={handlers.dismissAckAhead}
                                    disabled={sync.busy} />
                            </ErrorComponent>
                        {/if}

                        <div class="sync-peers-list">
                            <p class="sync-device-id">This device: <code>{sync.info.device_id}</code></p>
                            {#each sync.info.peers as peer}
                                <p class="sync-peer-info">
                                    Peer <code>{peer.device_id.slice(-8)}…</code>: changes incorporated through {peer.applied_lseq}
                                    {#if peer.waiting > 0}
                                        ({peer.waiting} bundle{peer.waiting === 1 ? '' : 's'} waiting)
                                    {/if}
                                </p>
                                {#if peer.needs_reemit}
                                    <NoteComponent
                                        variant="warning"
                                        text={`Device ${peer.device_id.slice(-8)}… needs to re-emit everything, or its earlier bundles are still on their way.`} />
                                {/if}
                            {/each}
                        </div>

                        <div class="setting-row">
                            {#each actions.syncOps as action (action)}
                                <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
                            {/each}
                        </div>

                        {#if sync.pruneArm}
                            {#each settingsText.pruneNotes as note}
                                <NoteComponent variant="warning" text={note} />
                            {/each}
                        {/if}
                    {/if}

                    {#each sync.problems as problem}
                        <ErrorComponent text={problem} />
                    {/each}
                    {#each sync.warnings as warning}
                        <WarningComponent text={warning} />
                    {/each}
                    {#if sync.ackAhead && !sync.info?.ack_ahead_seen}
                        <ErrorComponent text={sync.ackAhead} />
                    {/if}
                </div>

                <div class="settings-card">
                    <div class="card-header">
                        <h3 class="card-title">Device Identity</h3>
                        <Description text={settingsText.restampIntro} />
                    </div>
                    <div class="setting-row">
                        {#each actions.restamp as action (action)}
                            <Button
                                {...ACTIONS[action]}
                                kind="texture-button"
                                handler={handlers[action]}
                                disabled={disabled[action]} />
                        {/each}
                    </div>
                </div>
            </div>

            <!-- ================================================================= -->
            <!-- TAB 5: IMPORT & EXPORT                                            -->
            <!-- ================================================================= -->
        {:else if activeTab === 'transfer'}
            <div class="tab-panel" role="tabpanel" id="panel-transfer" aria-labelledby="tab-transfer">
                <div class="settings-card">
                    <div class="card-header">
                        <h3 class="card-title">{settingsText.exportTitle}</h3>
                        <Description text={settingsText.exportIntro} />
                        {#each settingsText.exportNotes as note}
                            <Description size="small" subtle text={note} />
                        {/each}
                    </div>

                    <Input
                        field={fields.exportPass}
                        value={exp.pass}
                        oninput={(e) => setExportPass(e.currentTarget.value)} />

                    <div class="setting-row">
                        {#each actions.export as action (action)}
                            <Button
                                {...ACTIONS[action]}
                                kind="texture-button"
                                handler={handlers[action]}
                                disabled={disabled[action]} />
                        {/each}
                    </div>
                </div>

                <div class="settings-card">
                    <div class="card-header">
                        <h3 class="card-title">{settingsText.importTitle}</h3>
                        <Description text={settingsText.importIntro} />
                    </div>

                    <div class="setting-row">
                        {#each actions.import as action (action)}
                            <Button
                                {...ACTIONS[action]}
                                kind="texture-button"
                                handler={handlers[action]}
                                disabled={disabled[action]} />
                        {/each}
                    </div>
                </div>
            </div>
        {/if}
    {:else if !general.error}
        <NoteComponent text={settingsText.loading} />
    {/if}
</div>

<style src="./styles.css"></style>
