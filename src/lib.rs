//! A Mosaic Server library
//!
//! NOTE: You must use Tokio as the async runtime in your `main()`

mod error;
pub use error::{Error, InnerError};

use std::net::SocketAddr;
use std::sync::Arc;

use mosaic_core::SecretKey;
use mosaic_net::Server as QuicServer;
use mosaic_net::ServerConfig as QuicServerConfig;
use mosaic_net::{Approver, IncomingClient};

use tokio::sync::SetOnce;

/// A configuration for creating a Mosaic `Server`
#[derive(Debug, Clone)]
pub struct ServerConfig<A: Approver> {
    /// Mosaic ed25519 secret key for the server
    pub secret_key: SecretKey,

    /// Socket address to bind to
    pub socket_addr: SocketAddr,

    /// IP Address approval
    pub approver: A,
    //pub listen_over_quic: bool,
    //pub listen_over_tcp: bool,
    //pub listen_over_websockets: bool,
}

/// A Mosaic server
pub struct Server<A: Approver> {
    quic_server: Arc<mosaic_net::Server>,

    approver: Arc<A>,

    // Set when shutdown starts. Stores the exit value.
    shutting_down: Arc<SetOnce<u32>>,

    // Set when shutdown completes.
    shutdown_complete: Arc<SetOnce<()>>,
}

impl<A: Approver + 'static> Server<A> {
    /// Create a new Mosaic server
    pub fn new(config: ServerConfig<A>) -> Result<Arc<Server<A>>, Error> {
        let quic_server = {
            let quic_server_config = QuicServerConfig::new(config.secret_key, config.socket_addr)?;
            QuicServer::new(quic_server_config)?
        };

        Ok(Arc::new(Server {
            quic_server: Arc::new(quic_server),
            approver: Arc::new(config.approver),
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
                        Ok(incoming_client) => {
                            let approver2 = self.approver.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_incoming_client(incoming_client, approver2).await {
                                    eprintln!("{e}");
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

async fn handle_incoming_client<A: Approver>(
    incoming_client: IncomingClient,
    approver: Arc<A>,
) -> Result<(), Error> {
    let connection = incoming_client.accept(&*approver).await?;

    // TBD
    connection.close(0, b"Not Implemented");

    Ok(())
}
