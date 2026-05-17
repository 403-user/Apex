use tokio::net::UnixStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::session::SessionManager;
use apex_protocol::wire::{Message, MessageType, MAX_MESSAGE_SIZE};

pub async fn handle_client(mut stream: UnixStream, manager: &mut SessionManager) -> anyhow::Result<()> {
    let mut len_buf = [0u8; 4];

    loop {
        // Read the 4-byte length prefix
        if stream.read_exact(&mut len_buf).await.is_err() {
            break;
        }
        let len = u32::from_le_bytes(len_buf) as usize;
        if len > MAX_MESSAGE_SIZE {
            let err = Message::error(format!("Message too large: {} bytes (max {})", len, MAX_MESSAGE_SIZE));
            stream.write_all(&err.to_bytes()).await?;
            break;
        }

        // Read the full payload directly into a buffer prefixed with the length
        let mut full = vec![0u8; 4 + len];
        full[..4].copy_from_slice(&len_buf);
        if stream.read_exact(&mut full[4..]).await.is_err() {
            break;
        }

        match Message::from_bytes(&full) {
            Ok(msg) => {
                let response = match msg.msg_type {
                    MessageType::CreateSession => {
                        let id = manager.create_session(msg.payload);
                        Message::ok(id.to_string())
                    }
                    MessageType::ListSessions => {
                        let sessions = manager.list_sessions();
                        let payload = serde_json::to_string(&sessions)?;
                        Message::ok(payload)
                    }
                    MessageType::SplitPane => {
                        let id = uuid::Uuid::parse_str(&msg.payload)?;
                        match manager.split_pane(id, true) {
                            Ok(_) => Message::ok("Pane split".into()),
                            Err(e) => Message::error(e.to_string()),
                        }
                    }
                    MessageType::KillSession => {
                        let id = uuid::Uuid::parse_str(&msg.payload)?;
                        manager.destroy_session(id);
                        Message::ok("Session destroyed".into())
                    }
                    _ => Message::error("Unknown command".into()),
                };
                let encoded = response.to_bytes();
                stream.write_all(&encoded).await?;
            }
            Err(e) => {
                let err = Message::error(format!("Parse error: {}", e));
                stream.write_all(&err.to_bytes()).await?;
            }
        }
    }

    Ok(())
}
