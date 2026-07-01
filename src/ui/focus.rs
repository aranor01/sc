#[derive(Debug, Clone)]
pub struct FocusRing {
    count: usize,
    current: usize,
}

impl FocusRing {
    pub fn new(count: usize) -> Self {
        Self { count, current: 0 }
    }

    pub fn next(&mut self) {
        if self.count > 0 {
            self.current = (self.current + 1) % self.count;
        }
    }

    pub fn prev(&mut self) {
        if self.count > 0 {
            self.current = (self.current + self.count - 1) % self.count;
        }
    }

    pub fn current(&self) -> usize {
        self.current
    }

    pub fn is_focused(&self, idx: usize) -> bool {
        self.current == idx
    }

    pub fn set(&mut self, idx: usize) {
        self.current = idx.min(self.count.saturating_sub(1));
    }
}
