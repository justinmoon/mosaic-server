use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mosaic_core::{
    EMPTY_TAG_SET, Kind, Message, MessageType, OwnedRecord, QueryId, RecordAddressData,
    RecordParts, RecordSigningData, ResultCode, SecretKey, Timestamp,
};
use mosaic_net::{
    AlwaysAllowedApprover, Channel, ClientConfig as QuicClientConfig, Error as NetError,
    InnerError as NetInnerError,
};
use mosaic_server::{LmdbStore, Logger, Server, ServerConfig, Store};
use quinn::WriteError;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Default)]
struct CaptureLogger {
    entries: Arc<Mutex<Vec<String>>>,
}

impl CaptureLogger {
    fn entries(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.entries)
    }
}

impl Logger for CaptureLogger {
    fn log_client_error(
        &self,
        e: mosaic_server::Error,
        socket_addr: SocketAddr,
        pubkey: Option<mosaic_core::PublicKey>,
    ) {
        let entry = format!("{socket_addr}: {:?} => {e}", pubkey);
        let mut guard = self.entries.lock().unwrap();
        guard.push(entry);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publish_smoke_quic_lmdb() -> Result<(), Box<dyn std::error::Error>> {
    let server_secret = SecretKey::generate();

    let udp = UdpSocket::bind("127.0.0.1:0")?;
    let server_addr = udp.local_addr()?;
    drop(udp);

    let temp_dir = tempfile::tempdir()?;
    let store: Arc<dyn Store> = Arc::new(LmdbStore::open(temp_dir.path(), 1)?);

    let logger = CaptureLogger::default();
    let log_handle = logger.entries();

    let server_config = ServerConfig {
        secret_key: server_secret.clone(),
        socket_addr: server_addr,
        approver: AlwaysAllowedApprover,
        logger: logger.clone(),
        store: Arc::clone(&store),
    };

    let server = Server::new(server_config)?;
    let server_task = {
        let server = Arc::clone(&server);
        tokio::spawn(async move {
            let _ = server.run().await;
        })
    };

    tokio::time::sleep(Duration::from_millis(100)).await;

    let client_secret = SecretKey::generate();
    let client_config = QuicClientConfig::new(
        server_secret.public(),
        server_addr,
        Some(client_secret.clone()),
    )?;

    let (client, mut channel) = match connect_and_send_hello(&client_config).await {
        Ok(result) => result,
        Err(e) => {
            let logs = log_handle.lock().unwrap().clone();
            panic!("failed to connect and send HELLO: {e:?}; logs={logs:?}");
        }
    };

    let hello_ack = timeout(TEST_TIMEOUT, channel.recv())
        .await??
        .expect("server should reply to HELLO");

    assert_eq!(hello_ack.message_type(), MessageType::HelloAck);
    assert_eq!(hello_ack.result_code(), Some(ResultCode::Success));
    assert_eq!(hello_ack.mosaic_major_version(), Some(0));
    assert_eq!(hello_ack.application_ids(), Some(vec![0]));

    let _ = channel.finish();
    let mut channel = timeout(TEST_TIMEOUT, client.new_channel()).await??;

    let record = build_record();

    match timeout(
        TEST_TIMEOUT,
        channel.send(Message::new_submission(&record)?),
    )
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            let logs = log_handle.lock().unwrap().clone();
            panic!("submission send failed: {e:?}; logs={logs:?}");
        }
        Err(e) => {
            let logs = log_handle.lock().unwrap().clone();
            panic!("submission send timeout: {e:?}; logs={logs:?}");
        }
    }

    let submission_result = timeout(TEST_TIMEOUT, channel.recv())
        .await??
        .expect("server should acknowledge submission");

    assert_eq!(
        submission_result.message_type(),
        MessageType::SubmissionResult
    );
    assert_eq!(submission_result.result_code(), Some(ResultCode::Accepted));
    assert_eq!(
        submission_result.id_prefix().unwrap(),
        &record.id().as_bytes()[..32]
    );

    let reference = record.id().to_reference();
    assert!(store.has_record(&reference)?);

    let mut channel = timeout(TEST_TIMEOUT, client.new_channel()).await??;
    let query_id = QueryId::from_bytes([0, 42]);
    let get_message = Message::new_get(query_id, &[&reference])?;

    match timeout(TEST_TIMEOUT, channel.send(get_message)).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            let logs = log_handle.lock().unwrap().clone();
            panic!("get send failed: {e:?}; logs={logs:?}");
        }
        Err(e) => {
            let logs = log_handle.lock().unwrap().clone();
            panic!("get send timeout: {e:?}; logs={logs:?}");
        }
    }

    let record_msg = timeout(TEST_TIMEOUT, channel.recv())
        .await??
        .expect("server should stream record");
    assert_eq!(record_msg.message_type(), MessageType::Record);
    assert_eq!(record_msg.query_id(), Some(query_id));
    let fetched = record_msg.record().unwrap();
    assert_eq!(fetched.as_bytes(), record.as_bytes());

    let closed_msg = timeout(TEST_TIMEOUT, channel.recv())
        .await??
        .expect("server should close query");
    assert_eq!(closed_msg.message_type(), MessageType::QueryClosed);
    assert_eq!(closed_msg.query_id(), Some(query_id));
    assert_eq!(closed_msg.result_code(), Some(ResultCode::Success));

    let _ = channel.finish();
    let mut channel = timeout(TEST_TIMEOUT, client.new_channel()).await??;

    match timeout(
        TEST_TIMEOUT,
        channel.send(Message::new_submission(&record)?),
    )
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            let logs = log_handle.lock().unwrap().clone();
            panic!("duplicate send failed: {e:?}; logs={logs:?}");
        }
        Err(e) => {
            let logs = log_handle.lock().unwrap().clone();
            panic!("duplicate send timeout: {e:?}; logs={logs:?}");
        }
    }

    let duplicate_result = timeout(TEST_TIMEOUT, channel.recv())
        .await??
        .expect("server should reply to duplicate submission");

    assert_eq!(
        duplicate_result.message_type(),
        MessageType::SubmissionResult
    );
    assert_eq!(duplicate_result.result_code(), Some(ResultCode::Duplicate));
    assert_eq!(
        duplicate_result.id_prefix().unwrap(),
        &record.id().as_bytes()[..32]
    );

    let _ = channel.finish();
    client.close(0, b"done").await;

    server.trigger_shut_down(0);
    server.wait_for_shut_down().await;
    let _ = server_task.await;

    let logs = log_handle.lock().unwrap();
    let unexpected: Vec<_> = logs
        .iter()
        .filter(|entry| {
            let stateless = entry.contains("Stateless retry required");
            let graceful_close = entry.contains("closed by peer");
            !(stateless || graceful_close)
        })
        .collect();
    assert!(
        unexpected.is_empty(),
        "unexpected server logs: {unexpected:?}"
    );

    Ok(())
}

fn build_record() -> OwnedRecord {
    let signing_key = SecretKey::generate();
    OwnedRecord::new(&RecordParts {
        signing_data: RecordSigningData::SecretKey(signing_key.clone()),
        address_data: RecordAddressData::Random(signing_key.public(), Kind::KEY_SCHEDULE),
        timestamp: Timestamp::now().unwrap(),
        flags: Default::default(),
        tag_set: &EMPTY_TAG_SET,
        payload: b"publish smoke payload",
    })
    .unwrap()
}

async fn connect_and_send_hello(
    config: &QuicClientConfig,
) -> Result<(mosaic_net::Client, Channel), Box<dyn std::error::Error>> {
    let hello =
        Message::new_hello(0, &[0]).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    for attempt in 0..3 {
        let client = timeout(TEST_TIMEOUT, config.client(None)).await??;
        let mut channel = match timeout(TEST_TIMEOUT, client.new_channel()).await {
            Ok(Ok(ch)) => ch,
            Ok(Err(err)) => {
                client.close(0, b"retry").await;
                return Err(Box::new(err));
            }
            Err(e) => {
                client.close(0, b"retry").await;
                return Err(Box::new(e));
            }
        };

        match timeout(TEST_TIMEOUT, channel.send(hello.clone())).await {
            Ok(Ok(_)) => return Ok((client, channel)),
            Ok(Err(err)) if is_stream_stopped(&err) && attempt < 2 => {
                client.close(0, b"retry").await;
                continue;
            }
            Ok(Err(err)) => {
                client.close(0, b"failure").await;
                return Err(Box::new(err));
            }
            Err(e) => {
                client.close(0, b"timeout").await;
                return Err(Box::new(e));
            }
        }
    }

    Err(Box::new(std::io::Error::new(
        std::io::ErrorKind::Other,
        "failed to send HELLO after retries",
    )))
}

fn is_stream_stopped(err: &NetError) -> bool {
    if let NetInnerError::QuicWrite(write_error) = &err.inner {
        matches!(**write_error, WriteError::Stopped(_))
    } else {
        false
    }
}
