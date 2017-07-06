use std::cell::RefCell;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct Backlog<T> {
    buffer: Vec<T>,
    revision: u32,
    next_index: usize,
    snapshot: RefCell<BacklogSnapshot<T>>,
}

#[derive(Clone, Debug, PartialEq)]
struct BacklogSnapshot<T> {
    buffer: Arc<Vec<T>>,
    revision: u32,
}

impl<T: Clone> Backlog<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        Backlog {
            buffer: Vec::with_capacity(capacity),
            revision: 0,
            next_index: 0,
            snapshot: RefCell::new(BacklogSnapshot {
                buffer: Arc::new(Vec::with_capacity(0)),
                revision: 0,
            }),
        }
    }

    pub fn push(&mut self, item: T) {
        if self.buffer.len() < self.buffer.capacity() {
            self.buffer.push(item);
        } else {
            self.buffer[self.next_index] = item;
        }
        self.next_index = (self.next_index + 1) % self.buffer.capacity();
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn snapshot(&self) -> Arc<Vec<T>> {
        let mut latest = self.snapshot.borrow_mut();

        if latest.revision == self.revision {
            latest.buffer.clone()
        } else {
            let (left, right) = self.buffer.split_at(self.next_index);

            let mut items = Vec::with_capacity(self.buffer.len());

            items.extend(right.into_iter().cloned());
            items.extend(left.into_iter().cloned());

            latest.revision = self.revision;
            latest.buffer = Arc::new(items);

            latest.buffer.clone()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn maintain_initial_capacity() {
        let mut backlog = Backlog::with_capacity(2);

        backlog.push(1);
        assert_eq!(*backlog.snapshot(), vec![1]);

        backlog.push(2);
        assert_eq!(*backlog.snapshot(), vec![1, 2]);

        backlog.push(3);
        assert_eq!(*backlog.snapshot(), vec![2, 3]);
    }

    #[test]
    fn multiple_overflows() {
        let mut backlog = Backlog::with_capacity(5);

        for i in 0..100 {
            backlog.push(i);
        }

        assert_eq!(*backlog.snapshot(), (95..100).collect::<Vec<_>>());
    }
}
