mod builder;
mod client;
mod worker;
mod subscription;

pub use self::builder::ClientBuilder;
pub use self::client::Client;
pub use self::subscription::Subscription;
pub use self::worker::Worker;

use error::*;
use futures::{Future, Poll};
use futures::unsync::oneshot;
use id_pool::Id as SubId;
use image_hash::ImageHash;
use model::{BossName, DateTime, RaidBoss, RaidBossMetadata, RaidTweet};
use raid::RaidInfo;
use std::sync::Arc;

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
    SubscriberGetTweets { id: SubId, boss_name: BossName },
    SubscriberHeartbeat,

    SubscriberSubscribe {
        subscriber: Sub,
        client: Client<Sub>,
        sender: oneshot::Sender<Subscription<Sub>>,
    },
    SubscriberUnsubscribe(SubId),

    ClientGetBosses(oneshot::Sender<Vec<RaidBoss>>),
    ClientGetTweets {
        boss_name: BossName,
        sender: oneshot::Sender<Vec<Arc<RaidTweet>>>,
    },
    ClientExportMetadata(oneshot::Sender<Vec<RaidBossMetadata>>),

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
