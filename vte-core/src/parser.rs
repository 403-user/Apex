use arrayvec::ArrayString;
use crate::grid::{Cell, CellFlags, Color, Grid};
use crate::state::{CursorState, TerminalMode};
use crate::scrollback::ScrollbackBuffer;
use vte::{Parser, Perform, Params};

pub struct VteProcessor {
    parser: Parser,
    pub grid: Grid,
    pub cursor: CursorState,
    pub mode: TerminalMode,
    pub scrollback: ScrollbackBuffer,
}

impl VteProcessor {
    pub fn new(rows: usize, cols: usize, scrollback_lines: usize) -> Self {
        VteProcessor {
            parser: Parser::new(),
            grid: Grid::new(rows, cols),
            cursor: CursorState::default(),
            mode: TerminalMode::default(),
            scrollback: ScrollbackBuffer::new(scrollback_lines),
        }
    }

    pub fn advance(&mut self, bytes: &[u8]) {
        let mut parser = std::mem::take(&mut self.parser);
        parser.advance(self, bytes);
        self.parser = parser;
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.grid.resize(rows, cols);
    }

    fn set_char(&mut self, c: char) {
        use unicode_width::UnicodeWidthChar;

        let row = self.cursor.row;
        let col = self.cursor.col;
        if row >= self.grid.rows.len() || col >= self.grid.cols {
            return;
        }

        let w = UnicodeWidthChar::width(c);

        // Skip control chars (w.is_none())
        let Some(w) = w else { return };
        let w = w as u8; // 0, 1, or 2

        if w == 0 {
            // Combining/zero-width: append to existing cell
            let cell = &mut self.grid.rows[row].cells[col];
            if !cell.content.is_empty() {
                let _ = cell.content.try_push(c);
            }
            return;
        }

        // w == 1 or 2: overwrite cell (insert mode pushes existing chars right)
        let cells = &mut self.grid.rows[row].cells;
        if self.mode.contains(TerminalMode::INSERT_MODE) && col + 1 < self.grid.cols {
            // Shift existing characters right by one position
            for c in (col + 1..self.grid.cols).rev() {
                cells[c] = cells[c - 1].clone();
            }
        }
        let cell = &mut cells[col];
        let mut s = ArrayString::new();
        s.push(c);
        cell.content = s;
        cell.width = w;
        cell.fg_color = self.cursor.fg_color;
        cell.bg_color = self.cursor.bg_color;
        cell.flags = Self::cursor_flags(&self.cursor);
        self.grid.damage.mark_row(row);

        // Wide char: mark next cell as spacer
        if w == 2 && col + 1 < self.grid.cols {
            let next = &mut self.grid.rows[row].cells[col + 1];
            next.content.clear();
            next.width = 0;
            next.fg_color = self.cursor.fg_color;
            next.bg_color = self.cursor.bg_color;
            next.flags = CellFlags::empty();
        }

        // Advance cursor by character width
        let advance = w as usize;
        if self.mode.contains(TerminalMode::AUTOWRAP) {
            self.cursor.col += advance;
            if self.cursor.col >= self.grid.cols {
                self.cursor.col = 0;
                self.newline();
            }
        } else {
            self.cursor.col = self.cursor.col.saturating_add(advance);
        }
    }

    fn cursor_flags(cursor: &CursorState) -> CellFlags {
        let mut f = CellFlags::empty();
        if cursor.bold { f.insert(CellFlags::BOLD); }
        if cursor.dim { f.insert(CellFlags::DIM); }
        if cursor.italic { f.insert(CellFlags::ITALIC); }
        if cursor.underline { f.insert(CellFlags::UNDERLINE); }
        if cursor.reverse { f.insert(CellFlags::REVERSE); }
        if cursor.hidden { f.insert(CellFlags::HIDDEN); }
        if cursor.strikethrough { f.insert(CellFlags::STRIKETHROUGH); }
        f
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

    fn clear_screen(&mut self, mode: u16) {
        match mode {
            0 => {
                let row = self.cursor.row;
                let col = self.cursor.col;
                if row < self.grid.rows.len() && col < self.grid.cols {
                    for c in self.grid.rows[row].cells.iter_mut().skip(col) {
                        *c = Cell::default();
                    }
                    self.grid.damage.mark_row(row);
                    for r in (row.saturating_add(1))..self.grid.rows.len() {
                        for c in self.grid.rows[r].cells.iter_mut() {
                            *c = Cell::default();
                        }
                        self.grid.damage.mark_row(r);
                    }
                }
            }
            1 => {
                let row = self.cursor.row;
                let col = self.cursor.col;
                for r in 0..row {
                    for c in self.grid.rows[r].cells.iter_mut() {
                        *c = Cell::default();
                    }
                    self.grid.damage.mark_row(r);
                }
                if row < self.grid.rows.len() {
                    for c in self.grid.rows[row].cells.iter_mut().take(col.saturating_add(1)) {
                        *c = Cell::default();
                    }
                    self.grid.damage.mark_row(row);
                }
            }
            2 | 3 => {
                for row in self.grid.rows.drain(..) {
                    self.scrollback.push(row);
                }
                for _ in 0..self.grid.rows_visible {
                    self.grid.rows.push_back(crate::grid::Row::new(self.grid.cols));
                }
                self.cursor.row = 0;
                self.cursor.col = 0;
                self.grid.damage.mark_all();
            }
            _ => {}
        }
    }

    fn clear_line(&mut self, mode: u16) {
        let row = self.cursor.row;
        if row >= self.grid.rows.len() { return; }
        let cells = &mut self.grid.rows[row].cells;
        match mode {
            0 => { // cursor to end of line
                let col = self.cursor.col.min(cells.len().saturating_sub(1));
                for c in cells.iter_mut().skip(col) {
                    *c = Cell::default();
                }
            }
            1 => { // start of line to cursor
                let col = self.cursor.col.min(cells.len().saturating_sub(1));
                for c in cells.iter_mut().take(col.saturating_add(1)) {
                    *c = Cell::default();
                }
            }
            2 => { // entire line
                for c in cells.iter_mut() {
                    *c = Cell::default();
                }
            }
            _ => {}
        }
        self.grid.damage.mark_row(row);
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
            self.grid.damage.mark_row(row);
        }
    }

    fn insert_characters(&mut self, count: usize) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        if row < self.grid.rows.len() && col < self.grid.cols {
            let cells = &mut self.grid.rows[row].cells;
            let count = count.min(self.grid.cols.saturating_sub(col));
            for _ in 0..count {
                cells.pop();
                cells.insert(col, Cell::default());
            }
            self.grid.damage.mark_row(row);
        }
    }

    fn erase_characters(&mut self, count: usize) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        if row < self.grid.rows.len() {
            let cells = &mut self.grid.rows[row].cells;
            let end = col.saturating_add(count).min(cells.len());
            for c in cells.iter_mut().take(end).skip(col) {
                *c = Cell::default();
            }
            self.grid.damage.mark_row(row);
        }
    }

    fn scroll_up_lines(&mut self, count: usize) {
        let n = self.grid.damage.dirty_rows.len();
        let c = count.min(self.grid.rows.len()).min(n);
        for _ in 0..c {
            if let Some(row) = self.grid.rows.pop_front() {
                self.scrollback.push(row);
                self.grid.rows.push_back(crate::grid::Row::new(self.grid.cols));
            }
        }
        self.grid.damage.dirty_rows.rotate_left(c);
        for i in n.saturating_sub(c)..n {
            self.grid.damage.dirty_rows[i] = true;
        }
    }

    fn scroll_down_lines(&mut self, count: usize) {
        let n = self.grid.damage.dirty_rows.len();
        let c = count.min(self.grid.rows.len()).min(n);
        for _ in 0..c {
            self.grid.rows.pop_back();
            self.grid.rows.push_front(crate::grid::Row::new(self.grid.cols));
        }
        self.grid.damage.dirty_rows.rotate_right(c);
        for i in 0..c {
            self.grid.damage.dirty_rows[i] = true;
        }
    }

    fn vertical_position_absolute(&mut self, row: usize) {
        self.set_cursor_row(row);
    }

    fn cursor_horizontal_absolute(&mut self, col: usize) {
        self.set_cursor_col(col);
    }

    fn tab_clear(&mut self, param: u16) {
        match param {
            0 => {}   // clear tab at cursor (we don't track tab stops - no-op)
            3 => {}   // clear all tabs (no-op)
            _ => {}
        }
    }

    fn set_mode(&mut self, params: &Params, private: bool) {
        for param in params.iter() {
            if let Some(&p) = param.first() {
                if private {
                    match p {
                        1 => self.mode.insert(TerminalMode::APPLICATION_CURSOR),
                        2 => {} // DECANM - no-op, keyboard mode
                        3 => {} // DECCOLM - 132 column mode, no-op
                        4 => {} // DECSCLM - smooth scroll, no-op
                        5 => self.mode.insert(TerminalMode::REVERSE_VIDEO),
                        6 => self.mode.insert(TerminalMode::ORIGIN),
                        7 => self.mode.insert(TerminalMode::AUTOWRAP),
                        8 => {} // DECARM - auto-repeat, no-op
                        9 => self.mode.insert(TerminalMode::MOUSE_REPORTING),
                        12 => {} // cursor blink
                        25 => self.mode.insert(TerminalMode::CURSOR_VISIBLE),
                        45 => {} // IRM reverse wrap
                        47 | 1047 | 1048 | 1049 => self.mode.insert(TerminalMode::ALT_SCREEN),
                        _ => {}
                    }
                } else {
                    match p {
                        4 => self.mode.insert(TerminalMode::INSERT_MODE),
                        20 => {} // LNM - line feed mode
                        _ => {}
                    }
                }
            }
        }
    }

    fn reset_mode(&mut self, params: &Params, private: bool) {
        for param in params.iter() {
            if let Some(&p) = param.first() {
                if private {
                    match p {
                        1 => self.mode.remove(TerminalMode::APPLICATION_CURSOR),
                        2 => {}
                        3 => {}
                        4 => {}
                        5 => self.mode.remove(TerminalMode::REVERSE_VIDEO),
                        6 => self.mode.remove(TerminalMode::ORIGIN),
                        7 => self.mode.remove(TerminalMode::AUTOWRAP),
                        8 => {}
                        9 => self.mode.remove(TerminalMode::MOUSE_REPORTING),
                        12 => {}
                        25 => self.mode.remove(TerminalMode::CURSOR_VISIBLE),
                        45 => {}
                        47 | 1047 | 1048 | 1049 => self.mode.remove(TerminalMode::ALT_SCREEN),
                        _ => {}
                    }
                } else {
                    match p {
                        4 => self.mode.remove(TerminalMode::INSERT_MODE),
                        20 => {}
                        _ => {}
                    }
                }
            }
        }
    }

    fn insert_lines(&mut self, count: usize) {
        let region_top = self.grid.scroll_top;
        let region_bottom = self.grid.scroll_bottom;
        let row = self.cursor.row.max(region_top).min(region_bottom);
        let max_lines = region_bottom.saturating_sub(row).saturating_add(1);
        let count = count.min(max_lines);
        for _ in 0..count {
            if self.grid.rows.len() > region_bottom {
                if let Some(removed) = self.grid.rows.remove(region_bottom) {
                    self.scrollback.push(removed);
                }
            }
            self.grid.rows.insert(row, crate::grid::Row::new(self.grid.cols));
            self.grid.rows.truncate(self.grid.rows_visible);
        }
        // Mark affected rows dirty
        for r in row..=region_bottom.min(self.grid.rows.len().saturating_sub(1)) {
            self.grid.damage.mark_row(r);
        }
    }

    fn delete_lines(&mut self, count: usize) {
        let region_top = self.grid.scroll_top;
        let region_bottom = self.grid.scroll_bottom;
        let row = self.cursor.row.max(region_top).min(region_bottom);
        let max_lines = region_bottom.saturating_sub(row).saturating_add(1);
        let count = count.min(max_lines);
        for _ in 0..count {
            if row < self.grid.rows.len() {
                self.grid.rows.remove(row);
            }
            let new_row = crate::grid::Row::new(self.grid.cols);
            if self.grid.rows.len() <= region_bottom {
                self.grid.rows.insert(region_bottom, new_row);
            } else {
                self.grid.rows.push_back(new_row);
            }
        }
        // Mark affected rows dirty
        for r in row..=region_bottom.min(self.grid.rows.len().saturating_sub(1)) {
            self.grid.damage.mark_row(r);
        }
    }

    fn set_cursor_row(&mut self, row: usize) {
        if self.mode.contains(TerminalMode::ORIGIN) {
            let scroll_top = self.grid.scroll_top;
            let max_row = self.grid.scroll_bottom.min(self.grid.rows.len().saturating_sub(1));
            self.cursor.row = (scroll_top + row).min(max_row);
        } else {
            self.cursor.row = row.min(self.grid.rows.len().saturating_sub(1));
        }
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
                21 => self.cursor.bold = false, // double-underline or bold off
                22 => { self.cursor.bold = false; self.cursor.dim = false; } // normal intensity
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

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        let default = |i: usize, d: u16| -> u16 {
            params.iter().nth(i).and_then(|p| p.first()).map(|&v| v as u16).unwrap_or(d)
        };
        let private = intermediates.first() == Some(&b'?');

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
            'J' => self.clear_screen(default(0, 0)),
            'K' => self.clear_line(default(0, 0)),
            '@' => self.insert_characters(default(0, 1) as usize),
            'G' => self.cursor_horizontal_absolute(default(0, 1).saturating_sub(1) as usize),
            'L' => self.insert_lines(default(0, 1) as usize),
            'M' => self.delete_lines(default(0, 1) as usize),
            'P' => self.delete_characters(default(0, 1) as usize),
            'S' => self.scroll_up_lines(default(0, 1) as usize),
            'T' => self.scroll_down_lines(default(0, 1) as usize),
            'X' => self.erase_characters(default(0, 1) as usize),
            'd' => self.vertical_position_absolute(default(0, 1).saturating_sub(1) as usize),
            'g' => self.tab_clear(default(0, 0)),
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
            'h' => self.set_mode(params, private),
            'l' => self.reset_mode(params, private),
            'n' => { /* DSR - Device Status Report, typically ignored without response channel */ }
            'c' => { /* DA - Device Attributes, typically ignored without response channel */ }
            't' => { /* XT - Window manipulation, typically ignored */ }
            'q' => { /* DECLL - Load LEDs, no-op */ }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'c' => self.clear_screen(2), // RIS
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
            b'6' => { // DECBI - Back Index (same as RI)
                if self.cursor.row > 0 {
                    self.cursor.row -= 1;
                } else {
                    self.grid.scroll_down(1);
                }
            }
            b'9' => { // DECFI - Forward Index (same as IND)
                self.newline();
            }
            b'=' => { // DECPAM - Application Keypad Mode
                self.mode.insert(TerminalMode::APPLICATION_KEYPAD);
            }
            b'>' => { // DECPNM - Normal Keypad Mode
                self.mode.remove(TerminalMode::APPLICATION_KEYPAD);
            }
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
