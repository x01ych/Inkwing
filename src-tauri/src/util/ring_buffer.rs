use std::collections::VecDeque;

/// Bounded FIFO buffer that drops oldest on overflow.
pub struct RingBuffer<T> {
    inner: VecDeque<T>,
    cap: usize,
    dropped: u64,
}

impl<T> RingBuffer<T> {
    pub fn new(cap: usize) -> Self {
        Self {
            inner: VecDeque::with_capacity(cap),
            cap,
            dropped: 0,
        }
    }

    pub fn push(&mut self, item: T) {
        if self.inner.len() == self.cap {
            self.inner.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.inner.push_back(item);
    }

    pub fn snapshot(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.inner.iter().cloned().collect()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn clear(&mut self) {
        self.inner.clear();
        self.dropped = 0;
    }
}
