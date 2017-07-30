use super::{Event, RaidBossEntry, Subscription};
use broadcast::{Broadcast, Subscriber};
use circular_buffer::CircularBuffer;
use error::*;
use futures::{Async, Future, Poll, Stream};
use futures::stream::{Map, OrElse, Select};
use futures::unsync::mpsc;
use id_pool::{Id as SubId, IdPool};
use image_hash::{BossImageHash, ImageHash, ImageHashReceiver, ImageHashSender, ImageHasher};
use model::{BossLevel, BossName, Message, RaidBoss};
use raid::RaidInfo;
use std::collections::{HashMap, HashSet};
use std::collections::hash_map::Entry;
use std::iter::FromIterator;
use std::sync::Arc;

const DEFAULT_BOSS_LEVEL: BossLevel = 0;

#[must_use = "futures do nothing unless polled"]
pub struct Worker<H, S, Sub, F>
where
    Sub: Subscriber,
    H: ImageHasher,
{
    pub(crate) hash_requester: ImageHashSender,
    pub(crate) id_pool: IdPool,
    pub(crate) events: Select<
        Map<S, fn(RaidInfo) -> Event<Sub>>,
        Select<
            OrElse<
                mpsc::UnboundedReceiver<Event<Sub>>,
                fn(()) -> Result<Event<Sub>>,
                Result<Event<Sub>>,
            >,
            Map<ImageHashReceiver<H>, fn(BossImageHash) -> Event<Sub>>,
        >,
    >,
    pub(crate) bosses: HashMap<BossName, RaidBossEntry<Sub>>,
    pub(crate) tweet_history_size: usize,
    pub(crate) requested_bosses: HashMap<BossName, Broadcast<SubId, Sub>>,
    pub(crate) subscribers: Broadcast<SubId, Sub>,
    pub(crate) map_message: F,
    pub(crate) cached_boss_list: Sub::Item,
    pub(crate) heartbeat: Sub::Item,
}

impl<H, S, Sub, F> Worker<H, S, Sub, F>
where
    H: ImageHasher,
    Sub: Subscriber + Clone,
    F: Fn(Message) -> Sub::Item,
{
    fn handle_event(&mut self, event: Event<Sub>) {
        use super::Event::*;

        match event {
            Subscribe {
                subscriber,
                sender,
                client,
            } => {
                let id = self.subscribe(subscriber);
                let _ = sender.send(Subscription {
                    id,
                    following: HashSet::new(),
                    client,
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
            GetCachedBossList(id) => {
                if let Some(sub) = self.subscribers.get_mut(&id) {
                    let _ = sub.send(&self.cached_boss_list);
                }
            }

            NewRaidInfo(r) => {
                self.handle_raid_info(r);
            }
            NewImageHash {
                boss_name,
                image_hash,
            } => {
                self.handle_image_hash(boss_name, image_hash);
            }

            GetBosses(tx) => {
                let _ = tx.send(Vec::from_iter(self.bosses.values().map(|e| e.boss.clone())));
            }
            GetRecentTweets { boss_name, sender } => {
                let tweets = self.bosses.get(&boss_name).map_or(vec![], |e| {
                    // Returns recent tweets, unsorted. The client is
                    // expected to do the sorting on their end.
                    e.recent_tweets.as_unordered_slice().to_vec()
                });

                let _ = sender.send(tweets);
            }
            Heartbeat => self.subscribers.send(&self.heartbeat),
            ReadError => {} // This should never happen
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

    fn handle_image_hash(&mut self, boss_name: BossName, image_hash: ImageHash) {
        // TODO: Is it possible to avoid finding the same boss twice?
        let (level, language) = match self.bosses.get_mut(&boss_name) {
            Some(mut entry) => {
                entry.image_hash = Some(image_hash);

                (entry.boss.level, entry.boss.language)
            }
            None => return,
        };

        let mut matches = Vec::new();

        for entry in self.bosses.values_mut() {
            if entry.boss.level == level && entry.boss.language != language &&
                entry.image_hash == Some(image_hash)
            {
                entry.boss.translations.insert(boss_name.clone());

                let message = (self.map_message)(Message::BossUpdate(&entry.boss));
                self.subscribers.send(&message);
                matches.push(entry.boss.name.clone());
            }
        }

        if !matches.is_empty() {
            if let Some(mut entry) = self.bosses.get_mut(&boss_name) {
                entry.boss.translations.extend(matches);

                let message = (self.map_message)(Message::BossUpdate(&entry.boss));
                self.subscribers.send(&message);
            }

            self.update_cached_boss_list();
        }
    }

    fn update_cached_boss_list(&mut self) {
        let updated = self.bosses
            .values()
            .map(|entry| &entry.boss)
            .collect::<Vec<_>>();

        self.cached_boss_list = (self.map_message)(Message::BossList(&updated))
    }

    fn handle_raid_info(&mut self, info: RaidInfo) {
        let is_new_boss = match self.bosses.entry(info.tweet.boss_name.clone()) {
            Entry::Occupied(mut entry) => {
                let value = entry.get_mut();

                value.last_seen = info.tweet.created_at;

                {
                    let message = Message::Tweet(&info.tweet);
                    value.broadcast.send(&(self.map_message)(message));
                }

                if value.boss.image.is_none() {
                    if let Some(image_url) = info.image {
                        self.hash_requester.request(
                            value.boss.name.clone(),
                            &image_url,
                        );
                        value.boss.image = Some(image_url);
                    }
                }

                value.recent_tweets.push(Arc::new(info.tweet));
                false
            }
            Entry::Vacant(entry) => {
                let name = entry.key().clone();

                let mut broadcast = self.requested_bosses.remove(&name).unwrap_or(
                    Broadcast::new(),
                );

                let last_seen = info.tweet.created_at.clone();
                let boss = RaidBoss {
                    level: name.parse_level().unwrap_or(DEFAULT_BOSS_LEVEL),
                    name: name,
                    image: info.image,
                    language: info.tweet.language,
                    translations: HashSet::with_capacity(1),
                };

                {
                    let boss_message = Message::BossUpdate(&boss);
                    self.subscribers.send(&(self.map_message)(boss_message));

                    let tweet_message = Message::Tweet(&info.tweet);
                    broadcast.send(&(self.map_message)(tweet_message));
                }

                if let Some(ref image_url) = boss.image {
                    self.hash_requester.request(boss.name.clone(), &image_url);
                }

                let mut recent_tweets = CircularBuffer::with_capacity(self.tweet_history_size);
                recent_tweets.push(Arc::new(info.tweet));

                entry.insert(RaidBossEntry {
                    boss,
                    broadcast,
                    last_seen,
                    recent_tweets,
                    image_hash: None,
                });

                true
            }
        };

        if is_new_boss {
            self.update_cached_boss_list();
        }
    }
}

impl<H, S, Sub, F> Future for Worker<H, S, Sub, F>
where
    H: ImageHasher,
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
