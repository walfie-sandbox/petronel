#[macro_use]
extern crate error_chain;

extern crate hyper;
extern crate hyper_tls;
extern crate futures;
extern crate tokio_core;
extern crate petronel;

use futures::{Future, Stream};
use hyper_tls::HttpsConnector;
use petronel::{ClientBuilder, Token};
use petronel::error::*;
use std::time::Duration;
use tokio_core::reactor::{Core, Interval};

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

    let handle = core.handle();

    let hyper_client = hyper::Client::configure()
        .connector(HttpsConnector::new(4, &handle).chain_err(|| "HTTPS error")?)
        .build(&handle);

    let (client, worker) = ClientBuilder::from_hyper_client(&hyper_client, &token).build();

    // Fetch boss list once per 5 seconds
    let interval = Interval::new(Duration::new(5, 0), &handle)
        .chain_err(|| "failed to create interval")?
        .then(|r| r.chain_err(|| "interval failed"))
        .and_then(move |_| client.bosses())
        .for_each(|mut bosses| {
            bosses.sort_by_key(|b| b.level);

            for boss in bosses.iter() {
                print!(
                    "{:<3} | {} ({:?})",
                    boss.level,
                    boss.name,
                    boss.language,
                );

                for image in boss.image.iter() {
                    println!(" {}", image);
                }
            }

            println!("");
            Ok(())
        });

    core.run(worker.join(interval)).chain_err(
        || "stream failed",
    )?;
    Ok(())
});
