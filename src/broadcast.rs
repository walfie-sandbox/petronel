use model::Message;
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;

pub trait Subscriber {
    type Id;
    type Item;

    fn send(&mut self, message: &Self::Item);
}

#[derive(Clone, Debug)]
pub struct EmptySubscriber<Id = (), T = Message>(PhantomData<Id>, PhantomData<T>);
impl<Id, T> Subscriber for EmptySubscriber<Id, T> {
    type Id = Id;
    type Item = T;

    fn send(&mut self, _message: &T) {}
}

pub struct Broadcast<S>
where
    S: Subscriber,
{
    subscribers: HashMap<S::Id, S>,
}

impl<S> Broadcast<S>
where
    S: Subscriber,
    S::Id: Eq + Hash,
{
    pub fn new() -> Self {
        Broadcast { subscribers: HashMap::new() }
    }
}

impl<S> Broadcast<S>
where
    S: Subscriber,
    S::Id: Eq + Hash,
{
    pub fn is_empty(&self) -> bool {
        self.subscribers.is_empty()
    }

    pub fn get(&self, id: &S::Id) -> Option<&S> {
        self.subscribers.get(id)
    }

    pub fn subscribe(&mut self, id: S::Id, subscriber: S) -> Option<S> {
        self.subscribers.insert(id, subscriber)
    }

    pub fn unsubscribe(&mut self, id: &S::Id) -> Option<S> {
        self.subscribers.remove(id)
    }

    pub fn send(&mut self, message: &S::Item) {
        for subscriber in self.subscribers.values_mut() {
            subscriber.send(message)
        }
    }
}
