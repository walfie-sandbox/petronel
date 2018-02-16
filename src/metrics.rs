use model::BossName;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

pub trait Metrics {
    type Export;

    fn set_total_subscriber_count(&mut self, count: u32);
    fn set_follower_count(&mut self, boss_name: &BossName, count: u32);
    fn inc_tweet_count(&mut self, boss_name: &BossName);
    fn remove_boss(&mut self, boss_name: &BossName);
    fn export(&self) -> Self::Export;
}

pub struct NoOp;
impl Metrics for NoOp {
    type Export = ();

    fn set_total_subscriber_count(&mut self, _count: u32) {}
    fn set_follower_count(&mut self, _boss_name: &BossName, _count: u32) {}
    fn inc_tweet_count(&mut self, _boss_name: &BossName) {}
    fn remove_boss(&mut self, _boss_name: &BossName) {}
    fn export(&self) -> Self::Export {}
}

pub fn simple<F, T>(export_function: F) -> Simple<F>
where
    F: Fn(&SimpleMetrics) -> T,
{
    Simple {
        inner: SimpleMetrics {
            total_subscriber_count: 0,
            boss_counts: HashMap::new(),
        },
        export_function,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Simple<F> {
    inner: SimpleMetrics,
    export_function: F,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SimpleMetrics {
    total_subscriber_count: u32,
    boss_counts: HashMap<BossName, Counts>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct Counts {
    followers: u32,
    tweets: u32,
}

impl<T, F> Metrics for Simple<F>
where
    F: Fn(&SimpleMetrics) -> T,
{
    type Export = T;

    fn set_total_subscriber_count(&mut self, count: u32) {
        self.inner.total_subscriber_count = count;
    }

    fn set_follower_count(&mut self, boss_name: &BossName, count: u32) {
        // TODO: Maybe have a way that doesn't require cloning
        match self.inner.boss_counts.entry(boss_name.clone()) {
            Entry::Occupied(mut e) => {
                e.get_mut().followers = count;
            }
            Entry::Vacant(e) => {
                e.insert(Counts {
                    followers: 0,
                    tweets: 1,
                });
            }
        }
    }

    fn inc_tweet_count(&mut self, boss_name: &BossName) {
        // TODO: Maybe have a way that doesn't require cloning
        match self.inner.boss_counts.entry(boss_name.clone()) {
            Entry::Occupied(mut e) => {
                let counts = e.get_mut();
                counts.tweets = counts.tweets.wrapping_add(1);
            }
            Entry::Vacant(e) => {
                e.insert(Counts {
                    followers: 0,
                    tweets: 1,
                });
            }
        }
    }

    fn remove_boss(&mut self, boss_name: &BossName) {
        self.inner.boss_counts.remove(boss_name);
    }

    fn export(&self) -> Self::Export {
        (self.export_function)(&self.inner)
    }
}
