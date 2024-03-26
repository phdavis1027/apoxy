use std::collections::HashMap;

use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Empty};
use hyper::{
    body::{Bytes, Incoming},
    Method, Request, Response, StatusCode,
};

use hyper_util::client::legacy::{connect::HttpConnector, Client};
use tokio::sync::oneshot;

use crate::error::compat::CompatibilityHyperError;

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

    fn select_backend(&self, _req: &IncomingRequest) -> &AppBackend {
        unimplemented!()
    }
}

pub enum ForwarderMsg {
    IncomingConnection(IncomingRequest, oneshot::Sender<OutgoingResponse>),
    PanicButton,
}

pub struct PathResolver {}

impl PathResolver {
    pub fn new() -> PathResolver {
        PathResolver {}
    }

    pub fn resolve(&self, _path: &str, _method: &Method) -> Option<&App> {
        unimplemented!()
    }
}

pub struct Forwarder {
    pub receiver: tokio::sync::mpsc::Receiver<ForwarderMsg>,
    pub loopback_sender: tokio::sync::mpsc::Sender<ForwarderMsg>,
    resolver: PathResolver,
}

impl Forwarder {
    fn new(
        receiver: tokio::sync::mpsc::Receiver<ForwarderMsg>,
        resolver: PathResolver,
        loopback_sender: tokio::sync::mpsc::Sender<ForwarderMsg>,
    ) -> Forwarder {
        Forwarder {
            receiver,
            resolver,
            loopback_sender,
        }
    }

    async fn handle_msg(
        &mut self,
        msg: ForwarderMsg,
        loopback_sender: tokio::sync::mpsc::Sender<ForwarderMsg>,
    ) -> Result<(), ()> {
        match msg {
            ForwarderMsg::PanicButton => {
                // We should shut down.
                Err(())
            }
            ForwarderMsg::IncomingConnection(req, doorman_reply_to) => {
                self.handle_incoming_connection(req, doorman_reply_to, loopback_sender)
                    .await;
                Ok(())
            }
        }
    }

    async fn handle_incoming_connection(
        &mut self,
        req: IncomingRequest,
        doorman_reply_to: oneshot::Sender<OutgoingResponse>,
        loopback_sender: tokio::sync::mpsc::Sender<ForwarderMsg>,
    ) {
        let app = match self.resolver.resolve(req.uri().path(), req.method()) {
            Some(a) => a,
            None => {
                let response = Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(
                        Empty::<Bytes>::new()
                            .map_err(CompatibilityHyperError::from)
                            .boxed_unsync(),
                    )
                    .unwrap();
                if doorman_reply_to.send(response).is_err() {
                    loopback_sender.blocking_send(ForwarderMsg::PanicButton);
                }
                return;
            }
        };

        // If the doorman fell over, we should shut down.
        // They hold the connection to the client, so we can't do anything.
        let shutdown: bool = match app.handle_request(req).await {
            Ok(response) => doorman_reply_to.send(response).is_err(),
            Err(_e) => {
                let response = Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(
                        Empty::<Bytes>::new()
                            .map_err(CompatibilityHyperError::from)
                            .boxed_unsync(),
                    )
                    .unwrap();
                doorman_reply_to.send(response).is_err()
            }
        };

        if shutdown {
            loopback_sender.blocking_send(ForwarderMsg::PanicButton);
        }
    }
}

#[derive(Clone)]
pub struct ForwarderHandle {
    pub sender: tokio::sync::mpsc::Sender<ForwarderMsg>,
}

impl ForwarderHandle {
    pub fn new() -> ForwarderHandle {
        let (sender, receiver) = tokio::sync::mpsc::channel(100);

        let resolver = PathResolver::new();

        let forwarder = Forwarder::new(receiver, resolver, sender.clone());

        tokio::spawn(run_forwarder(forwarder));

        ForwarderHandle { sender }
    }
}

async fn run_forwarder(mut forwarder: Forwarder) {
    while let Some(msg) = forwarder.receiver.recv().await {
        let loopback_sender = forwarder.loopback_sender.clone();
        if let Err(_) = forwarder.handle_msg(msg, loopback_sender).await {
            return;
        }
    }
}
