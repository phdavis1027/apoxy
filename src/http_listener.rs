use std::convert::Infallible;

use anyhow::anyhow;
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Empty, Full};
use hyper::{
    body::{Bytes, Incoming},
    service::service_fn,
    Request, Response, StatusCode,
};
use hyper_util::rt::TokioIo;
use tokio::sync::{mpsc, oneshot};

use crate::{
    error::compat::CompatibilityHyperError,
    forwarder::{ForwarderHandle, ForwarderMsg, IncomingRequest, OutgoingResponse},
    local_executor::LocalExecutor,
};

struct HttpDoorman {
    listener: tokio::net::TcpListener,
    http_connection_servicer: hyper_util::server::conn::auto::Builder<LocalExecutor>,
    receiver: mpsc::Receiver<HttpDoormanMsg>,
    forwarder_handle: ForwarderHandle,
}

impl HttpDoorman {
    fn new(
        listener: tokio::net::TcpListener,
        http_connection_servicer: hyper_util::server::conn::auto::Builder<LocalExecutor>,
        receiver: mpsc::Receiver<HttpDoormanMsg>,
        forwarder_handle: ForwarderHandle,
    ) -> Self {
        Self {
            listener,
            http_connection_servicer,
            receiver,
            forwarder_handle,
        }
    }
}

enum HttpDoormanMsg {}

async fn serve_connection(
    req: IncomingRequest,
    forwarder: ForwarderHandle,
) -> Result<OutgoingResponse, CompatibilityHyperError> {
    let (tx, rx) = oneshot::channel();
    forwarder
        .sender
        .send(ForwarderMsg::IncomingConnection(req, tx))
        .await;

    match rx.await {
        Ok(response) => Ok(response), // This cringe is needed to convince the compiler that the error type is correct
        Err(e) => {
            let response = Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Empty::new().boxed_unsync());

            Ok(response)
        }
    }
}

async fn run_http_listener(doorman: HttpDoorman) {
    while let Ok((stream, addr)) = doorman.listener.accept().await {
        let io = TokioIo::new(stream);
        let forwarder = doorman.forwarder_handle.clone();

        doorman
            .http_connection_servicer
            .serve_connection_with_upgrades(
                io,
                service_fn(move |req| serve_connection(req, forwarder.clone())),
            )
            .await;
    }
}

pub struct HttpDoormanHandle {
    sender: mpsc::Sender<HttpDoormanMsg>,
}

impl HttpDoormanHandle {
    pub fn new(
        listener: tokio::net::TcpListener,
        forwarder_handle: ForwarderHandle,
        http_connection_servicer: hyper_util::server::conn::auto::Builder<LocalExecutor>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(100);
        let actor = HttpDoorman::new(
            listener,
            http_connection_servicer,
            receiver,
            forwarder_handle,
        );
        tokio::spawn(run_http_listener(actor));

        Self { sender }
    }

    // Desn't receive messages so doesn't need a public send interface
}
