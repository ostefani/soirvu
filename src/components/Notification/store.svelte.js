import { EVENT_TYPE } from '@/constants.js';

/** Reactive list rendered by the stack, newest first. Mutated in place, never reassigned. */
export const notificationItems = $state([]);

/** Timers are not UI state, so a plain Map is correct. */
const timers = new Map();

const AUTO_DISMISS_MS = 6000;
const MAX_VISIBLE = 5;
const STICKY_EVENTS = new Set([EVENT_TYPE.ERROR, EVENT_TYPE.WARNING]);

/**
 * Push a notification onto the stack. Every call adds a new item.
 * Pass `key` only to update an existing item in place (e.g. a progress toast).
 * Errors and warnings stay until dismissed; the rest auto-dismiss.
 * Pass `duration: 0` to make any notification sticky.
 */
export function notify(
    event,
    message,
    { key, title = '', type = null, duration, dismissible = true, onDismiss = null } = {},
) {
    const id = key ?? crypto.randomUUID();
    const ms = duration ?? (STICKY_EVENTS.has(event) ? 0 : AUTO_DISMISS_MS);
    const fields = { event, type, title, message, duration: ms, dismissible, onDismiss };

    const existing = key ? notificationItems.find((item) => item.id === id) : undefined;
    if (existing) {
        Object.assign(existing, fields);
    } else {
        notificationItems.unshift({ id, ...fields });
        // Oldest items are at the end; drop them so the stack never outgrows the screen.
        while (notificationItems.length > MAX_VISIBLE) {
            dismiss(notificationItems[notificationItems.length - 1].id);
        }
    }

    clearTimeout(timers.get(id));
    if (ms > 0) timers.set(id, setTimeout(() => dismiss(id), ms));
    return id;
}

export function dismiss(id) {
    clearTimeout(timers.get(id));
    timers.delete(id);
    const index = notificationItems.findIndex((item) => item.id === id);
    if (index !== -1) notificationItems.splice(index, 1);
}

export function clearNotifications() {
    timers.forEach(clearTimeout);
    timers.clear();
    notificationItems.length = 0;
}
