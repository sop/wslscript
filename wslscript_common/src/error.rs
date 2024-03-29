use crate::wcstring;
use std::fmt::{self, Display};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Path contains invalid UTF-8 characters.")]
    StringToPathUTF8Error,

    #[fail(display = "Failed to convert Windows path to WSL path.")]
    WinToUnixPathError,

    #[fail(display = "WSL not found or not installed.")]
    WSLNotFound,

    #[fail(display = "Failed to start WSL process.")]
    WSLProcessError,

    #[fail(display = "Invalid path.")]
    InvalidPathError,

    #[fail(display = "Command is too long.")]
    CommandTooLong,

    #[fail(display = "String is not nul terminated.")]
    MissingNulError,

    #[fail(display = "Operation was cancelled.")]
    Cancel,

    #[fail(display = "Registry error: {}", e)]
    RegistryError { e: std::io::Error },

    #[fail(display = "IO error: {}", e)]
    IOError { e: std::io::Error },

    #[fail(display = "Dynamic library error: {}", s)]
    LibraryError { s: String },

    #[fail(display = "WinAPI error: {}", s)]
    WinAPIError { s: String },

    #[fail(display = "Drop handler error: {}", s)]
    DropHandlerError { s: String },

    #[fail(display = "Error: {}", s)]
    GenericError { s: String },

    #[fail(display = "Logic error: {}", s)]
    LogicError { s: &'static str },
}

#[derive(Debug)]
pub struct Error {
    inner: failure::Context<ErrorKind>,
}

impl Error {
    pub fn to_wide(&self) -> widestring::WideCString {
        wcstring(self.to_string())
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error {
            inner: failure::Context::new(kind),
        }
    }
}

impl From<failure::Context<ErrorKind>> for Error {
    fn from(kind: failure::Context<ErrorKind>) -> Error {
        Error { inner: kind }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::from(ErrorKind::IOError { e })
    }
}

impl From<widestring::error::MissingNulTerminator> for Error {
    fn from(_: widestring::error::MissingNulTerminator) -> Error {
        Error::from(ErrorKind::MissingNulError)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.inner, f)
    }
}
