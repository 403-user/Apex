use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use serde::Serialize;

pub type SessionId = Uuid;

#[derive(Clone, Serialize)]
pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub windows: Vec<Window>,
    pub active_window: usize,
}

#[derive(Clone, Serialize)]
pub struct Window {
    pub id: Uuid,
    pub title: String,
    pub panes: Vec<Pane>,
    pub active_pane: usize,
}

#[derive(Clone, Serialize)]
pub struct Pane {
    pub id: Uuid,
    pub pty_fd: Option<i32>,
    pub rows: usize,
    pub cols: usize,
    pub scroll_offset: i64,
}

impl Session {
    pub fn new(name: String) -> Self {
        let main_pane = Pane {
            id: Uuid::new_v4(),
            pty_fd: None,
            rows: 24,
            cols: 80,
            scroll_offset: 0,
        };
        let main_window = Window {
            id: Uuid::new_v4(),
            title: "main".into(),
            panes: vec![main_pane],
            active_pane: 0,
        };
        Session {
            id: Uuid::new_v4(),
            name,
            windows: vec![main_window],
            active_window: 0,
        }
    }
}

#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<SessionId, Session>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        SessionManager {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn create_session(&self, name: String) -> SessionId {
        let session = Session::new(name);
        let id = session.id;
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.insert(id, session);
        id
    }

    pub fn split_pane(&self, session_id: SessionId, _vertical: bool) -> anyhow::Result<()> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions.get_mut(&session_id)
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;
        if session.active_window >= session.windows.len() {
            anyhow::bail!("active_window {} out of bounds ({} windows)", session.active_window, session.windows.len());
        }
        let window = &mut session.windows[session.active_window];
        let new_pane = Pane {
            id: Uuid::new_v4(),
            pty_fd: None,
            rows: window.panes[0].rows,
            cols: window.panes[0].cols / 2,
            scroll_offset: 0,
        };
        window.panes.push(new_pane);
        Ok(())
    }

    pub fn list_sessions(&self) -> Vec<Session> {
        let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.values().cloned().collect()
    }

    pub fn get_session(&self, id: SessionId) -> Option<Session> {
        let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.get(&id).cloned()
    }

    pub fn destroy_session(&self, id: SessionId) {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.remove(&id);
    }
}
