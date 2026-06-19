use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use arrayvec::ArrayString;
use bitflags::bitflags;

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Color {
    Default,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

bitflags! {
    #[derive(Copy, Clone, Debug, Default, PartialEq)]
    pub struct CellFlags: u8 {
        const BOLD = 1 << 0;
        const DIM = 1 << 1;
        const ITALIC = 1 << 2;
        const UNDERLINE = 1 << 3;
        const REVERSE = 1 << 4;
        const HIDDEN = 1 << 5;
        const STRIKETHROUGH = 1 << 6;
    }
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub content: ArrayString<32>,
    pub width: u8,
    pub fg_color: Color,
    pub bg_color: Color,
    pub flags: CellFlags,
    pub hyperlink: Option<ArrayString<64>>,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            content: ArrayString::new(),
            width: 1,
            fg_color: Color::Default,
            bg_color: Color::Default,
            flags: CellFlags::empty(),
            hyperlink: None,
        }
    }
}

pub struct Row {
    pub cells: Vec<Cell>,
}

impl Row {
    pub fn new(cols: usize) -> Self {
        Row {
            cells: vec![Cell::default(); cols],
        }
    }
}

pub struct DamageTracker {
    pub dirty_rows: Vec<bool>,
    pub full_redraw: bool,
}

impl DamageTracker {
    pub fn new(rows: usize) -> Self {
        Self {
            dirty_rows: vec![true; rows],
            full_redraw: true,
        }
    }

    pub fn mark_row(&mut self, row: usize) {
        if let Some(d) = self.dirty_rows.get_mut(row) {
            *d = true;
        }
    }

    pub fn mark_all(&mut self) {
        self.full_redraw = true;
        for d in &mut self.dirty_rows {
            *d = true;
        }
    }

    pub fn clear(&mut self) {
        self.full_redraw = false;
        for d in &mut self.dirty_rows {
            *d = false;
        }
    }

    pub fn any_dirty(&self) -> bool {
        self.full_redraw || self.dirty_rows.iter().any(|&d| d)
    }

    pub fn resize(&mut self, new_rows: usize) {
        self.dirty_rows.resize(new_rows, true);
        self.full_redraw = true;
    }
}

pub struct Grid {
    pub rows: VecDeque<Row>,
    pub damage: DamageTracker,
    pub cols: usize,
    pub rows_visible: usize,
    pub scroll_top: usize,
    pub scroll_bottom: usize,
}

impl Grid {
    pub fn new(rows: usize, cols: usize) -> Self {
        let mut row_vec = VecDeque::with_capacity(rows);
        for _ in 0..rows {
            row_vec.push_back(Row::new(cols));
        }
        Grid {
            rows: row_vec,
            damage: DamageTracker::new(rows),
            cols,
            rows_visible: rows,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
        }
    }

    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        while self.rows.len() < new_rows {
            self.rows.push_back(Row::new(new_cols));
        }
        while self.rows.len() > new_rows {
            self.rows.pop_front();
        }
        for row in self.rows.iter_mut() {
            while row.cells.len() < new_cols {
                row.cells.push(Cell::default());
            }
            if row.cells.len() > new_cols {
                row.cells.truncate(new_cols);
            }
        }
        self.cols = new_cols;
        self.rows_visible = new_rows;
        self.scroll_top = self.scroll_top.min(new_rows.saturating_sub(1));
        self.scroll_bottom = new_rows.saturating_sub(1);
        self.damage.resize(new_rows);
    }

    pub fn scroll_up(&mut self, count: usize) -> VecDeque<Row> {
        let mut scrolled = VecDeque::new();
        for _ in 0..count {
            if let Some(row) = self.rows.pop_front() {
                scrolled.push_back(row);
                self.rows.push_back(Row::new(self.cols));
            }
        }
        // Rotate damage bits to stay in sync with row indices; new rows are dirty
        let n = self.damage.dirty_rows.len();
        let c = count.min(n);
        self.damage.dirty_rows.rotate_left(c);
        for i in n.saturating_sub(c)..n {
            self.damage.dirty_rows[i] = true;
        }
        scrolled
    }

    pub fn scroll_down(&mut self, count: usize) {
        for _ in 0..count {
            self.rows.pop_back();
            self.rows.push_front(Row::new(self.cols));
        }
        let n = self.damage.dirty_rows.len();
        let c = count.min(n);
        self.damage.dirty_rows.rotate_right(c);
        for i in 0..c {
            self.damage.dirty_rows[i] = true;
        }
    }

    pub fn set_cell(&mut self, row: usize, col: usize, cell: Cell) {
        if row < self.rows.len() && col < self.cols {
            self.rows[row].cells[col] = cell;
            self.damage.mark_row(row);
        }
    }

    pub fn clear_region(&mut self, start_row: usize, start_col: usize, end_row: usize, end_col: usize) {
        if self.rows.is_empty() || self.cols == 0 {
            return;
        }
        let max_row = self.rows.len() - 1;
        let max_col = self.cols - 1;
        let end_row = end_row.min(max_row);
        let end_col = end_col.min(max_col);
        if start_row > end_row {
            return;
        }
        for row in start_row..=end_row {
            let start = if row == start_row { start_col.min(max_col) } else { 0 };
            let end = if row == end_row { end_col.min(max_col) } else { max_col };
            for col in start..=end {
                self.rows[row].cells[col] = Cell::default();
            }
            self.damage.mark_row(row);
        }
    }

    pub fn scroll_region_up(&mut self, top: usize, bottom: usize) {
        if top >= bottom || bottom >= self.rows.len() {
            return;
        }
        for r in top..bottom {
            let src = self.rows[r + 1].cells.clone();
            self.rows[r].cells = src;
            self.damage.mark_row(r);
        }
        for cell in self.rows[bottom].cells.iter_mut() {
            *cell = Cell::default();
        }
        self.damage.mark_row(bottom);
    }

    pub fn scroll_region_down(&mut self, top: usize, bottom: usize) {
        if top >= bottom || bottom >= self.rows.len() {
            return;
        }
        for r in (top + 1..=bottom).rev() {
            let src = self.rows[r - 1].cells.clone();
            self.rows[r].cells = src;
            self.damage.mark_row(r);
        }
        for cell in self.rows[top].cells.iter_mut() {
            *cell = Cell::default();
        }
        self.damage.mark_row(top);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_default() {
        let cell = Cell::default();
        assert_eq!(cell.width, 1);
        assert!(cell.content.is_empty());
        assert!(!cell.flags.contains(CellFlags::BOLD));
        assert!(cell.hyperlink.is_none());
    }

    #[test]
    fn test_cell_set_content() {
        let mut cell = Cell::default();
        cell.content.push('A');
        cell.width = 1;
        assert_eq!(cell.content.as_str(), "A");
    }

    #[test]
    fn test_grid_new() {
        let grid = Grid::new(24, 80);
        assert_eq!(grid.rows.len(), 24);
        assert_eq!(grid.cols, 80);
        assert_eq!(grid.rows_visible, 24);
        assert_eq!(grid.scroll_top, 0);
        assert_eq!(grid.scroll_bottom, 23);
    }

    #[test]
    fn test_grid_resize() {
        let mut grid = Grid::new(24, 80);
        grid.resize(50, 120);
        assert_eq!(grid.rows.len(), 50);
        assert_eq!(grid.cols, 120);
        assert_eq!(grid.rows_visible, 50);
    }

    #[test]
    fn test_scroll_up() {
        let mut grid = Grid::new(4, 10);
        let removed = grid.scroll_up(1);
        assert_eq!(removed.len(), 1);
    }

    #[test]
    fn test_damage_tracker() {
        let mut tracker = DamageTracker::new(10);
        assert!(tracker.full_redraw);
        tracker.clear();
        assert!(!tracker.full_redraw);
        assert!(!tracker.any_dirty());
        tracker.mark_row(3);
        assert!(tracker.any_dirty());
        assert!(tracker.dirty_rows[3]);
    }

    #[test]
    fn test_clear_region() {
        let mut grid = Grid::new(4, 10);
        grid.clear_region(0, 0, 3, 9);
        assert!(grid.rows[0].cells[0].content.is_empty());
        assert!(grid.rows[3].cells[9].content.is_empty());
    }
}
