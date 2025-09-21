use std::fmt;
use std::net::SocketAddr;

use mosaic_core::{PublicKey, SecretKey};
use mosaic_net::Approver;
use std::sync::Arc;

use crate::{Error, Store};

/// A trait for logging errors
pub trait Logger: Send + Sync {
    /// Log a client error
    fn log_client_error(&self, e: Error, socket_addr: SocketAddr, pubkey: Option<PublicKey>);
}

/// A configuration for creating a Mosaic `Server`
#[derive(Clone)]
pub struct ServerConfig<A: Approver, L: Logger> {
    /// Mosaic ed25519 secret key for the server
    pub secret_key: SecretKey,

    /// Socket address to bind to
    pub socket_addr: SocketAddr,

    /// IP Address approval
    pub approver: A,

    /// Logging of errors
    pub logger: L,

    /// Storage backend used for submissions
    pub store: Arc<dyn Store>,
    //pub listen_over_quic: bool,
    //pub listen_over_tcp: bool,
    //pub listen_over_websockets: bool,
}

impl<A: Approver, L: Logger> fmt::Debug for ServerConfig<A, L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerConfig")
            .field("socket_addr", &self.socket_addr)
            .field("public_key", &self.secret_key.public())
            .field("approver", &"<approver>")
            .field("logger", &"<logger>")
            .field("store", &"<store>")
            .finish()
    }
}
