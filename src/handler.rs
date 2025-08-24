use std::net::SocketAddr;

use mosaic_core::{Message, PublicKey};

use crate::Error;

pub(crate) async fn handle_mosaic_message(
    _message: Message,
    _pubkey: Option<PublicKey>,
    _remote: Option<SocketAddr>,
) -> Result<Option<Message>, Error> {
    Ok(Some(Message::new_unrecognized()))
}
