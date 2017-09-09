mod phash;

pub use self::phash::ImageHash;
use error::*;
use futures::{Async, Future, IntoFuture, Poll, Stream};
use futures::stream::BufferUnordered;
use futures::unsync::mpsc;
use hyper::{Client, Uri};
use hyper::client::Connect;
use image::{self, GenericImage};
use model::BossName;
use std::collections::HashSet;

#[derive(Debug)]
pub struct BossImageHash {
    pub boss_name: BossName,
    pub image_hash: ImageHash,
}

pub fn channel<H, F>(image_hasher: H, concurrency: usize) -> (ImageHashSender, ImageHashReceiver<H>)
where
    H: ImageHasher<Future = F>,
    F: Future<Item = BossImageHash, Error = Error>,
{
    let (sink, stream) = mpsc::unbounded();
    let inner = Inner {
        image_hasher: image_hasher,
        stream,
        outstanding: HashSet::new(),
    };

    (
        ImageHashSender { sink },
        ImageHashReceiver(inner.buffer_unordered(concurrency)),
    )
}

// TODO: Rename to something like "requester"
#[derive(Debug)]
pub struct ImageHashSender {
    sink: mpsc::UnboundedSender<(BossName, Uri)>,
}

impl ImageHashSender {
    pub fn request(&self, boss_name: BossName, image_url: &str) {
        if let Ok(url) = image_url.parse() {
            let _ = self.sink.unbounded_send((boss_name, url));
        }
    }
}

#[must_use = "streams do nothing unless polled"]
pub struct ImageHashReceiver<H>(BufferUnordered<Inner<H>>)
where
    H: ImageHasher;

impl<H> Stream for ImageHashReceiver<H>
where
    H: ImageHasher,
{
    type Item = BossImageHash;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if let Some(result) = try_ready!(self.0.poll()) {
            self.0.get_mut().outstanding.remove(&result.boss_name);
            Ok(Async::Ready(Some(result)))
        } else {
            Ok(Async::Ready(None))
        }
    }
}

pub trait ImageHasher {
    type Future: Future<Item = BossImageHash, Error = Error>;

    fn hash(&self, boss_name: BossName, uri: Uri) -> Self::Future;
}

pub struct HyperImageHasher<'a, C>(pub &'a Client<C>)
where
    C: Connect + 'a;

impl<'a, C> ImageHasher for HyperImageHasher<'a, C>
where
    C: Connect + 'a,
{
    type Future = Box<Future<Item = BossImageHash, Error = Error>>;

    fn hash(&self, boss_name: BossName, uri: Uri) -> Self::Future {
        let result = self.0
            .get(uri)
            .and_then(|resp| resp.body().concat2())
            .then(|r| r.chain_err(|| ErrorKind::ImageHash))
            .and_then(|bytes| crop_and_hash(&bytes).into_future())
            .map(move |image_hash| {
                BossImageHash {
                    boss_name,
                    image_hash,
                }
            });

        Box::new(result)
    }
}


#[derive(Debug)]
#[must_use = "streams do nothing unless polled"]
struct Inner<H> {
    image_hasher: H,
    outstanding: HashSet<BossName>,
    stream: mpsc::UnboundedReceiver<(BossName, Uri)>,
}

impl<H> Stream for Inner<H>
where
    H: ImageHasher,
{
    type Item = H::Future;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let polled = self.stream.poll().map_err(|()| ErrorKind::ImageHash);

            if let Some((boss_name, uri)) = try_ready!(polled) {
                if !self.outstanding.contains(&boss_name) {
                    self.outstanding.insert(boss_name.clone());
                    let result = self.image_hasher.hash(boss_name, uri);
                    return Ok(Async::Ready(Some(result)));
                }
            } else {
                return Ok(Async::Ready(None));
            }
        }
    }
}

// Specifically for raid boss images. Remove the lower 25% of the image
// to get the boss image without the language-specific boss name.
fn crop_and_hash(bytes: &[u8]) -> Result<ImageHash> {
    let mut img = image::load_from_memory(bytes).chain_err(
        || ErrorKind::ImageHash,
    )?;
    let (w, h) = img.dimensions();
    img = img.crop(0, 0, w, h * 3 / 4);

    Ok(ImageHash::new(&img))
}
