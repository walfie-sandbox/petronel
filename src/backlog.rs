#[derive(Clone, Debug, PartialEq)]
pub struct Backlog<T> {
    buffer: Vec<T>,
    next_index: usize,
}

impl<T> Backlog<T>
where
    T: Clone,
{
    pub fn with_capacity(capacity: usize) -> Self {
        Backlog {
            buffer: Vec::with_capacity(capacity),
            next_index: 0,
        }
    }

    pub fn push(&mut self, item: T) {
        if self.buffer.len() < self.buffer.capacity() {
            self.buffer.push(item);
        } else {
            self.buffer[self.next_index] = item;
        }
        self.next_index = (self.next_index + 1) % self.buffer.capacity();
    }

    pub fn snapshot(&self) -> Vec<T> {
        let (left, right) = self.buffer.split_at(self.next_index);

        let mut items = Vec::with_capacity(self.buffer.len());

        items.extend(right.into_iter().cloned());
        items.extend(left.into_iter().cloned());

        items
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn maintain_initial_capacity() {
        let mut backlog = Backlog::with_capacity(2);

        backlog.push(1);
        assert_eq!(backlog.snapshot(), vec![1]);

        backlog.push(2);
        assert_eq!(backlog.snapshot(), vec![1, 2]);

        backlog.push(3);
        assert_eq!(backlog.snapshot(), vec![2, 3]);
    }

    #[test]
    fn multiple_overflows() {
        let mut backlog = Backlog::with_capacity(5);

        for i in 0..100 {
            backlog.push(i);
        }

        assert_eq!(backlog.snapshot(), (95..100).collect::<Vec<_>>());
    }
}
