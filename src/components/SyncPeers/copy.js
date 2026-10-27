/**
 * Wording used by SyncPeers. Swap the whole object (the `copy` prop) to
 * localise it; functions receive the values that vary.
 *
 * @typedef {{
 *   thisDevice: string,
 *   peer: string,
 *   incorporatedThrough: string,
 *   waiting: (count: number) => string,
 *   reemit: (shortId: string) => string,
 * }} SyncPeersCopy
 */

/** @type {SyncPeersCopy} */
export const DEFAULT_COPY = {
    thisDevice: 'This device:',
    peer: 'Peer',
    incorporatedThrough: 'changes incorporated through',
    waiting: (count) => `(${count} bundle${count === 1 ? '' : 's'} waiting)`,
    reemit: (shortId) =>
        `Device ${shortId}… needs to re-emit everything, or its earlier bundles are still on their way.`,
};
