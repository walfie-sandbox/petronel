#[derive(Clone, Debug, PartialEq)]
pub struct CircularBuffer<T> {
    buffer: Vec<T>,
    next_index: usize,
}

impl<T> CircularBuffer<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        CircularBuffer {
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

    pub fn as_unordered_slice(&self) -> &[T] {
        self.buffer.as_slice()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::fmt::Debug;

    fn unordered_eq<T>(buf: &CircularBuffer<T>, other: Vec<T>)
    where
        T: Ord + Clone + Debug + PartialEq,
    {
        let mut latest = buf.as_unordered_slice().to_vec();
        latest.sort();

        assert_eq!(latest, other);
    }

    #[test]
    fn maintain_initial_capacity() {
        let mut buf = CircularBuffer::with_capacity(2);

        buf.push(1);
        unordered_eq(&buf, vec![1]);

        buf.push(2);
        unordered_eq(&buf, vec![1, 2]);

        buf.push(3);
        unordered_eq(&buf, vec![2, 3]);
    }

    #[test]
    fn unordered() {
        let mut buf = CircularBuffer::with_capacity(4);

        for i in 0..50 {
            buf.push(i);
        }

        let mut latest = buf.as_unordered_slice().to_vec();
        latest.sort();

        assert_eq!(latest, (46..50).collect::<Vec<_>>());
    }

    #[test]
    fn multiple_overflows() {
        let mut buf = CircularBuffer::with_capacity(5);

        for i in 0..100 {
            buf.push(i);
        }

        unordered_eq(&buf, vec![95, 96, 97, 98, 99]);
    }
}
