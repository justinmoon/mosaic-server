use crate::client::ClientData;

use mosaic_core::{Error as CoreError, InnerError, Message, MessageType, OwnedRecord, ResultCode};

/// Outcome of validating a Submission message before persistence.
#[derive(Debug)]
pub enum SubmissionValidationError {
    /// Client attempted to submit a record before completing HELLO/HELLO ACK.
    HandshakeNotComplete,
    /// Message was not a Submission frame.
    WrongMessageType,
    /// Record failed structural or cryptographic verification.
    RecordInvalid(CoreError),
}

impl SubmissionValidationError {
    /// Map validation failures to the shared Mosaic `ResultCode` enumeration.
    #[must_use]
    pub fn result_code(&self) -> ResultCode {
        match self {
            SubmissionValidationError::HandshakeNotComplete
            | SubmissionValidationError::WrongMessageType => ResultCode::Invalid,
            SubmissionValidationError::RecordInvalid(err) => match err.inner {
                InnerError::RecordTooLong => ResultCode::TooLarge,
                _ => ResultCode::Invalid,
            },
        }
    }
}

/// Validate a `Submission` message according to the Mosaic specification.
///
/// Returns an owned, verified record on success so downstream code can persist it.
pub fn validate_submission(
    message: &Message,
    client: &ClientData,
) -> Result<OwnedRecord, SubmissionValidationError> {
    if message.message_type() != MessageType::Submission {
        return Err(SubmissionValidationError::WrongMessageType);
    }

    if client.mosaic_version.is_none() || client.applications.is_none() {
        return Err(SubmissionValidationError::HandshakeNotComplete);
    }

    let record_bytes = &message.as_bytes()[8..];
    OwnedRecord::from_vec(record_bytes.to_vec()).map_err(SubmissionValidationError::RecordInvalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use mosaic_core::{
        EMPTY_TAG_SET, Kind, OwnedRecord, RecordAddressData, RecordParts, RecordSigningData,
        SecretKey, Timestamp,
    };

    fn client_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9000)
    }

    fn make_client(handshake_complete: bool) -> ClientData {
        if handshake_complete {
            ClientData {
                remote_address: client_addr(),
                peer: None,
                mosaic_version: Some(0),
                applications: Some(vec![0]),
                closing_result: None,
            }
        } else {
            ClientData {
                remote_address: client_addr(),
                peer: None,
                mosaic_version: None,
                applications: None,
                closing_result: None,
            }
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
            payload: b"hello world",
        })
        .unwrap()
    }

    #[test]
    fn submission_requires_handshake() {
        let client = make_client(false);
        let record = build_record();
        let message = mosaic_core::Message::new_submission(&record).unwrap();

        let err = validate_submission(&message, &client).unwrap_err();
        assert!(matches!(
            err,
            SubmissionValidationError::HandshakeNotComplete
        ));
        assert_eq!(err.result_code(), ResultCode::Invalid);
    }

    #[test]
    fn submission_wrong_type_rejected() {
        let client = make_client(true);
        let message = mosaic_core::Message::new_hello(0, &[]).unwrap();

        let err = validate_submission(&message, &client).unwrap_err();
        assert!(matches!(err, SubmissionValidationError::WrongMessageType));
        assert_eq!(err.result_code(), ResultCode::Invalid);
    }

    #[test]
    fn valid_submission_passes() {
        let client = make_client(true);
        let record = build_record();
        let message = mosaic_core::Message::new_submission(&record).unwrap();

        let validated = validate_submission(&message, &client).unwrap();
        assert_eq!(validated.as_bytes(), record.as_bytes());
    }

    #[test]
    fn invalid_record_is_caught() {
        let client = make_client(true);
        let record = build_record();
        let message = mosaic_core::Message::new_submission(&record).unwrap();

        let mut corrupted = message.as_bytes().to_vec();
        // Flip a bit inside the record payload to break the signature/hash invariant.
        corrupted[8] ^= 0xFF;
        let invalid_message = unsafe { mosaic_core::Message::from_bytes_unchecked(corrupted) };

        let err = validate_submission(&invalid_message, &client).unwrap_err();
        assert!(matches!(err, SubmissionValidationError::RecordInvalid(_)));
        assert_eq!(err.result_code(), ResultCode::Invalid);
    }

    #[test]
    fn record_too_long_maps_to_toolarge() {
        let err = SubmissionValidationError::RecordInvalid(InnerError::RecordTooLong.into_err());
        assert_eq!(err.result_code(), ResultCode::TooLarge);
    }
}
