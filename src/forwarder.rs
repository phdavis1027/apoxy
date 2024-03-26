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

use crate::{error::compat::CompatibilityHyperError, local_executor::LocalExecutor};

pub type OutgoingResponse = Response<UnsyncBoxBody<Bytes, CompatibilityHyperError>>;
pub type IncomingResponse = Response<Incoming>;

pub type OutgoingRequest = Request<UnsyncBoxBody<Bytes, CompatibilityHyperError>>;
pub type IncomingRequest = Request<Incoming>;

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
    pub async fn send_incoming_request(
        &self,
        req: Request<Incoming>,
    ) -> Result<IncomingResponse, CompatibilityHyperError> {
        self.client
            .request(req)
            .await
            .map_err(CompatibilityHyperError::from)
    }
}

pub struct App {
    load_balancing_strategy: LoadBalancingStrategy,
    backends: HashMap<String, AppBackend>,
}

impl App {
    pub async fn handle_request(
        &self,
        req: IncomingRequest,
    ) -> Result<OutgoingResponse, CompatibilityHyperError> {
        let backend = self.select_backend(&req);
        let incoming = backend.send_incoming_request(req).await;
        self.incoming_to_outgoing(incoming).await
    }

    // Part of why this is necessary is to convert away from a legacy error
    async fn incoming_to_outgoing(
        &self,
        incoming: Result<Response<Incoming>, CompatibilityHyperError>,
    ) -> Result<OutgoingResponse, CompatibilityHyperError> {
        let (parts, body) = incoming?.into_parts();
        let body = body
            .collect()
            .await?
            .map_err(CompatibilityHyperError::from)
            .boxed_unsync();

        Response::builder()
            .status(parts.status)
            .body(body)
            .map_err(CompatibilityHyperError::from)
    }

    fn select_backend(&self, req: &IncomingRequest) -> &AppBackend {
        unimplemented!()
    }
}

pub enum ForwarderMsg {
    IncomingConnection(IncomingRequest, oneshot::Sender<OutgoingResponse>),
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
        req: IncomingRequest,
        doorman_reply_to: oneshot::Sender<OutgoingResponse>,
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
