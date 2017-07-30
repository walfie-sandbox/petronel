pub use client::Client;

use id_pool::Id as SubId;
use model::BossName;
use std::collections::HashSet;

// TODO: Figure out if there is a way to do this without owning `Client`
#[must_use = "Subscriptions are cancelled when they go out of scope"]
#[derive(Debug)]
pub struct Subscription<Sub> {
    pub(crate) id: SubId,
    pub(crate) following: HashSet<BossName>,
    pub(crate) client: Client<Sub>,
}

impl<Sub> Subscription<Sub> {
    pub fn follow<B>(&mut self, boss_name: B)
    where
        B: Into<BossName>,
    {
        let name = boss_name.into();
        self.following.insert(name.clone());
        self.client.subscriber_follow(self.id.clone(), name);
    }

    pub fn unfollow<B>(&mut self, boss_name: B)
    where
        B: Into<BossName>,
    {
        let name = boss_name.into();
        self.following.remove(&name);
        self.client.subscriber_unfollow(self.id.clone(), name);
    }

    pub fn get_bosses(&self) {
        self.client.subscriber_get_bosses(self.id.clone())
    }

    pub fn get_tweets<B>(&self, boss_name: B)
    where
        B: Into<BossName>,
    {
        self.client.subscriber_get_tweets(
            self.id.clone(),
            boss_name.into(),
        )
    }


    #[inline]
    pub fn unsubscribe(self) {
        self.non_consuming_unsubscribe()
    }

    // This is needed for the Drop implementation
    fn non_consuming_unsubscribe(&self) {
        self.client.subscriber_unsubscribe(self.id.clone())
    }
}

impl<Sub> Drop for Subscription<Sub> {
    fn drop(&mut self) {
        let mut following = ::std::mem::replace(&mut self.following, HashSet::with_capacity(0));

        for boss_name in following.drain() {
            self.unfollow(boss_name);
        }

        self.non_consuming_unsubscribe();
    }
}
