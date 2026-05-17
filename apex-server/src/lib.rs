pub mod session;
pub mod socket;

use session::SessionManager;
use tokio::net::UnixListener;
use tokio::task::JoinSet;
use tokio::sync::Semaphore;
use std::sync::Arc;
use std::path::PathBuf;

pub const SOCKET_PATH: &str = "/tmp/apex-terminal.sock";

pub async fn run_server() -> anyhow::Result<()> {
    let socket_path = PathBuf::from(SOCKET_PATH);

    let _ = std::fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path)?;
    let session_manager = SessionManager::new();
    let mut join_set: JoinSet<()> = JoinSet::new();
    let semaphore = Arc::new(Semaphore::new(16));

    tracing::info!("Apex Terminal server listening on {}", SOCKET_PATH);

    loop {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let (stream, _addr) = listener.accept().await?;
        let mut mgr = session_manager.clone();
        join_set.spawn(async move {
            let _permit = permit;
            if let Err(e) = socket::handle_client(stream, &mut mgr).await {
                tracing::error!("Client handler error: {}", e);
            }
        });
        while let Some(result) = join_set.try_join_next() {
            if let Err(panic_err) = result {
                tracing::error!("Client handler panicked: {:?}", panic_err);
            }
        }
    }
}
