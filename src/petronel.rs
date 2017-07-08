use broadcast::{Broadcast, Subscriber};
use circular_buffer::CircularBuffer;
use error::*;
use futures::{Async, Future, Poll, Stream};
use futures::stream::{Map, OrElse, Select};
use futures::unsync::mpsc;
use futures::unsync::oneshot;
use raid::{BossImageUrl, BossLevel, BossName, DateTime, Language, RaidInfo, RaidTweet};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::Hash;
use std::iter::FromIterator;
use std::marker::PhantomData;
use std::sync::Arc;

const DEFAULT_BOSS_LEVEL: BossLevel = 0;

pub type Message = Arc<RaidTweet>;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RaidBoss {
    pub name: BossName,
    pub level: BossLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<BossImageUrl>,
    pub language: Language,
}

struct RaidBossEntry<SubId, Sub> {
    boss: RaidBoss,
    last_seen: DateTime,
    recent_tweets: CircularBuffer<Arc<RaidTweet>>,
    broadcast: Broadcast<SubId, Sub, Message>,
}

#[derive(Debug)]
enum Event<SubId, Sub> {
    NewRaidInfo(RaidInfo),
    Subscribe {
        boss_name: BossName,
        id: SubId,
        subscriber: Sub,
    },
    Unsubscribe { boss_name: BossName, id: SubId },
    GetBosses(oneshot::Sender<Vec<RaidBoss>>),
    GetRecentTweets {
        boss_name: BossName,
        sender: oneshot::Sender<Vec<Arc<RaidTweet>>>,
    },
    ReadError,
}

pub struct AsyncResult<T>(oneshot::Receiver<T>);
impl<T> Future for AsyncResult<T> {
    type Item = T;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0.poll().map_err(|_| ErrorKind::Closed.into())
    }
}

#[derive(Debug)]
pub struct Petronel<SubId, Sub>(
    mpsc::UnboundedSender<Event<SubId, Sub>>,
    PhantomData<SubId>,
    PhantomData<Sub>
);

impl<SubId, Sub> Clone for Petronel<SubId, Sub> {
    fn clone(&self) -> Self {
        Petronel(self.0.clone(), PhantomData, PhantomData)
    }
}

impl<SubId, Sub> Petronel<SubId, Sub> {
    fn request<T, F>(&self, f: F) -> AsyncResult<T>
    where
        F: FnOnce(oneshot::Sender<T>) -> Event<SubId, Sub>,
    {
        let (tx, rx) = oneshot::channel();
        let _ = mpsc::UnboundedSender::send(&self.0, f(tx));
        AsyncResult(rx)
    }

    pub fn subscribe<B>(&self, boss_name: B, id: SubId, subscriber: Sub)
    where
        B: AsRef<str>,
    {
        let event = Event::Subscribe {
            boss_name: BossName::new(boss_name),
            id,
            subscriber,
        };
        let _ = mpsc::UnboundedSender::send(&self.0, event);
    }

    pub fn unsubscribe<B>(&self, boss_name: B, id: SubId)
    where
        B: AsRef<str>,
    {
        let event = Event::Unsubscribe {
            boss_name: BossName::new(boss_name),
            id,
        };
        let _ = mpsc::UnboundedSender::send(&self.0, event);
    }


    pub fn bosses(&self) -> AsyncResult<Vec<RaidBoss>> {
        self.request(Event::GetBosses)
    }

    pub fn recent_tweets<B>(&self, boss_name: B) -> AsyncResult<Vec<Arc<RaidTweet>>>
    where
        B: AsRef<str>,
    {
        self.request(|tx| {
            Event::GetRecentTweets {
                boss_name: BossName::new(boss_name),
                sender: tx,
            }
        })
    }
}

pub struct PetronelFuture<S, SubId, Sub> {
    events: Select<
        Map<S, fn(RaidInfo) -> Event<SubId, Sub>>,
        OrElse<
            mpsc::UnboundedReceiver<Event<SubId, Sub>>,
            fn(()) -> Result<Event<SubId, Sub>>,
            Result<Event<SubId, Sub>>,
        >,
    >,
    bosses: HashMap<BossName, RaidBossEntry<SubId, Sub>>,
    tweet_history_size: usize,
}

impl<SubId, Sub> Petronel<SubId, Sub> {
    fn events_read_error(_: ()) -> Result<Event<SubId, Sub>> {
        Ok(Event::ReadError)
    }

    // TODO: Builder
    pub fn from_stream<S>(
        stream: S,
        tweet_history_size: usize,
    ) -> (Self, PetronelFuture<S, SubId, Sub>)
    where
        S: Stream<Item = RaidInfo, Error = Error>,
    {
        let (tx, rx) = mpsc::unbounded();

        let stream_events = stream.map(Event::NewRaidInfo as fn(RaidInfo) -> Event<SubId, Sub>);
        let rx = rx.or_else(
            Self::events_read_error as fn(()) -> Result<Event<SubId, Sub>>,
        );

        let future = PetronelFuture {
            events: stream_events.select(rx),
            bosses: HashMap::new(),
            tweet_history_size,
        };

        (Petronel(tx, PhantomData, PhantomData), future)
    }
}

impl<S, SubId, Sub> PetronelFuture<S, SubId, Sub>
where
    SubId: Hash + Ord,
    Sub: Subscriber<Message>,
{
    fn handle_event(&mut self, event: Event<SubId, Sub>) {
        use self::Event::*;

        match event {
            Subscribe {
                boss_name,
                id,
                subscriber,
            } => {
                self.subscribe(boss_name, id, subscriber);
            }
            Unsubscribe { boss_name, id } => {
                self.unsubscribe(boss_name, id);
            }
            NewRaidInfo(r) => {
                self.handle_raid_info(r);
            }
            GetBosses(tx) => {
                let _ = tx.send(Vec::from_iter(self.bosses.values().map(|e| e.boss.clone())));
            }
            GetRecentTweets { boss_name, sender } => {
                let backlog = self.bosses.get(&boss_name).map_or(vec![], |e| {
                    // Returns recent tweets, unsorted. The client is
                    // expected to do the sorting on their end.
                    e.recent_tweets.as_unordered_slice().to_vec()
                });

                let _ = sender.send(backlog);
            }
            ReadError => {} // This should never happen
        }
    }

    fn subscribe(&mut self, boss_name: BossName, id: SubId, subscriber: Sub) {
        match self.bosses.entry(boss_name) {
            Entry::Occupied(mut entry) => {
                // TODO: Create subscription handle with custom Drop
                entry.get_mut().broadcast.subscribe(id, subscriber);
            }
            Entry::Vacant(_entry) => {
                // TODO: Create temporary broadcast
            }
        }
    }

    fn unsubscribe(&mut self, boss_name: BossName, id: SubId) {
        if let Some(entry) = self.bosses.get_mut(&boss_name) {
            entry.broadcast.unsubscribe(id);
        }
        // TODO: If None, look up temporary broadcast
    }

    fn handle_raid_info(&mut self, info: RaidInfo) {
        match self.bosses.entry(info.tweet.boss_name.clone()) {
            Entry::Occupied(mut entry) => {
                let value = entry.get_mut();

                value.last_seen = info.tweet.created_at;

                let tweet = Arc::new(info.tweet);
                value.broadcast.send(&tweet);
                value.recent_tweets.push(tweet);

                if value.boss.image.is_none() && info.image.is_some() {
                    // TODO: Image hash
                    value.boss.image = info.image;
                }
            }
            Entry::Vacant(entry) => {
                let name = entry.key().clone();

                let boss = RaidBoss {
                    level: name.parse_level().unwrap_or(DEFAULT_BOSS_LEVEL),
                    name: name,
                    image: info.image,
                    language: info.tweet.language,
                };

                entry.insert(RaidBossEntry {
                    boss,
                    last_seen: info.tweet.created_at.clone(),
                    recent_tweets: {
                        let mut recent_tweets =
                            CircularBuffer::with_capacity(self.tweet_history_size);
                        recent_tweets.push(Arc::new(info.tweet));
                        recent_tweets
                    },
                    broadcast: Broadcast::new(),
                });

            }
        }
    }
}

impl<S, SubId, Sub> Future for PetronelFuture<S, SubId, Sub>
where
    S: Stream<Item = RaidInfo, Error = Error>,
    SubId: Hash + Ord,
    Sub: Subscriber<Message>
{
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            if let Some(event) = try_ready!(self.events.poll()) {
                self.handle_event(event)
            } else {
                return Ok(Async::Ready(()));
            }
        }
    }
}
