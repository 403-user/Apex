use std::collections::HashMap;
use apex_server::session::SessionManager;
use crate::layout::Layout;

pub struct Multiplexer {
    session_manager: SessionManager,
    layouts: HashMap<uuid::Uuid, Layout>,
}

impl Multiplexer {
    pub fn new() -> Self {
        Multiplexer {
            session_manager: SessionManager::new(),
            layouts: HashMap::new(),
        }
    }

    pub fn create_session(&mut self, name: &str) -> uuid::Uuid {
        let id = self.session_manager.create_session(name.to_string());
        let initial_layout = Layout::new_leaf(uuid::Uuid::new_v4());
        self.layouts.insert(id, initial_layout);
        id
    }

    pub fn get_layout(&self, session_id: uuid::Uuid) -> Option<&Layout> {
        self.layouts.get(&session_id)
    }

    pub fn set_layout(&mut self, session_id: uuid::Uuid, layout: Layout) {
        self.layouts.insert(session_id, layout);
    }

    pub fn session_manager(&self) -> &SessionManager {
        &self.session_manager
    }
}
