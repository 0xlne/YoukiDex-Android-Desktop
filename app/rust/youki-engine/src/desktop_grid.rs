//! desktop_grid — the launcher desktop's cell-occupancy grid: collision
//! detection, Nova-style "push neighbors out of the way" drag-drop
//! resolution, and pixel<->cell conversion.
//!
//! Ported 1:1 from the original Kotlin (DesktopGridPrefs.kt's CellRect /
//! DesktopOccupancyGrid / PushDirection, plus LauncherActivity.kt's
//! pixelToCell/cellToPixel/resolveDrop) per the project decision that all
//! logic — including grid math like this, which has zero Android Framework
//! dependency — belongs in Rust. Kotlin's LauncherActivity/DesktopWidgetManager
//! now only marshal ints across JNI and apply the returned pixel positions to
//! views; see jni_bridge.rs's desktop_grid bridge section.
//!
//! Every method here is a direct behavioral port: same recursion structure,
//! same depth guard, same rollback-on-failure semantics as the Kotlin
//! original, so the exact same drag-drop feel carries over (Nova-style
//! push/cascade). Where the port differs is representation only: the
//! Kotlin `LinkedHashMap<String, CellRect>` (ordered, snapshot/rollback via
//! full-map clone) becomes an `IndexMap<String, CellRect>` here for the same
//! ordered-iteration + easy-clone properties, rather than a plain HashMap
//! (whose iteration order is unspecified — the original's push-resolution
//! order over `blockers` is otherwise not fully deterministic run to run).

use indexmap::IndexMap;

/// A rectangular region of grid cells: (col, row) is the top-left cell,
/// spanning col_span x row_span cells. Mirrors Kotlin's `data class CellRect`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellRect {
    pub col: i32,
    pub row: i32,
    pub col_span: i32,
    pub row_span: i32,
}

impl CellRect {
    pub fn new(col: i32, row: i32) -> Self {
        Self { col, row, col_span: 1, row_span: 1 }
    }

    pub fn with_span(col: i32, row: i32, col_span: i32, row_span: i32) -> Self {
        Self { col, row, col_span, row_span }
    }

    pub fn col_end(&self) -> i32 {
        self.col + self.col_span - 1
    }

    pub fn row_end(&self) -> i32 {
        self.row + self.row_span - 1
    }

    pub fn overlaps(&self, other: &CellRect) -> bool {
        self.col <= other.col_end()
            && self.col_end() >= other.col
            && self.row <= other.row_end()
            && self.row_end() >= other.row
    }

    /// Returns a copy with `col` replaced — mirrors Kotlin's `rect.copy(col = ...)`.
    pub fn with_col(&self, col: i32) -> Self {
        Self { col, ..*self }
    }

    /// Returns a copy with `row` replaced — mirrors Kotlin's `rect.copy(row = ...)`.
    pub fn with_row(&self, row: i32) -> Self {
        Self { row, ..*self }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushDirection {
    Left,
    Right,
    Up,
    Down,
}

/// One shared occupancy map for the whole desktop (icons, folders, and
/// widgets all register their CellRect here). Mirrors Kotlin's
/// `DesktopOccupancyGrid` exactly — see module docs above.
pub struct DesktopOccupancyGrid {
    pub columns: i32,
    pub rows: i32,
    occupied: IndexMap<String, CellRect>,
}

impl DesktopOccupancyGrid {
    pub fn new(columns: i32, rows: i32) -> Self {
        Self { columns, rows, occupied: IndexMap::new() }
    }

    pub fn clear(&mut self) {
        self.occupied.clear();
    }

    pub fn place(&mut self, item_id: &str, rect: CellRect) {
        let clamped = self.clamp_to_grid(rect);
        self.occupied.insert(item_id.to_string(), clamped);
    }

    pub fn remove(&mut self, item_id: &str) {
        self.occupied.shift_remove(item_id);
    }

    pub fn rect_of(&self, item_id: &str) -> Option<CellRect> {
        self.occupied.get(item_id).copied()
    }

    fn clamp_to_grid(&self, rect: CellRect) -> CellRect {
        let col = rect.col.clamp(0, (self.columns - rect.col_span).max(0));
        let row = rect.row.clamp(0, (self.rows - rect.row_span).max(0));
        CellRect { col, row, ..rect }
    }

    /// Every item whose rect overlaps `rect`, excluding `exclude_item_id`
    /// (typically the item being moved).
    pub fn collisions(&self, rect: &CellRect, exclude_item_id: Option<&str>) -> Vec<String> {
        self.occupied
            .iter()
            .filter(|(id, r)| Some(id.as_str()) != exclude_item_id && r.overlaps(rect))
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn is_free(&self, rect: &CellRect, exclude_item_id: Option<&str>) -> bool {
        self.collisions(rect, exclude_item_id).is_empty()
            && rect.col >= 0
            && rect.row >= 0
            && rect.col_end() < self.columns
            && rect.row_end() < self.rows
    }

    /// Attempts to move `item_id` to `target_rect`. If the target is free,
    /// moves it directly. If occupied, tries to push every colliding item
    /// one step further in `direction` (Nova-style shove) and recursively
    /// resolves any chain reaction those pushes cause. Returns true if the
    /// move (and any pushes it required) succeeded without anything falling
    /// off the grid; false if there wasn't room and nothing was changed
    /// (full rollback to the pre-call state).
    pub fn move_with_push(
        &mut self,
        item_id: &str,
        target_rect: CellRect,
        direction: PushDirection,
    ) -> bool {
        let snapshot = self.occupied.clone();
        let ok = self.try_place_with_push(item_id, target_rect, direction, 0);
        if !ok {
            self.occupied = snapshot;
        }
        ok
    }

    fn try_place_with_push(
        &mut self,
        item_id: &str,
        target_rect: CellRect,
        direction: PushDirection,
        depth: i32,
    ) -> bool {
        // Guard against pathological chains (e.g. a fully-packed grid)
        // turning into unbounded recursion — a launcher grid realistically
        // never needs more than a couple dozen cascading pushes to resolve
        // or fail. Matches the Kotlin original's `depth > 64` guard exactly.
        if depth > 64 {
            return false;
        }
        let clamped = self.clamp_to_grid(target_rect);
        if clamped.col != target_rect.col || clamped.row != target_rect.row {
            // Target rect doesn't fit on the grid at all in this position —
            // reject rather than silently clamping into a different
            // collision than the caller intended.
            if clamped.col_end() < target_rect.col_end() || clamped.row_end() < target_rect.row_end()
            {
                return false;
            }
        }

        let blockers = self.collisions(&clamped, Some(item_id));
        if blockers.is_empty() {
            self.occupied.insert(item_id.to_string(), clamped);
            return true;
        }

        // Try to push every blocker one cell further in the drag direction,
        // then re-check.
        for blocker_id in &blockers {
            let blocker_rect = match self.occupied.get(blocker_id) {
                Some(r) => *r,
                None => continue,
            };
            let pushed = match direction {
                PushDirection::Right => blocker_rect.with_col(blocker_rect.col + 1),
                PushDirection::Left => blocker_rect.with_col(blocker_rect.col - 1),
                PushDirection::Down => blocker_rect.with_row(blocker_rect.row + 1),
                PushDirection::Up => blocker_rect.with_row(blocker_rect.row - 1),
            };
            if !self.try_place_with_push(blocker_id, pushed, direction, depth + 1) {
                return false;
            }
        }
        self.occupied.insert(item_id.to_string(), clamped);
        true
    }

    /// First free rect of the given span, scanning row-major from (0,0) —
    /// used for initial/default placement.
    pub fn first_free_rect(
        &self,
        col_span: i32,
        row_span: i32,
        exclude_item_id: Option<&str>,
    ) -> Option<CellRect> {
        for r in 0..=(self.rows - row_span) {
            for c in 0..=(self.columns - col_span) {
                let candidate = CellRect::with_span(c, r, col_span, row_span);
                if self.is_free(&candidate, exclude_item_id) {
                    return Some(candidate);
                }
            }
        }
        None
    }

    /// Places `item_id` in (`preferred_col`, `preferred_row`) if free,
    /// otherwise the first available free cell — used when loading the
    /// desktop, so a saved position is honored when possible but never
    /// causes an overlap. Mirrors LauncherActivity.kt's `placeInFreeCell`.
    pub fn place_in_free_cell(
        &mut self,
        item_id: &str,
        preferred_col: i32,
        preferred_row: i32,
    ) -> CellRect {
        let preferred = CellRect::new(
            preferred_col.clamp(0, self.columns - 1),
            preferred_row.clamp(0, self.rows - 1),
        );
        let rect = if self.is_free(&preferred, None) {
            preferred
        } else {
            self.first_free_rect(1, 1, None).unwrap_or(preferred)
        };
        self.place(item_id, rect);
        rect
    }
}

// ── pixel<->cell conversion (mirrors LauncherActivity.kt) ──────────────

/// Converts a pixel position to a grid cell, clamped to the grid bounds.
/// `container_width`/`container_height` are the desktop container's
/// current pixel dimensions; `columns`/`rows` the grid size.
pub fn pixel_to_cell(
    x: i32,
    y: i32,
    container_width: i32,
    container_height: i32,
    columns: i32,
    rows: i32,
) -> (i32, i32) {
    let cell_w = (container_width / columns).max(1);
    let cell_h = (container_height / rows).max(1);
    let col = (x / cell_w).clamp(0, columns - 1);
    let row = (y / cell_h).clamp(0, rows - 1);
    (col, row)
}

/// Converts a grid cell to its top-left pixel position.
pub fn cell_to_pixel(
    col: i32,
    row: i32,
    container_width: i32,
    container_height: i32,
    columns: i32,
    rows: i32,
) -> (i32, i32) {
    let cell_w = (container_width / columns).max(1);
    let cell_h = (container_height / rows).max(1);
    (col * cell_w, row * cell_h)
}

/// One resolved item's new on-screen position after a drop — returned to
/// Kotlin so it can apply the pixel position to the corresponding view and
/// persist it. Mirrors the per-child loop at the end of LauncherActivity.kt's
/// `resolveDrop`.
#[derive(Clone, Debug)]
pub struct ResolvedPlacement {
    pub item_id: String,
    pub col: i32,
    pub row: i32,
    pub pixel_x: i32,
    pub pixel_y: i32,
}

/// Full drag-drop resolution: rebuilds nothing itself (the grid passed in
/// is assumed already ground-truth-accurate — see note below), removes the
/// dragged item from consideration against itself, then either places it
/// directly (if the target cell is free) or pushes blockers out of the way.
/// Returns the full list of items whose position changed as a result (the
/// dragged item plus any pushed neighbors), each with both its new cell and
/// the equivalent pixel position, ready for Kotlin to apply to views and
/// persist.
///
/// NOTE: unlike the Kotlin original's `resolveDrop` (which also owned
/// rebuilding `desktopGrid` from live view state, since that state only
/// exists on the Kotlin/View side), this function takes the already-built
/// `grid` as a parameter — Kotlin is still responsible for calling
/// `rebuild`-equivalent logic first (walking its own View children), since
/// that step inherently touches View/LayoutParams and cannot move to Rust
/// per the project's own "Framework API calls stay Kotlin" boundary. Once
/// the grid snapshot crosses into Rust, everything else (collision
/// resolution, cascading pushes, rollback) is pure computation here.
pub fn resolve_drop(
    grid: &mut DesktopOccupancyGrid,
    item_id: &str,
    dragged_current_col: i32,
    dragged_current_row: i32,
    drop_px_x: i32,
    drop_px_y: i32,
    container_width: i32,
    container_height: i32,
) -> Vec<ResolvedPlacement> {
    grid.remove(item_id); // the dragged item's old position shouldn't count as a "blocker" against itself

    let (target_col, target_row) =
        pixel_to_cell(drop_px_x, drop_px_y, container_width, container_height, grid.columns, grid.rows);
    let target = CellRect::new(target_col, target_row);

    let resolved = if grid.is_free(&target, None) {
        grid.place(item_id, target);
        true
    } else {
        let (dragged_px_x, dragged_px_y) = cell_to_pixel(
            dragged_current_col,
            dragged_current_row,
            container_width,
            container_height,
            grid.columns,
            grid.rows,
        );
        let dx = drop_px_x - dragged_px_x;
        let dy = drop_px_y - dragged_px_y;
        let direction = if dx.abs() > dy.abs() {
            if dx > 0 { PushDirection::Right } else { PushDirection::Left }
        } else if dy > 0 {
            PushDirection::Down
        } else {
            PushDirection::Up
        };
        grid.move_with_push(item_id, target, direction)
    };

    if !resolved {
        let existing = grid.rect_of(item_id).unwrap_or(target);
        grid.place(item_id, existing);
    }

    // Collect every currently-occupied item's resolved position — matches
    // the Kotlin original's "walk all desktop children, apply grid.rectOf"
    // loop, just expressed as a returned list instead of direct View
    // mutation (which Kotlin still performs, using this list).
    let mut placements = Vec::new();
    for (id, rect) in grid.occupied_snapshot() {
        let (px, py) =
            cell_to_pixel(rect.col, rect.row, container_width, container_height, grid.columns, grid.rows);
        placements.push(ResolvedPlacement {
            item_id: id,
            col: rect.col,
            row: rect.row,
            pixel_x: px,
            pixel_y: py,
        });
    }
    placements
}

impl DesktopOccupancyGrid {
    /// Snapshot of every occupied (item_id, rect) pair, in insertion order.
    /// Used by `resolve_drop` to report every item's resolved position back
    /// to Kotlin (including neighbors that were pushed, not just the item
    /// that was dragged).
    pub fn occupied_snapshot(&self) -> Vec<(String, CellRect)> {
        self.occupied.iter().map(|(id, r)| (id.clone(), *r)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_and_free_check() {
        let mut grid = DesktopOccupancyGrid::new(5, 7);
        grid.place("app:a", CellRect::new(0, 0));
        assert!(!grid.is_free(&CellRect::new(0, 0), None));
        assert!(grid.is_free(&CellRect::new(1, 0), None));
    }

    #[test]
    fn push_right_cascades() {
        let mut grid = DesktopOccupancyGrid::new(5, 7);
        grid.place("app:a", CellRect::new(0, 0));
        grid.place("app:b", CellRect::new(1, 0));
        grid.place("app:c", CellRect::new(2, 0));
        // Push "a" onto "b"'s cell, direction RIGHT — should cascade b->1,
        // c->2 shifted to make room, i.e. b moves to col 2? Actually b is
        // pushed right, landing on c's cell, which itself gets pushed right.
        let ok = grid.move_with_push("app:a", CellRect::new(1, 0), PushDirection::Right);
        assert!(ok);
        assert_eq!(grid.rect_of("app:a").unwrap().col, 1);
        assert_eq!(grid.rect_of("app:b").unwrap().col, 2);
        assert_eq!(grid.rect_of("app:c").unwrap().col, 3);
    }

    #[test]
    fn push_fails_when_grid_full_rolls_back() {
        let mut grid = DesktopOccupancyGrid::new(2, 1);
        grid.place("app:a", CellRect::new(0, 0));
        grid.place("app:b", CellRect::new(1, 0));
        let before_a = grid.rect_of("app:a").unwrap();
        let before_b = grid.rect_of("app:b").unwrap();
        // Grid is 2x1 and fully packed; pushing right has nowhere to go.
        let ok = grid.move_with_push("app:a", CellRect::new(1, 0), PushDirection::Right);
        assert!(!ok);
        // Rollback: positions unchanged.
        assert_eq!(grid.rect_of("app:a").unwrap(), before_a);
        assert_eq!(grid.rect_of("app:b").unwrap(), before_b);
    }

    #[test]
    fn first_free_rect_row_major() {
        let mut grid = DesktopOccupancyGrid::new(3, 3);
        grid.place("x", CellRect::new(0, 0));
        let free = grid.first_free_rect(1, 1, None).unwrap();
        assert_eq!((free.col, free.row), (1, 0));
    }

    #[test]
    fn pixel_cell_roundtrip() {
        let (col, row) = pixel_to_cell(150, 90, 500, 700, 5, 7);
        assert_eq!((col, row), (1, 0));
        let (px, py) = cell_to_pixel(col, row, 500, 700, 5, 7);
        assert_eq!((px, py), (100, 0));
    }
}
