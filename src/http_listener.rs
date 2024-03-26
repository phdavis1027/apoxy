use std::{convert::Infallible, net::SocketAddr};

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
    error::{compat::CompatibilityHyperError, ApoxyError},
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

enum HttpDoormanMsg {
    ForwarderFellDown,
}

async fn serve_connection(
    req: IncomingRequest,
    forwarder: ForwarderHandle,
    doorman_sender: mpsc::Sender<HttpDoormanMsg>,
) -> Result<OutgoingResponse, CompatibilityHyperError> {
    let (tx, rx) = oneshot::channel();
    forwarder
        .sender
        .send(ForwarderMsg::IncomingConnection(req, tx))
        .await;

    match rx.await {
        Ok(response) => Ok(response), // This cringe is needed to convince the compiler that the error type is correct
        Err(_) => {
            let response = Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(
                    Empty::<Bytes>::new()
                        .map_err(CompatibilityHyperError::from)
                        .boxed_unsync(),
                )
                .unwrap(); // UNWRAP: This conversion is infallible
            doorman_sender.blocking_send(HttpDoormanMsg::ForwarderFellDown);

            Ok(response)
        }
    }
}

fn handle_doorman_msg(doorman: &mut HttpDoorman, msg: Option<HttpDoormanMsg>) {
    match msg {
        Some(HttpDoormanMsg::ForwarderFellDown) => {
            // Our forwarder somehow died, so let's stand a new one up.
            doorman.forwarder_handle = ForwarderHandle::new();
        }

        None => {
            // Something has gone horribly wrong, somehow our recevier has been closed
        }
    }
}

async fn handle_incoming_connection(
    doorman: &mut HttpDoorman,
    conn_attempt: Result<(tokio::net::TcpStream, SocketAddr), std::io::Error>,
    loopback_sender: mpsc::Sender<HttpDoormanMsg>,
) {
    let forwarder = doorman.forwarder_handle.clone();

    match conn_attempt {
        Ok((stream, addr)) => {
            doorman
                .http_connection_servicer
                .serve_connection_with_upgrades(
                    TokioIo::new(stream),
                    service_fn(move |req| {
                        serve_connection(req, forwarder.clone(), loopback_sender.clone())
                    }),
                )
                .await;
        }

        Err(e) => {
            // Log the error
        }
    }
}

async fn run_http_listener(
    mut doorman: HttpDoorman,
    loopback_sender: mpsc::Sender<HttpDoormanMsg>,
) {
    loop {
        tokio::select! {
            biased; // We want need to stand up a new forwarder if the old one dies.
                    // otherwise, sending more messages to them would be useless.

            msg = doorman.receiver.recv() => {
                handle_doorman_msg(&mut doorman, msg);
            }

            conn_attempt = doorman.listener.accept() => {
                handle_incoming_connection(&mut doorman, conn_attempt, loopback_sender.clone()).await;
            }
        };
    }
    /*
    while let Ok((stream, addr)) = doorman.listener.accept().await {
        let forwarder = doorman.forwarder_handle.clone();

    }
    */
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
            sender.clone(),
        );
        tokio::spawn(run_http_listener(actor));

        Self { sender }
    }

    // Desn't receive messages so doesn't need a public send interface
}
