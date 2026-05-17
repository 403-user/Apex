use bitflags::bitflags;

bitflags! {
    pub struct TerminalMode: u32 {
        const CURSOR_VISIBLE  = 1 << 0;
        const CURSOR_BLINK    = 1 << 1;
        const INSERT_MODE     = 1 << 2;
        const APPLICATION_KEYPAD = 1 << 3;
        const WRAP            = 1 << 4;
        const ORIGIN          = 1 << 5;
        const NEWLINE         = 1 << 6;
        const REVERSE_VIDEO   = 1 << 7;
        const RELATIVE_ORIGIN = 1 << 8;
        const AUTOWRAP        = 1 << 9;
        const BRACKETED_PASTE = 1 << 10;
        const MOUSE_REPORTING = 1 << 11;
        const FOCUS_EVENTS    = 1 << 12;
        const APPLICATION_CURSOR = 1 << 13;
        const ALT_SCREEN      = 1 << 14;
        const SMOOTH_SCROLL   = 1 << 15;
    }
}

impl Default for TerminalMode {
    fn default() -> Self {
        TerminalMode::CURSOR_VISIBLE
            | TerminalMode::WRAP
            | TerminalMode::AUTOWRAP
    }
}

use crate::grid::Color;

pub struct CursorState {
    pub row: usize,
    pub col: usize,
    pub saved_row: usize,
    pub saved_col: usize,
    pub style: CursorStyle,
    pub fg_color: Color,
    pub bg_color: Color,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
    pub hidden: bool,
    pub strikethrough: bool,
}

impl Default for CursorState {
    fn default() -> Self {
        CursorState {
            row: 0,
            col: 0,
            saved_row: 0,
            saved_col: 0,
            style: CursorStyle::Block,
            fg_color: Color::Default,
            bg_color: Color::Default,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            reverse: false,
            hidden: false,
            strikethrough: false,
        }
    }
}

pub enum CursorStyle {
    Block,
    Underline,
    Beam,
    BlinkingBlock,
    BlinkingUnderline,
    BlinkingBeam,
}
