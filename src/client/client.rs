use super::{AsyncResult, Event, Subscription};
use futures::unsync::{mpsc, oneshot};
use id_pool::Id as SubId;
use model::{BossName, RaidBoss, RaidTweet};
use std::sync::Arc;

#[derive(Debug)]
pub struct Client<Sub>(pub(crate) mpsc::UnboundedSender<Event<Sub>>);

impl<Sub> Clone for Client<Sub> {
    fn clone(&self) -> Self {
        Client(self.0.clone())
    }
}

impl<Sub> Client<Sub> {
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
                client: self.clone(),
            }
        })
    }

    pub(crate) fn unsubscribe(&self, id: SubId) {
        self.send(Event::Unsubscribe(id));
    }

    pub(crate) fn follow(&self, id: SubId, boss_name: BossName) {
        self.send(Event::Follow { id, boss_name });
    }

    pub(crate) fn unfollow(&self, id: SubId, boss_name: BossName) {
        self.send(Event::Unfollow { id, boss_name });
    }

    pub(crate) fn get_cached_boss_list(&self, id: SubId) {
        self.send(Event::GetCachedBossList(id))
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
