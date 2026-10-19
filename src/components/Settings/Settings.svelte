<script>
    import Button from '../Button/Button.svelte';
    import Input from '../Input/Input.svelte';
    import Select from '../Select/Select.svelte';

    let { model = $bindable(), fields, actions, handlers, onBack } = $props();
</script>
<style src="./Settings.css"></style>

<div class="settings">
    <h2>Settings</h2>

    {#if model.settings}
        <p>
            This diary lives in:
            <strong>{model.settings.vault_location ?? '—'}</strong>
        </p>

        <Input field={fields.timeout} bind:value={model.timeoutMin} />
        {#each actions.timeout as action (action.name)}
            <Button {...action} />
        {/each}

        <label>
            Strip location metadata (EXIF) from photos on import
            <input type="checkbox" bind:checked={model.stripExif} onchange={handlers.saveStrip} />
        </label>

        {#if model.settings?.platform === 'macos'}
            <label>
                Clear the clipboard 30 seconds after copying from the diary
                <input type="checkbox" bind:checked={model.clipboardClear} onchange={handlers.saveClipboard} />
            </label>
        {/if}

        <p>
            Backup location:
            {#if model.settings.backup_location}
                <strong>{model.settings.backup_location}</strong>
            {:else}
                <span class="note">not set</span>
            {/if}
        </p>
        <div class="row">
            {#each actions.backupFolder as action (action.name)}
                <Button {...action} />
            {/each}
        </div>

        <Select field={fields.schedule} bind:value={model.backupSchedule} onchange={handlers.saveSchedule} />

        <div class="row">
            {#each actions.backupNow as action (action.name)}
                <Button {...action} />
            {/each}
        </div>
        <p class="note">
            A backup is an encrypted copy of this vault (the newest 7 are kept). To restore one, choose its folder from
            the lock screen.
        </p>
        <h3>Passphrase and recovery key</h3>

        {#each fields.security as field (field.name)}
            <Input {field} bind:value={model[field.name]} />
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

        {#if model.rotating}
            <p class="note progress">
                <span class="spinner"></span>
                <span>
                    Changing the master key… a fresh copy of the diary is being built and checked. This can take a while
                    for a large diary — don't close the app.
                </span>
            </p>
        {/if}

        {#if model.newRecoveryKey}
            <p>Your new recovery key: <code>{model.newRecoveryKey}</code></p>
            <p class="note">Save it now — it is shown once, and the old key no longer works.</p>
        {/if}
        {#if model.secNote}<p class="note">{model.secNote}</p>{/if}

        <p class="note">
            Only the diary's header changes — nothing is re-encrypted and the diary stays open. Backups made earlier
            keep the passphrase and recovery key they were made with.
        </p>
        {#if model.touchId?.capable || model.touchId?.enrolled}<h3>Touch ID</h3>{/if}
        {#if model.touchId?.enrolled && !model.touchId.capable}
            <p class="note">
                This diary has Touch ID enrolled, but this build cannot use it — it
                {model.touchId.built_in ? 'is not signed with the keychain profile' : 'was made without Touch ID'}. The
                passphrase and recovery key work as always. You can remove the enrollment here; removing it needs no
                keychain, only the diary's header changes.
            </p>
            <div class="row">
                <Button label="Remove Touch ID enrollment" kind="quiet" handler={handlers.touchIdDisable} disabled={model.tidBusy} />
            </div>
        {:else if model.touchId && !model.touchId.capable && model.touchId.built_in}
            <p class="note">
                Touch ID needs this app signed with a development profile that carries the keychain entitlement. This
                build is not signed that way, so Touch ID is unavailable — the passphrase and recovery key work as
                always.
            </p>
        {:else if model.touchId?.enrolled}
            <p class="note">This diary can be unlocked with Touch ID on this Mac.</p>
            <div class="row">
                <Button label="Turn off Touch ID" kind="quiet" handler={handlers.touchIdDisable} disabled={model.tidBusy} />
            </div>
        {:else if model.touchId?.capable}
            <p class="note">
                Unlock this diary with your fingerprint instead of typing the passphrase. The passphrase and recovery
                key keep working — Touch ID is one more key, never the only one.
            </p>
            <div class="row">
                <Button label="Turn on Touch ID" handler={handlers.touchIdEnable} disabled={model.tidBusy} />
            </div>
        {/if}
        {#if model.tidNote}<p class="note">{model.tidNote}</p>{/if}

        <h3>Export</h3>
        <p class="note">
            Export writes the diary as plain, unencrypted files. You will be asked to confirm, to type your passphrase
            again, and to pick an empty folder.
        </p>
        <Input field={fields.exportPass} bind:value={model.expPass} />
        <div class="row">
            {#each actions.export as action (action.name)}
                <Button {...action} />
            {/each}
        </div>
        <p class="note">
            Markdown is for reading and moving to other apps. JSON keeps everything — ids, deleted entries — and is made
            for importing back into Soirvu.
        </p>
        {#if model.expNote}<p class="note">{model.expNote}</p>{/if}

        <h3>Import</h3>
        <p class="note">
            Import adds entries to this diary — nothing is overwritten or merged. A Markdown folder can be a Soirvu
            export or notes from another app. A JSON file must be a Soirvu export; entries whose id already exists are
            skipped.
        </p>
        <div class="row">
            {#each actions.import as action (action.name)}
                <Button {...action} />
            {/each}
        </div>
        {#if model.impNote}<p class="note">{model.impNote}</p>{/if}

        <h3>Sync</h3>
        <p class="note">
            Sync writes this diary's changes as encrypted bundles into a folder. Another tool — Syncthing, a cloud
            folder, a USB stick — carries the folder between your devices; Soirvu itself never uses the network.
        </p>
        <p>
            Sync folder:
            {#if model.settings.sync_location}
                <strong>{model.settings.sync_location}</strong>
            {:else}
                <span class="note">not set</span>
            {/if}
        </p>
        <div class="row">
            {#each actions.syncFolder as action (action.name)}
                <Button {...action} />
            {/each}
        </div>
        {#if model.settings.sync_location && model.syncInfo}
            {#if model.syncInfo.ack_ahead_seen}
                <p class="error">
                    An ack in this folder has claimed more than this vault holds — a copy may share its identity, or
                    this vault was restored. Entries a peer pruned on the strength of it return when that peer re-emits
                    everything. If THIS diary is the copy, restamp it below; if it is the original, the copy is the one
                    to restamp. Once the missing entries are back, dismiss this note.
                    <Button label="Dismiss" kind="quiet" handler={handlers.dismissAckAhead} disabled={model.syncBusy} />
                </p>
            {/if}
            {#each model.syncInfo.peers as peer}
                {#if peer.needs_reemit}
                    <p class="note">
                        Device <code>{peer.device_id.slice(-8)}…</code> needs to re-emit everything, or its earlier bundles
                        are still on their way.
                    </p>
                {/if}
            {/each}
            <p class="note">This device: <code>{model.syncInfo.device_id}</code></p>
            {#each model.syncInfo.peers as peer}
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
            {#if model.pruneArm}
                <p class="note">
                    Prune removes the bundles and blobs every known peer has already consumed. A device that uses this
                    folder but has not synced yet — a fresh copy, for instance — can lose changes it never received.
                    Prune only once every device has synced.
                </p>
            {/if}
        {/if}
        {#if model.syncNote}<p class="note">{model.syncNote}</p>{/if}
        {#each model.syncProblems as problem}
            <p class="error">{problem}</p>
        {/each}
        {#each model.syncWarnings as warning}
            <p class="warning">{warning}</p>
        {/each}
        {#if model.syncAckAhead && !model.syncInfo?.ack_ahead_seen}
            <p class="error">{model.syncAckAhead}</p>
        {/if}
        <p class="note">
            If this diary is a copy of a folder from another device, both copies share one device identity and cannot
            sync with each other. Restamping gives THIS copy a new identity — the other device is untouched, and this
            copy's changes are emitted again under the new identity.
        </p>
        <div class="row">
            {#each actions.restamp as action (action.name)}
                <Button {...action} />
            {/each}
        </div>
        {#if model.restampNote}<p class="note">{model.restampNote}</p>{/if}
    {:else if !model.error}
        <p class="note">Loading…</p>
    {/if}

    {#if model.note}<p class="note">{model.note}</p>{/if}
    {#if model.error}<p class="error">{model.error}</p>{/if}

    <Button label="Back to the diary" kind="quiet" handler={() => onBack()} />
</div>
