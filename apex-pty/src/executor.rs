use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncReadExt;

static COMMAND_ID: AtomicU64 = AtomicU64::new(1);

pub fn random_delimiter() -> String {
    let id = COMMAND_ID.fetch_add(1, Ordering::Relaxed);
    format!("APEX_DELIM_{:016x}", id)
}

fn shell_quote(s: &str) -> String {
    let escaped = s.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

pub struct RemoteExecutor;

impl RemoteExecutor {
    pub fn new() -> Self {
        RemoteExecutor
    }

    pub fn wrap_command(command: &str) -> WrappedCommand {
        let start = random_delimiter();
        let end = random_delimiter();
        let code = random_delimiter();

        let wrapped = format!(
            "echo; echo {}; {}; R=$?; echo {}; echo $R; echo {}",
            start, shell_quote(command), end, code
        );

        WrappedCommand {
            start_delim: start,
            end_delim: end,
            code_delim: code,
            wrapped,
        }
    }
}

pub struct WrappedCommand {
    pub start_delim: String,
    pub end_delim: String,
    pub code_delim: String,
    pub wrapped: String,
}

impl WrappedCommand {
    pub async fn execute(&self) -> anyhow::Result<String> {
        const MAX_OUTPUT: u64 = 1024 * 1024;
        let mut child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&self.wrapped)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        let mut stdout = Vec::new();
        if let Some(ref mut out) = child.stdout {
            out.take(MAX_OUTPUT).read_to_end(&mut stdout).await?;
        }
        let _ = child.wait().await;

        Ok(String::from_utf8_lossy(&stdout).to_string())
    }
}

pub fn which_binary(binary: &str) -> String {
    format!("which {} 2>/dev/null || command -v {} 2>/dev/null || echo NOT_FOUND", binary, binary)
}
