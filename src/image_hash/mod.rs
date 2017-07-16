mod phash;

pub use self::phash::ImageHash;
use error::*;
use futures::{Future, IntoFuture, Stream};
use hyper::{Client, Uri};
use hyper::client::Connect;
use image::{self, GenericImage};
use model::BossName;

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

fn fetch_and_hash<C>(
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
