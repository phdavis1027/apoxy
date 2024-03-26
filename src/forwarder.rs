use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use http_body_util::Full;
use hyper::{
    body::{Bytes, Incoming},
    Method, Request, Response,
};
use hyper_util::client::legacy::Builder;
use tokio::sync::oneshot;

use crate::local_executor::LocalExecutor;

pub enum ForwarderMsg {
    IncomingConnection(
        Request<Incoming>,
        oneshot::Sender<Result<Response<Full<Bytes>>, hyper::Error>>,
    ),
}

pub struct PathResolver {}

impl PathResolver {
    pub fn new() -> PathResolver {
        PathResolver {}
    }

    pub fn resolve(&self, path: &str, method: &Method) -> Option<Builder> {
        unimplemented!()
    }
}

pub struct Forwarder {
    pub receiver: tokio::sync::mpsc::Receiver<ForwarderMsg>,
    pub resolver: PathResolver,
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
        doorman_reply_to: oneshot::Sender<Result<Response<Full<Bytes>>, hyper::Error>>,
    ) -> Result<Response<Full<Bytes>>, hyper::Error> {
        match self.resolver.resolve(req.uri().path(), req.method()) {
            Some(builder) => {
                let client = builder.build();
                let response = client.request(req).await?;
                doorman_reply_to.send(Ok(response)).unwrap();
                Ok(response)
            }
            None => {
                let response = Response::builder()
                    .status(404)
                    .body(Full::new(Bytes::from("Not Found")))
                    .unwrap();
                doorman_reply_to.send(Ok(response.clone())).unwrap();
                Ok(response)
            }
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
