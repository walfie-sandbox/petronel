use super::{AsyncResult, Event, RemoveBossesPredicate, Subscription};
use futures::unsync::{mpsc, oneshot};
use id_pool::Id as SubId;
use model::{BossName, RaidBoss, RaidBossMetadata, RaidTweet};
use std::sync::Arc;

#[derive(Debug)]
pub struct Client<Sub, M = ()>(pub(crate) mpsc::UnboundedSender<Event<Sub, M>>);

impl<Sub, M> Clone for Client<Sub, M> {
    fn clone(&self) -> Self {
        Client(self.0.clone())
    }
}

impl<Sub, M> Client<Sub, M> {
    fn send(&self, event: Event<Sub, M>) {
        let _ = self.0.unbounded_send(event);
    }

    fn request<T, F>(&self, f: F) -> AsyncResult<T>
    where
        F: FnOnce(oneshot::Sender<T>) -> Event<Sub, M>,
    {
        let (tx, rx) = oneshot::channel();
        self.send(f(tx));
        AsyncResult(rx)
    }

    pub fn subscribe(&self, subscriber: Sub) -> AsyncResult<Subscription<Sub, M>> {
        self.request(|sender| {
            Event::SubscriberSubscribe {
                subscriber,
                sender,
                client: self.clone(),
            }
        })
    }

    pub(crate) fn subscriber_unsubscribe(&self, id: SubId) {
        self.send(Event::SubscriberUnsubscribe(id));
    }

    pub(crate) fn subscriber_follow(&self, id: SubId, boss_name: BossName) {
        self.send(Event::SubscriberFollow { id, boss_name });
    }

    pub(crate) fn subscriber_unfollow(&self, id: SubId, boss_name: BossName) {
        self.send(Event::SubscriberUnfollow { id, boss_name });
    }

    pub(crate) fn subscriber_get_bosses(&self, id: SubId) {
        self.send(Event::SubscriberGetBosses(id))
    }

    pub(crate) fn subscriber_get_tweets(&self, id: SubId, boss_name: BossName) {
        self.send(Event::SubscriberGetTweets { id, boss_name })
    }

    pub fn bosses(&self) -> AsyncResult<Vec<RaidBoss>> {
        self.request(Event::ClientGetBosses)
    }

    pub fn tweets<B>(&self, boss_name: B) -> AsyncResult<Vec<Arc<RaidTweet>>>
    where
        B: Into<BossName>,
    {
        self.request(|tx| {
            Event::ClientGetTweets {
                boss_name: boss_name.into(),
                sender: tx,
            }
        })
    }

    pub fn export_metadata(&self) -> AsyncResult<Vec<RaidBossMetadata>> {
        self.request(Event::ClientExportMetadata)
    }

    pub fn export_metrics(&self) -> AsyncResult<M> {
        self.request(Event::ClientExportMetrics)
    }

    pub fn remove_bosses<F>(&self, f: F)
    where
        F: Fn(&RaidBossMetadata) -> bool + 'static,
    {
        self.send(Event::ClientRemoveBosses(
            RemoveBossesPredicate(Box::new(f)),
        ));
    }

    pub fn heartbeat(&self) {
        self.send(Event::SubscriberHeartbeat);
    }
}
