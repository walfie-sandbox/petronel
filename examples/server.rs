#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate lazy_static;

extern crate bytes;
extern crate futures;
extern crate hyper;
extern crate hyper_tls;
extern crate percent_encoding;
extern crate petronel;
extern crate regex;
extern crate serde;
extern crate serde_json;
extern crate tokio_core;

use bytes::Bytes;
use futures::{Future, Poll, Sink, Stream};
use futures::sync::mpsc;
use hyper::header;
use hyper::server::{Http, Request, Response, Service};
use hyper_tls::HttpsConnector;
use petronel::{Client, ClientBuilder, Subscriber, Subscription, Token};
use petronel::error::*;
use petronel::model::Message;
use regex::Regex;
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

    // TODO: Configurable port
    let bind_address = "127.0.0.1:3000".parse().chain_err(
        || "failed to parse address",
    )?;
    let listener = tokio_core::net::TcpListener::bind(&bind_address, &handle)
        .chain_err(|| "failed to bind TCP listener")?;

    let hyper_client = hyper::Client::configure()
        .connector(HttpsConnector::new(4, &handle).chain_err(|| "HTTPS error")?)
        .build(&handle);

    let (petronel_client, petronel_worker) =
        ClientBuilder::from_hyper_client(&hyper_client, &token)
            .with_history_size(10)
            .with_subscriber::<Sender>()
            .map_message(|msg| match msg {
                Message::Heartbeat => "\n".into(),
                other => {
                    let mut bytes = serde_json::to_vec(&other).unwrap();
                    bytes.push(b'\n');
                    bytes.into()
                }
            })
            .build();

    let petronel_server = PetronelServer(petronel_client.clone());

    println!("Listening on {}", bind_address);

    let http = Http::new();
    let server = listener
        .incoming()
        .for_each(move |(sock, addr)| {
            http.bind_connection(&handle, sock, addr, petronel_server.clone());
            Ok(())
        })
        .then(|r| r.chain_err(|| "server failed"));

    // Send heartbeat every 30 seconds
    let heartbeat = Interval::new(Duration::new(30, 0), &core.handle())
        .chain_err(|| "failed to create Interval")?
        .for_each(move |_| Ok(petronel_client.heartbeat()))
        .then(|r| r.chain_err(|| "heartbeat failed"));

    core.run(server.join3(petronel_worker, heartbeat))
        .chain_err(|| "stream failed")?;
    Ok(())
});

#[derive(Clone)]
struct Sender(mpsc::Sender<hyper::Result<hyper::Chunk>>);

impl Subscriber for Sender {
    type Item = Bytes;

    fn send(&mut self, bytes: &Bytes) -> std::result::Result<(), ()> {
        self.0
            .start_send(Ok(bytes.clone().into()))
            .and_then(|_| self.0.poll_complete().map(|_| ()))
            .map_err(|_| ())
    }
}

struct PetronelServer(Client<Sender>);

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

struct Body {
    body: hyper::Body,
    _subscription: Option<Subscription<Sender>>,
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
            let name = captures.name("boss_name").unwrap().as_str().to_string();

            let (sender, chunks) = hyper::Body::pair();

            let response = self.0
                .subscribe(Sender(sender))
                .map(move |mut subscription| {
                    subscription.follow(name);

                    let body = Body {
                        body: chunks,
                        _subscription: Some(subscription),
                    };

                    Response::new()
                        .with_header(header::TransferEncoding::chunked())
                        .with_header(header::Connection::keep_alive())
                        .with_body(body)
                })
                .map_err(|_| hyper::Error::Incomplete);

            Box::new(response) as Self::Future
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
