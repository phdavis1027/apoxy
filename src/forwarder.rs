use std::{
    collections::HashMap,
    convert::Infallible,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full};
use hyper::{
    body::{Bytes, Incoming},
    Method, Request, Response,
};

use hyper_util::client::legacy::{connect::HttpConnector, Builder as ConnBuilder, Client};
use tokio::sync::oneshot;

use crate::local_executor::LocalExecutor;

enum LoadBalancingStrategy {
    RoundRobin,
    Random,
    LeastConnections,
}

pub struct AppBackendMetrics {
    pub active_connections: u64,
    pub requests: u64,
    pub errors: u64,
}

pub struct AppBackend {
    client: Client<HttpConnector, Incoming>,
    metrics: AppBackendMetrics,
}

impl AppBackend {
    pub async fn send_request(
        &self,
        req: Request<Incoming>,
    ) -> Result<Response<Incoming>, hyper_util::client::legacy::Error> {
        self.client.request(req).await
    }
}

pub struct App {
    load_balancing_strategy: LoadBalancingStrategy,
    backends: HashMap<String, AppBackend>,
}

impl App {
    pub async fn handle_request(
        &self,
        req: Request<Incoming>,
    ) -> Result<Response<UnsyncBoxBody<Bytes, hyper::Error>>, hyper::Error> {
        let backend = self.select_backend(&req);
        let incoming = backend.send_request(req).await;
        self.incoming_to_outgoing(incoming).await
    }

    // Part of why this is necessary is to convert away from a legacy error
    async fn incoming_to_outgoing(
        &self,
        incoming: Result<Response<Incoming>, hyper_util::client::legacy::Error>,
    ) -> Result<Response<UnsyncBoxBody<Bytes, hyper::Error>>, hyper::Error> {
        match incoming {
            Ok(incoming) => Response::builder().status(incoming.status()).body(
                incoming
                    .into_body()
                    .collect()
                    .await?
                    .map_err(|_| hyper::Error::from("Infallible".into()))
                    .boxed_unsync(),
            ),
            Err(e) => Err(hyper::Error::from(e)),
        }
    }

    fn select_backend(&self, req: &Request<Incoming>) -> &AppBackend {
        unimplemented!()
    }
}

pub enum ForwarderMsg {
    IncomingConnection(
        Request<Incoming>,
        oneshot::Sender<Result<Response<Bytes>, hyper::Error>>,
    ),
}

pub struct PathResolver {}

impl PathResolver {
    pub fn new() -> PathResolver {
        PathResolver {}
    }

    pub fn resolve(&self, path: &str, method: &Method) -> Option<&App> {
        unimplemented!()
    }
}

pub struct Forwarder {
    pub receiver: tokio::sync::mpsc::Receiver<ForwarderMsg>,
    resolver: PathResolver,
}

impl Forwarder {
    async fn handle_msg(&mut self, msg: ForwarderMsg) {
        match msg {
            ForwarderMsg::IncomingConnection(req, doorman_reply_to) => {
                self.handle_incoming_connection(req, doorman_reply_to).await;
            }
        }
    }

    async fn handle_incoming_connection(
        &mut self,
        req: Request<Incoming>,
        doorman_reply_to: oneshot::Sender<
            Result<Response<UnsyncBoxBody<Bytes, hyper::Error>>, hyper_util::client::legacy::Error>,
        >,
    ) {
        let app = match self.resolver.resolve(req.uri().path(), req.method()) {
            Some(a) => a,
            None => {
                let response = Response::builder()
                    .status(404)
                    .body(Bytes::from("Not Found"))
                    .unwrap();
                doorman_reply_to.send(Ok(response)).unwrap();
                return;
            }
        };
        doorman_reply_to
            .send(app.handle_request(req).await)
            .unwrap();
    }
}

#[derive(Clone)]
pub struct ForwarderHandle {
    pub sender: tokio::sync::mpsc::Sender<ForwarderMsg>,
}

impl ForwarderHandle {
    pub fn new() -> ForwarderHandle {
        let (sender, receiver) = tokio::sync::mpsc::channel(100);
        let forwarder = Forwarder { receiver };
        tokio::spawn(run_forwarder(forwarder));
        ForwarderHandle { sender }
    }
}

async fn run_forwarder(mut forwarder: Forwarder) {
    while let Some(msg) = forwarder.receiver.recv().await {
        forwarder.handle_msg(msg).await;
    }
}
