use Token;
use broadcast::{Broadcast, NoOpSubscriber, Subscriber};
use circular_buffer::CircularBuffer;
use client::{Client, Event, Worker};
use client::worker::RaidBossEntry;
use error::*;
use futures::Stream;
use futures::unsync::mpsc;
use hyper;
use hyper::client::Connect;
use id_pool::IdPool;
use image_hash::{self, BossImageHash, HyperImageHasher, ImageHasher};
use model::{Message, RaidBossMetadata};
use raid::{RaidInfo, RaidInfoStream};
use std::collections::HashMap;
use std::marker::PhantomData;

#[derive(Clone, Debug)]
pub struct ClientBuilder<H, S, Sub, F> {
    stream: S,
    history_size: usize,
    image_hasher: H,
    filter_map_message: F,
    bosses: Vec<RaidBossMetadata>,
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
            filter_map_message: (),
            bosses: Vec::new(),
            subscriber_type: PhantomData,
        }
    }
}

impl<'a, C>
    ClientBuilder<
        HyperImageHasher<'a, C>,
        RaidInfoStream,
        NoOpSubscriber,
        fn(Message) -> Option<()>,
    >
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
            bosses: Vec::new(),
            filter_map_message: (|_| None) as fn(Message) -> Option<()>,
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
            bosses: self.bosses,
            filter_map_message: self.filter_map_message,
            subscriber_type: self.subscriber_type,
        }
    }

    pub fn with_image_hasher<H2>(self, image_hasher: H2) -> ClientBuilder<H2, S, Sub, F> {
        ClientBuilder {
            stream: self.stream,
            history_size: self.history_size,
            image_hasher,
            bosses: self.bosses,
            filter_map_message: self.filter_map_message,
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
            bosses: self.bosses,
            filter_map_message: self.filter_map_message,
            subscriber_type: PhantomData,
        }
    }

    pub fn filter_map_message<F2, T>(self, f: F2) -> ClientBuilder<H, S, Sub, F2>
    where
        F2: Fn(Message) -> Option<T>,
    {
        ClientBuilder {
            stream: self.stream,
            history_size: self.history_size,
            image_hasher: self.image_hasher,
            bosses: self.bosses,
            filter_map_message: f,
            subscriber_type: self.subscriber_type,
        }
    }

    pub fn with_bosses(mut self, bosses: Vec<RaidBossMetadata>) -> Self {
        self.bosses = bosses;
        self
    }

    pub fn build(self) -> (Client<Sub>, Worker<H, S, Sub, F>)
    where
        S: Stream<Item = RaidInfo, Error = Error>,
        H: ImageHasher,
        Sub: Subscriber,
        F: Fn(Message) -> Option<Sub::Item>,
    {
        let (tx, rx) = mpsc::unbounded();

        let stream_events = self.stream.map(
            Event::NewRaidInfo as fn(RaidInfo) -> Event<Sub>,
        );
        let rx = rx.or_else((|()| Ok(Event::ClientReadError)) as fn(()) -> Result<Event<Sub>>);

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

        let cached_boss_list = (self.filter_map_message)(Message::BossList(&[]));


        let mut bosses = HashMap::new();
        for boss_data in self.bosses.into_iter() {
            let boss_name = boss_data.boss.name.clone();
            let entry = RaidBossEntry {
                boss_data,
                broadcast: Broadcast::new(),
                recent_tweets: CircularBuffer::with_capacity(self.history_size),
            };

            bosses.insert(boss_name, entry);
        }

        let future = Worker {
            hash_requester,
            id_pool: IdPool::new(),
            events: stream_events.select(rx.select(hash_events)),
            bosses,
            tweet_history_size: self.history_size,
            requested_bosses: HashMap::new(),
            subscribers: Broadcast::new(),
            heartbeat: (self.filter_map_message)(Message::Heartbeat),
            filter_map_message: self.filter_map_message,
            cached_boss_list,
        };

        (Client(tx), future)
    }
}
