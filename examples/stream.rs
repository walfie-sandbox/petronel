#[macro_use]
extern crate error_chain;

extern crate futures;
extern crate tokio_core;
extern crate twitter_stream;
extern crate petronel;

use futures::Stream;

use petronel::error::*;
use tokio_core::reactor::Core;
use twitter_stream::Token;

fn env(name: &str) -> Result<String> {
    ::std::env::var(name).chain_err(|| {
        format!("invalid value for {} environment variable", name)
    })
}

quick_main!(|| -> Result<()> {
    let token = Token::new(
        env("CONSUMER_KEY")?,
        env("CONSUMER_SECRET")?,
        env("ACCESS_TOKEN")?,
        env("ACCESS_TOKEN_SECRET")?,
    );

    let mut core = Core::new().chain_err(|| "failed to create Core")?;

    let future = petronel::raid::RaidInfoStream::with_handle(&core.handle(), &token)
        .for_each(|raid_info| Ok(println!("{:#?}", raid_info)));

    core.run(future).chain_err(|| "stream failed")?;
    Ok(())
});
