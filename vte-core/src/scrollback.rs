use std::collections::VecDeque;
use crate::grid::Row;

pub struct ScrollbackBuffer {
    pub lines: VecDeque<Row>,
    pub max_lines: usize,
}

impl ScrollbackBuffer {
    pub fn new(max_lines: usize) -> Self {
        ScrollbackBuffer {
            lines: VecDeque::with_capacity(max_lines.min(1000)),
            max_lines,
        }
    }

    pub fn push(&mut self, row: Row) {
        if self.lines.len() >= self.max_lines {
            self.lines.pop_front();
        }
        self.lines.push_back(row);
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn get(&self, index: usize) -> Option<&Row> {
        self.lines.get(index)
    }
}
