package com.youki.dex.livewallpaper

/**
 * The launcher desktop's cell-occupancy grid: collision detection,
 * Nova-style "push neighbors out of the way" drag-drop resolution, and
 * pixel<->cell conversion.
 *
 * Reverted from the Rust/JNI experiment (desktop_grid.rs) back to plain
 * Kotlin — this class now owns the occupancy map directly instead of
 * marshaling ints across JNI to a native handle. Every method here is a
 * direct behavioral port of the Rust version (itself a 1:1 port of the
 * original Kotlin), so drag-drop behavior (Nova-style push/cascade) is
 * unchanged: same recursion structure, same depth guard (64), same
 * rollback-on-failure semantics.
 *
 * Usage (from LauncherActivity): create one instance per Activity lifecycle
 * (e.g. in onCreate, matching gridColumns()/gridRows()), call it on every
 * grid mutation, and call [close] in onDestroy (kept for API compatibility
 * with the previous native-handle version; a no-op now since there's no
 * native resource to release).
 */
class DesktopGridManager(val columns: Int, val rows: Int) : AutoCloseable {

    /** A rectangular region of grid cells: (col, row) is the top-left cell, spanning colSpan x rowSpan cells. */
    data class CellRect(
        val col: Int,
        val row: Int,
        val colSpan: Int = 1,
        val rowSpan: Int = 1,
    ) {
        val colEnd: Int get() = col + colSpan - 1
        val rowEnd: Int get() = row + rowSpan - 1

        fun overlaps(other: CellRect): Boolean =
            col <= other.colEnd && colEnd >= other.col &&
            row <= other.rowEnd && rowEnd >= other.row
    }

    /** One resolved item's new on-screen position after a drop. */
    data class ResolvedPlacement(
        val itemId: String,
        val col: Int,
        val row: Int,
        val pixelX: Int,
        val pixelY: Int,
    )

    private enum class PushDirection { LEFT, RIGHT, UP, DOWN }

    // Ordered map (insertion order matters — mirrors the original's
    // LinkedHashMap-backed occupancy grid) so push-resolution order over
    // blockers is deterministic run to run.
    private val occupied = LinkedHashMap<String, CellRect>()
    private var closed = false

    fun clear() {
        check(!closed) { "DesktopGridManager used after close()" }
        occupied.clear()
    }

    fun place(itemId: String, col: Int, row: Int, colSpan: Int = 1, rowSpan: Int = 1) {
        check(!closed) { "DesktopGridManager used after close()" }
        occupied[itemId] = clampToGrid(CellRect(col, row, colSpan, rowSpan))
    }

    fun remove(itemId: String) {
        check(!closed) { "DesktopGridManager used after close()" }
        occupied.remove(itemId)
    }

    fun rectOf(itemId: String): CellRect? = occupied[itemId]

    private fun clampToGrid(rect: CellRect): CellRect {
        val col = rect.col.coerceIn(0, (columns - rect.colSpan).coerceAtLeast(0))
        val row = rect.row.coerceIn(0, (rows - rect.rowSpan).coerceAtLeast(0))
        return rect.copy(col = col, row = row)
    }

    /** Every item whose rect overlaps [rect], excluding [excludeItemId] (typically the item being moved). */
    private fun collisions(rect: CellRect, excludeItemId: String?): List<String> =
        occupied.filter { (id, r) -> id != excludeItemId && r.overlaps(rect) }.keys.toList()

    private fun isFree(rect: CellRect, excludeItemId: String?): Boolean =
        collisions(rect, excludeItemId).isEmpty() &&
        rect.col >= 0 && rect.row >= 0 &&
        rect.colEnd < columns && rect.rowEnd < rows

    /**
     * Attempts to move [itemId] to [targetRect]. If the target is free,
     * moves it directly. If occupied, tries to push every colliding item
     * one step further in [direction] (Nova-style shove) and recursively
     * resolves any chain reaction those pushes cause. Returns true if the
     * move (and any pushes it required) succeeded without anything falling
     * off the grid; false if there wasn't room and nothing was changed
     * (full rollback to the pre-call state).
     */
    private fun moveWithPush(itemId: String, targetRect: CellRect, direction: PushDirection): Boolean {
        val snapshot = LinkedHashMap(occupied)
        val ok = tryPlaceWithPush(itemId, targetRect, direction, 0)
        if (!ok) {
            occupied.clear()
            occupied.putAll(snapshot)
        }
        return ok
    }

    private fun tryPlaceWithPush(itemId: String, targetRect: CellRect, direction: PushDirection, depth: Int): Boolean {
        // Guard against pathological chains (e.g. a fully-packed grid)
        // turning into unbounded recursion — a launcher grid realistically
        // never needs more than a couple dozen cascading pushes to resolve
        // or fail.
        if (depth > 64) return false

        val clamped = clampToGrid(targetRect)
        if (clamped.col != targetRect.col || clamped.row != targetRect.row) {
            // Target rect doesn't fit on the grid at all in this position —
            // reject rather than silently clamping into a different
            // collision than the caller intended.
            if (clamped.colEnd < targetRect.colEnd || clamped.rowEnd < targetRect.rowEnd) {
                return false
            }
        }

        val blockers = collisions(clamped, itemId)
        if (blockers.isEmpty()) {
            occupied[itemId] = clamped
            return true
        }

        // Try to push every blocker one cell further in the drag direction, then re-check.
        for (blockerId in blockers) {
            val blockerRect = occupied[blockerId] ?: continue
            val pushed = when (direction) {
                PushDirection.RIGHT -> blockerRect.copy(col = blockerRect.col + 1)
                PushDirection.LEFT  -> blockerRect.copy(col = blockerRect.col - 1)
                PushDirection.DOWN  -> blockerRect.copy(row = blockerRect.row + 1)
                PushDirection.UP    -> blockerRect.copy(row = blockerRect.row - 1)
            }
            if (!tryPlaceWithPush(blockerId, pushed, direction, depth + 1)) return false
        }
        occupied[itemId] = clamped
        return true
    }

    /** First free rect of the given span, scanning row-major from (0,0) — used for initial/default placement. */
    fun firstFreeRect(colSpan: Int, rowSpan: Int): Pair<Int, Int>? {
        check(!closed) { "DesktopGridManager used after close()" }
        for (r in 0..(rows - rowSpan)) {
            for (c in 0..(columns - colSpan)) {
                val candidate = CellRect(c, r, colSpan, rowSpan)
                if (isFree(candidate, null)) return c to r
            }
        }
        return null
    }

    /**
     * Places [itemId] in ([preferredCol], [preferredRow]) if free, otherwise
     * the first available free cell — used when loading the desktop, so a
     * saved position is honored when possible but never causes an overlap.
     */
    fun placeInFreeCell(itemId: String, preferredCol: Int, preferredRow: Int): Pair<Int, Int> {
        check(!closed) { "DesktopGridManager used after close()" }
        val preferred = CellRect(
            preferredCol.coerceIn(0, columns - 1),
            preferredRow.coerceIn(0, rows - 1),
        )
        val rect = if (isFree(preferred, null)) {
            preferred
        } else {
            firstFreeRect(1, 1)?.let { (c, r) -> CellRect(c, r) } ?: preferred
        }
        occupied[itemId] = clampToGrid(rect)
        return rect.col to rect.row
    }

    // ── pixel<->cell conversion ──────────────────────────────────────────

    /** Converts a pixel position to a grid cell, clamped to the grid bounds. */
    private fun pixelToCell(x: Int, y: Int, containerWidth: Int, containerHeight: Int): Pair<Int, Int> {
        val cellW = (containerWidth / columns).coerceAtLeast(1)
        val cellH = (containerHeight / rows).coerceAtLeast(1)
        val col = (x / cellW).coerceIn(0, columns - 1)
        val row = (y / cellH).coerceIn(0, rows - 1)
        return col to row
    }

    /** Converts a grid cell to its top-left pixel position. */
    private fun cellToPixel(col: Int, row: Int, containerWidth: Int, containerHeight: Int): Pair<Int, Int> {
        val cellW = (containerWidth / columns).coerceAtLeast(1)
        val cellH = (containerHeight / rows).coerceAtLeast(1)
        return (col * cellW) to (row * cellH)
    }

    /**
     * Resolves a drag-drop release. [containerWidth]/[containerHeight] are
     * the desktop container's current pixel dimensions (needed for
     * pixel<->cell conversion); [draggedCurrentCol]/[draggedCurrentRow] is
     * the dragged item's cell before the drop (needed to determine push
     * direction). Returns every item whose position is now current — apply
     * each to its corresponding View's LayoutParams and persist it.
     */
    fun resolveDrop(
        itemId: String,
        draggedCurrentCol: Int,
        draggedCurrentRow: Int,
        dropPxX: Int,
        dropPxY: Int,
        containerWidth: Int,
        containerHeight: Int,
    ): List<ResolvedPlacement> {
        check(!closed) { "DesktopGridManager used after close()" }

        occupied.remove(itemId) // the dragged item's old position shouldn't count as a "blocker" against itself

        val (targetCol, targetRow) = pixelToCell(dropPxX, dropPxY, containerWidth, containerHeight)
        val target = CellRect(targetCol, targetRow)

        val resolved = if (isFree(target, null)) {
            occupied[itemId] = clampToGrid(target)
            true
        } else {
            val (draggedPxX, draggedPxY) = cellToPixel(draggedCurrentCol, draggedCurrentRow, containerWidth, containerHeight)
            val dx = dropPxX - draggedPxX
            val dy = dropPxY - draggedPxY
            val direction = if (kotlin.math.abs(dx) > kotlin.math.abs(dy)) {
                if (dx > 0) PushDirection.RIGHT else PushDirection.LEFT
            } else if (dy > 0) {
                PushDirection.DOWN
            } else {
                PushDirection.UP
            }
            moveWithPush(itemId, target, direction)
        }

        if (!resolved) {
            val existing = rectOf(itemId) ?: target
            occupied[itemId] = clampToGrid(existing)
        }

        // Collect every currently-occupied item's resolved position — walk
        // all desktop children, apply their resolved cell, so callers can
        // apply pixel positions to every affected View (the dragged item
        // plus any pushed neighbors), not just the one that was dragged.
        return occupied.map { (id, rect) ->
            val (px, py) = cellToPixel(rect.col, rect.row, containerWidth, containerHeight)
            ResolvedPlacement(id, rect.col, rect.row, px, py)
        }
    }

    override fun close() {
        // No native resource to release — kept for API compatibility with
        // callers that treat this as an AutoCloseable resource.
        closed = true
    }
}
