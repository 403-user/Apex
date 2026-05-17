use std::collections::HashMap;
use uuid::Uuid;

pub struct ShareManager {
    pub sessions: HashMap<Uuid, Vec<Uuid>>,
}

impl ShareManager {
    pub fn new() -> Self {
        ShareManager {
            sessions: HashMap::new(),
        }
    }

    pub fn share_session(&mut self, session_id: Uuid, peer_id: Uuid) {
        self.sessions.entry(session_id).or_default().push(peer_id);
    }

    pub fn get_peers(&self, session_id: &Uuid) -> Vec<Uuid> {
        self.sessions.get(session_id).cloned().unwrap_or_default()
    }
}
