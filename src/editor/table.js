// Table editing (Phase 7, §6.5) — our own commands over the
// table nodes from schema.js. A cell holds exactly one
// paragraph, so Enter and Tab are navigation, never splits:
// Enter goes to the cell below (adding a row at the end),
// Tab walks the cells. Structural changes rebuild the table
// node in one replacement, which keeps each change a single
// undo step.

import { TextSelection } from "./state.js";

/** The table around the cursor, with absolute positions of
 *  every row and cell — or null when the cursor is elsewhere. */
export function tableContext(state) {
    const sel = state.selection;
    if (sel.type !== "text") return null;
    const doc = state.doc;
    const $from = doc.resolve(sel.from);
    let tableDepth = -1;
    for (let d = $from.depth; d > 0; d--) {
        if ($from.node(d).type.name === "table") {
            tableDepth = d;
            break;
        }
    }
    if (tableDepth < 0) return null;
    const table = $from.node(tableDepth);
    const tablePos = $from.before(tableDepth);
    const rows = [];
    let rowIdx = -1;
    let colIdx = -1;
    let offset = 0;
    table.forEach((row, _o, r) => {
        const rowPos = tablePos + 1 + offset;
        offset += row.nodeSize;
        const cells = [];
        let cellOffset = 0;
        row.forEach((cell, _o2, c) => {
            const cellPos = rowPos + 1 + cellOffset;
            cellOffset += cell.nodeSize;
            cells.push({ pos: cellPos, node: cell });
            if (sel.from > cellPos && sel.from < cellPos + cell.nodeSize) {
                rowIdx = r;
                colIdx = c;
            }
        });
        rows.push({ pos: rowPos, node: row, cells });
    });
    if (rowIdx < 0) return null;
    return { table, tablePos, rows, rowIdx, colIdx };
}

function cellCursor(ctx, r, c) {
    // Table border, row border, cell border, paragraph border.
    return ctx.rows[r].cells[c].pos + 2;
}

function moveToCell(state, dispatch, ctx, r, c) {
    const tr = state.tr();
    tr.setSelection(new TextSelection(cellCursor(ctx, r, c)));
    if (dispatch) dispatch(tr);
    return true;
}

function emptyCell(schema, header, align) {
    return schema.nodes.table_cell.create({ header, align }, schema.nodes.paragraph.create());
}

function rebuildTable(state, dispatch, ctx, rows, selRow, selCol) {
    const tr = state.tr();
    const table = state.schema.nodes.table.create(null, rows);
    tr.replaceWith(ctx.tablePos, ctx.tablePos + ctx.table.nodeSize, [table]);
    const next = { rows: [] };
    let offset = 0;
    table.forEach((row, _o, r) => {
        const rowPos = ctx.tablePos + 1 + offset;
        offset += row.nodeSize;
        const cells = [];
        let cellOffset = 0;
        row.forEach((cell, _o2, c) => {
            cells.push({ pos: rowPos + 1 + cellOffset, node: cell });
            cellOffset += cell.nodeSize;
        });
        next.rows.push({ cells });
    });
    const r = Math.min(selRow, next.rows.length - 1);
    const c = Math.min(selCol, next.rows[r].cells.length - 1);
    tr.setSelection(new TextSelection(next.rows[r].cells[c].pos + 2));
    if (dispatch) dispatch(tr);
    return true;
}

/** Tab / Shift-Tab inside a table: walk the cells. At the end
 *  Tab adds a row; at the start Shift-Tab leaves the table. */
export function tabInTable(dir) {
    return (state, dispatch) => {
        const ctx = tableContext(state);
        if (!ctx) return false;
        const cols = ctx.rows[0].cells.length;
        let r = ctx.rowIdx;
        let c = ctx.colIdx + dir;
        if (c >= cols) {
            r += 1;
            c = 0;
        }
        if (c < 0) {
            r -= 1;
            c = cols - 1;
        }
        if (r >= ctx.rows.length) {
            if (dir < 0) return false;
            return addRow(state, dispatch);
        }
        if (r < 0) return false;
        return moveToCell(state, dispatch, ctx, r, c);
    };
}

/** Enter inside a cell: the cell below, or a fresh row. */
export function enterInTable(state, dispatch) {
    const ctx = tableContext(state);
    if (!ctx) return false;
    if (ctx.rowIdx + 1 < ctx.rows.length) {
        return moveToCell(state, dispatch, ctx, ctx.rowIdx + 1, ctx.colIdx);
    }
    return addRow(state, dispatch);
}

function addRow(state, dispatch) {
    const ctx = tableContext(state);
    if (!ctx) return false;
    const schema = state.schema;
    const headerRow = ctx.rows[0];
    const newRow = schema.nodes.table_row.create(
        null,
        headerRow.cells.map((cell) => emptyCell(schema, false, cell.node.attrs.align)),
    );
    const rows = ctx.rows.map((r) => r.node);
    rows.splice(ctx.rowIdx + 1, 0, newRow);
    return rebuildTable(state, dispatch, ctx, rows, ctx.rowIdx + 1, ctx.colIdx);
}

export function addRowCommand(state, dispatch) {
    const ctx = tableContext(state);
    if (!ctx) return false;
    return addRow(state, dispatch);
}

export function addColumnCommand(state, dispatch) {
    const ctx = tableContext(state);
    if (!ctx) return false;
    const schema = state.schema;
    const rows = ctx.rows.map((r, ri) => {
        const cells = r.cells.map((c) => c.node);
        cells.splice(
            ctx.colIdx + 1,
            0,
            emptyCell(schema, ri === 0, null),
        );
        return schema.nodes.table_row.create(null, cells);
    });
    return rebuildTable(state, dispatch, ctx, rows, ctx.rowIdx, ctx.colIdx + 1);
}

export function deleteRowCommand(state, dispatch) {
    const ctx = tableContext(state);
    if (!ctx) return false;
    if (ctx.rows.length === 1) return deleteTable(state, dispatch, ctx);
    const rows = ctx.rows.map((r) => r.node);
    rows.splice(ctx.rowIdx, 1);
    return rebuildTable(
        state,
        dispatch,
        ctx,
        rows,
        Math.min(ctx.rowIdx, rows.length - 1),
        ctx.colIdx,
    );
}

export function deleteColumnCommand(state, dispatch) {
    const ctx = tableContext(state);
    if (!ctx) return false;
    if (ctx.rows[0].cells.length === 1) return deleteTable(state, dispatch, ctx);
    const schema = state.schema;
    const rows = ctx.rows.map((r) => {
        const cells = r.cells.map((c) => c.node);
        cells.splice(ctx.colIdx, 1);
        return schema.nodes.table_row.create(null, cells);
    });
    return rebuildTable(
        state,
        dispatch,
        ctx,
        rows,
        ctx.rowIdx,
        Math.min(ctx.colIdx, rows[0].childCount - 1),
    );
}

function deleteTable(state, dispatch, ctx) {
    const tr = state.tr();
    tr.replaceWith(ctx.tablePos, ctx.tablePos + ctx.table.nodeSize, [
        state.schema.nodes.paragraph.create(),
    ]);
    tr.setSelection(new TextSelection(ctx.tablePos + 1));
    if (dispatch) dispatch(tr);
    return true;
}

/** Inserts a 3×3 table (header row plus two rows) at the
 *  cursor: an empty paragraph is replaced, anything else gets
 *  the table right after its top-level block. */
export function insertTableCommand(state, dispatch) {
    const schema = state.schema;
    const nodes = schema.nodes;
    if (!nodes.table) return false;
    const sel = state.selection;
    if (sel.type !== "text") return false;
    const header = nodes.table_row.create(
        null,
        [0, 1, 2].map(() => emptyCell(schema, true, null)),
    );
    const body = () =>
        nodes.table_row.create(
            null,
            [0, 1, 2].map(() => emptyCell(schema, false, null)),
        );
    const table = nodes.table.create(null, [header, body(), body()]);

    const $from = state.doc.resolve(sel.from);
    const block = $from.node(1);
    const blockPos = $from.before(1);
    const tr = state.tr();
    if (block.type === nodes.paragraph && block.content.size === 0) {
        tr.replaceWith(blockPos, blockPos + block.nodeSize, [table, nodes.paragraph.create()]);
        tr.setSelection(new TextSelection(blockPos + 4));
    } else {
        const after = blockPos + block.nodeSize;
        tr.insert(after, [table, nodes.paragraph.create()]);
        tr.setSelection(new TextSelection(after + 4));
    }
    if (dispatch) dispatch(tr);
    return true;
}
