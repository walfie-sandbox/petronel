use super::{Event, Subscription};
use broadcast::{Broadcast, Subscriber};
use circular_buffer::CircularBuffer;
use error::*;
use futures::{Async, Future, Poll, Stream};
use futures::stream::{Chain, FilterMap, Map, Once, OrElse, Select};
use futures::unsync::mpsc;
use id_pool::{Id as SubId, IdPool};
use image_hash::{BossImageHash, ImageHash, ImageHashReceiver, ImageHashSender, ImageHasher};
use metrics::Metrics;
use model::{BossLevel, BossName, Message, RaidBoss, RaidBossMetadata, RaidTweet};
use raid::RaidInfo;
use std::collections::{HashMap, HashSet};
use std::collections::hash_map::Entry;
use std::iter::FromIterator;
use std::sync::Arc;

const DEFAULT_BOSS_LEVEL: BossLevel = 0;

pub(crate) struct RaidBossEntry<Sub> {
    pub(crate) boss_data: RaidBossMetadata,
    pub(crate) recent_tweets: CircularBuffer<Arc<RaidTweet>>,
    pub(crate) broadcast: Broadcast<SubId, Sub>,
}

#[must_use = "futures do nothing unless polled"]
pub struct Worker<H, S, Sub, F, M>
where
    Sub: Subscriber,
    H: ImageHasher,
    M: Metrics,
{
    pub(crate) hash_requester: ImageHashSender,
    pub(crate) id_pool: IdPool,
    pub(crate) events: Select<
        Map<
            Chain<S, Once<RaidInfo, Error>>,
            fn(RaidInfo) -> Event<Sub, M::Export>,
        >,
        Select<
            OrElse<
                mpsc::UnboundedReceiver<Event<Sub, M::Export>>,
                fn(()) -> Result<Event<Sub, M::Export>>,
                Result<Event<Sub, M::Export>>,
            >,
            FilterMap<
                ImageHashReceiver<H>,
                fn(BossImageHash) -> Option<Event<Sub, M::Export>>,
            >,
        >,
    >,
    pub(crate) bosses: HashMap<BossName, RaidBossEntry<Sub>>,
    pub(crate) tweet_history_size: usize,
    pub(crate) requested_bosses: HashMap<BossName, Broadcast<SubId, Sub>>,
    pub(crate) subscribers: Broadcast<SubId, Sub>,
    pub(crate) filter_map_message: F,
    pub(crate) cached_boss_list: Option<Sub::Item>,
    pub(crate) heartbeat: Option<Sub::Item>,
    pub(crate) metrics: M,
}

impl<H, S, Sub, F, M> Worker<H, S, Sub, F, M>
where
    H: ImageHasher,
    Sub: Subscriber + Clone,
    F: Fn(Message) -> Option<Sub::Item>,
    M: Metrics,
{
    fn handle_event(&mut self, event: Event<Sub, M::Export>) {
        use super::Event::*;

        match event {
            SubscriberSubscribe {
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
            SubscriberUnsubscribe(id) => {
                self.unsubscribe(&id);
            }
            SubscriberFollow { id, boss_name } => {
                self.follow(id, boss_name);
            }
            SubscriberUnfollow { id, boss_name } => {
                self.unfollow(&id, boss_name);
            }
            SubscriberGetBosses(id) => {
                if let Some(sub) = self.subscribers.get_mut(&id) {
                    let _ = sub.maybe_send(self.cached_boss_list.as_ref());
                }
            }
            SubscriberGetTweets { id, boss_name } => {
                if let Some(sub) = self.subscribers.get_mut(&id) {
                    let tweets = self.bosses.get(&boss_name).map_or(&[][..], |e| {
                        e.recent_tweets.as_unordered_slice()
                    });

                    let message = (self.filter_map_message)(Message::TweetList(tweets));

                    let _ = sub.maybe_send(message.as_ref());
                }
            }
            SubscriberHeartbeat => self.subscribers.maybe_send(self.heartbeat.as_ref()),

            NewRaidInfo(r) => {
                self.handle_raid_info(r);
            }
            NewImageHash {
                boss_name,
                image_hash,
            } => {
                self.handle_image_hash(boss_name, image_hash);
            }

            ClientGetBosses(tx) => {
                let _ = tx.send(Vec::from_iter(
                    self.bosses.values().map(|e| e.boss_data.boss.clone()),
                ));
            }
            ClientGetTweets { boss_name, sender } => {
                let tweets = self.bosses.get(&boss_name).map_or(vec![], |e| {
                    // Returns recent tweets, unsorted. The client is
                    // expected to do the sorting on their end.
                    e.recent_tweets.as_unordered_slice().to_vec()
                });

                let _ = sender.send(tweets);
            }
            ClientExportMetadata(tx) => {
                let _ = tx.send(Vec::from_iter(
                    self.bosses.values().map(|e| e.boss_data.clone()),
                ));
            }
            ClientExportMetrics(tx) => {
                let _ = tx.send(self.metrics.export());
            }
            ClientRemoveBosses(f) => {
                self.remove_bosses(f.0);
            }
            ClientReadError => {} // This should never happen
        }
    }

    fn remove_bosses(&mut self, f: Box<Fn(&RaidBossMetadata) -> bool>) {
        let (filter_map, subscribers, requested_bosses, metrics) = (
            &self.filter_map_message,
            &mut self.subscribers,
            &mut self.requested_bosses,
            &mut self.metrics,
        );

        self.bosses.retain(|_, entry| {
            let should_remove = (f)(&entry.boss_data);

            if should_remove {
                let boss_name = &entry.boss_data.boss.name;
                let message = (filter_map)(Message::BossRemove(boss_name));
                subscribers.maybe_send(message.as_ref());

                // If there are existing subscribers, move them to `requested_bosses`
                if !entry.broadcast.is_empty() {
                    let broadcast = ::std::mem::replace(&mut entry.broadcast, Broadcast::new());
                    requested_bosses.insert(boss_name.clone(), broadcast);
                }

                metrics.remove_boss(boss_name);
            }

            !should_remove
        });
    }

    fn subscribe(&mut self, subscriber: Sub) -> SubId {
        let id = self.id_pool.get();
        self.subscribers.subscribe(id.clone(), subscriber);
        self.metrics.set_total_subscriber_count(
            self.subscribers.subscriber_count() as u32,
        );
        id
    }

    fn unsubscribe(&mut self, id: &SubId) {
        self.subscribers.unsubscribe(id);
        self.metrics.set_total_subscriber_count(
            self.subscribers.subscriber_count() as u32,
        );
        self.id_pool.recycle(id.clone());
    }

    fn follow(&mut self, id: SubId, boss_name: BossName) {
        if let Some(sub) = self.subscribers.get(&id) {
            let subscriber = sub.clone();

            if let Some(entry) = self.bosses.get_mut(&boss_name) {
                entry.broadcast.subscribe(id, subscriber);
                self.metrics.set_follower_count(
                    &boss_name,
                    entry.broadcast.subscriber_count() as u32,
                );
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
            self.metrics.set_follower_count(
                &boss_name,
                entry.broadcast.subscriber_count() as u32,
            );
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
            Some(entry) => {
                entry.boss_data.image_hash = Some(image_hash);

                (entry.boss_data.boss.level, entry.boss_data.boss.language)
            }
            None => return,
        };

        let mut matches = Vec::new();

        for entry in self.bosses.values_mut() {
            if entry.boss_data.boss.level == level && entry.boss_data.boss.language != language &&
                entry.boss_data.image_hash == Some(image_hash)
            {
                entry.boss_data.boss.translations.insert(boss_name.clone());

                let message = (self.filter_map_message)(Message::BossUpdate(&entry.boss_data.boss));
                self.subscribers.maybe_send(message.as_ref());
                matches.push(entry.boss_data.boss.name.clone());
            }
        }

        if !matches.is_empty() {
            if let Some(entry) = self.bosses.get_mut(&boss_name) {
                entry.boss_data.boss.translations.extend(matches);

                let message = (self.filter_map_message)(Message::BossUpdate(&entry.boss_data.boss));
                self.subscribers.maybe_send(message.as_ref());
            }

            self.update_cached_boss_list();
        }
    }

    pub(crate) fn update_cached_boss_list(&mut self) {
        let updated = self.bosses
            .values()
            .map(|entry| &entry.boss_data.boss)
            .collect::<Vec<_>>();

        self.cached_boss_list = (self.filter_map_message)(Message::BossList(&updated))
    }

    fn handle_raid_info(&mut self, info: RaidInfo) {
        self.metrics.inc_tweet_count(&info.tweet.boss_name);

        let mapped_tweet_message = (self.filter_map_message)(Message::Tweet(&info.tweet));

        // Currently, only one translated boss should exist at most, but in
        // case the game gets translated to another language, this should still
        // handle that case. This enum exists because we don't want to allocate
        // a Vec in the cases where only one translation exists.
        enum TranslationsExist {
            One {
                boss_name: BossName,
                tweet: Arc<RaidTweet>,
            },
            Multiple {
                boss_names: Vec<BossName>,
                tweet: Arc<RaidTweet>,
            },
        }

        let mut translations: Option<TranslationsExist> = None;

        let is_new_boss = match self.bosses.entry(info.tweet.boss_name.clone()) {
            Entry::Occupied(mut entry) => {
                let value = entry.get_mut();

                value.boss_data.last_seen = info.tweet.created_at;

                value.broadcast.maybe_send(mapped_tweet_message.as_ref());

                if value.boss_data.boss.image.is_none() {
                    if let Some(image_url) = info.image {
                        self.hash_requester.request(
                            value.boss_data.boss.name.clone(),
                            &image_url,
                        );
                        value.boss_data.boss.image = Some(image_url);
                    }
                }

                let arc_tweet = Arc::new(info.tweet);

                // If this boss has translations, send the tweet to that boss' subscribers too
                let boss_translations = &value.boss_data.boss.translations;
                match boss_translations.len() {
                    1 => {
                        translations = Some(TranslationsExist::One {
                            boss_name: boss_translations.iter().cloned().next().unwrap(),
                            tweet: arc_tweet.clone(),
                        });
                    }
                    0 => {}
                    _ => {
                        translations = Some(TranslationsExist::Multiple {
                            boss_names: boss_translations.iter().cloned().collect(),
                            tweet: arc_tweet.clone(),
                        });
                    }
                }

                value.recent_tweets.push(arc_tweet);
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
                    self.subscribers.maybe_send(
                        (self.filter_map_message)(boss_message)
                            .as_ref(),
                    );

                    broadcast.maybe_send(mapped_tweet_message.as_ref());
                }

                if let Some(ref image_url) = boss.image {
                    self.hash_requester.request(boss.name.clone(), &image_url);
                }

                let mut recent_tweets = CircularBuffer::with_capacity(self.tweet_history_size);
                recent_tweets.push(Arc::new(info.tweet));

                entry.insert(RaidBossEntry {
                    boss_data: RaidBossMetadata {
                        boss,
                        last_seen,
                        image_hash: None,
                    },
                    broadcast,
                    recent_tweets,
                });

                true
            }
        };

        // Broadcast the tweet to the equivalent translated bosses
        match translations {
            Some(TranslationsExist::One { boss_name, tweet }) => {
                if let Some(value) = self.bosses.get_mut(&boss_name) {
                    value.broadcast.maybe_send(mapped_tweet_message.as_ref());
                    value.recent_tweets.push(tweet);
                }
            }
            None => {}
            Some(TranslationsExist::Multiple { boss_names, tweet }) => {
                for boss_name in boss_names {
                    if let Some(value) = self.bosses.get_mut(&boss_name) {
                        value.broadcast.maybe_send(mapped_tweet_message.as_ref());
                        value.recent_tweets.push(tweet.clone());
                    }
                }
            }
        }

        if is_new_boss {
            self.update_cached_boss_list();
        }
    }
}

impl<H, S, Sub, F, M> Future for Worker<H, S, Sub, F, M>
where
    H: ImageHasher,
    S: Stream<Item = RaidInfo, Error = Error>,
    Sub: Subscriber + Clone,
    F: Fn(Message) -> Option<Sub::Item>,
    M: Metrics,
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
