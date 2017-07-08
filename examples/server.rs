#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate serde_derive;

extern crate futures;
extern crate tokio_core;
extern crate petronel;
extern crate hyper;
extern crate percent_encoding;
extern crate serde_json;

use futures::{Future, Stream};
use hyper::header;
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

#[derive(Serialize)]
struct JsonError {
    error: String,
    cause: Vec<String>,
}

impl Service for PetronelServer {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;

    type Future = Box<Future<Item = Self::Response, Error = hyper::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        use hyper::Method::*;

        match (req.method(), req.path()) {
            (&Get, "/bosses") => {
                let resp = self.0
                    .get_bosses()
                    .map(|bosses| {
                        let json = serde_json::to_string(&bosses).unwrap();

                        Response::new()
                            .with_header(header::ContentLength(json.len() as u64))
                            .with_header(header::ContentType::json())
                            .with_body(json)
                    })
                    .map_err(|_| hyper::Error::Incomplete);

                Box::new(resp) as Self::Future
            }
            (_, path) => {
                let path = percent_encoding::percent_decode(path.as_bytes()).decode_utf8_lossy();
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
}
