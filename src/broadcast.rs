use futures::Sink;
use std::collections::HashMap;
use std::hash::Hash;

pub trait Subscriber {
    type Item;

    fn send(&mut self, message: &Self::Item) -> Result<(), ()>;
    fn maybe_send(&mut self, message: Option<&Self::Item>) -> Result<(), ()> {
        if let Some(msg) = message {
            self.send(msg)
        } else {
            Ok(())
        }
    }
}

impl<S> Subscriber for S
where
    S: Sink,
    S::SinkItem: Clone,
{
    type Item = S::SinkItem;

    fn send(&mut self, message: &Self::Item) -> Result<(), ()> {
        self.start_send(message.clone().into())
            .and_then(|_| self.poll_complete().map(|_| ()))
            .map_err(|_| ())
    }
}

#[derive(Clone, Debug)]
pub struct NoOpSubscriber;
impl Subscriber for NoOpSubscriber {
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
        Broadcast {
            subscribers: HashMap::new(),
        }
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

    pub fn get_mut(&mut self, id: &Id) -> Option<&mut S> {
        self.subscribers.get_mut(id)
    }

    pub fn subscribe(&mut self, id: Id, subscriber: S) -> Option<S> {
        self.subscribers.insert(id, subscriber)
    }

    pub fn unsubscribe(&mut self, id: &Id) -> Option<S> {
        self.subscribers.remove(id)
    }

    pub(crate) fn maybe_send(&mut self, message: Option<&S::Item>) {
        if let Some(msg) = message {
            self.send(msg)
        }
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }

    pub fn send(&mut self, message: &S::Item) {
        // Remove any subscribers that return an error
        self.subscribers
            .retain(|_, subscriber| subscriber.send(message).is_ok())
    }
}
