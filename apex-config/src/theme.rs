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
    pub fn from_name(name: &str) -> Self {
        match name {
            "kali-dark" | "default" => Theme::kali_dark(),
            "backtrack" => Theme::backtrack(),
            _ => {
                // Try to load from file
                let mut paths = vec![
                    format!("/etc/apex/themes/{name}.toml"),
                    format!("themes/{name}.toml"),
                ];
                if let Some(config_dir) = directories::ProjectDirs::from("com", "apex", "apex") {
                    paths.push(config_dir.config_dir().join("themes").join(format!("{name}.toml")).to_string_lossy().to_string());
                }
                for path in &paths {
                    if let Ok(content) = std::fs::read_to_string(path) {
                        if let Ok(theme) = toml::from_str::<Theme>(&content) {
                            return theme;
                        }
                    }
                }
                Theme::kali_dark()
            }
        }
    }

    /// Parse a hex color string (#rrggbb) to (f32, f32, f32)
    pub fn parse_hex(s: &str) -> (f32, f32, f32) {
        let s = s.trim_start_matches('#');
        if s.len() == 6 {
            if let Ok(r) = u8::from_str_radix(&s[0..2], 16) {
                if let Ok(g) = u8::from_str_radix(&s[2..4], 16) {
                    if let Ok(b) = u8::from_str_radix(&s[4..6], 16) {
                        return (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
                    }
                }
            }
        }
        (0.0, 0.0, 0.0)
    }

    pub fn background_rgb(&self) -> (f32, f32, f32) { Self::parse_hex(&self.background) }
    pub fn foreground_rgb(&self) -> (f32, f32, f32) { Self::parse_hex(&self.foreground) }
    pub fn cursor_rgb(&self) -> (f32, f32, f32) { Self::parse_hex(&self.cursor) }
    pub fn selection_rgb(&self) -> (f32, f32, f32) { Self::parse_hex(&self.selection_bg) }

    pub fn color_rgb(&self, idx: u8, is_bright: bool) -> (f32, f32, f32) {
        let s = match (idx, is_bright) {
            (0, false) => &self.black,
            (1, false) => &self.red,
            (2, false) => &self.green,
            (3, false) => &self.yellow,
            (4, false) => &self.blue,
            (5, false) => &self.magenta,
            (6, false) => &self.cyan,
            (7, false) => &self.white,
            (0, true) => &self.bright_black,
            (1, true) => &self.bright_red,
            (2, true) => &self.bright_green,
            (3, true) => &self.bright_yellow,
            (4, true) => &self.bright_blue,
            (5, true) => &self.bright_magenta,
            (6, true) => &self.bright_cyan,
            (7, true) => &self.bright_white,
            _ => &self.white,
        };
        Self::parse_hex(s)
    }

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
