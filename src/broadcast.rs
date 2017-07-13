use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;

pub trait Subscriber {
    type Item;

    fn send(&mut self, message: &Self::Item) -> Result<(), ()>;
}

#[derive(Clone, Debug)]
pub struct EmptySubscriber<T = ::model::Message>(PhantomData<T>);
impl<T> Subscriber for EmptySubscriber<T> {
    type Item = T;

    fn send(&mut self, _message: &T) -> Result<(), ()> {
        Ok(())
    }
}

pub struct Broadcast<Id, S> {
    subscribers: HashMap<Id, S>,
}

impl<Id, S> Broadcast<Id, S>
where
    Id: Eq + Hash,
    S: Subscriber,
{
    pub fn new() -> Self {
        Broadcast { subscribers: HashMap::new() }
    }
}

impl<Id, S> Broadcast<Id, S>
where
    Id: Eq + Hash,
    S: Subscriber,
{
    pub fn is_empty(&self) -> bool {
        self.subscribers.is_empty()
    }

    pub fn get(&self, id: &Id) -> Option<&S> {
        self.subscribers.get(id)
    }

    pub fn subscribe(&mut self, id: Id, subscriber: S) -> Option<S> {
        self.subscribers.insert(id, subscriber)
    }

    pub fn unsubscribe(&mut self, id: &Id) -> Option<S> {
        self.subscribers.remove(id)
    }

    pub fn send(&mut self, message: &S::Item) {
        // Remove any subscribers that return an error
        self.subscribers.retain(|_, subscriber| {
            subscriber.send(message).is_ok()
        })
    }
}
