use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Cell {
    pub character: char,
    pub fg_color: Color,
    pub bg_color: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub dim: bool,
    pub reverse: bool,
    pub hidden: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            character: ' ',
            fg_color: Color::Default,
            bg_color: Color::Default,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            dim: false,
            reverse: false,
            hidden: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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

pub struct Grid {
    pub rows: VecDeque<Row>,
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
    }

    pub fn scroll_up(&mut self, count: usize) -> VecDeque<Row> {
        let mut scrolled = VecDeque::new();
        for _ in 0..count {
            if let Some(row) = self.rows.pop_front() {
                scrolled.push_back(row);
                self.rows.push_back(Row::new(self.cols));
            }
        }
        scrolled
    }

    pub fn scroll_down(&mut self, count: usize) {
        for _ in 0..count {
            self.rows.pop_back();
            self.rows.push_front(Row::new(self.cols));
        }
    }

    pub fn set_cell(&mut self, row: usize, col: usize, cell: Cell) {
        if row < self.rows.len() && col < self.cols {
            self.rows[row].cells[col] = cell;
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
        }
    }

    pub fn scroll_region_up(&mut self, top: usize, bottom: usize) {
        if top >= bottom || bottom >= self.rows.len() {
            return;
        }
        for r in top..bottom {
            let src = self.rows[r + 1].cells.clone();
            self.rows[r].cells = src;
        }
        for cell in self.rows[bottom].cells.iter_mut() {
            *cell = Cell::default();
        }
    }

    pub fn scroll_region_down(&mut self, top: usize, bottom: usize) {
        if top >= bottom || bottom >= self.rows.len() {
            return;
        }
        for r in (top + 1..=bottom).rev() {
            let src = self.rows[r - 1].cells.clone();
            self.rows[r].cells = src;
        }
        for cell in self.rows[top].cells.iter_mut() {
            *cell = Cell::default();
        }
    }
}
