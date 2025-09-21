//! A Mosaic Server library
//!
//! NOTE: You must use Tokio as the async runtime in your `main()`

mod client;
pub use client::ClientData;

mod config;
pub use config::{Logger, ServerConfig};

mod error;
pub use error::{Error, InnerError};

mod handler;
use handler::handle_mosaic_message;

use std::sync::Arc;

// use dashmap::DashMap;
use tokio::sync::SetOnce;

use mosaic_core::Message;
use mosaic_net::Server as QuicServer;
use mosaic_net::ServerConfig as QuicServerConfig;
use mosaic_net::{Approver, IncomingClient};

/// A Mosaic server
pub struct Server<A: Approver, L: Logger> {
    quic_server: Arc<mosaic_net::Server>,

    approver: Arc<A>,

    logger: Arc<L>,

    // Connected clients
    // client_map: Arc<DashMap<SocketAddr, ClientData>>,

    // Set when shutdown starts. Stores the exit value.
    shutting_down: Arc<SetOnce<u32>>,

    // Set when shutdown completes.
    shutdown_complete: Arc<SetOnce<()>>,
}

impl<A: Approver + 'static, L: Logger + 'static> Server<A, L> {
    /// Create a new Mosaic server
    pub fn new(config: ServerConfig<A, L>) -> Result<Arc<Server<A, L>>, Error> {
        let ServerConfig {
            secret_key,
            socket_addr,
            approver,
            logger,
        } = config;

        let quic_server = {
            let quic_server_config = QuicServerConfig::new(secret_key, socket_addr)?;
            QuicServer::new(quic_server_config)?
        };

        Ok(Arc::new(Server {
            quic_server: Arc::new(quic_server),
            approver: Arc::new(approver),
            logger: Arc::new(logger),
            // client_map: Arc::new(DashMap::new()),
            shutting_down: Arc::new(SetOnce::new()),
            shutdown_complete: Arc::new(SetOnce::new()),
        }))
    }

    /// Run the Mosaic server
    pub async fn run(&self) -> Result<(), Error> {
        // TBD: Start WebSocket Server
        // TBD: Start TCP Server

        loop {
            tokio::select! {
                v = self.quic_server.accept() => {
                    match v {
                        Ok(quic_client) => {
                            let approver2 = self.approver.clone();
                            let logger2 = self.logger.clone();
                            // let client_map2 = self.client_map.clone();
                            tokio::spawn(async move {
                                handle_quic_client(quic_client, approver2, logger2).await;
                            });
                        },
                        Err(e) => {
                            eprintln!("{e}");
                            continue;
                        }
                    }
                },
                v = self.shutting_down.wait() => {
                    self.quic_server.shut_down(*v, b"Shutting down").await;
                    let _ = self.shutdown_complete.set(());
                    break;
                }
            }
        }

        Ok(())
    }

    /// True if the server is shutting down
    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.initialized()
    }

    /// Trigger a shut down of the server
    pub fn trigger_shut_down(&self, exit_code: u32) {
        let _ = self.shutting_down.set(exit_code);
    }

    /// Wait for shut down
    pub async fn wait_for_shut_down(&self) {
        self.shutdown_complete.wait().await;
    }
}

async fn handle_quic_client<A: Approver, L: Logger>(
    client: IncomingClient,
    approver: Arc<A>,
    logger: Arc<L>,
    // client_map: Arc<DashMap<SocketAddr, ClientData>>,
) {
    let remote_address = client.inner().remote_address();

    let connection: mosaic_net::ClientConnection = match client.accept(&*approver).await {
        Ok(c) => c,
        Err(e) => {
            logger.log_client_error(e.into(), remote_address, None);
            return;
        }
    };

    let peer = connection.peer();

    let mut client_data = ClientData {
        remote_address,
        peer,
        mosaic_version: None,
        applications: None,
        closing_result: None,
    };

    const NO_CHANNEL: &[u8] = b"No QUIC channel";

    let close_reason;

    loop {
        // Get the next channel from the client
        let mut channel = match connection.next_channel().await {
            Ok(c) => c,
            Err(e) => {
                logger.log_client_error(e.into(), remote_address, peer);
                return;
            }
        };

        // Get the next message from the channel
        match channel.recv().await {
            Ok(None) => {
                close_reason = NO_CHANNEL;
                break;
            }
            Ok(Some(message)) => match handle_mosaic_message(message, &mut client_data).await {
                Ok(Some(response_message)) => {
                    if let Err(e) = channel.send(response_message).await {
                        logger.log_client_error(e.into(), remote_address, peer);
                        return;
                    }
                    if let Some(result_code) = client_data.closing_result.take() {
                        let closing = Message::new_closing(result_code);
                        if let Err(e) = channel.send(closing).await {
                            logger.log_client_error(e.into(), remote_address, peer);
                        }
                        connection.close(result_code.to_u8().into(), b"closing");
                        return;
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    logger.log_client_error(e, remote_address, peer);
                    return;
                }
            },
            Err(e) => {
                logger.log_client_error(e.into(), remote_address, peer);
                return;
            }
        }
    }

    // TBD
    connection.close(0, close_reason);
}
