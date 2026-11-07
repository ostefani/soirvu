// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
import { test } from "node:test";
import assert from "node:assert/strict";

import { nextCursorAfterPage } from "../list.js";

test("a failed reset invalidates the cursor until a reset succeeds", () => {
    // The reported repro: the first page fails under the new
    // scope, then "More…" must not become possible on the old
    // cursor — pagination stays disabled and the error stands
    // until a reset succeeds.
    assert.equal(
        nextCursorAfterPage({ reset: true, failed: true, pageCursor: null, current: "old-cursor" }),
        null,
    );
    // A later successful reset restores pagination.
    assert.equal(
        nextCursorAfterPage({ reset: true, failed: false, pageCursor: "new-cursor", current: null }),
        "new-cursor",
    );
    // A failed next page keeps its cursor: retrying the same
    // position under the same filter is still correct.
    assert.equal(
        nextCursorAfterPage({ reset: false, failed: true, pageCursor: null, current: "c1" }),
        "c1",
    );
    // A successful next page advances to its own cursor.
    assert.equal(
        nextCursorAfterPage({ reset: false, failed: false, pageCursor: "c2", current: "c1" }),
        "c2",
    );
});
