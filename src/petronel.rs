use broadcast::{Broadcast, Subscriber};
use circular_buffer::CircularBuffer;
use error::*;
use futures::{Async, Future, Poll, Stream};
use futures::stream::{Map, OrElse, Select};
use futures::unsync::mpsc;
use futures::unsync::oneshot;
use raid::{BossImageUrl, BossLevel, BossName, DateTime, Language, RaidInfo, RaidTweet};
use std::collections::{HashMap, HashSet};
use std::collections::hash_map::Entry;
use std::hash::Hash;
use std::iter::FromIterator;
use std::marker::PhantomData;
use std::sync::Arc;

const DEFAULT_BOSS_LEVEL: BossLevel = 0;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Message {
    Heartbeat,
    Tweet(Arc<RaidTweet>),
}

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

    Follow { id: SubId, boss_name: BossName },
    Unfollow { id: SubId, boss_name: BossName },

    Subscribe { id: SubId, subscriber: Sub },
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

// TODO: Figure out if there is a way to do this without owning `Petronel`
#[must_use = "Subscriptions are cancelled when they go out of scope"]
pub struct Subscription<SubId, Sub>
where
    SubId: Clone,
{
    id: SubId,
    following: HashSet<BossName>,
    petronel: Petronel<SubId, Sub>,
}

impl<SubId, Sub> Subscription<SubId, Sub>
where
    SubId: Clone,
{
    pub fn follow<B>(&mut self, boss_name: B)
    where
        B: AsRef<str>,
    {
        let name = BossName::new(boss_name);
        self.following.insert(name.clone());
        self.petronel.follow(self.id.clone(), name);
    }

    pub fn unfollow<B>(&mut self, boss_name: B)
    where
        B: AsRef<str>,
    {
        let name = BossName::new(boss_name);
        self.following.remove(&name);
        self.petronel.unfollow(self.id.clone(), name);
    }

    pub fn unsubscribe(&self) {
        self.petronel.unsubscribe(self.id.clone())
    }
}

impl<SubId, Sub> Drop for Subscription<SubId, Sub>
where
    SubId: Clone,
{
    fn drop(&mut self) {
        let mut following = ::std::mem::replace(&mut self.following, HashSet::with_capacity(0));

        for boss_name in following.drain() {
            self.unfollow(boss_name);
        }

        self.unsubscribe()
    }
}


impl<SubId, Sub> Petronel<SubId, Sub> {
    fn send(&self, event: Event<SubId, Sub>) {
        let _ = mpsc::UnboundedSender::send(&self.0, event);
    }

    fn request<T, F>(&self, f: F) -> AsyncResult<T>
    where
        F: FnOnce(oneshot::Sender<T>) -> Event<SubId, Sub>,
    {
        let (tx, rx) = oneshot::channel();
        self.send(f(tx));
        AsyncResult(rx)
    }

    pub fn subscribe(&self, id: SubId, subscriber: Sub) -> Subscription<SubId, Sub>
    where
        SubId: Clone,
    {
        let event = Event::Subscribe {
            id: id.clone(),
            subscriber,
        };
        let _ = mpsc::UnboundedSender::send(&self.0, event);

        Subscription {
            id,
            following: HashSet::new(),
            petronel: self.clone(),
        }
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
        B: AsRef<str>,
    {
        self.request(|tx| {
            Event::GetRecentTweets {
                boss_name: BossName::new(boss_name),
                sender: tx,
            }
        })
    }

    pub fn heartbeat(&self) {
        self.send(Event::Heartbeat);
    }
}

#[must_use = "futures do nothing unless polled"]
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
    requested_bosses: HashMap<BossName, Broadcast<SubId, Sub, Message>>,
    subscribers: Broadcast<SubId, Sub, Message>,
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
        Sub: Subscriber<Message>,
        SubId: Hash + Eq,
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
            requested_bosses: HashMap::new(),
            subscribers: Broadcast::new(),
        };

        (Petronel(tx, PhantomData, PhantomData), future)
    }
}

impl<S, SubId, Sub> PetronelFuture<S, SubId, Sub>
where
    SubId: Hash + Eq,
    Sub: Subscriber<Message> + Clone,
{
    fn handle_event(&mut self, event: Event<SubId, Sub>) {
        use self::Event::*;

        match event {
            Subscribe { id, subscriber } => {
                self.subscribe(id, subscriber);
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
            Heartbeat => self.subscribers.send(&Message::Heartbeat),
        }
    }

    fn subscribe(&mut self, id: SubId, subscriber: Sub) {
        // TODO: Choose ID randomly?
        self.subscribers.subscribe(id, subscriber);
    }

    fn unsubscribe(&mut self, id: &SubId) {
        self.subscribers.unsubscribe(id);
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
                value.broadcast.send(&Message::Tweet(tweet.clone()));
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
                broadcast.send(&Message::Tweet(tweet.clone()));

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

impl<S, SubId, Sub> Future for PetronelFuture<S, SubId, Sub>
where
    S: Stream<Item = RaidInfo, Error = Error>,
    SubId: Hash + Eq,
    Sub: Subscriber<Message> + Clone
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
