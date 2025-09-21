use mosaic_core::*;
use mosaic_net::*;
use quinn::WriteError;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client_secret_key = SecretKey::generate();
    println!("Client public key: {}", client_secret_key.public());

    let server_public_key =
        PublicKey::from_printable("mopub03ctpjer5jfkd49rxe4767hk9ij6f8sdtryjnnru1bpwxhcykk54o")?;

    let server_addr =
        std::env::var("MOSAIC_SERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:8081".to_owned());
    let server_socket: SocketAddr = server_addr.parse()?;

    let client_config = ClientConfig::new(
        server_public_key,
        server_socket,
        Some(client_secret_key.clone()),
    )?;

    let (client, mut channel) = connect_and_send_hello(&client_config).await?;

    let hello_ack = channel
        .recv()
        .await?
        .expect("server should reply with HELLO ACK");
    if hello_ack.message_type() != MessageType::HelloAck
        || hello_ack.result_code() != Some(ResultCode::Success)
    {
        eprintln!("Unexpected HELLO ACK response: {hello_ack:?}");
        client.close(0, b"handshake failure").await;
        return Ok(());
    }

    channel.finish()?;
    let mut channel = client.new_channel().await?;

    let record = {
        let payload = b"hello";
        let tags = OwnedTagSet::new();
        let parts = RecordParts {
            signing_data: RecordSigningData::SecretKey(client_secret_key.clone()),
            address_data: RecordAddressData::Random(client_secret_key.public(), Kind::CHAT_MESSAGE),
            timestamp: Timestamp::now()?,
            flags: RecordFlags::empty(),
            tag_set: &tags,
            payload: payload.as_slice(),
        };
        OwnedRecord::new(&parts)?
    };

    let message = Message::new_submission(&record)?;

    channel.send(message).await?;

    let submission_result = channel
        .recv()
        .await?
        .expect("server should acknowledge submission");
    match submission_result.message_type() {
        MessageType::SubmissionResult => {
            eprintln!(
                "Submission result: {:?}",
                submission_result.result_code().unwrap()
            );
        }
        mt => {
            eprintln!("Unexpected response: {mt:?}={submission_result:?}");
        }
    }

    channel.finish()?;

    let mut channel = client.new_channel().await?;
    let query_id = QueryId::from_bytes([0, 7]);
    let reference = record.id().to_reference();
    let get_message = Message::new_get(query_id, &[&reference])?;
    channel.send(get_message).await?;

    let record_msg = channel.recv().await?.expect("server should stream record");
    if record_msg.message_type() == MessageType::Record {
        let fetched = record_msg.record().unwrap();
        println!("Fetched record id: {}", fetched.id());
    } else {
        eprintln!("Unexpected response to GET: {record_msg:?}");
    }

    let query_closed = channel.recv().await?.expect("server should close query");
    println!(
        "Query closed with result: {:?}",
        query_closed.result_code().unwrap_or(ResultCode::Invalid)
    );

    client.close(0, b"client is done").await;

    Ok(())
}

async fn connect_and_send_hello(
    config: &ClientConfig,
) -> Result<(Client, Channel), Box<dyn std::error::Error>> {
    let hello = Message::new_hello(0, &[0])?;

    for attempt in 0..3 {
        let client = config.client(None).await?;
        let mut channel = client.new_channel().await?;

        match channel.send(hello.clone()).await {
            Ok(_) => return Ok((client, channel)),
            Err(err) if is_stream_stopped(&err) && attempt < 2 => {
                client.close(0, b"retry").await;
                continue;
            }
            Err(err) => {
                client.close(0, b"failure").await;
                return Err(Box::new(err));
            }
        }
    }

    Err(Box::new(std::io::Error::new(
        std::io::ErrorKind::Other,
        "failed to open channel for HELLO",
    )))
}

fn is_stream_stopped(err: &mosaic_net::Error) -> bool {
    if let mosaic_net::InnerError::QuicWrite(write_error) = &err.inner {
        matches!(**write_error, WriteError::Stopped(_))
    } else {
        false
    }
}
