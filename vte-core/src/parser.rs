use crate::grid::{Cell, Color, Grid};
use crate::state::{CursorState, TerminalMode};
use crate::scrollback::ScrollbackBuffer;
use vte::{Parser, Perform, Params};

pub struct VteProcessor {
    parser: Parser,
    pub grid: Grid,
    pub cursor: CursorState,
    pub mode: TerminalMode,
    pub scrollback: ScrollbackBuffer,
    dirty: bool,
}

impl VteProcessor {
    pub fn new(rows: usize, cols: usize, scrollback_lines: usize) -> Self {
        VteProcessor {
            parser: Parser::new(),
            grid: Grid::new(rows, cols),
            cursor: CursorState::default(),
            mode: TerminalMode::default(),
            scrollback: ScrollbackBuffer::new(scrollback_lines),
            dirty: false,
        }
    }

    pub fn advance(&mut self, bytes: &[u8]) {
        self.dirty = false;
        let mut parser = std::mem::take(&mut self.parser);
        parser.advance(self, bytes);
        self.parser = parser;
    }

    pub fn is_dirty(&self) -> bool { self.dirty }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.grid.resize(rows, cols);
        self.dirty = true;
    }

    fn set_char(&mut self, c: char) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        if row < self.grid.rows.len() && col < self.grid.cols {
            let cell = &mut self.grid.rows[row].cells[col];
            cell.character = c;
            cell.fg_color = self.cursor.fg_color;
            cell.bg_color = self.cursor.bg_color;
            cell.bold = self.cursor.bold;
            cell.dim = self.cursor.dim;
            cell.italic = self.cursor.italic;
            cell.underline = self.cursor.underline;
            cell.reverse = self.cursor.reverse;
            cell.hidden = self.cursor.hidden;
            cell.strikethrough = self.cursor.strikethrough;
            self.dirty = true;
        }
        if self.mode.contains(TerminalMode::AUTOWRAP) {
            self.cursor.col = self.cursor.col.saturating_add(1);
            if self.cursor.col >= self.grid.cols {
                self.cursor.col = 0;
                self.newline();
            }
        } else {
            self.cursor.col = self.cursor.col.saturating_add(1);
        }
    }

    fn newline(&mut self) {
        self.cursor.col = 0;
        if self.cursor.row + 1 >= self.grid.rows.len() {
            for row in self.grid.scroll_up(1) {
                self.scrollback.push(row);
            }
        } else {
            self.cursor.row = self.cursor.row.saturating_add(1);
        }
        self.dirty = true;
    }

    fn carriage_return(&mut self) {
        self.cursor.col = 0;
    }

    fn tab(&mut self) {
        let tab_stop = 8;
        self.cursor.col = (self.cursor.col / tab_stop + 1).saturating_mul(tab_stop);
        if self.cursor.col >= self.grid.cols {
            self.cursor.col = self.grid.cols - 1;
        }
    }

    fn backspace(&mut self) {
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        }
    }

    fn clear_screen(&mut self) {
        for row in self.grid.rows.iter_mut() {
            for cell in row.cells.iter_mut() {
                *cell = Cell::default();
            }
        }
        self.cursor.row = 0;
        self.cursor.col = 0;
        self.dirty = true;
    }

    fn clear_line(&mut self) {
        if self.cursor.row < self.grid.rows.len() {
            for cell in self.grid.rows[self.cursor.row].cells.iter_mut() {
                *cell = Cell::default();
            }
        }
        self.dirty = true;
    }

    fn delete_characters(&mut self, count: usize) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        if row < self.grid.rows.len() {
            let cells = &mut self.grid.rows[row].cells;
            let end = col.saturating_add(count).min(cells.len());
            if col < end {
                cells.drain(col..end);
            }
            while cells.len() < self.grid.cols {
                cells.push(Cell::default());
            }
            self.dirty = true;
        }
    }

    fn insert_lines(&mut self, count: usize) {
        let row = self.cursor.row;
        let count = count.min(self.grid.rows.len().saturating_sub(row));
        for _ in 0..count {
            self.grid.rows.remove(self.grid.rows.len() - 1);
            self.grid.rows.insert(row, crate::grid::Row::new(self.grid.cols));
        }
        self.dirty = true;
    }

    fn delete_lines(&mut self, count: usize) {
        let row = self.cursor.row;
        let count = count.min(self.grid.rows.len().saturating_sub(row));
        for _ in 0..count {
            self.grid.rows.remove(row);
            self.grid.rows.push_back(crate::grid::Row::new(self.grid.cols));
        }
        self.dirty = true;
    }

    fn set_cursor_row(&mut self, row: usize) {
        self.cursor.row = row.min(self.grid.rows.len().saturating_sub(1));
    }

    fn set_cursor_col(&mut self, col: usize) {
        self.cursor.col = col.min(self.grid.cols.saturating_sub(1));
    }

    fn dispatch_sgr(&mut self, params: &Params) {
        let param = |i: usize| -> u16 {
            params.iter().nth(i).and_then(|p| p.first()).copied().unwrap_or(0)
        };
        let count = params.len();
        let mut i = 0;
        while i < count {
            let p = param(i);
            match p {
                0 => {
                    self.cursor.fg_color = Color::Default;
                    self.cursor.bg_color = Color::Default;
                    self.cursor.bold = false;
                    self.cursor.dim = false;
                    self.cursor.italic = false;
                    self.cursor.underline = false;
                    self.cursor.reverse = false;
                    self.cursor.hidden = false;
                    self.cursor.strikethrough = false;
                }
                1 => self.cursor.bold = true,
                2 => self.cursor.dim = true,
                3 => self.cursor.italic = true,
                4 => self.cursor.underline = true,
                5 | 6 => {}
                7 => self.cursor.reverse = true,
                8 => self.cursor.hidden = true,
                9 => self.cursor.strikethrough = true,
                21 | 22 => self.cursor.bold = false,
                23 => self.cursor.italic = false,
                24 => self.cursor.underline = false,
                27 => self.cursor.reverse = false,
                28 => self.cursor.hidden = false,
                29 => self.cursor.strikethrough = false,
                30..=37 => {
                    const C: [Color; 8] = [
                        Color::Black, Color::Red, Color::Green, Color::Yellow,
                        Color::Blue, Color::Magenta, Color::Cyan, Color::White,
                    ];
                    self.cursor.fg_color = C[(p - 30) as usize];
                }
                38 => {
                    let sub1 = param(i + 1);
                    if sub1 == 5 && i + 2 < count {
                        self.cursor.fg_color = Color::Indexed(param(i + 2).min(255) as u8);
                        i += 2;
                    } else if sub1 == 2 && i + 4 < count {
                        self.cursor.fg_color = Color::Rgb(
                            param(i + 2).min(255) as u8, param(i + 3).min(255) as u8, param(i + 4).min(255) as u8,
                        );
                        i += 4;
                    }
                }
                39 => self.cursor.fg_color = Color::Default,
                40..=47 => {
                    const C: [Color; 8] = [
                        Color::Black, Color::Red, Color::Green, Color::Yellow,
                        Color::Blue, Color::Magenta, Color::Cyan, Color::White,
                    ];
                    self.cursor.bg_color = C[(p - 40) as usize];
                }
                48 => {
                    let sub1 = param(i + 1);
                    if sub1 == 5 && i + 2 < count {
                        self.cursor.bg_color = Color::Indexed(param(i + 2).min(255) as u8);
                        i += 2;
                    } else if sub1 == 2 && i + 4 < count {
                        self.cursor.bg_color = Color::Rgb(
                            param(i + 2).min(255) as u8, param(i + 3).min(255) as u8, param(i + 4).min(255) as u8,
                        );
                        i += 4;
                    }
                }
                49 => self.cursor.bg_color = Color::Default,
                90..=97 => {
                    const C: [Color; 8] = [
                        Color::BrightBlack, Color::BrightRed, Color::BrightGreen, Color::BrightYellow,
                        Color::BrightBlue, Color::BrightMagenta, Color::BrightCyan, Color::BrightWhite,
                    ];
                    self.cursor.fg_color = C[(p - 90) as usize];
                }
                100..=107 => {
                    const C: [Color; 8] = [
                        Color::BrightBlack, Color::BrightRed, Color::BrightGreen, Color::BrightYellow,
                        Color::BrightBlue, Color::BrightMagenta, Color::BrightCyan, Color::BrightWhite,
                    ];
                    self.cursor.bg_color = C[(p - 100) as usize];
                }
                _ => {}
            }
            i += 1;
        }
    }
}

impl Perform for VteProcessor {
    fn print(&mut self, c: char) {
        self.set_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x08 => self.backspace(),       // BS
            0x09 => self.tab(),              // HT
            0x0A | 0x0B | 0x0C => self.newline(), // LF, VT, FF
            0x0D => self.carriage_return(),  // CR
            0x1B => {}                       // ESC (handled by parser)
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        let default = |i: usize, d: u16| -> u16 {
            params.iter().nth(i).and_then(|p| p.first()).map(|&v| v as u16).unwrap_or(d)
        };

        match action {
            'A' => { // Cursor Up
                let n = default(0, 1) as usize;
                self.cursor.row = self.cursor.row.saturating_sub(n);
            }
            'B' => { // Cursor Down
                let n = default(0, 1) as usize;
                self.cursor.row = self.cursor.row.saturating_add(n).min(self.grid.rows.len().saturating_sub(1));
            }
            'C' => { // Cursor Forward
                let n = default(0, 1) as usize;
                self.cursor.col = self.cursor.col.saturating_add(n).min(self.grid.cols.saturating_sub(1));
            }
            'D' => { // Cursor Backward
                let n = default(0, 1) as usize;
                self.cursor.col = self.cursor.col.saturating_sub(n);
            }
            'H' | 'f' => { // Cursor Position
                let row = default(0, 1).saturating_sub(1) as usize;
                let col = default(1, 1).saturating_sub(1) as usize;
                self.set_cursor_row(row);
                self.set_cursor_col(col);
            }
            'J' => match default(0, 0) {
                0 | 1 | 2 => self.clear_screen(),
                _ => {}
            },
            'K' => match default(0, 0) {
                0 | 1 | 2 => self.clear_line(),
                _ => {}
            },
            'L' => self.insert_lines(default(0, 1) as usize),
            'M' => self.delete_lines(default(0, 1) as usize),
            'P' => self.delete_characters(default(0, 1) as usize),
            'm' => self.dispatch_sgr(params),
            'r' => {
                let max_row = self.grid.rows.len().saturating_sub(1);
                let top = (default(0, 1).saturating_sub(1) as usize).min(max_row);
                let bottom = (default(1, self.grid.rows_visible as u16).saturating_sub(1) as usize).min(max_row);
                self.grid.scroll_top = top;
                self.grid.scroll_bottom = bottom;
            }
            's' => { /* save cursor */ self.cursor.saved_row = self.cursor.row; self.cursor.saved_col = self.cursor.col; }
            'u' => { /* restore cursor */ self.cursor.row = self.cursor.saved_row; self.cursor.col = self.cursor.saved_col; }
            _ => {}
        }
        self.dirty = true;
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'c' => self.clear_screen(), // RIS
            b'D' => self.newline(),      // IND
            b'M' => {
                if self.cursor.row > 0 {
                    self.cursor.row -= 1;
                } else {
                    self.grid.scroll_down(1);
                }
            }
            b'E' => { self.newline(); }  // NEL
            b'H' => { self.tab(); }      // HTS
            b'7' => { self.cursor.saved_row = self.cursor.row; self.cursor.saved_col = self.cursor.col; }
            b'8' => { self.cursor.row = self.cursor.saved_row; self.cursor.col = self.cursor.saved_col; }
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_term: bool) {}

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
}
