#[macro_use]
extern crate error_chain;

extern crate futures;
extern crate tokio_core;
extern crate twitter_stream;
extern crate petronel;

use futures::{Future, Stream};

use petronel::error::*;
use tokio_core::reactor::Core;
use twitter_stream::{Token, TwitterStreamBuilder};
use twitter_stream::message::StreamMessage;

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

    // Lv15 ... Lv175
    let mut tracking = (3..35)
        .map(|i| format!("Lv{}", i * 5))
        .collect::<Vec<_>>()
        .join(",");

    tracking.push_str(",I need backup!Battle ID:");

    let future = TwitterStreamBuilder::filter(&token)
        .handle(&core.handle())
        .user_agent(Some("petronel"))
        .timeout(None)
        .track(Some(&tracking))
        .listen()
        .flatten_stream()
        .for_each(|json| {
            if let Ok(StreamMessage::Tweet(tweet)) = StreamMessage::from_str(&json) {
                if let Some(r) = petronel::raid::RaidInfo::from_tweet(*tweet) {
                    println!("{:#?}", r);
                }
            }

            Ok(())
        });

    core.run(future).chain_err(|| "stream failed")?;
    Ok(())
});
