use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notebook {
    pub id: Uuid,
    pub title: String,
    pub cells: Vec<NotebookCell>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotebookCell {
    Markdown { content: String },
    Code { language: String, content: String, output: Option<String> },
    Command { command: String, output: String, exit_code: i32 },
    Finding { title: String, severity: String, description: String },
}

impl Notebook {
    pub fn new(title: &str) -> Self {
        Notebook {
            id: Uuid::new_v4(),
            title: title.to_string(),
            cells: Vec::new(),
            created_at: chrono_now(),
        }
    }

    pub fn add_cell(&mut self, cell: NotebookCell) {
        self.cells.push(cell);
    }
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", secs)
}
