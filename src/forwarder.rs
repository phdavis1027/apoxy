use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use http_body_util::Full;
use hyper::{
    body::{Bytes, Incoming},
    Method, Request, Response,
};
use tokio::sync::oneshot;

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

    pub async fn resolve(&self, path: &str, method: Method) -> Result<SocketAddr, anyhow::Error> {
        Ok(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            8080,
        ))
    }
}

pub struct Forwarder {
    pub receiver: tokio::sync::mpsc::Receiver<ForwarderMsg>,
    pub resolver: PathResolver,
}

impl Forwarder {
    async fn handle_msg(&self, msg: ForwarderMsg) {
        match msg {
            ForwarderMsg::IncomingConnection(req, doorman_reply_to) => {}
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
