// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

/** Local calendar date as YYYY-MM-DD, the format the API takes. */
export function todayISO(now = new Date()) {
    const pad = (n) => String(n).padStart(2, '0');
    return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}
