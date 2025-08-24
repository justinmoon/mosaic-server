//! A Mosaic Server library
//!
//! NOTE: You must use Tokio as the async runtime in your `main()`

mod error;
pub use error::{Error, InnerError};

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mosaic_core::SecretKey;
use mosaic_net::Server as QuicServer;
use mosaic_net::ServerConfig as QuicServerConfig;
use mosaic_net::{Approver, IncomingClient};

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
    shutting_down: AtomicBool,
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
            shutting_down: AtomicBool::new(false),
        }))
    }

    /// Run the Mosaic server
    pub async fn run(&self) -> Result<(), Error> {
        let quic_server = self.quic_server.clone();
        let approver = self.approver.clone();

        let quic_task = tokio::spawn(async move {
            loop {
                let incoming_client: IncomingClient = match quic_server.accept().await {
                    Ok(ic) => ic,
                    Err(e) => {
                        eprintln!("{e}");
                        continue;
                    }
                };

                let approver2 = approver.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_incoming_client(incoming_client, approver2).await {
                        eprintln!("{e}");
                    }
                });

                if self.shutting_down.load(Ordering::Relaxed) {
                    break;
                }
            }

            quic_server.close(0, b"Shutting Down");
        });

        // TBD: Start WebSocket Server

        // TBD: Start TCP Server

        // Wait for all servers to complete
        quic_task.await?;

        Ok(())
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
