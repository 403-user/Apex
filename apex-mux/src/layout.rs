use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LayoutDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Layout {
    Leaf { id: uuid::Uuid, ratio: f32 },
    Split {
        direction: LayoutDirection,
        children: Vec<Layout>,
        ratios: Vec<f32>,
    },
}

impl Layout {
    pub fn new_leaf(id: uuid::Uuid) -> Self {
        Layout::Leaf { id, ratio: 1.0 }
    }

    pub fn split_horizontal(left: Layout, right: Layout, ratio: f32) -> Self {
        Layout::Split {
            direction: LayoutDirection::Horizontal,
            children: vec![left, right],
            ratios: vec![ratio, 1.0 - ratio],
        }
    }

    pub fn split_vertical(top: Layout, bottom: Layout, ratio: f32) -> Self {
        Layout::Split {
            direction: LayoutDirection::Vertical,
            children: vec![top, bottom],
            ratios: vec![ratio, 1.0 - ratio],
        }
    }

    pub fn pane_ids(&self) -> Vec<uuid::Uuid> {
        match self {
            Layout::Leaf { id, .. } => vec![*id],
            Layout::Split { children, .. } => {
                children.iter().flat_map(|c| c.pane_ids()).collect()
            }
        }
    }
}
