#[macro_use]
extern crate error_chain;

extern crate futures;
extern crate tokio_core;
extern crate petronel;
extern crate hyper;

use futures::{Future, IntoFuture, Stream};
use hyper::header::ContentLength;
use hyper::server::{Http, Request, Response, Service};
use petronel::{Petronel, Token};
use petronel::error::*;
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

#[derive(Debug, Clone)]
struct PetronelServer(Petronel);

type StringFuture = Box<Future<Item = String, Error = Error>>;
impl Service for PetronelServer {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;

    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        use hyper::Method::*;

        let body = match (req.method(), req.path()) {
            (&Get, "/bosses") => {
                Box::new(self.0.get_bosses().map(|bosses| format!("{:#?}", bosses))) as StringFuture
            }
            (_, path) => {
                // TODO: 404
                let message = format!("Unrecognized path: {}", path);
                Box::new(Ok(message).into_future()) as StringFuture
            }
        };

        Box::new(
            body.map(|b| {
                Response::new()
                    .with_header(ContentLength(b.len() as u64))
                    .with_body(b)
            }).map_err(|_| hyper::Error::Incomplete),
        )
    }
}
