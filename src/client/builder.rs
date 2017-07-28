use Token;
use broadcast::{Broadcast, NoOpSubscriber, Subscriber};
use client::{Client, Event, Worker};
use error::*;
use futures::Stream;
use futures::unsync::mpsc;
use hyper;
use hyper::client::Connect;
use id_pool::IdPool;
use image_hash::{self, BossImageHash, HyperImageHasher, ImageHasher};
use model::Message;
use raid::{RaidInfo, RaidInfoStream};
use std::collections::HashMap;
use std::marker::PhantomData;

#[derive(Clone, Debug)]
pub struct ClientBuilder<H, S, Sub, F> {
    stream: S,
    history_size: usize,
    image_hasher: H,
    map_message: F,
    subscriber_type: PhantomData<Sub>,
}

const DEFAULT_HISTORY_SIZE: usize = 10;
const MAX_CONCURRENT_IMAGE_HASHER_REQUESTS: usize = 5;

impl ClientBuilder<(), (), (), ()> {
    pub fn new() -> Self {
        ClientBuilder {
            stream: (),
            history_size: DEFAULT_HISTORY_SIZE,
            image_hasher: (),
            map_message: (),
            subscriber_type: PhantomData,
        }
    }
}

impl<'a, C>
    ClientBuilder<HyperImageHasher<'a, C>, RaidInfoStream, NoOpSubscriber, fn(Message) -> ()>
where
    C: Connect,
{
    pub fn from_hyper_client(hyper_client: &'a hyper::Client<C>, token: &Token) -> Self {
        let stream = RaidInfoStream::with_client(hyper_client, token);

        let image_hasher = HyperImageHasher(hyper_client);

        ClientBuilder {
            stream,
            history_size: DEFAULT_HISTORY_SIZE,
            image_hasher,
            map_message: (|_| ()) as fn(Message) -> (),
            subscriber_type: PhantomData,
        }
    }
}

impl<H, S, Sub, F> ClientBuilder<H, S, Sub, F> {
    pub fn with_history_size(mut self, size: usize) -> Self {
        self.history_size = size;
        self
    }

    pub fn with_stream<S2>(self, stream: S2) -> ClientBuilder<H, S2, Sub, F>
    where
        S: Stream<Item = RaidInfo, Error = Error>,
    {
        ClientBuilder {
            stream,
            history_size: self.history_size,
            image_hasher: self.image_hasher,
            map_message: self.map_message,
            subscriber_type: self.subscriber_type,
        }
    }

    pub fn with_image_hasher<H2>(self, image_hasher: H2) -> ClientBuilder<H2, S, Sub, F> {
        ClientBuilder {
            stream: self.stream,
            history_size: self.history_size,
            image_hasher,
            map_message: self.map_message,
            subscriber_type: self.subscriber_type,
        }
    }

    pub fn with_subscriber<Sub2>(self) -> ClientBuilder<H, S, Sub2, F>
    where
        Sub2: Subscriber,
    {
        ClientBuilder {
            stream: self.stream,
            history_size: self.history_size,
            image_hasher: self.image_hasher,
            map_message: self.map_message,
            subscriber_type: PhantomData,
        }
    }

    pub fn map_message<F2, T>(self, f: F2) -> ClientBuilder<H, S, Sub, F2>
    where
        F2: Fn(Message) -> T,
    {
        ClientBuilder {
            stream: self.stream,
            history_size: self.history_size,
            image_hasher: self.image_hasher,
            map_message: f,
            subscriber_type: self.subscriber_type,
        }
    }

    pub fn build(self) -> (Client<Sub>, Worker<H, S, Sub, F>)
    where
        S: Stream<Item = RaidInfo, Error = Error>,
        H: ImageHasher,
        Sub: Subscriber,
        F: Fn(Message) -> Sub::Item,
    {
        let (tx, rx) = mpsc::unbounded();

        let stream_events = self.stream.map(
            Event::NewRaidInfo as fn(RaidInfo) -> Event<Sub>,
        );
        let rx = rx.or_else((|()| Ok(Event::ReadError)) as fn(()) -> Result<Event<Sub>>);

        let (hash_requester, hash_receiver) =
            image_hash::channel(self.image_hasher, MAX_CONCURRENT_IMAGE_HASHER_REQUESTS);
        let hash_events = hash_receiver.map(
            (|msg| {
                 Event::NewImageHash {
                     boss_name: msg.boss_name,
                     image_hash: msg.image_hash,
                 }
             }) as fn(BossImageHash) -> Event<Sub>,
        );

        let future = Worker {
            hash_requester,
            id_pool: IdPool::new(),
            events: stream_events.select(rx.select(hash_events)),
            bosses: HashMap::new(),
            tweet_history_size: self.history_size,
            requested_bosses: HashMap::new(),
            subscribers: Broadcast::new(),
            heartbeat: (self.map_message)(Message::Heartbeat),
            map_message: self.map_message,
        };

        (Client(tx), future)
    }
}
