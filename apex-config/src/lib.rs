pub mod lua;
pub mod theme;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApexConfig {
    pub font_size: f64,
    pub font_family: String,
    pub theme: String,
    pub opacity: f64,
    pub padding_x: u32,
    pub padding_y: u32,
    pub scrollback_lines: u32,
    pub cursor_style: String,
    pub enable_gpu: bool,
    pub multiplexer: MultiplexerConfig,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MultiplexerConfig {
    pub socket_path: String,
    pub auto_attach: bool,
    pub resurrect: bool,
}

impl Default for ApexConfig {
    fn default() -> Self {
        ApexConfig {
            font_size: 14.0,
            font_family: "JetBrains Mono".into(),
            theme: "kali-dark".into(),
            opacity: 0.95,
            padding_x: 8,
            padding_y: 4,
            scrollback_lines: 10000,
            cursor_style: "block".into(),
            enable_gpu: true,
            multiplexer: MultiplexerConfig {
                socket_path: "/tmp/apex-terminal.sock".into(),
                auto_attach: true,
                resurrect: true,
            },
        }
    }
}

pub fn load_config(path: Option<&str>) -> anyhow::Result<ApexConfig> {
    match path {
        Some(p) => {
            let content = std::fs::read_to_string(p)?;
            let config: ApexConfig = toml::from_str(&content)?;
            Ok(config)
        }
        None => Ok(ApexConfig::default()),
    }
}
