<script>
    import NoteComponent from '@/components/NoteComponent/index.svelte';
    import { DEFAULT_COPY } from './copy.js';

    /**
     * This device's id and what is known about each peer.
     *
     * @type {{
     *   deviceId: string,
     *   peers?: Array<{
     *     device_id: string,
     *     applied_lseq: number,
     *     waiting: number,
     *     needs_reemit?: boolean,
     *   }>,
     *   copy?: import('./copy.js').SyncPeersCopy,
     * }}
     */
    let { deviceId, peers = [], copy = DEFAULT_COPY } = $props();

    /** @param {string} id */
    const shortId = (id) => id.slice(-8);
</script>

<div class="sync-peers-list">
    <p class="sync-device-id">{copy.thisDevice} <code>{deviceId}</code></p>
    {#each peers ?? [] as peer}
        <p class="sync-peer-info">
            {copy.peer} <code>{shortId(peer.device_id)}…</code>: {copy.incorporatedThrough}
            {peer.applied_lseq}
            {#if peer.waiting > 0}
                {copy.waiting(peer.waiting)}
            {/if}
        </p>
        {#if peer.needs_reemit}
            <NoteComponent variant="warning" text={copy.reemit(shortId(peer.device_id))} />
        {/if}
    {/each}
</div>

<style src="./styles.css"></style>
