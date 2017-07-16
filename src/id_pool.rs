#[derive(Clone, Hash, Debug, PartialEq, Eq)]
pub struct Id(u32);

#[derive(Debug)]
pub struct IdPool {
    max_id: u32,
    available: Vec<Id>,
}

impl IdPool {
    pub fn new() -> Self {
        IdPool {
            max_id: 0,
            available: Vec::new(),
        }
    }

    pub fn get(&mut self) -> Id {
        self.available.pop().unwrap_or_else(|| {
            let id = self.max_id;

            // This will panic when max_id is at u32's max value,
            // but in practice, there would never be so many
            // IDs being used at the same time.
            self.max_id += 1;

            Id(id)
        })
    }

    // This should never be called with `id` less than `max_id`
    pub fn recycle(&mut self, id: Id) {
        debug_assert!(id.0 <= self.max_id);
        self.available.push(id);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn use_id() {
        let mut pool = IdPool::new();

        assert_eq!(pool.get(), Id(0));
        assert_eq!(pool.get(), Id(1));
        assert_eq!(pool.get(), Id(2));

        pool.recycle(Id(1));
        pool.recycle(Id(0));
        assert_eq!(pool.get(), Id(0));
        assert_eq!(pool.get(), Id(1));
        assert_eq!(pool.get(), Id(3));
        assert_eq!(pool.get(), Id(4));
    }
}
