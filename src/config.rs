use std::net::SocketAddr;

use mosaic_core::{PublicKey, SecretKey};
use mosaic_net::Approver;

use crate::Error;

/// A trait for logging errors
pub trait Logger: Send + Sync {
    /// Log a client error
    fn log_client_error(&self, e: Error, socket_addr: SocketAddr, pubkey: Option<PublicKey>);
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
