<script>
    import Button from '../Button/Button.svelte';
    import Input from '../Input/Input.svelte';
    import Select from '../Select/Select.svelte';
    import ErrorComponent from '../ErrorComponent/ErrorComponent.svelte';
    import NoteComponent from '../NoteComponent/NoteComponent.svelte';
    import WarningComponent from '../WarningComponent/WarningComponent.svelte';

    let {
        general = $bindable(),
        security = $bindable(),
        timeoutMin = $bindable(),
        exp = $bindable(),
        touchId,
        imp,
        sync,
        copy,
        fields,
        actions,
        handlers,
        onBack,
    } = $props();
</script>
<style src="./Settings.css"></style>

<div class="settings">
    <h2>{copy.title}</h2>

    {#if general.settings}
        <p>
            {copy.vaultLocation}
            <strong>{general.settings.vault_location ?? '—'}</strong>
        </p>

        <Input field={fields.timeout} bind:value={timeoutMin} />
        {#each actions.timeout as action (action.name)}
            <Button {...action} />
        {/each}

        <label>
            {copy.stripExif}
            <input type="checkbox" bind:checked={general.settings.strip_exif} onchange={handlers.saveStrip} />
        </label>

        {#if general.settings?.platform === 'macos'}
            <label>
                {copy.clipboardClear}
                <input type="checkbox" bind:checked={general.settings.clipboard_clear} onchange={handlers.saveClipboard} />
            </label>
        {/if}

        <p>
            {copy.backupLocation}
            {#if general.settings.backup_location}
                <strong>{general.settings.backup_location}</strong>
            {:else}
                <span class="note">{copy.notSet}</span>
            {/if}
        </p>
        <div class="row">
            {#each actions.backupFolder as action (action.name)}
                <Button {...action} />
            {/each}
        </div>

        <Select field={fields.schedule} bind:value={general.settings.backup_schedule} onchange={handlers.saveSchedule} />

        <div class="row">
            {#each actions.backupNow as action (action.name)}
                <Button {...action} />
            {/each}
        </div>
        {#each copy.backupNotes as text}
            <NoteComponent {text} />
        {/each}

        <h3>{copy.securityTitle}</h3>

        {#each fields.security as field (field.name)}
            <Input {field} bind:value={security[field.name]} />
        {/each}

        <div class="row">
            {#each actions.security as action (action.name)}
                <Button {...action} />
            {/each}
        </div>

        <div class="row">
            {#each actions.rotate as action (action.name)}
                <Button {...action} />
            {/each}
        </div>

        {#if security.rotating}
            <p class="note progress">
                <span class="spinner"></span>
                <span>{copy.rotating}</span>
            </p>
        {/if}

        {#if security.newRecoveryKey}
            <p>{copy.newKeyLead} <code>{security.newRecoveryKey}</code></p>
            <NoteComponent text={copy.newKeyNote} />
        {/if}
        <NoteComponent text={security.note} />

        {#each copy.securityNotes as text}
            <NoteComponent {text} />
        {/each}

        {#if touchId.status?.capable || touchId.status?.enrolled}<h3>{copy.touchIdTitle}</h3>{/if}
        {#if touchId.status?.enrolled && !touchId.status.capable}
            <NoteComponent
                text={`${copy.touchIdEnrolledNotCapableLead} ${touchId.status.built_in ? copy.touchIdNotSignedProfile : copy.touchIdNotBuilt}${copy.touchIdEnrolledNotCapableTail}`} />
            <div class="row">
                <Button label={copy.touchIdRemoveEnrollment} kind="quiet" handler={handlers.touchIdDisable} disabled={touchId.busy} />
            </div>
        {:else if touchId.status && !touchId.status.capable && touchId.status.built_in}
            <NoteComponent text={copy.touchIdNotSigned} />
        {:else if touchId.status?.enrolled}
            <NoteComponent text={copy.touchIdEnrolled} />
            <div class="row">
                <Button label={copy.touchIdTurnOff} kind="quiet" handler={handlers.touchIdDisable} disabled={touchId.busy} />
            </div>
        {:else if touchId.status?.capable}
            <NoteComponent text={copy.touchIdCapable} />
            <div class="row">
                <Button label={copy.touchIdTurnOn} handler={handlers.touchIdEnable} disabled={touchId.busy} />
            </div>
        {/if}
        <NoteComponent text={touchId.note} />

        <h3>{copy.exportTitle}</h3>
        <NoteComponent text={copy.exportIntro} />
        <Input field={fields.exportPass} bind:value={exp.pass} />
        <div class="row">
            {#each actions.export as action (action.name)}
                <Button {...action} />
            {/each}
        </div>
        {#each copy.exportNotes as text}
            <NoteComponent {text} />
        {/each}
        <NoteComponent text={exp.note} />

        <h3>{copy.importTitle}</h3>
        <NoteComponent text={copy.importIntro} />
        <div class="row">
            {#each actions.import as action (action.name)}
                <Button {...action} />
            {/each}
        </div>
        <NoteComponent text={imp.note} />

        <h3>{copy.syncTitle}</h3>
        <NoteComponent text={copy.syncIntro} />
        <p>
            {copy.syncFolder}
            {#if general.settings.sync_location}
                <strong>{general.settings.sync_location}</strong>
            {:else}
                <span class="note">{copy.notSet}</span>
            {/if}
        </p>
        <div class="row">
            {#each actions.syncFolder as action (action.name)}
                <Button {...action} />
            {/each}
        </div>
        {#if general.settings.sync_location && sync.info}
            {#if sync.info.ack_ahead_seen}
                <ErrorComponent text={copy.ackAhead}>
                    <Button label={copy.dismiss} kind="quiet" handler={handlers.dismissAckAhead} disabled={sync.busy} />
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
                {#each actions.syncOps as action (action.name)}
                    <Button {...action} />
                {/each}
            </div>
            {#if sync.pruneArm}
                {#each copy.pruneNotes as text}
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
        <NoteComponent text={copy.restampIntro} />
        <div class="row">
            {#each actions.restamp as action (action.name)}
                <Button {...action} />
            {/each}
        </div>
        <NoteComponent text={sync.restampNote} />
    {:else if !general.error}
        <NoteComponent text={copy.loading} />
    {/if}

    <NoteComponent text={general.note} />
    <ErrorComponent text={general.error} />

    <Button label={copy.back} kind="quiet" handler={() => onBack()} />
</div>
