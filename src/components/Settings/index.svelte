<script>
    import Button from '@/components/Button/index.svelte';
    import Input from '@/components/Input/index.svelte';
    import Select from '@/components/Select/index.svelte';
    import ErrorComponent from '@/components/ErrorComponent/index.svelte';
    import NoteComponent from '@/components/NoteComponent/index.svelte';
    import WarningComponent from '@/components/WarningComponent/index.svelte';
    import Tabs from '@/components/Tabs/index.svelte';

    let {
        general = $bindable(),
        security = $bindable(),
        timeoutMin = $bindable(),
        exp = $bindable(),
        touchId,
        imp,
        sync,
        settingsText,
        fields,
        ACTIONS,
        actions,
        disabled,
        handlers,
    } = $props();

    let activeTab = $state('general');

    const tabs = [
        { id: 'general', label: 'General' },
        { id: 'security', label: 'Security' },
        { id: 'backup', label: 'Backups' },
        { id: 'sync', label: 'Sync' },
        { id: 'transfer', label: 'Import & Export' },
    ];
</script>

<div class="settings-wrapper">
    <Tabs {tabs} {activeTab} onChange={(id) => (activeTab = id)} />

    {#if general.settings}
        <!-- ================================================================= -->
        <!-- TAB 1: GENERAL                                                    -->
        <!-- ================================================================= -->
        {#if activeTab === 'general'}
            <div class="tab-panel" role="tabpanel" id="panel-general" aria-labelledby="tab-general">
                <div class="settings-card">
                    <div class="card-header">
                        <h3 class="card-title">Diary Storage & Inactivity Lock</h3>
                        <p class="card-description">View storage location and configure automatic locking.</p>
                    </div>

                    <div class="setting-item">
                        <span class="setting-label">{settingsText.vaultLocation}</span>
                        <span class="setting-value">
                            <strong>{general.settings.vault_location ?? '—'}</strong>
                        </span>
                    </div>

                    <hr class="settings-divider" />

                    <div class="setting-item">
                        <Input field={fields.timeout} bind:value={timeoutMin} />
                        <div class="setting-row">
                            {#each actions.timeout as action (action)}
                                <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
                            {/each}
                        </div>
                    </div>
                </div>

                <div class="settings-card">
                    <div class="card-header">
                        <h3 class="card-title">Privacy & Clipboard</h3>
                        <p class="card-description">Control location metadata handling and clipboard auto-clearing.</p>
                    </div>

                    <div class="checkbox-group">
                        <label class="checkbox-label">
                            <input
                                type="checkbox"
                                bind:checked={general.settings.strip_exif}
                                onchange={handlers.saveStrip} />
                            <span>{settingsText.stripExif}</span>
                        </label>

                        {#if general.settings?.platform === 'macos'}
                            <label class="checkbox-label">
                                <input
                                    type="checkbox"
                                    bind:checked={general.settings.clipboard_clear}
                                    onchange={handlers.saveClipboard} />
                                <span>{settingsText.clipboardClear}</span>
                            </label>
                        {/if}
                    </div>
                </div>

                {#if general.note}
                    <NoteComponent text={general.note} />
                {/if}
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
                            <p class="card-description">{note}</p>
                        {/each}
                    </div>

                    {#each fields.security as field (field.name)}
                        <Input {field} bind:value={security[field.name]} />
                    {/each}

                    <div class="setting-row">
                        {#each actions.security as action (action)}
                            <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
                        {/each}
                        {#each actions.rotate as action (action)}
                            <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
                        {/each}
                    </div>

                    {#if security.rotating}
                        <div class="progress">
                            <span class="spinner"></span>
                            <span>{settingsText.rotating}</span>
                        </div>
                    {/if}

                    {#if security.newRecoveryKey}
                        <NoteComponent variant="warning" title={settingsText.newKeyLead} text={settingsText.newKeyNote}>
                            <p><code>{security.newRecoveryKey}</code></p>
                        </NoteComponent>
                    {/if}

                    {#if security.note}
                        <NoteComponent text={security.note} />
                    {/if}
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
                            <p class="card-description">{settingsText.touchIdNotSigned}</p>
                        {:else if touchId.status?.enrolled}
                            <p class="card-description">{settingsText.touchIdEnrolled}</p>
                            <div class="setting-row">
                                <Button
                                    label={settingsText.touchIdTurnOff}
                                    kind="quiet"
                                    handler={handlers.touchIdDisable}
                                    disabled={touchId.busy} />
                            </div>
                        {:else if touchId.status?.capable}
                            <p class="card-description">{settingsText.touchIdCapable}</p>
                            <div class="setting-row">
                                <Button
                                    label={settingsText.touchIdTurnOn}
                                    handler={handlers.touchIdEnable}
                                    disabled={touchId.busy} />
                            </div>
                        {/if}

                        {#if touchId.note}
                            <NoteComponent text={touchId.note} />
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
                            <p class="card-description">{note}</p>
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
                                <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
                            {/each}
                        </div>
                    </div>

                    <hr class="settings-divider" />

                    <div class="setting-item">
                        <Select
                            field={fields.schedule}
                            bind:value={general.settings.backup_schedule}
                            onchange={handlers.saveSchedule} />
                        <div class="setting-row">
                            {#each actions.backupNow as action (action)}
                                <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
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
                        <p class="card-description">{settingsText.syncIntro}</p>
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
                                <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
                            {/each}
                        </div>
                    </div>

                    {#if general.settings.sync_location && sync.info}
                        <hr class="settings-divider" />

                        {#if sync.info.ack_ahead_seen}
                            <ErrorComponent text={settingsText.ackAhead}>
                                <Button
                                    label={settingsText.dismiss}
                                    kind="quiet"
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

                    {#if sync.note}
                        <NoteComponent text={sync.note} />
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
                        <p class="card-description">{settingsText.restampIntro}</p>
                    </div>
                    <div class="setting-row">
                        {#each actions.restamp as action (action)}
                            <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
                        {/each}
                    </div>
                    {#if sync.restampNote}
                        <NoteComponent text={sync.restampNote} />
                    {/if}
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
                        <p class="card-description">{settingsText.exportIntro}</p>
                        {#each settingsText.exportNotes as note}
                            <p class="helper-text-block">{note}</p>
                        {/each}
                    </div>

                    <Input field={fields.exportPass} bind:value={exp.pass} />

                    <div class="setting-row">
                        {#each actions.export as action (action)}
                            <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
                        {/each}
                    </div>

                    {#if exp.note}
                        <NoteComponent text={exp.note} />
                    {/if}
                </div>

                <div class="settings-card">
                    <div class="card-header">
                        <h3 class="card-title">{settingsText.importTitle}</h3>
                        <p class="card-description">{settingsText.importIntro}</p>
                    </div>

                    <div class="setting-row">
                        {#each actions.import as action (action)}
                            <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
                        {/each}
                    </div>

                    {#if imp.note}
                        <NoteComponent text={imp.note} />
                    {/if}
                </div>
            </div>
        {/if}

    {:else if !general.error}
        <NoteComponent text={settingsText.loading} />
    {/if}
</div>

<style src="./styles.css"></style>
