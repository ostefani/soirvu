<script>
    // App settings (Phase 3): the lock timeout and the backup
    // location. The backup folder is picked through Rust — the
    // UI only ever sees its display name, never the path (§3).
    import { onMount } from "svelte";
    import * as api from "../api.js";

    let { onBack } = $props();

    let settings = $state(null);
    let timeoutMin = $state(5);
    let stripExif = $state(true);
    let clipboardClear = $state(false);
    let backupSchedule = $state("off");
    let error = $state("");
    let note = $state("");

    onMount(async () => {
        try {
            settings = await api.settingsGet();
            timeoutMin = Math.max(1, Math.round(settings.lock_timeout_secs / 60));
            stripExif = settings.strip_exif;
            clipboardClear = settings.clipboard_clear;
            backupSchedule = settings.backup_schedule;
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    });

    async function saveTimeout() {
        error = "";
        note = "";
        try {
            const next = { ...settings, lock_timeout_secs: Math.max(1, Math.round(timeoutMin)) * 60 };
            await api.settingsSet(next);
            settings = await api.settingsGet();
            note = "Saved.";
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    }

    async function saveStrip() {
        error = "";
        note = "";
        try {
            await api.settingsSet({ ...settings, strip_exif: stripExif });
            settings = await api.settingsGet();
            note = "Saved.";
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    }

    async function saveClipboard() {
        error = "";
        note = "";
        try {
            await api.settingsSet({ ...settings, clipboard_clear: clipboardClear });
            settings = await api.settingsGet();
            note = "Saved.";
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    }

    async function saveSchedule() {
        error = "";
        note = "";
        try {
            await api.settingsSet({ ...settings, backup_schedule: backupSchedule });
            settings = await api.settingsGet();
            note = "Saved.";
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    }

    async function backUpNow() {
        error = "";
        note = "";
        try {
            const name = await api.backupNow();
            note = `Backed up to ${name}.`;
        } catch (e) {
            error =
                e === "backup_unavailable"
                    ? "The backup folder is not available."
                    : `Something went wrong (${e}).`;
        }
    }

    async function pickBackup() {
        error = "";
        note = "";
        try {
            const updated = await api.settingsPickBackupDir();
            if (updated) settings = updated;
        } catch (e) {
            error = `Something went wrong (${e}).`;
        }
    }

    async function clearBackup() {
        error = "";
        note = "";
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
            <strong>{settings.vault_location ?? "—"}</strong>
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

        <label>
            Clear the clipboard 30 seconds after copying from the diary
            <input type="checkbox" bind:checked={clipboardClear} onchange={saveClipboard} />
        </label>

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
            A backup is an encrypted copy of this vault (the newest 7 are kept). To restore one,
            choose its folder from the lock screen.
        </p>
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
</style>
