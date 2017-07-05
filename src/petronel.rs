use error::*;
use futures::{Async, Future, Poll, Stream};
use futures::stream::{Map, OrElse, Select};
use futures::unsync::mpsc;
use futures::unsync::oneshot;

use raid::{BossImageUrl, BossLevel, BossName, DateTime, RaidInfo};
use std::collections::HashMap;
use std::collections::hash_map::Entry;

const DEFAULT_BOSS_LEVEL: BossLevel = 0;

#[derive(Clone, Debug, PartialEq)]
pub struct RaidBoss {
    pub name: BossName,
    pub level: BossLevel,
    pub image: Option<BossImageUrl>,
    pub last_seen: DateTime,
}

enum Event {
    NewRaidInfo(RaidInfo),
    GetBosses(oneshot::Sender<String>),
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


pub struct Petronel(mpsc::UnboundedSender<Event>);
impl Petronel {
    fn request<T, F>(&self, f: F) -> AsyncResult<T>
    where
        F: FnOnce(oneshot::Sender<T>) -> Event,
    {
        let (tx, rx) = oneshot::channel();
        let _ = mpsc::UnboundedSender::send(&self.0, f(tx));
        AsyncResult(rx)
    }

    // Return something other than String
    pub fn get_bosses(&self) -> AsyncResult<String> {
        self.request(Event::GetBosses)
    }
}

pub struct PetronelFuture<S> {
    events: Select<
        Map<S, fn(RaidInfo) -> Event>,
        OrElse<mpsc::UnboundedReceiver<Event>, fn(()) -> Result<Event>, Result<Event>>,
    >,
    bosses: HashMap<BossName, RaidBoss>,
}

impl Petronel {
    fn events_read_error(_: ()) -> Result<Event> {
        Ok(Event::ReadError)
    }

    pub fn from_stream<S>(stream: S) -> (Self, PetronelFuture<S>)
    where
        S: Stream<Item = RaidInfo, Error = Error>,
    {
        let (tx, rx) = mpsc::unbounded();

        let stream_events = stream.map(Event::NewRaidInfo as fn(RaidInfo) -> Event);
        let rx = rx.or_else(Self::events_read_error as fn(()) -> Result<Event>);

        let future = PetronelFuture {
            events: stream_events.select(rx),
            bosses: HashMap::new(),
        };

        (Petronel(tx), future)
    }
}

impl<S> PetronelFuture<S> {
    fn handle_event(&mut self, event: Event) {
        use self::Event::*;

        match event {
            NewRaidInfo(r) => {
                self.handle_raid_info(r);
            }
            GetBosses(tx) => {
                let s = format!("{:#?}", self.bosses);
                let _ = tx.send(s);
            }
            ReadError => {} // This should never happen
        }
    }

    fn handle_raid_info(&mut self, info: RaidInfo) {
        match self.bosses.entry(info.tweet.boss_name) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().last_seen = info.tweet.created_at;
            }
            Entry::Vacant(entry) => {
                let name = entry.key().clone();

                entry.insert(RaidBoss {
                    level: name.parse_level().unwrap_or(DEFAULT_BOSS_LEVEL),
                    name: name,
                    image: info.image,
                    last_seen: info.tweet.created_at,
                });
            }
        }
    }
}

impl<S> Future for PetronelFuture<S>
where
    S: Stream<Item = RaidInfo, Error = Error>,
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
