mod builder;
mod client;
mod worker;
mod subscription;

pub use self::builder::ClientBuilder;
pub use self::client::Client;
pub use self::subscription::Subscription;
pub use self::worker::Worker;

use broadcast::Broadcast;
use circular_buffer::CircularBuffer;
use error::*;
use futures::{Future, Poll};
use futures::unsync::oneshot;
use id_pool::Id as SubId;
use image_hash::ImageHash;
use model::{BossName, DateTime, RaidBoss, RaidTweet};
use raid::RaidInfo;
use std::sync::Arc;

pub(crate) struct RaidBossEntry<Sub> {
    boss: RaidBoss,
    last_seen: DateTime,
    image_hash: Option<ImageHash>,
    recent_tweets: CircularBuffer<Arc<RaidTweet>>,
    broadcast: Broadcast<SubId, Sub>,
}

#[derive(Debug)]
pub(crate) enum Event<Sub> {
    NewRaidInfo(RaidInfo),
    NewImageHash {
        boss_name: BossName,
        image_hash: ImageHash,
    },

    SubscriberFollow { id: SubId, boss_name: BossName },
    SubscriberUnfollow { id: SubId, boss_name: BossName },
    SubscriberGetBosses(SubId),
    SubscriberHeartbeat,

    SubscriberSubscribe {
        subscriber: Sub,
        client: Client<Sub>,
        sender: oneshot::Sender<Subscription<Sub>>,
    },
    SubscriberUnsubscribe(SubId),

    ClientGetBosses(oneshot::Sender<Vec<RaidBoss>>),
    ClientGetRecentTweets {
        boss_name: BossName,
        sender: oneshot::Sender<Vec<Arc<RaidTweet>>>,
    },

    ClientReadError,
}

pub struct AsyncResult<T>(oneshot::Receiver<T>);
impl<T> Future for AsyncResult<T> {
    type Item = T;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0.poll().map_err(|_| ErrorKind::Closed.into())
    }
}
