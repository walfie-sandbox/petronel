type Id = u32;

pub struct IdPool {
    max_id: Id,
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

            self.max_id += 1;

            id
        })
    }

    // This should never be called with `id` less than `max_id`
    pub fn push(&mut self, id: Id) {
        debug_assert!(id <= self.max_id);
        self.available.push(id)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn use_id() {
        let mut pool = IdPool::new();

        assert_eq!(pool.get(), 0);
        assert_eq!(pool.get(), 1);
        assert_eq!(pool.get(), 2);

        pool.push(1);
        pool.push(0);
        assert_eq!(pool.get(), 0);
        assert_eq!(pool.get(), 1);
        assert_eq!(pool.get(), 3);
        assert_eq!(pool.get(), 4);
    }
}
