<script>
    // The shell: which screen shows follows the Rust lock state,
    // never local guesses. A vault-status event saying anything
    // but "unlocked" (manual lock, idle lock) tears the diary down
    // and returns to the lock screen; the editor component's own
    // cleanup saves its tail first.
    import { onMount } from "svelte";
    import * as api from "./api.js";
    import Onboarding from "./components/Onboarding.svelte";
    import LockScreen from "./components/LockScreen.svelte";
    import EntryList from "./components/EntryList.svelte";
    import Calendar from "./components/Calendar.svelte";
    import EntryEditor from "./components/EntryEditor.svelte";
    import Settings from "./components/Settings.svelte";

    let screen = $state("boot"); // boot | welcome | create | locked | diary
    let vaults = $state([]);
    let currentVault = $state(null);
    let selectedId = $state(null);
    let listEl = $state(null);
    let showSettings = $state(false);
    let listDate = $state(null);
    let calVersion = $state(0);
    let welcomeNote = $state("");

    async function refresh() {
        vaults = await api.vaultList();
        const open = vaults.find((v) => v.state === "unlocked");
        if (open) {
            currentVault = open;
            screen = "diary";
        } else if (vaults.length === 0) {
            screen = "welcome";
        } else {
            currentVault = null;
            screen = "locked";
        }
    }

    onMount(async () => {
        await api.onVaultStatus(async ({ status }) => {
            if (status !== "unlocked") {
                selectedId = null;
                await refresh();
            }
        });
        // §9 clipboard clearing: report copies; Rust checks the
        // setting and clears after 30 s if the clipboard is
        // untouched since. The content is never read.
        document.addEventListener("copy", () => {
            api.clipboardNoteCopy().catch(() => {});
        });
        // Manual lock (§9): Cmd/Ctrl+Shift+L from anywhere in
        // the diary. The shell lives for the app's lifetime and
        // the document reloads on lock, so no cleanup is needed.
        window.addEventListener("keydown", (e) => {
            if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === "l" && screen === "diary") {
                e.preventDefault();
                lock();
            }
        });
        await refresh();
    });

    const FS_WARNINGS = {
        cloud_sync:
            "That folder is in a cloud-synced location. Sync clients and the diary's database files do not mix — the vault is registered, but keeping it there risks corruption.",
        network_fs:
            "That folder is on a network share. The diary's database is unreliable over network filesystems — the vault is registered, but keeping it there is risky.",
    };

    async function openExisting() {
        welcomeNote = "";
        const picked = await api.vaultPickFolder("open");
        if (picked) {
            if (picked.warning) welcomeNote = FS_WARNINGS[picked.warning] || "";
            await refresh();
        }
    }

    async function lock() {
        await api.vaultLock();
        selectedId = null;
        await refresh();
    }
</script>

{#if screen === "boot"}
    <p class="note">Starting…</p>
{:else if screen === "welcome"}
    <div class="center-col">
        <h1>Soirvu</h1>
        <p>An encrypted diary. It lives in a folder you choose; nothing leaves your machine.</p>
        <button onclick={() => (screen = "create")}>Create a new diary</button>
        <button class="quiet" onclick={openExisting}>Open an existing diary folder…</button>
        {#if welcomeNote}<p class="note">{welcomeNote}</p>{/if}
    </div>
{:else if screen === "create"}
    <Onboarding onDone={refresh} onCancel={vaults.length > 0 ? refresh : undefined} />
{:else if screen === "locked"}
    <LockScreen {vaults} onUnlocked={refresh} onVaultsChanged={refresh} onCreate={() => (screen = "create")} />
{:else if screen === "diary"}
    <div class="diary">
        <header class="diary-head">
            <span class="vault-label">{currentVault?.name}</span>
            <span>
                <button class="quiet" onclick={() => (showSettings = true)}>Settings</button>
                <button class="quiet" onclick={lock}>Lock</button>
            </span>
        </header>
        <div class="diary-body">
            <aside>
                <EntryList
                    bind:this={listEl}
                    selectedId={selectedId}
                    onSelect={(id) => (selectedId = id)}
                    date={listDate}
                    onClearDate={() => (listDate = null)}
                />
            </aside>
            <main>
                {#if showSettings}
                    <Settings onBack={() => (showSettings = false)} />
                {:else if selectedId}
                    {#key selectedId}
                        <EntryEditor
                            entryId={selectedId}
                            onBack={() => (selectedId = null)}
                            onDeleted={() => {
                                selectedId = null;
                                listEl?.refresh();
                                calVersion++;
                            }}
                            onSaved={() => {
                                listEl?.refresh();
                                calVersion++;
                            }}
                        />
                    {/key}
                {:else}
                    <div class="calendar-home">
                        <Calendar large selectedDate={listDate} onSelect={(d) => (listDate = d)} version={calVersion} />
                        <p class="note">Pick a day to see its entries in the list — or pick a recent entry on the left.</p>
                    </div>
                {/if}
            </main>
        </div>
    </div>
{/if}

<style>
    /* The diary shell: header bar and the two-pane body. */
    .diary {
        display: flex;
        flex-direction: column;
        height: 100vh;
    }

    .diary-head {
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: 0.5rem 1rem;
        border-bottom: 1px solid rgba(127, 127, 127, 0.35);
    }

    .vault-label {
        font-weight: 600;
    }

    .diary-body {
        display: flex;
        flex: 1;
        min-height: 0;
    }

    .diary-body aside {
        width: 19rem;
        flex-shrink: 0;
        border-right: 1px solid rgba(127, 127, 127, 0.35);
        overflow-y: auto;
    }

    .diary-body main {
        flex: 1;
        overflow-y: auto;
        padding: 1rem 1.5rem;
    }
</style>
