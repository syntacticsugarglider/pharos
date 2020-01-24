use crate::import::*;

/// The error type for errors happening in `pharos`.
///
/// Use [`Error::kind()`] to know which kind of error happened.
//
#[derive(Debug)]
//
pub struct Error {
    pub(crate) inner: Option<Box<dyn ErrorTrait + Send + Sync>>,
    pub(crate) kind: ErrorKind,
}

impl Error {
    /// Identify which error happened.
    //
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error { inner: None, kind }
    }
}

impl From<FutSendError> for Error {
    fn from(inner: FutSendError) -> Error {
        Error {
            inner: Some(Box::new(inner)),
            kind: ErrorKind::SendError,
        }
    }
}

/// The different kind of errors that can happen when you use the `pharos` API.
//
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
//
pub enum ErrorKind {
    #[doc(hidden)]
    //
    //This variant is only used internally.
    //
    SendError,

    /// The pharos object is already closed. You can no longer send messages or observe it.
    /// This should only happen if you call [SinkExt::close](https://docs.rs/futures-preview/0.3.0-alpha.19/futures/sink/trait.SinkExt.html#method.close) on it.
    //
    Closed,

    /// The minimum valid buffer size for [`Channel::Bounded`](crate::observable::Channel) is `1`, you sent in `0`.
    //
    MinChannelSizeOne,

    #[doc(hidden)]
    //
    __NonExhaustive__,
}

impl PartialEq<&ErrorKind> for ErrorKind {
    fn eq(&self, other: &&ErrorKind) -> bool {
        self == *other
    }
}

impl PartialEq<ErrorKind> for &ErrorKind {
    fn eq(&self, other: &ErrorKind) -> bool {
        *self == other
    }
}

impl ErrorTrait for Error {
    fn source(&self) -> Option<&(dyn ErrorTrait + 'static)> {
        self.inner
            .as_ref()
            .map(|e| -> &(dyn ErrorTrait + 'static) { e.deref() })
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SendError => fmt::Display::fmt("Channel closed.", f),
            Self::MinChannelSizeOne => fmt::Display::fmt(
                "The minimum valid buffer size for Channel::Bounded is 1, you send in 0.",
                f,
            ),

            _ => unreachable!(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = match self.source() {
            Some(e) => format!(" Caused by: {}", e),
            None => String::new(),
        };

        write!(f, "pharos::Error: {}{}", self.kind, inner)
    }
}
