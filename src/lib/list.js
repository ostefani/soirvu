// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

/**
 * Which cursor pagination may use after a page settles. A failed
 * first page invalidates the cursor: it still names the previous
 * filter's position, so "More…" must stay impossible until a reset
 * succeeds — otherwise the old cursor submits under the new scope,
 * appending foreign rows and clearing the failure. A failed next
 * page keeps its cursor: retrying the same position is still
 * correct. Success always takes the page's cursor.
 * @param {{ reset: boolean, failed: boolean, pageCursor: string | null, current: string | null }} args
 * @returns {string | null} the cursor pagination may use, if any.
 */
export function nextCursorAfterPage({ reset, failed, pageCursor, current }) {
    if (failed) return reset ? null : current;
    return pageCursor;
}
