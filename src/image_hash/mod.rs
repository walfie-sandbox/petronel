mod phash;

pub use self::phash::ImageHash;
use error::*;
use futures::{Async, Future, IntoFuture, Poll, Stream};
use futures::stream::BufferUnordered;
use futures::unsync::mpsc;
use hyper::{Client, Uri};
use hyper::client::Connect;
use image::{self, GenericImage};
use model::{BossImageUrl, BossName};
use std::collections::HashSet;

pub fn channel<'a, C>(
    client: &'a Client<C>,
    concurrency: usize,
) -> (ImageHashSender, ImageHashReceiver<'a, C>)
where
    C: 'a + Connect,
{
    let (sink, stream) = mpsc::unbounded();
    let sender = ImageHashSender { sink };
    let receiver = Inner {
        client,
        stream,
        outstanding: HashSet::new(),
    };

    (
        sender,
        ImageHashReceiver(receiver.buffer_unordered(concurrency)),
    )
}

pub struct ImageHashSender {
    sink: mpsc::UnboundedSender<(BossName, Uri)>,
}

impl ImageHashSender {
    fn request(&self, boss_name: BossName, image_url: BossImageUrl) {
        if let Ok(url) = image_url.as_str().parse() {
            let _ = mpsc::UnboundedSender::send(&self.sink, (boss_name, url));
        }
    }
}

#[must_use = "streams do nothing unless polled"]
pub struct ImageHashReceiver<'a, C>(BufferUnordered<Inner<'a, C>>)
where
    C: 'a + Connect;

impl<'a, C> Stream for ImageHashReceiver<'a, C>
where
    C: 'a + Connect,
{
    type Item = (BossName, ImageHash);
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if let Some(result) = try_ready!(self.0.poll()) {
            self.0.get_mut().outstanding.remove(&result.0); // TODO: Named
            Ok(Async::Ready(Some(result)))
        } else {
            Ok(Async::Ready(None))
        }
    }
}


#[must_use = "streams do nothing unless polled"]
struct Inner<'a, C: 'a> {
    client: &'a Client<C>,
    outstanding: HashSet<BossName>,
    stream: mpsc::UnboundedReceiver<(BossName, Uri)>,
}

impl<'a, C> Stream for Inner<'a, C>
where
    C: 'a + Connect,
{
    type Item = Box<Future<Item = (BossName, ImageHash), Error = Error>>;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let polled = self.stream.poll().map_err(|_| ErrorKind::ImageHash);

            if let Some((boss_name, uri)) = try_ready!(polled) {
                if !self.outstanding.contains(&boss_name) {
                    self.outstanding.insert(boss_name.clone());
                    let result = fetch_and_hash(self.client, boss_name, uri);
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

pub fn fetch_and_hash<C>(
    client: &Client<C>,
    boss_name: BossName,
    uri: Uri,
) -> Box<Future<Item = (BossName, ImageHash), Error = Error>>
where
    C: Connect,
{
    let result = client
        .get(uri)
        .and_then(|resp| resp.body().concat2())
        .then(|r| r.chain_err(|| ErrorKind::ImageHash))
        .and_then(|bytes| crop_and_hash(&bytes).into_future())
        .map(move |hash| (boss_name, hash));

    Box::new(result)
}
