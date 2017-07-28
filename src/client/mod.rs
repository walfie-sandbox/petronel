mod builder;
mod client;
mod subscription;

pub use self::builder::ClientBuilder;
pub use self::client::{Client, ClientWorker};
pub use self::subscription::Subscription;

use broadcast::Broadcast;
use circular_buffer::CircularBuffer;
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

    Follow { id: SubId, boss_name: BossName },
    Unfollow { id: SubId, boss_name: BossName },

    Subscribe {
        subscriber: Sub,
        client: Client<Sub>,
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
