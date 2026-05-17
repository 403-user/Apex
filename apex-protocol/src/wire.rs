use serde::{Deserialize, Serialize};

pub const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024; // 10 MiB

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MessageType {
    CreateSession,
    ListSessions,
    SplitPane,
    KillSession,
    DetachSession,
    AttachSession,
    ResizePane,
    SendInput,
    ReceiveOutput,
    Ack,
    Error,
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub msg_type: MessageType,
    pub payload: String,
    pub correlation_id: u64,
    pub flags: u32,
}

impl Message {
    pub fn new(msg_type: MessageType, payload: String) -> Self {
        Message {
            msg_type,
            payload,
            correlation_id: 0,
            flags: 0,
        }
    }

    pub fn ok(payload: String) -> Self {
        Message::new(MessageType::Ack, payload)
    }

    pub fn error(payload: String) -> Self {
        Message::new(MessageType::Error, payload)
    }

    pub fn ping() -> Self {
        Message::new(MessageType::Ping, String::new())
    }

    pub fn pong() -> Self {
        Message::new(MessageType::Pong, String::new())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let json = serde_json::to_string(self).unwrap_or_default();
        let len = u32::try_from(json.len()).unwrap_or(MAX_MESSAGE_SIZE as u32);
        let mut bytes = Vec::with_capacity(4 + json.len());
        bytes.extend_from_slice(&len.to_le_bytes());
        bytes.extend_from_slice(json.as_bytes());
        bytes
    }

    pub fn from_bytes(data: &[u8]) -> anyhow::Result<Self> {
        if data.len() < 4 {
            return Err(anyhow::anyhow!("Message too short"));
        }
        let len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if len > MAX_MESSAGE_SIZE {
            return Err(anyhow::anyhow!("Message too large: {} bytes (max {})", len, MAX_MESSAGE_SIZE));
        }
        if data.len() < 4 + len {
            return Err(anyhow::anyhow!("Incomplete message"));
        }
        let json = std::str::from_utf8(&data[4..4 + len])?;
        let msg: Message = serde_json::from_str(json)?;
        Ok(msg)
    }
}
