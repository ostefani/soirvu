<script>
    import Button from '@/components/Button/index.svelte';
    import Input from '@/components/Input/index.svelte';
    import Select from '@/components/Select/index.svelte';
    import ErrorComponent from '@/components/ErrorComponent/index.svelte';
    import NoteComponent from '@/components/NoteComponent/index.svelte';
    import WarningComponent from '@/components/WarningComponent/index.svelte';

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
        onBack,
    } = $props();
    // console.log('actions: ', actions);
</script>

<div class="settings">
    <h2>{settingsText.title}</h2>

    {#if general.settings}
        <p>
            {settingsText.vaultLocation}
            <strong>{general.settings.vault_location ?? '—'}</strong>
        </p>

        <Input field={fields.timeout} bind:value={timeoutMin} />
        {#each actions.timeout as action (action)}
            <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
        {/each}

        <label>
            {settingsText.stripExif}
            <input type="checkbox" bind:checked={general.settings.strip_exif} onchange={handlers.saveStrip} />
        </label>

        {#if general.settings?.platform === 'macos'}
            <label>
                {settingsText.clipboardClear}
                <input
                    type="checkbox"
                    bind:checked={general.settings.clipboard_clear}
                    onchange={handlers.saveClipboard} />
            </label>
        {/if}

        <p>
            {settingsText.backupLocation}
            {#if general.settings.backup_location}
                <strong>{general.settings.backup_location}</strong>
            {:else}
                <span class="note">{settingsText.notSet}</span>
            {/if}
        </p>
        <div class="row">
            {#each actions.backupFolder as action (action)}
                <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
            {/each}
        </div>

        <Select
            field={fields.schedule}
            bind:value={general.settings.backup_schedule}
            onchange={handlers.saveSchedule} />

        <div class="row">
            {#each actions.backupNow as action (action)}
                <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
            {/each}
        </div>
        {#each settingsText.backupNotes as text}
            <NoteComponent {text} />
        {/each}

        <h3>{settingsText.securityTitle}</h3>
        settingsText
        {#each fields.security as field (field.name)}
            <Input {field} bind:value={security[field.name]} />
        {/each}

        <div class="row">
            {#each actions.security as action (action)}
                <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
            {/each}
        </div>

        <div class="row">
            {#each actions.rotate as action (action)}
                <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
            {/each}
        </div>

        {#if security.rotating}
            <p class="note progress">
                <span class="spinner"></span>
                <span>{settingsText.rotating}</span>
            </p>
        {/if}

        {#if security.newRecoveryKey}
            <p>{settingsText.newKeyLead} <code>{security.newRecoveryKey}</code></p>
            <NoteComponent text={settingsText.newKeyNote} />
        {/if}
        <NoteComponent text={security.note} />

        {#each settingsText.securityNotes as text}
            <NoteComponent {text} />
        {/each}

        {#if touchId.status?.capable || touchId.status?.enrolled}<h3>{settingsText.touchIdTitle}</h3>{/if}
        {#if touchId.status?.enrolled && !touchId.status.capable}
            <NoteComponent
                text={`${settingsText.touchIdEnrolledNotCapableLead} ${touchId.status.built_in ? settingsText.touchIdNotSignedProfile : settingsText.touchIdNotBuilt}${settingsText.touchIdEnrolledNotCapableTail}`} />
            <div class="row">
                <Button
                    label={settingsText.touchIdRemoveEnrollment}
                    kind="quiet"
                    handler={handlers.touchIdDisable}
                    disabled={touchId.busy} />
            </div>
        {:else if touchId.status && !touchId.status.capable && touchId.status.built_in}
            <NoteComponent text={settingsText.touchIdNotSigned} />
        {:else if touchId.status?.enrolled}
            <NoteComponent text={settingsText.touchIdEnrolled} />
            <div class="row">
                <Button
                    label={settingsText.touchIdTurnOff}
                    kind="quiet"
                    handler={handlers.touchIdDisable}
                    disabled={touchId.busy} />
            </div>
        {:else if touchId.status?.capable}
            <NoteComponent text={settingsText.touchIdCapable} />
            <div class="row">
                <Button label={settingsText.touchIdTurnOn} handler={handlers.touchIdEnable} disabled={touchId.busy} />
            </div>
        {/if}
        <NoteComponent text={touchId.note} />

        <h3>{settingsText.exportTitle}</h3>
        <NoteComponent text={settingsText.exportIntro} />
        <Input field={fields.exportPass} bind:value={exp.pass} />
        <div class="row">
            {#each actions.export as action (action)}
                <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
            {/each}
        </div>
        {#each settingsText.exportNotes as text}
            <NoteComponent {text} />
        {/each}
        <NoteComponent text={exp.note} />

        <h3>{settingsText.importTitle}</h3>
        <NoteComponent text={settingsText.importIntro} />
        <div class="row">
            {#each actions.import as action (action)}
                <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
            {/each}
        </div>
        <NoteComponent text={imp.note} />

        <h3>{settingsText.syncTitle}</h3>
        <NoteComponent text={settingsText.syncIntro} />
        <p>
            {settingsText.syncFolder}
            {#if general.settings.sync_location}
                <strong>{general.settings.sync_location}</strong>
            {:else}
                <span class="note">{settingsText.notSet}</span>
            {/if}
        </p>
        <div class="row">
            {#each actions.syncFolder as action (action)}
                <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
            {/each}
        </div>
        {#if general.settings.sync_location && sync.info}
            {#if sync.info.ack_ahead_seen}
                <ErrorComponent text={settingsText.ackAhead}>
                    <Button
                        label={settingsText.dismiss}
                        kind="quiet"
                        handler={handlers.dismissAckAhead}
                        disabled={sync.busy} />
                </ErrorComponent>
            {/if}
            {#each sync.info.peers as peer}
                {#if peer.needs_reemit}
                    <p class="note">
                        Device <code>{peer.device_id.slice(-8)}…</code> needs to re-emit everything, or its earlier bundles
                        are still on their way.
                    </p>
                {/if}
            {/each}
            <p class="note">This device: <code>{sync.info.device_id}</code></p>
            {#each sync.info.peers as peer}
                <p class="note">
                    Peer <code>{peer.device_id.slice(-8)}…</code>: their changes incorporated through
                    {peer.applied_lseq}{#if peer.waiting > 0}
                        — {peer.waiting} bundle{peer.waiting === 1 ? '' : 's'} waiting{/if}
                </p>
            {/each}
            <div class="row">
                {#each actions.syncOps as action (action)}
                    <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
                {/each}
            </div>
            {#if sync.pruneArm}
                {#each settingsText.pruneNotes as text}
                    <NoteComponent {text} />
                {/each}
            {/if}
        {/if}
        <NoteComponent text={sync.note} />
        {#each sync.problems as problem}
            <ErrorComponent text={problem} />
        {/each}
        {#each sync.warnings as warning}
            <WarningComponent text={warning} />
        {/each}
        {#if sync.ackAhead && !sync.info?.ack_ahead_seen}
            <ErrorComponent text={sync.ackAhead} />
        {/if}
        <NoteComponent text={settingsText.restampIntro} />
        <div class="row">
            {#each actions.restamp as action (action)}
                <Button {...ACTIONS[action]} handler={handlers[action]} disabled={disabled[action]} />
            {/each}
        </div>
        <NoteComponent text={sync.restampNote} />
    {:else if !general.error}
        <NoteComponent text={settingsText.loading} />
    {/if}

    <NoteComponent text={general.note} />
    <ErrorComponent text={general.error} />

    <Button label={settingsText.back} kind="quiet" handler={() => onBack()} />
</div>

<style src="./styles.css"></style>
