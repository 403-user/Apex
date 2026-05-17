use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub background: String,
    pub foreground: String,
    pub cursor: String,
    pub selection_bg: String,
    pub black: String,
    pub red: String,
    pub green: String,
    pub yellow: String,
    pub blue: String,
    pub magenta: String,
    pub cyan: String,
    pub white: String,
    pub bright_black: String,
    pub bright_red: String,
    pub bright_green: String,
    pub bright_yellow: String,
    pub bright_blue: String,
    pub bright_magenta: String,
    pub bright_cyan: String,
    pub bright_white: String,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::kali_dark()
    }
}

impl Theme {
    pub fn kali_dark() -> Self {
        Theme {
            name: "kali-dark".into(),
            background: "#1a1a2e".into(),
            foreground: "#e0e0e0".into(),
            cursor: "#00ffaa".into(),
            selection_bg: "#2d2d5e".into(),
            black: "#000000".into(),
            red: "#ff4c4c".into(),
            green: "#00ffaa".into(),
            yellow: "#ffcc00".into(),
            blue: "#4a9eff".into(),
            magenta: "#ff6b9d".into(),
            cyan: "#00d4ff".into(),
            white: "#e0e0e0".into(),
            bright_black: "#555555".into(),
            bright_red: "#ff6b6b".into(),
            bright_green: "#66ffcc".into(),
            bright_yellow: "#ffdd44".into(),
            bright_blue: "#77b9ff".into(),
            bright_magenta: "#ff8db5".into(),
            bright_cyan: "#33ddff".into(),
            bright_white: "#ffffff".into(),
        }
    }

    pub fn backtrack() -> Self {
        Theme {
            name: "backtrack".into(),
            background: "#0a0a0a".into(),
            foreground: "#00ff00".into(),
            cursor: "#00ff00".into(),
            selection_bg: "#003300".into(),
            black: "#000000".into(),
            red: "#ff0000".into(),
            green: "#00ff00".into(),
            yellow: "#ffff00".into(),
            blue: "#0066ff".into(),
            magenta: "#ff00ff".into(),
            cyan: "#00ffff".into(),
            white: "#c0c0c0".into(),
            bright_black: "#808080".into(),
            bright_red: "#ff4444".into(),
            bright_green: "#44ff44".into(),
            bright_yellow: "#ffff44".into(),
            bright_blue: "#4488ff".into(),
            bright_magenta: "#ff44ff".into(),
            bright_cyan: "#44ffff".into(),
            bright_white: "#ffffff".into(),
        }
    }
}
