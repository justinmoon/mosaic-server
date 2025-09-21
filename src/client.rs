use std::net::SocketAddr;

use mosaic_core::{PublicKey, ResultCode};

pub struct ClientData {
    pub remote_address: SocketAddr,
    pub peer: Option<PublicKey>,
    pub mosaic_version: Option<u16>,
    pub applications: Option<Vec<u32>>,
    pub closing_result: Option<ResultCode>,
}
