#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate lazy_static;

extern crate regex;
extern crate futures;
extern crate tokio_core;
extern crate petronel;
extern crate hyper;
extern crate percent_encoding;
extern crate serde;
extern crate serde_json;

use futures::{Future, Poll, Sink, Stream};
use futures::sync::mpsc;
use hyper::header;
use hyper::server::{Http, Request, Response, Service};
use petronel::{Petronel, Subscriber, Subscription, Token};
use petronel::error::*;
use regex::Regex;
use serde::Serialize;
use tokio_core::reactor::Core;

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


    let bind_address = "127.0.0.1:3000".parse().chain_err(
        || "failed to parse address",
    )?;
    let listener = tokio_core::net::TcpListener::bind(&bind_address, &handle)
        .chain_err(|| "failed to bind TCP listener")?;

    let stream = petronel::raid::RaidInfoStream::with_handle(&core.handle(), &token);

    let (petronel, petronel_worker) = Petronel::from_stream(stream, 10);

    let petronel = PetronelServer(petronel);

    println!("Listening on {}", bind_address);

    let server = listener
        .incoming()
        .for_each(move |(sock, addr)| {
            Http::new().bind_connection(&handle, sock, addr, petronel.clone());
            Ok(())
        })
        .then(|r| r.chain_err(|| "server failed"));

    core.run(server.join(petronel_worker)).chain_err(
        || "stream failed",
    )?;
    Ok(())
});

#[derive(Clone)]
struct Sender(mpsc::Sender<hyper::Result<hyper::Chunk>>);

impl<T> Subscriber<T> for Sender
where
    T: Serialize,
{
    fn send(&mut self, message: &T) {
        let mut chunk = serde_json::to_string(message).unwrap();
        chunk.push('\n');

        let _ = self.0.start_send(Ok(chunk.into()));
        let _ = self.0.poll_complete();
    }
}

struct PetronelServer(Petronel<u16, Sender>);

impl Clone for PetronelServer {
    fn clone(&self) -> Self {
        PetronelServer(self.0.clone())
    }
}

#[derive(Serialize)]
struct JsonError {
    error: String,
    cause: Vec<String>,
}

lazy_static! {
    static ref REGEX_BOSS_TWEETS: Regex = Regex::new(
        r"^/bosses/(?P<boss_name>.+)/tweets$"
    ).unwrap();

    static ref REGEX_BOSS_STREAM: Regex = Regex::new(
        r"^/bosses/(?P<boss_name>.+)/stream$"
    ).unwrap();
}

// TODO: This isn't actually dropped until something attempts to write
// to it. Maybe add a heartbeat message.
struct Body {
    body: hyper::Body,
    _subscription: Option<Subscription<u16, Sender>>,
}

impl Stream for Body {
    type Item = hyper::Chunk;
    type Error = hyper::Error;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.body.poll()
    }
}

impl<T> From<T> for Body
where
    T: Into<hyper::Body>,
{
    fn from(t: T) -> Self {
        Body {
            body: t.into(),
            _subscription: None,
        }
    }
}

impl Service for PetronelServer {
    type Request = Request;
    type Response = Response<Body>;
    type Error = hyper::Error;

    type Future = Box<Future<Item = Self::Response, Error = hyper::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        let path = percent_encoding::percent_decode(req.path().as_bytes()).decode_utf8_lossy();

        if path == "/bosses" {
            let resp = self.0
                .bosses()
                .map(|bosses| {
                    let json = serde_json::to_string(&bosses).unwrap();

                    Response::new()
                        .with_header(header::ContentLength(json.len() as u64))
                        .with_header(header::ContentType::json())
                        .with_body(json)
                })
                .map_err(|_| hyper::Error::Incomplete);

            Box::new(resp) as Self::Future
        } else if let Some(captures) = REGEX_BOSS_TWEETS.captures(&path) {
            let name = captures.name("boss_name").unwrap().as_str();
            let resp = self.0
                .recent_tweets(name)
                .map(|tweets| {
                    let json = serde_json::to_string(
                        &tweets.into_iter().map(|t| t).collect::<Vec<_>>(),
                    ).unwrap();

                    Response::new()
                        .with_header(header::ContentLength(json.len() as u64))
                        .with_header(header::ContentType::json())
                        .with_body(json)
                })
                .map_err(|_| hyper::Error::Incomplete);

            Box::new(resp) as Self::Future
        } else if let Some(captures) = REGEX_BOSS_STREAM.captures(&path) {
            let name = captures.name("boss_name").unwrap().as_str();

            let (sender, chunks) = hyper::Body::pair();

            // TODO: Should this fail instead of returning 0?
            let id = req.remote_addr().map(|addr| addr.port()).unwrap_or(0);

            let mut subscription = self.0.subscribe(id, Sender(sender));
            subscription.follow(name);

            let body = Body {
                body: chunks,
                _subscription: Some(subscription),
            };

            let response = Response::new()
                .with_header(header::TransferEncoding::chunked())
                .with_header(header::Connection::keep_alive())
                .with_body(body);

            Box::new(futures::future::ok(response)) as Self::Future
        } else {
            let json = serde_json::to_string(&JsonError {
                error: format!("Unrecognized path: {}", path),
                cause: vec![],
            }).unwrap();

            Box::new(futures::future::ok(
                Response::new()
                    .with_status(hyper::StatusCode::NotFound)
                    .with_header(header::ContentLength(json.len() as u64))
                    .with_header(header::ContentType::json())
                    .with_body(json),
            )) as Self::Future
        }
    }
}
