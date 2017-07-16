use broadcast::{Broadcast, Subscriber};
use circular_buffer::CircularBuffer;
use error::*;
use futures::{Async, Future, Poll, Stream};
use futures::stream::{Map, OrElse, Select};
use futures::unsync::mpsc;
use futures::unsync::oneshot;
use id_pool::{Id as SubId, IdPool};
use model::{BossLevel, BossName, DateTime, Message, RaidBoss, RaidTweet};
use raid::RaidInfo;
use std::collections::{HashMap, HashSet};
use std::collections::hash_map::Entry;
use std::iter::FromIterator;
use std::sync::Arc;

const DEFAULT_BOSS_LEVEL: BossLevel = 0;

struct RaidBossEntry<Sub> {
    boss: RaidBoss,
    last_seen: DateTime,
    recent_tweets: CircularBuffer<Arc<RaidTweet>>,
    broadcast: Broadcast<SubId, Sub>,
}

#[derive(Debug)]
enum Event<Sub> {
    NewRaidInfo(RaidInfo),

    Follow { id: SubId, boss_name: BossName },
    Unfollow { id: SubId, boss_name: BossName },

    Subscribe {
        subscriber: Sub,
        petronel: Petronel<Sub>,
        sender: oneshot::Sender<Subscription<Sub>>,
    },
    Unsubscribe(SubId),

    GetBosses(oneshot::Sender<Vec<RaidBoss>>),
    GetRecentTweets {
        boss_name: BossName,
        sender: oneshot::Sender<Vec<Arc<RaidTweet>>>,
    },

    Heartbeat,

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
pub struct Petronel<Sub>(mpsc::UnboundedSender<Event<Sub>>);

impl<Sub> Clone for Petronel<Sub> {
    fn clone(&self) -> Self {
        Petronel(self.0.clone())
    }
}

// TODO: Figure out if there is a way to do this without owning `Petronel`
#[must_use = "Subscriptions are cancelled when they go out of scope"]
#[derive(Debug)]
pub struct Subscription<Sub> {
    id: SubId,
    following: HashSet<BossName>,
    petronel: Petronel<Sub>,
}

impl<Sub> Subscription<Sub> {
    pub fn follow<B>(&mut self, boss_name: B)
    where
        B: Into<BossName>,
    {
        let name = boss_name.into();
        self.following.insert(name.clone());
        self.petronel.follow(self.id.clone(), name);
    }

    pub fn unfollow<B>(&mut self, boss_name: B)
    where
        B: Into<BossName>,
    {
        let name = boss_name.into();
        self.following.remove(&name);
        self.petronel.unfollow(self.id.clone(), name);
    }

    pub fn unsubscribe(&self) {
        self.petronel.unsubscribe(self.id.clone())
    }
}

impl<Sub> Drop for Subscription<Sub> {
    fn drop(&mut self) {
        let mut following = ::std::mem::replace(&mut self.following, HashSet::with_capacity(0));

        for boss_name in following.drain() {
            self.unfollow(boss_name);
        }

        self.unsubscribe()
    }
}


impl<Sub> Petronel<Sub> {
    fn send(&self, event: Event<Sub>) {
        let _ = mpsc::UnboundedSender::send(&self.0, event);
    }

    fn request<T, F>(&self, f: F) -> AsyncResult<T>
    where
        F: FnOnce(oneshot::Sender<T>) -> Event<Sub>,
    {
        let (tx, rx) = oneshot::channel();
        self.send(f(tx));
        AsyncResult(rx)
    }

    pub fn subscribe(&self, subscriber: Sub) -> AsyncResult<Subscription<Sub>> {
        self.request(|sender| {
            Event::Subscribe {
                subscriber,
                sender,
                petronel: self.clone(),
            }
        })
    }

    fn unsubscribe(&self, id: SubId) {
        self.send(Event::Unsubscribe(id));
    }

    fn follow(&self, id: SubId, boss_name: BossName) {
        self.send(Event::Follow { id, boss_name });
    }

    fn unfollow(&self, id: SubId, boss_name: BossName) {
        self.send(Event::Unfollow { id, boss_name });
    }

    pub fn bosses(&self) -> AsyncResult<Vec<RaidBoss>> {
        self.request(Event::GetBosses)
    }

    pub fn recent_tweets<B>(&self, boss_name: B) -> AsyncResult<Vec<Arc<RaidTweet>>>
    where
        B: Into<BossName>,
    {
        self.request(|tx| {
            Event::GetRecentTweets {
                boss_name: boss_name.into(),
                sender: tx,
            }
        })
    }

    pub fn heartbeat(&self) {
        self.send(Event::Heartbeat);
    }
}

#[must_use = "futures do nothing unless polled"]
pub struct PetronelFuture<S, Sub, F> {
    id_pool: IdPool,
    events: Select<
        Map<S, fn(RaidInfo) -> Event<Sub>>,
        OrElse<
            mpsc::UnboundedReceiver<Event<Sub>>,
            fn(()) -> Result<Event<Sub>>,
            Result<Event<Sub>>,
        >,
    >,
    bosses: HashMap<BossName, RaidBossEntry<Sub>>,
    tweet_history_size: usize,
    requested_bosses: HashMap<BossName, Broadcast<SubId, Sub>>,
    subscribers: Broadcast<SubId, Sub>,
    map_message: F,
}

impl<Sub> Petronel<Sub> {
    fn events_read_error(_: ()) -> Result<Event<Sub>> {
        Ok(Event::ReadError)
    }

    // TODO: Builder
    pub fn from_stream<S, F>(
        stream: S,
        tweet_history_size: usize,
        map_message: F,
    ) -> (Self, PetronelFuture<S, Sub, F>)
    where
        S: Stream<Item = RaidInfo, Error = Error>,
        Sub: Subscriber,
        F: Fn(Message) -> Sub::Item,
    {
        let (tx, rx) = mpsc::unbounded();

        let stream_events = stream.map(Event::NewRaidInfo as fn(RaidInfo) -> Event<Sub>);
        let rx = rx.or_else(Self::events_read_error as fn(()) -> Result<Event<Sub>>);

        let future = PetronelFuture {
            id_pool: IdPool::new(),
            events: stream_events.select(rx),
            bosses: HashMap::new(),
            tweet_history_size,
            requested_bosses: HashMap::new(),
            subscribers: Broadcast::new(),
            map_message,
        };

        (Petronel(tx), future)
    }
}

impl<S, Sub, F> PetronelFuture<S, Sub, F>
where
    Sub: Subscriber + Clone,
    F: Fn(Message) -> Sub::Item,
{
    fn handle_event(&mut self, event: Event<Sub>) {
        use self::Event::*;

        match event {
            Subscribe {
                subscriber,
                sender,
                petronel,
            } => {
                let id = self.subscribe(subscriber);
                let _ = sender.send(Subscription {
                    id,
                    following: HashSet::new(),
                    petronel,
                });
            }
            Unsubscribe(id) => {
                self.unsubscribe(&id);
            }
            Follow { id, boss_name } => {
                self.follow(id, boss_name);
            }
            Unfollow { id, boss_name } => {
                self.unfollow(&id, boss_name);
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
            Heartbeat => {
                // TODO: Map this just once and cache it
                let message = (self.map_message)(Message::Heartbeat);
                self.subscribers.send(&message)
            }
        }
    }

    fn subscribe(&mut self, subscriber: Sub) -> SubId {
        let id = self.id_pool.get();
        self.subscribers.subscribe(id.clone(), subscriber);
        id
    }

    fn unsubscribe(&mut self, id: &SubId) {
        self.subscribers.unsubscribe(id);
        self.id_pool.recycle(id.clone());
    }

    fn follow(&mut self, id: SubId, boss_name: BossName) {
        if let Some(sub) = self.subscribers.get(&id) {
            let subscriber = sub.clone();

            if let Some(entry) = self.bosses.get_mut(&boss_name) {
                entry.broadcast.subscribe(id, subscriber);
            } else {
                match self.requested_bosses.entry(boss_name) {
                    Entry::Occupied(mut entry) => {
                        entry.get_mut().subscribe(id, subscriber);
                    }
                    Entry::Vacant(entry) => {
                        let mut broadcast = Broadcast::new();
                        broadcast.subscribe(id, subscriber);
                        entry.insert(broadcast);
                    }
                }
            }
        }
    }

    fn unfollow(&mut self, id: &SubId, boss_name: BossName) {
        if let Some(entry) = self.bosses.get_mut(&boss_name) {
            entry.broadcast.unsubscribe(&id);
        } else if let Entry::Occupied(mut entry) = self.requested_bosses.entry(boss_name) {
            let is_empty = {
                let broadcast = entry.get_mut();
                broadcast.unsubscribe(&id);
                broadcast.is_empty()
            };

            if is_empty {
                entry.remove();
            }
        }
    }

    fn handle_raid_info(&mut self, info: RaidInfo) {
        match self.bosses.entry(info.tweet.boss_name.clone()) {
            Entry::Occupied(mut entry) => {
                let value = entry.get_mut();

                value.last_seen = info.tweet.created_at;

                let tweet = Arc::new(info.tweet);
                let message = Message::Tweet(tweet.clone());
                value.broadcast.send(&(self.map_message)(message));
                value.recent_tweets.push(tweet);

                if value.boss.image.is_none() && info.image.is_some() {
                    // TODO: Image hash
                    value.boss.image = info.image;
                }
            }
            Entry::Vacant(entry) => {
                let name = entry.key().clone();

                let mut broadcast = self.requested_bosses.remove(&name).unwrap_or(
                    Broadcast::new(),
                );

                let boss = RaidBoss {
                    level: name.parse_level().unwrap_or(DEFAULT_BOSS_LEVEL),
                    name: name,
                    image: info.image,
                    language: info.tweet.language,
                };

                let last_seen = info.tweet.created_at.clone();

                let tweet = Arc::new(info.tweet);
                let message = Message::Tweet(tweet.clone());
                broadcast.send(&(self.map_message)(message));

                entry.insert(RaidBossEntry {
                    boss,
                    broadcast,
                    last_seen,
                    recent_tweets: {
                        let mut recent_tweets =
                            CircularBuffer::with_capacity(self.tweet_history_size);
                        recent_tweets.push(tweet);
                        recent_tweets
                    },
                });

            }
        }
    }
}

impl<S, Sub, F> Future for PetronelFuture<S, Sub, F>
where
    S: Stream<Item = RaidInfo, Error = Error>,
    Sub: Subscriber + Clone,
    F: Fn(Message) -> Sub::Item,
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
