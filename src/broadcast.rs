use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;

pub trait Subscriber<T> {
    fn send(&mut self, message: &T);
}

pub struct EmptySubscriber<T>(PhantomData<T>);
impl<T> Subscriber<T> for EmptySubscriber<T> {
    fn send(&mut self, _message: &T) {}
}

pub struct Broadcast<Id, S, T> {
    subscribers: HashMap<Id, S>,
    message_type: PhantomData<T>,
}

impl<Id, S, T> Broadcast<Id, S, T>
where
    Id: Eq + Hash,
    S: Subscriber<T>,
    T: Clone,
{
    pub fn new() -> Self {
        Broadcast {
            subscribers: HashMap::new(),
            message_type: PhantomData,
        }
    }
}

impl<Id, S, T> Broadcast<Id, S, T>
where
    Id: Eq + Hash,
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

    pub fn send(&mut self, message: &T)
    where
        S: Subscriber<T>,
    {
        for subscriber in self.subscribers.values_mut() {
            subscriber.send(message)
        }
    }
}
