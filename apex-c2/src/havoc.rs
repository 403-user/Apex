use anyhow::{Context, Result};
use futures_util::SinkExt;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HavocDemon {
    pub id: u32,
    pub name: String,
    pub hostname: String,
    pub username: String,
    pub domain: String,
    pub os: String,
    pub version: String,
    pub arch: String,
    pub transport: String,
    pub external_ip: String,
    pub internal_ip: String,
    pub process_name: String,
    pub process_id: u32,
    pub integrity: String,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HavocCommandResult {
    pub demon_id: u32,
    pub command_id: u32,
    pub output: String,
    pub completed: bool,
}

#[derive(Debug)]
pub struct HavocConnector {
    pub teamserver_url: String,
    pub websocket_endpoint: String,
    password: String,
    #[allow(dead_code)]
    agent_id: Option<String>,
    connected: bool,
}

impl HavocConnector {
    pub fn new(teamserver: &str, password: &str) -> Self {
        HavocConnector {
            teamserver_url: teamserver.to_string(),
            websocket_endpoint: format!("{}/ws", teamserver.trim_end_matches('/')),
            password: password.to_string(),
            agent_id: None,
            connected: false,
        }
    }

    pub async fn connect_websocket(&mut self) -> Result<()> {
        tracing::info!("Connecting to Havoc TeamServer WebSocket at {}", self.websocket_endpoint);

        let (mut ws_stream, _response) = connect_async(&self.websocket_endpoint)
            .await
            .context("Failed to connect to Havoc WebSocket")?;

        tracing::info!("Havoc WebSocket connected");

        let auth_msg = serde_json::json!({
            "Head": {
                "Event": "Authentication"
            },
            "Body": {
                "Password": self.password,
                "Type": "Operator"
            }
        });

        ws_stream.send(Message::Text(auth_msg.to_string())).await?;
        tracing::info!("Havoc authentication sent");

        self.connected = true;
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub async fn list_demons(&self) -> Result<Vec<HavocDemon>> {
        anyhow::bail!("Havoc list_demons: not implemented - requires Havoc TeamServer WebSocket protocol stubs");
    }

    pub async fn send_command(&self, demon_id: u32, command: &str) -> Result<HavocCommandResult> {
        anyhow::bail!("Havoc send_command({}, {}): not implemented - requires Havoc TeamServer WebSocket stubs", demon_id, command);
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        self.connected = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_havoc_connector_creation() {
        let c = HavocConnector::new("https://10.10.10.1:40056", "password123");
        assert_eq!(c.teamserver_url, "https://10.10.10.1:40056");
        assert_eq!(c.websocket_endpoint, "https://10.10.10.1:40056/ws");
        assert!(!c.is_connected());
    }

    #[tokio::test]
    async fn test_havoc_list_demons_not_implemented() {
        let c = HavocConnector::new("https://10.10.10.1:40056", "test");
        let err = c.list_demons().await.unwrap_err();
        assert!(err.to_string().contains("not implemented"));
    }
}
