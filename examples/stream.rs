#[macro_use]
extern crate error_chain;

extern crate futures;
extern crate petronel;
extern crate tokio_core;

use futures::Stream;
use petronel::Token;
use petronel::error::*;
use tokio_core::reactor::Core;

fn env(name: &str) -> Result<String> {
    ::std::env::var(name).chain_err(|| format!("invalid value for {} environment variable", name))
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
