use std::path::Path;

use mosaic_core::{OwnedRecord, Record, Reference};
use mosaic_store_lmdb::{InnerError as LmdbInnerError, Store as RawLmdbStore};

use crate::{Error, InnerError};

/// Result of attempting to insert a record into storage
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutResult {
    Inserted,
    Duplicate,
}

/// Minimal storage abstraction needed by the server for submissions.
pub trait Store: Send + Sync {
    /// Store a record, returning whether it was newly inserted or a duplicate.
    fn put_record(&self, record: &Record) -> Result<PutResult, Error>;

    /// Returns true if the record is already present (used for tests).
    fn has_record(&self, reference: &Reference) -> Result<bool, Error>;

    /// Fetch a record by reference.
    fn get_record(&self, reference: &Reference) -> Result<Option<OwnedRecord>, Error>;
}

/// LMDB-backed store adapter using `mosaic-store-lmdb`.
pub struct LmdbStore(RawLmdbStore);

impl LmdbStore {
    /// Open or create a LMDB-backed store at `dir`.
    pub fn open<P: AsRef<Path>>(dir: P, max_size_gb: usize) -> Result<Self, Error> {
        let inner = RawLmdbStore::new(dir, vec![], max_size_gb).map_err(convert_store_error)?;
        Ok(Self(inner))
    }
}

impl Store for LmdbStore {
    fn put_record(&self, record: &Record) -> Result<PutResult, Error> {
        match self.0.store_record(record) {
            Ok(_) => Ok(PutResult::Inserted),
            Err(e) => match e.inner {
                LmdbInnerError::Duplicate => Ok(PutResult::Duplicate),
                other => Err(map_store_error(other)),
            },
        }
    }

    fn has_record(&self, reference: &Reference) -> Result<bool, Error> {
        self.0.has_record(*reference).map_err(convert_store_error)
    }

    fn get_record(&self, reference: &Reference) -> Result<Option<OwnedRecord>, Error> {
        match self.0.get_record_by_ref(*reference) {
            Ok(Some(record)) => {
                let bytes = record.as_bytes().to_vec();
                Ok(Some(OwnedRecord::from_vec(bytes)?))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(convert_store_error(e)),
        }
    }
}

fn convert_store_error(error: mosaic_store_lmdb::Error) -> Error {
    map_store_error(error.inner)
}

fn map_store_error(inner: LmdbInnerError) -> Error {
    let message = match inner {
        LmdbInnerError::General(msg) => msg,
        LmdbInnerError::Io(err) => format!("lmdb io error: {err}"),
        LmdbInnerError::Lmdb(err) => format!("lmdb backend error: {err}"),
        LmdbInnerError::BufferTooSmall => "lmdb store buffer too small".to_owned(),
        LmdbInnerError::EndOfInput => "lmdb store unexpected end of input".to_owned(),
        LmdbInnerError::FilterTooWide => "lmdb store filter too wide".to_owned(),
        LmdbInnerError::MosaicCore(err) => format!("lmdb core error: {err}"),
        LmdbInnerError::Duplicate => "lmdb store duplicate".to_owned(),
    };

    InnerError::General(message).into_err()
}

#[cfg(test)]
mod tests {
    use super::*;

    use mosaic_core::{
        EMPTY_TAG_SET, Kind, OwnedRecord, RecordAddressData, RecordParts, RecordSigningData,
        SecretKey, Timestamp,
    };

    fn build_record() -> OwnedRecord {
        let signing_key = SecretKey::generate();
        OwnedRecord::new(&RecordParts {
            signing_data: RecordSigningData::SecretKey(signing_key.clone()),
            address_data: RecordAddressData::Random(signing_key.public(), Kind::KEY_SCHEDULE),
            timestamp: Timestamp::now().unwrap(),
            flags: Default::default(),
            tag_set: &EMPTY_TAG_SET,
            payload: b"store test payload",
        })
        .unwrap()
    }

    #[test]
    fn insert_and_detect_duplicate() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = LmdbStore::open(temp_dir.path(), 1).unwrap();
        let record = build_record();

        let result = store.put_record(record.as_ref()).unwrap();
        assert_eq!(result, PutResult::Inserted);

        let duplicate = store.put_record(record.as_ref()).unwrap();
        assert_eq!(duplicate, PutResult::Duplicate);

        let reference = record.id().to_reference();
        assert!(store.has_record(&reference).unwrap());

        let fetched = store.get_record(&reference).unwrap().unwrap();
        assert_eq!(fetched.as_bytes(), record.as_bytes());
    }
}
