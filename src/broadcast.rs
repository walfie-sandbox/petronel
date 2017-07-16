use futures::Sink;
use std::collections::HashMap;
use std::hash::Hash;

pub trait Subscriber {
    type Item;

    fn send(&mut self, message: &Self::Item) -> Result<(), ()>;
}

pub struct SinkSubscriber<T>(T);

impl<T> From<T> for SinkSubscriber<T>
where
    T: Sink,
    T::SinkItem: Clone,
{
    fn from(sink: T) -> Self {
        SinkSubscriber(sink)
    }
}

impl<T> Subscriber for SinkSubscriber<T>
where
    T: Sink,
    T::SinkItem: Clone,
{
    type Item = T::SinkItem;

    fn send(&mut self, message: &Self::Item) -> Result<(), ()> {
        self.0
            .start_send(message.clone().into())
            .and_then(|_| self.0.poll_complete().map(|_| ()))
            .map_err(|_| ())
    }
}

#[derive(Clone, Debug)]
pub struct EmptySubscriber;
impl Subscriber for EmptySubscriber {
    type Item = ();

    fn send(&mut self, _message: &Self::Item) -> Result<(), ()> {
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
