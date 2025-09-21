use std::sync::Arc;

use mosaic_core::{Message, MessageType, OwnedRecord, QueryId, Record, ResultCode};

use crate::{
    Error, InnerError, Logger, PutResult, Store, SubmissionValidationError, client::ClientData,
    validate_submission,
};

const SUPPORTED_MAJOR_VERSION: u8 = 0;

pub(crate) struct GetResponse {
    pub query_id: QueryId,
    pub records: Vec<OwnedRecord>,
    pub result_code: ResultCode,
}

pub(crate) fn handle_get(
    message: &Message,
    client_data: &ClientData,
    store: &Arc<dyn Store>,
) -> Result<GetResponse, Error> {
    let Some(query_id) = message.query_id() else {
        return Err(InnerError::General("GET message missing query id".to_owned()).into_err());
    };

    if client_data.mosaic_version.is_none() || client_data.applications.is_none() {
        return Ok(GetResponse {
            query_id,
            records: Vec::new(),
            result_code: ResultCode::Invalid,
        });
    }

    let Some(references) = message.references() else {
        return Ok(GetResponse {
            query_id,
            records: Vec::new(),
            result_code: ResultCode::Invalid,
        });
    };

    let mut found_records = Vec::with_capacity(references.len());
    for reference in references {
        if let Some(record) = store.get_record(&reference)? {
            found_records.push(record);
        }
    }

    let result_code = if found_records.is_empty() {
        ResultCode::NotFound
    } else {
        ResultCode::Success
    };

    Ok(GetResponse {
        query_id,
        records: found_records,
        result_code,
    })
}

pub(crate) async fn handle_mosaic_message<L: Logger>(
    message: Message,
    client_data: &mut ClientData,
    store: &Arc<dyn Store>,
    logger: &Arc<L>,
) -> Result<Option<Message>, Error> {
    match message.message_type() {
        MessageType::Hello => handle_hello(message, client_data),
        MessageType::Get => Ok(None),
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
            let response = handle_submission(message, client_data, store, logger)?;
            Ok(Some(response))
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

fn handle_submission<L: Logger>(
    message: Message,
    client_data: &mut ClientData,
    store: &Arc<dyn Store>,
    logger: &Arc<L>,
) -> Result<Message, Error> {
    match validate_submission(&message, client_data) {
        Ok(record) => {
            let id = record.id();
            match store.put_record(record.as_ref()) {
                Ok(PutResult::Inserted) => {
                    Ok(Message::new_submission_result(id, ResultCode::Accepted))
                }
                Ok(PutResult::Duplicate) => {
                    Ok(Message::new_submission_result(id, ResultCode::Duplicate))
                }
                Err(store_err) => {
                    logger.log_client_error(
                        store_err,
                        client_data.remote_address,
                        client_data.peer,
                    );
                    Ok(Message::new_submission_result(id, ResultCode::GeneralError))
                }
            }
        }
        Err(validation_err) => {
            log_validation_error(logger, client_data, &validation_err);
            let result_code = validation_err.result_code();

            if let Some(record_id) = extract_record_id(&message) {
                Ok(Message::new_submission_result(record_id, result_code))
            } else {
                logger.log_client_error(
                    InnerError::General("submission contained an unreadable record".to_owned())
                        .into_err(),
                    client_data.remote_address,
                    client_data.peer,
                );
                Ok(Message::new_closing(ResultCode::Invalid))
            }
        }
    }
}

fn log_validation_error<L: Logger>(
    logger: &Arc<L>,
    client_data: &ClientData,
    validation_err: &SubmissionValidationError,
) {
    let message = format!("submission validation failed: {:?}", validation_err);
    logger.log_client_error(
        InnerError::General(message).into_err(),
        client_data.remote_address,
        client_data.peer,
    );
}

fn extract_record_id(message: &Message) -> Option<mosaic_core::Id> {
    if message.message_type() != MessageType::Submission {
        return None;
    }

    Record::from_bytes(&message.as_bytes()[8..])
        .ok()
        .map(|record| record.id())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use crate::{Logger, Store};
    use mosaic_core::{
        EMPTY_TAG_SET, Kind, Message, MessageType, OwnedRecord, QueryId, RecordAddressData,
        RecordParts, RecordSigningData, Reference, ResultCode, SecretKey, Timestamp,
    };

    #[derive(Default)]
    struct InMemoryStore {
        records: Mutex<HashMap<[u8; 48], OwnedRecord>>,
    }

    impl InMemoryStore {
        fn record_count(&self) -> usize {
            self.records.lock().unwrap().len()
        }

        fn contains(&self, id: &[u8; 48]) -> bool {
            self.records.lock().unwrap().contains_key(id)
        }
    }

    impl Store for InMemoryStore {
        fn put_record(&self, record: &mosaic_core::Record) -> Result<PutResult, Error> {
            let mut guard = self.records.lock().unwrap();
            let id_bytes = *record.id().as_bytes();
            if guard.contains_key(&id_bytes) {
                Ok(PutResult::Duplicate)
            } else {
                let owned = OwnedRecord::from_vec(record.as_bytes().to_vec())?;
                guard.insert(id_bytes, owned);
                Ok(PutResult::Inserted)
            }
        }

        fn has_record(&self, reference: &mosaic_core::Reference) -> Result<bool, Error> {
            let target = *reference.as_bytes();
            Ok(self.records.lock().unwrap().contains_key(&target))
        }

        fn get_record(
            &self,
            reference: &mosaic_core::Reference,
        ) -> Result<Option<OwnedRecord>, Error> {
            let target = *reference.as_bytes();
            Ok(self.records.lock().unwrap().get(&target).cloned())
        }
    }

    #[derive(Default)]
    struct TestLogger {
        entries: Mutex<Vec<String>>,
    }

    impl TestLogger {
        fn entries(&self) -> Vec<String> {
            self.entries.lock().unwrap().clone()
        }
    }

    impl Logger for TestLogger {
        fn log_client_error(
            &self,
            e: Error,
            _socket_addr: std::net::SocketAddr,
            _pubkey: Option<mosaic_core::PublicKey>,
        ) {
            self.entries.lock().unwrap().push(e.to_string());
        }
    }

    struct TestEnv {
        store_impl: Arc<InMemoryStore>,
        store: Arc<dyn Store>,
        logger: Arc<TestLogger>,
    }

    impl TestEnv {
        fn new() -> Self {
            let store_impl = Arc::new(InMemoryStore::default());
            let store: Arc<dyn Store> = store_impl.clone();
            let logger = Arc::new(TestLogger::default());
            Self {
                store_impl,
                store,
                logger,
            }
        }
    }

    struct FailingStore;

    impl Store for FailingStore {
        fn put_record(&self, _record: &mosaic_core::Record) -> Result<PutResult, Error> {
            Err(InnerError::General("store failure".to_owned()).into_err())
        }

        fn has_record(&self, _reference: &mosaic_core::Reference) -> Result<bool, Error> {
            Ok(false)
        }

        fn get_record(
            &self,
            _reference: &mosaic_core::Reference,
        ) -> Result<Option<OwnedRecord>, Error> {
            Ok(None)
        }
    }

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

    fn build_record() -> OwnedRecord {
        let signing_key = SecretKey::generate();
        OwnedRecord::new(&RecordParts {
            signing_data: RecordSigningData::SecretKey(signing_key.clone()),
            address_data: RecordAddressData::Random(signing_key.public(), Kind::KEY_SCHEDULE),
            timestamp: Timestamp::now().unwrap(),
            flags: Default::default(),
            tag_set: &EMPTY_TAG_SET,
            payload: b"handler test payload",
        })
        .unwrap()
    }

    #[tokio::test]
    async fn hello_success() {
        let mut client = make_client();
        let hello = Message::new_hello(SUPPORTED_MAJOR_VERSION, &[0]).unwrap();
        let env = TestEnv::new();

        let response = handle_mosaic_message(hello, &mut client, &env.store, &env.logger)
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
        let env = TestEnv::new();

        let _ = handle_mosaic_message(hello.clone(), &mut client, &env.store, &env.logger)
            .await
            .unwrap();

        let response = handle_mosaic_message(hello, &mut client, &env.store, &env.logger)
            .await
            .unwrap();

        assert!(response.is_none());
        assert!(client.closing_result.is_none());
    }

    #[tokio::test]
    async fn hello_incompatible_version_requests_close() {
        let mut client = make_client();
        let hello = Message::new_hello(SUPPORTED_MAJOR_VERSION + 1, &[0]).unwrap();
        let env = TestEnv::new();

        let response = handle_mosaic_message(hello, &mut client, &env.store, &env.logger)
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
        let env = TestEnv::new();

        let response = handle_mosaic_message(malformed, &mut client, &env.store, &env.logger)
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
        let env = TestEnv::new();

        let response = handle_mosaic_message(hello, &mut client, &env.store, &env.logger)
            .await
            .unwrap()
            .expect("response");

        assert_eq!(response.message_type(), MessageType::HelloAck);
        assert_eq!(response.result_code(), Some(ResultCode::Success));
        assert_eq!(client.applications, Some(Vec::new()));
        assert!(client.closing_result.is_none());
    }

    #[tokio::test]
    async fn submission_before_handshake_is_invalid() {
        let mut client = make_client();
        let record = build_record();
        let message = Message::new_submission(&record).unwrap();
        let env = TestEnv::new();

        let response = handle_mosaic_message(message, &mut client, &env.store, &env.logger)
            .await
            .unwrap()
            .expect("response");

        assert_eq!(response.message_type(), MessageType::SubmissionResult);
        assert_eq!(response.result_code(), Some(ResultCode::Invalid));
        assert_eq!(response.id_prefix().unwrap(), &record.id().as_bytes()[..32]);
        assert_eq!(env.store_impl.record_count(), 0);
        assert!(!env.logger.entries().is_empty());
    }

    #[tokio::test]
    async fn submission_persists_and_acknowledges() {
        let mut client = make_client();
        client.mosaic_version = Some(0);
        client.applications = Some(vec![0]);
        let record = build_record();
        let message = Message::new_submission(&record).unwrap();
        let env = TestEnv::new();

        let response = handle_mosaic_message(message.clone(), &mut client, &env.store, &env.logger)
            .await
            .unwrap()
            .expect("response");

        assert_eq!(response.message_type(), MessageType::SubmissionResult);
        assert_eq!(response.result_code(), Some(ResultCode::Accepted));
        assert!(env.store_impl.contains(record.id().as_bytes()));

        let duplicate = handle_mosaic_message(message, &mut client, &env.store, &env.logger)
            .await
            .unwrap()
            .expect("response");

        assert_eq!(duplicate.result_code(), Some(ResultCode::Duplicate));
        assert_eq!(env.store_impl.record_count(), 1);
    }

    #[tokio::test]
    async fn submission_store_error_surfaces_as_general_error() {
        let mut client = make_client();
        client.mosaic_version = Some(0);
        client.applications = Some(vec![0]);
        let record = build_record();
        let message = Message::new_submission(&record).unwrap();
        let store: Arc<dyn Store> = Arc::new(FailingStore);
        let logger = Arc::new(TestLogger::default());

        let response = handle_mosaic_message(message, &mut client, &store, &logger)
            .await
            .unwrap()
            .expect("response");

        assert_eq!(response.message_type(), MessageType::SubmissionResult);
        assert_eq!(response.result_code(), Some(ResultCode::GeneralError));
        assert_eq!(response.id_prefix().unwrap(), &record.id().as_bytes()[..32]);
        assert_eq!(logger.entries().len(), 1);
    }

    #[tokio::test]
    async fn submission_with_unreadable_record_triggers_closing() {
        let mut client = make_client();
        client.mosaic_version = Some(0);
        client.applications = Some(vec![0]);

        let record = build_record();
        let mut bytes = Message::new_submission(&record)
            .unwrap()
            .as_bytes()
            .to_vec();
        // Corrupt the embedded record payload to invalidate the signature/hash.
        bytes[8] ^= 0xFF;
        let corrupted = unsafe { Message::from_bytes_unchecked(bytes) };

        let env = TestEnv::new();

        let response = handle_mosaic_message(corrupted, &mut client, &env.store, &env.logger)
            .await
            .unwrap()
            .expect("closing response");

        assert_eq!(response.message_type(), MessageType::Closing);
        assert_eq!(response.result_code(), Some(ResultCode::Invalid));
        assert!(env.store_impl.record_count() == 0);
    }

    #[test]
    fn get_requires_handshake() {
        let env = TestEnv::new();
        let record = build_record();
        let reference = record.id().to_reference();
        env.store_impl
            .put_record(record.as_ref())
            .expect("insert record");
        let query_id = QueryId::from_bytes([0, 1]);
        let get_message = Message::new_get(query_id, &[&reference]).unwrap();
        let client = make_client();

        let response = handle_get(&get_message, &client, &env.store).unwrap();
        assert_eq!(response.result_code, ResultCode::Invalid);
        assert!(response.records.is_empty());
    }

    #[test]
    fn get_returns_records_and_success() {
        let env = TestEnv::new();
        let record = build_record();
        let reference = record.id().to_reference();
        env.store_impl
            .put_record(record.as_ref())
            .expect("insert record");
        let query_id = QueryId::from_bytes([0, 2]);
        let get_message = Message::new_get(query_id, &[&reference]).unwrap();

        let mut client = make_client();
        client.mosaic_version = Some(0);
        client.applications = Some(vec![0]);

        let response = handle_get(&get_message, &client, &env.store).unwrap();
        assert_eq!(response.result_code, ResultCode::Success);
        assert_eq!(response.records.len(), 1);
        assert_eq!(response.records[0].as_bytes(), record.as_bytes());
    }

    #[test]
    fn get_not_found_returns_notfound() {
        let env = TestEnv::new();
        let mut client = make_client();
        client.mosaic_version = Some(0);
        client.applications = Some(vec![0]);

        let reference_bytes: [u8; 48] = [0; 48];
        let reference = Reference::from_bytes(&reference_bytes).unwrap();
        let query_id = QueryId::from_bytes([0, 3]);
        let get_message = Message::new_get(query_id, &[&reference]).unwrap();

        let response = handle_get(&get_message, &client, &env.store).unwrap();
        assert_eq!(response.result_code, ResultCode::NotFound);
        assert!(response.records.is_empty());
    }
}
