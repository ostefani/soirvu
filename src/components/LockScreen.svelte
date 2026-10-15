<script>
    // The lock screen: pick a known vault, unlock it with the
    // passphrase or the recovery key. Missing vaults can be located
    // or forgotten here. Error text comes from the bridge's stable
    // codes — never from Rust error prose.
    import * as api from "../api.js";

    let { vaults, onUnlocked, onVaultsChanged, onCreate } = $props();

    let selectedId = $state(null);
    let kind = $state("passphrase");
    let secret = $state("");
    let error = $state("");
    let busy = $state(false);
    let forgetArmed = $state(null);
    let note = $state("");

    const FS_WARNINGS = {
        cloud_sync:
            "That folder is in a cloud-synced location. Sync clients and the diary's database files do not mix — the vault is registered, but keeping it there risks corruption.",
        network_fs:
            "That folder is on a network share. The diary's database is unreliable over network filesystems — the vault is registered, but keeping it there is risky.",
    };

    const selected = $derived(
        vaults.find((v) => v.id === (selectedId ?? vaults[0]?.id)) ?? null,
    );

    function describeError(e) {
        if (typeof e === "string" && e.startsWith("backoff:")) {
            return `Too many wrong attempts. Wait ${e.slice("backoff:".length)} seconds and try again.`;
        }
        return (
            {
                wrong_passphrase: "Wrong passphrase or recovery key.",
                invalid_recovery_key: "That does not look like a recovery key.",
                storage: "The vault folder is not available right now.",
                vault_not_found: "This vault is no longer in the list.",
                vault_mismatch: "That folder holds a different vault.",
                corrupt: "The vault looks damaged. The CLI can check it: soirvu-cli verify.",
            }[e] || `Something went wrong (${e}).`
        );
    }

    async function unlock() {
        if (!selected || !secret) return;
        busy = true;
        error = "";
        try {
            await api.vaultUnlock(selected.id, kind, secret);
            secret = "";
            onUnlocked(selected);
        } catch (e) {
            error = describeError(e);
        } finally {
            busy = false;
        }
    }

    async function relocate(vault) {
        error = "";
        try {
            if (await api.vaultRelocate(vault.id)) onVaultsChanged();
        } catch (e) {
            error = describeError(e);
        }
    }

    async function forget(vault) {
        await api.vaultForget(vault.id);
        forgetArmed = null;
        onVaultsChanged();
    }

    async function openAnother() {
        error = "";
        note = "";
        try {
            const picked = await api.vaultPickFolder("open");
            if (picked) {
                selectedId = picked.vault_id;
                if (picked.warning) note = FS_WARNINGS[picked.warning] || "";
                onVaultsChanged();
            }
        } catch (e) {
            error = describeError(e);
        }
    }
</script>

<div class="center-col">
    <h1>Soirvu</h1>

    <div class="vault-list">
        {#each vaults as vault (vault.id)}
            <div class="vault-row">
                <button
                    class="vault-name"
                    class:active={selected?.id === vault.id}
                    onclick={() => (selectedId = vault.id)}
                >
                    {vault.name}
                    {#if vault.state === "not_found"}<span class="missing"> — folder not found</span>{/if}
                    {#if vault.state === "volume_missing"}<span class="missing"> — volume not mounted</span>{/if}
                </button>
                {#if vault.state === "not_found"}
                    <button onclick={() => relocate(vault)}>Locate…</button>
                {/if}
                {#if vault.state === "volume_missing"}
                    <button onclick={() => onVaultsChanged()}>Retry</button>
                {/if}
                {#if forgetArmed === vault.id}
                    <button onclick={() => forget(vault)}>Really forget?</button>
                {:else}
                    <button class="quiet" onclick={() => (forgetArmed = vault.id)}>Forget</button>
                {/if}
            </div>
        {/each}
    </div>

    {#if selected && selected.state === "available"}
        <div class="kind-toggle">
            <button class:active={kind === "passphrase"} onclick={() => (kind = "passphrase")}>
                Passphrase
            </button>
            <button class:active={kind === "recovery"} onclick={() => (kind = "recovery")}>
                Recovery key
            </button>
        </div>
        <input
            type="password"
            placeholder={kind === "passphrase" ? "Passphrase" : "XXXX-XXXX-…"}
            bind:value={secret}
            onkeydown={(e) => e.key === "Enter" && unlock()}
        />
        <button onclick={unlock} disabled={busy || !secret}>Unlock</button>
    {/if}

    <button class="quiet" onclick={openAnother}>Open another diary folder…</button>
    <button class="quiet" onclick={() => onCreate()}>Create a new diary…</button>

    {#if note}<p class="note">{note}</p>{/if}
    {#if error}<p class="error">{error}</p>{/if}
</div>

<style>
    /* The vault rows and the passphrase/recovery toggle. */
    .vault-list {
        display: flex;
        flex-direction: column;
        gap: 0.4rem;
    }

    .vault-row {
        display: flex;
        gap: 0.4rem;
        align-items: center;
    }

    .vault-name {
        flex: 1;
        text-align: left;
    }

    .missing {
        opacity: 0.6;
        font-size: 0.85em;
    }

    .kind-toggle {
        display: flex;
        gap: 0.4rem;
    }

    .kind-toggle button {
        flex: 1;
    }
</style>
