use mosaic_core::{Message, MessageType, ResultCode};

use crate::Error;
use crate::client::ClientData;

const SUPPORTED_MAJOR_VERSION: u8 = 0;

pub(crate) async fn handle_mosaic_message(
    message: Message,
    client_data: &mut ClientData,
) -> Result<Option<Message>, Error> {
    match message.message_type() {
        MessageType::Hello => handle_hello(message, client_data),
        MessageType::Get => {
            todo!()
        }
        MessageType::Query => {
            todo!()
        }
        MessageType::Subscribe => {
            todo!()
        }
        MessageType::Unsubscribe => {
            todo!()
        }
        MessageType::Submission => {
            todo!()
        }
        MessageType::Unrecognized => Ok(Some(Message::new_unrecognized())),
        _ => Ok(Some(Message::new_unrecognized())),
    }
}

fn handle_hello(message: Message, client_data: &mut ClientData) -> Result<Option<Message>, Error> {
    if client_data.mosaic_version.is_some() {
        // Spec does not mandate a response to redundant HELLO; ignore to keep behavior minimal.
        return Ok(None);
    }

    let client_version = match message.mosaic_major_version() {
        Some(v) => v,
        None => {
            client_data.closing_result = Some(ResultCode::Invalid);
            let ack = Message::new_hello_ack(ResultCode::Invalid, SUPPORTED_MAJOR_VERSION, &[])?;
            return Ok(Some(ack));
        }
    };

    if client_version != SUPPORTED_MAJOR_VERSION {
        client_data.closing_result = Some(ResultCode::Invalid);
        let ack = Message::new_hello_ack(ResultCode::Invalid, SUPPORTED_MAJOR_VERSION, &[])?;
        return Ok(Some(ack));
    }

    let frame_len = message.len();
    if frame_len < 8 || ((frame_len - 8) % 4) != 0 {
        client_data.closing_result = Some(ResultCode::Invalid);
        let ack = Message::new_hello_ack(ResultCode::Invalid, SUPPORTED_MAJOR_VERSION, &[])?;
        return Ok(Some(ack));
    }

    let requested_apps = match message.application_ids() {
        Some(apps) => apps,
        None => {
            client_data.closing_result = Some(ResultCode::Invalid);
            let ack = Message::new_hello_ack(ResultCode::Invalid, SUPPORTED_MAJOR_VERSION, &[])?;
            return Ok(Some(ack));
        }
    };

    let mut accepted_apps: Vec<u32> = requested_apps.into_iter().filter(|app| *app == 0).collect();

    if accepted_apps.is_empty() {
        accepted_apps.shrink_to_fit();
    }

    let ack = Message::new_hello_ack(ResultCode::Success, SUPPORTED_MAJOR_VERSION, &accepted_apps)?;

    client_data.mosaic_version = Some(u16::from(SUPPORTED_MAJOR_VERSION));
    client_data.applications = Some(accepted_apps);
    client_data.closing_result = None;

    Ok(Some(ack))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_client() -> ClientData {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};

        ClientData {
            remote_address: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            peer: None,
            mosaic_version: None,
            applications: None,
            closing_result: None,
        }
    }

    #[tokio::test]
    async fn hello_success() {
        let mut client = make_client();
        let hello = Message::new_hello(SUPPORTED_MAJOR_VERSION, &[0]).unwrap();

        let response = handle_mosaic_message(hello, &mut client)
            .await
            .unwrap()
            .expect("response");

        assert_eq!(response.message_type(), MessageType::HelloAck);
        assert_eq!(response.result_code(), Some(ResultCode::Success));
        assert_eq!(
            client.mosaic_version,
            Some(u16::from(SUPPORTED_MAJOR_VERSION))
        );
        assert_eq!(client.applications, Some(vec![0]));
        assert!(client.closing_result.is_none());
    }

    #[tokio::test]
    async fn hello_repeated_returns_unexpected() {
        let mut client = make_client();
        let hello = Message::new_hello(SUPPORTED_MAJOR_VERSION, &[0]).unwrap();

        let _ = handle_mosaic_message(hello.clone(), &mut client)
            .await
            .unwrap();

        let response = handle_mosaic_message(hello, &mut client).await.unwrap();

        assert!(response.is_none());
        assert!(client.closing_result.is_none());
    }

    #[tokio::test]
    async fn hello_incompatible_version_requests_close() {
        let mut client = make_client();
        let hello = Message::new_hello(SUPPORTED_MAJOR_VERSION + 1, &[0]).unwrap();

        let response = handle_mosaic_message(hello, &mut client)
            .await
            .unwrap()
            .expect("response");

        assert_eq!(response.message_type(), MessageType::HelloAck);
        assert_eq!(response.result_code(), Some(ResultCode::Invalid));
        assert_eq!(client.closing_result, Some(ResultCode::Invalid));
        assert_eq!(client.mosaic_version, None);
    }

    #[tokio::test]
    async fn hello_with_malformed_length_requests_close() {
        let mut client = make_client();
        let hello = Message::new_hello(SUPPORTED_MAJOR_VERSION, &[0]).unwrap();
        let mut bytes = hello.as_bytes().to_vec();
        // Corrupt the advertised length (bytes 4..=7) so it is no longer a multiple of 4 bytes.
        bytes[4] = 9;
        let malformed = unsafe { Message::from_bytes_unchecked(bytes) };

        let response = handle_mosaic_message(malformed, &mut client)
            .await
            .unwrap()
            .expect("response");

        assert_eq!(response.message_type(), MessageType::HelloAck);
        assert_eq!(response.result_code(), Some(ResultCode::Invalid));
        assert_eq!(client.closing_result, Some(ResultCode::Invalid));
        assert_eq!(client.mosaic_version, None);
        assert!(client.applications.is_none());
    }

    #[tokio::test]
    async fn hello_without_app_zero_acknowledges_none() {
        let mut client = make_client();
        let hello = Message::new_hello(SUPPORTED_MAJOR_VERSION, &[]).unwrap();

        let response = handle_mosaic_message(hello, &mut client)
            .await
            .unwrap()
            .expect("response");

        assert_eq!(response.message_type(), MessageType::HelloAck);
        assert_eq!(response.result_code(), Some(ResultCode::Success));
        assert_eq!(client.applications, Some(Vec::new()));
        assert!(client.closing_result.is_none());
    }
}
