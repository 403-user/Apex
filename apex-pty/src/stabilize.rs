#[derive(Debug, Clone)]
pub struct PtyProbe {
    pub binary: String,
    pub payload: String,
    pub priority: u32,
}

pub struct PtyStabilizer {
    pub probes: Vec<PtyProbe>,
    pub has_pty: bool,
    pub current_shell: String,
    pub better_shells: Vec<String>,
}

impl Default for PtyStabilizer {
    fn default() -> Self {
        PtyStabilizer::new()
    }
}

impl PtyStabilizer {
    pub fn new() -> Self {
        let probes = vec![
            PtyProbe {
                binary: "script".into(),
                payload: "script -qc /bin/bash /dev/null 2>&1".into(),
                priority: 1,
            },
            PtyProbe {
                binary: "python3".into(),
                payload: "python3 -c 'import pty; pty.spawn(\"/bin/bash\")' 2>&1".into(),
                priority: 2,
            },
            PtyProbe {
                binary: "python".into(),
                payload: "python -c 'import pty; pty.spawn(\"/bin/bash\")' 2>&1".into(),
                priority: 3,
            },
            PtyProbe {
                binary: "python2".into(),
                payload: "python2 -c 'import pty; pty.spawn(\"/bin/bash\")' 2>&1".into(),
                priority: 4,
            },
            PtyProbe {
                binary: "socat".into(),
                payload: "socat -,echo=0,rawer".into(),
                priority: 5,
            },
            PtyProbe {
                binary: "perl".into(),
                payload: "perl -e 'use POSIX qw(setsid); require POSIX; my $name = POSIX::ctermid(); system qq(exec /bin/bash -i <$name >$name 2>$name)' 2>&1".into(),
                priority: 6,
            },
        ];

        PtyStabilizer {
            probes,
            has_pty: false,
            current_shell: "sh".into(),
            better_shells: vec!["bash".into(), "zsh".into(), "ksh".into(), "fish".into()],
        }
    }

    pub fn check_has_pty(&self) -> String {
        "[ -t 1 ] && echo HAS_PTY || echo NO_PTY".into()
    }

    pub fn get_probe_command(&self, probe: &PtyProbe) -> String {
        format!("which {} 2>/dev/null && echo FOUND:{} || echo NOT_FOUND:{}",
            probe.binary, probe.binary, probe.binary)
    }

    pub fn detect_shell_command(&self) -> String {
        "basename $(readlink /proc/$$/exe 2>/dev/null || echo sh) 2>/dev/null".into()
    }

    pub fn upgrade_shell_command(&self, shell: &str) -> String {
        format!("exec {} 2>/dev/null || true", shell)
    }

    pub fn set_interactive_env(&self, rows: u16, cols: u16, term: &str) -> String {
        format!(
            "stty sane 2>/dev/null; stty rows {} columns {} 2>/dev/null; export TERM='{}'; export PS1='{}'",
            rows, cols, term,
            "$(command printf '(remote) \\\\u@\\\\h:\\\\w\\\\$ ')"
        )
    }

    pub fn set_noninteractive_env(&self) -> String {
        "stty -echo nl lnext ^V 2>/dev/null; export PS1=".into()
    }

    pub fn get_term_size_command(&self) -> String {
        "echo \"ROWS=$(stty size 2>/dev/null | cut -d' ' -f1) COLS=$(stty size 2>/dev/null | cut -d' ' -f2)\"".into()
    }

    pub fn sync_paths_command(&self) -> String {
        let wanted = [
            "/bin", "/usr/bin", "/usr/local/bin",
            "/sbin", "/usr/sbin", "/usr/local/sbin",
        ];
        let path_check: Vec<String> = wanted.iter().map(|p| {
            format!("case :$PATH: in *:{path}:*) ;; *) export PATH=\"$PATH:{path}\" ;; esac", path=p)
        }).collect();
        path_check.join("; ")
    }

    pub fn query_shell_history_command(&self) -> String {
        "echo \"[apex:histoff]\"; export HISTFILE=/dev/null; unset HISTFILE 2>/dev/null".into()
    }
}

