//! A Mosaic Server library
//!
//! NOTE: You must use Tokio as the async runtime in your `main()`

mod error;
pub use error::{Error, InnerError};

use std::net::SocketAddr;
use std::sync::Arc;

use mosaic_core::{Message, PublicKey, SecretKey};
use mosaic_net::Server as QuicServer;
use mosaic_net::ServerConfig as QuicServerConfig;
use mosaic_net::{Approver, IncomingClient};

use tokio::sync::SetOnce;

/// A trait for logging errors
pub trait Logger: Send + Sync {
    /// Log a client error
    fn log_client_error(&self, e: Error);
}

/// A configuration for creating a Mosaic `Server`
#[derive(Debug, Clone)]
pub struct ServerConfig<A: Approver, L: Logger> {
    /// Mosaic ed25519 secret key for the server
    pub secret_key: SecretKey,

    /// Socket address to bind to
    pub socket_addr: SocketAddr,

    /// IP Address approval
    pub approver: A,

    /// Logging of errors
    pub logger: L,
    //pub listen_over_quic: bool,
    //pub listen_over_tcp: bool,
    //pub listen_over_websockets: bool,
}

/// A Mosaic server
pub struct Server<A: Approver, L: Logger> {
    quic_server: Arc<mosaic_net::Server>,

    approver: Arc<A>,

    logger: Arc<L>,

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
                            tokio::spawn(async move {
                                if let Err(e) = handle_quic_client(quic_client, approver2).await {
                                    logger2.log_client_error(e);
                                }
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

async fn handle_quic_client<A: Approver>(client: IncomingClient, approver: Arc<A>) -> Result<(), Error> {
    // Accept the connection if approved
    let connection: mosaic_net::ClientConnection = client.accept(&*approver).await?;

    const NO_CHANNEL: &[u8] = b"No QUIC channel";

    let close_reason;

    loop {
        // Get the next channel from the client
        let mut channel = connection.next_channel().await?;

        // Get the next message from the channel
        match channel.recv().await? {
            None => {
                close_reason = NO_CHANNEL;
                break;
            },
            Some(message) => {
                if let Some(response_message) = handle_mosaic_message(
                    message,
                    connection.peer(),
                    Some(connection.remote_socket_addr()),
                ).await? {
                    channel.send(response_message).await?;
                }
            }
        }
    }

    // TBD
    connection.close(0, close_reason);

    Ok(())
}

async fn handle_mosaic_message(
    _message: Message,
    _pubkey: Option<PublicKey>,
    _remote: Option<SocketAddr>,
) -> Result<Option<Message>, Error> {
    Ok(Some(Message::new_unrecognized()))
}
