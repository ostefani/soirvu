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
    import EntryEditor from "./components/EntryEditor.svelte";

    let screen = $state("boot"); // boot | welcome | create | locked | diary
    let vaults = $state([]);
    let currentVault = $state(null);
    let selectedId = $state(null);
    let listEl = $state(null);

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
        await refresh();
    });

    async function openExisting() {
        const picked = await api.vaultPickFolder("open");
        if (picked) await refresh();
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
    </div>
{:else if screen === "create"}
    <Onboarding onDone={refresh} />
{:else if screen === "locked"}
    <LockScreen {vaults} onUnlocked={refresh} onVaultsChanged={refresh} />
{:else if screen === "diary"}
    <div class="diary">
        <header class="diary-head">
            <span class="vault-label">{currentVault?.name}</span>
            <button class="quiet" onclick={lock}>Lock</button>
        </header>
        <div class="diary-body">
            <aside>
                <EntryList bind:this={listEl} selectedId={selectedId} onSelect={(id) => (selectedId = id)} />
            </aside>
            <main>
                {#if selectedId}
                    {#key selectedId}
                        <EntryEditor
                            entryId={selectedId}
                            onDeleted={() => {
                                selectedId = null;
                                listEl?.refresh();
                            }}
                            onSaved={() => listEl?.refresh()}
                        />
                    {/key}
                {:else}
                    <p class="note">Pick an entry, or write a new one.</p>
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
