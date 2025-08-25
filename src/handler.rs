use mosaic_core::{HelloErrorCode, Message, MessageType};

use crate::Error;
use crate::client::ClientData;

pub(crate) async fn handle_mosaic_message(
    message: Message,
    client_data: &mut ClientData,
) -> Result<Option<Message>, Error> {

    match message.message_type() {
        MessageType::Hello => {
            // We can ignore their major version, the best we can support is 0
            let version: u16 = 0;
            client_data.mosaic_version = Some(version);

            // These are the apps they are requesting. We can only support 0
            // (which maybe doesn't need to be specified, but if they did, we do).
            let req_app_ids = message.application_ids().unwrap();
            let apps: Vec<u32> = if req_app_ids.contains(&0) {
                vec![0]
            } else {
                vec![]
            };
            client_data.applications = Some(apps.clone());

            let code = if client_data.mosaic_version.is_some() {
                HelloErrorCode::UnexpectedHello
            } else {
                HelloErrorCode::NoError
            };

            Ok(Some(Message::new_hello_ack(
                code,
                version,
                &apps,
            )?))
        },
        MessageType::Get => {
            todo!()
        },
        MessageType::Query => {
            todo!()
        },
        MessageType::Subscribe => {
            todo!()
        },
        MessageType::Unsubscribe => {
            todo!()
        },
        MessageType::Submission => {
            todo!()
        },
        MessageType::Unrecognized => {
            Ok(Some(Message::new_unrecognized()))
        }
        _ => {
            Ok(Some(Message::new_unrecognized()))
        }
    }

}
