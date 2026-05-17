use uuid::Uuid;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub panes: Vec<WorkspacePane>,
    pub active_pane: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspacePane {
    pub id: Uuid,
    pub title: String,
    pub pane_type: PaneType,
    pub bounds: Bounds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaneType {
    Terminal,
    Editor,
    Browser,
    Preview,
    Dashboard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Workspace {
    pub fn new(name: &str) -> Self {
        Workspace {
            id: Uuid::new_v4(),
            name: name.to_string(),
            panes: vec![WorkspacePane {
                id: Uuid::new_v4(),
                title: "terminal".into(),
                pane_type: PaneType::Terminal,
                bounds: Bounds { x: 0.0, y: 0.0, width: 1.0, height: 1.0 },
            }],
            active_pane: 0,
        }
    }

    pub fn add_pane(&mut self, pane_type: PaneType, title: &str) {
        self.panes.push(WorkspacePane {
            id: Uuid::new_v4(),
            title: title.to_string(),
            pane_type,
            bounds: Bounds { x: 0.0, y: 0.0, width: 0.5, height: 1.0 },
        });
    }
}
