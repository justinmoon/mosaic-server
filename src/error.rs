use std::convert::Infallible;
use std::error::Error as StdError;
use std::panic::Location;

/// A Mosaic server error
#[derive(Debug)]
pub struct Error {
    /// The error itself
    pub inner: InnerError,
    location: &'static Location<'static>,
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.inner)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}, {}", self.inner, self.location)
    }
}

/// Errors that can occur in this crate
#[derive(Debug)]
pub enum InnerError {
    /// General error
    General(String),

    /// Mosaic Core
    MosaicCore(mosaic_core::Error),

    /// Mosaic Net
    MosaicNet(mosaic_net::Error),

    /// Tokio Join
    TokioJoin(tokio::task::JoinError),
}

impl std::fmt::Display for InnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InnerError::General(s) => write!(f, "General Error: {s}"),
            InnerError::MosaicCore(e) => write!(f, "Mosaic Core: {e}"),
            InnerError::MosaicNet(e) => write!(f, "Mosaic Net: {e}"),
            InnerError::TokioJoin(e) => write!(f, "Tokio Join: {e}"),
        }
    }
}

impl StdError for InnerError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            InnerError::MosaicCore(e) => Some(e),
            InnerError::MosaicNet(e) => Some(e),
            _ => None,
        }
    }
}

// Note: we impl Into because our typical pattern is InnerError::Variant.into()
//       when we tried implementing From, the location was deep in rust code's
//       blanket into implementation, which wasn't the line number we wanted.
//
//       As for converting other error types, the try! macro uses From so it
//       is correct.
#[allow(clippy::from_over_into)]
impl Into<Error> for InnerError {
    #[track_caller]
    fn into(self) -> Error {
        Error {
            inner: self,
            location: Location::caller(),
        }
    }
}

// Use this to avoid complex type qualification
impl InnerError {
    /// Convert an `InnerError` into an `Error`
    #[track_caller]
    #[must_use]
    pub fn into_err(self) -> Error {
        Error {
            inner: self,
            location: Location::caller(),
        }
    }
}

impl From<Infallible> for Error {
    #[track_caller]
    fn from(_: Infallible) -> Self {
        panic!("INFALLIBLE")
    }
}

impl From<()> for Error {
    #[track_caller]
    fn from((): ()) -> Self {
        Error {
            inner: InnerError::General("Error".to_owned()),
            location: Location::caller(),
        }
    }
}

impl From<&str> for Error {
    #[track_caller]
    fn from(s: &str) -> Self {
        Error {
            inner: InnerError::General(s.to_owned()),
            location: Location::caller(),
        }
    }
}

impl From<String> for Error {
    #[track_caller]
    fn from(s: String) -> Self {
        Error {
            inner: InnerError::General(s),
            location: Location::caller(),
        }
    }
}

impl From<mosaic_core::Error> for Error {
    #[track_caller]
    fn from(e: mosaic_core::Error) -> Self {
        Error {
            inner: InnerError::MosaicCore(e),
            location: Location::caller(),
        }
    }
}

impl From<mosaic_net::Error> for Error {
    #[track_caller]
    fn from(e: mosaic_net::Error) -> Self {
        Error {
            inner: InnerError::MosaicNet(e),
            location: Location::caller(),
        }
    }
}

impl From<tokio::task::JoinError> for Error {
    #[track_caller]
    fn from(e: tokio::task::JoinError) -> Self {
        Error {
            inner: InnerError::TokioJoin(e),
            location: Location::caller(),
        }
    }
}
