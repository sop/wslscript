use crate::wcstring;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Path contains invalid UTF-8 characters.")]
    StringToPathUTF8Error,

    #[error("Failed to convert Windows path to WSL path.")]
    WinToUnixPathError,

    #[error("WSL not found or not installed.")]
    WSLNotFound,

    #[error("Failed to start WSL process.")]
    WSLProcessError,

    #[error("Invalid path.")]
    InvalidPathError,

    #[error("Command is too long.")]
    CommandTooLong,

    #[error("String is not nul terminated.")]
    MissingNulError,

    #[error("Operation was cancelled.")]
    Cancel,

    #[error("Registry error: {0}")]
    RegistryError(std::io::Error),

    #[error("IO error: {0}")]
    IOError(std::io::Error),

    #[error("Dynamic library error: {0}")]
    LibraryError(String),

    #[error("WinAPI error: {0}")]
    WinAPIError(String),

    #[error("Drop handler error: {0}")]
    DropHandlerError(String),

    #[error("Error: {0}")]
    GenericError(String),

    #[error("Logic error: {0}")]
    LogicError(&'static str),
}

impl Error {
    pub fn to_wide(&self) -> widestring::WideCString {
        wcstring(self.to_string())
    }
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Error {
        e.downcast::<Error>()
            .unwrap_or_else(|e: anyhow::Error| Error::GenericError(e.to_string()))
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::IOError(e)
    }
}

impl From<widestring::error::MissingNulTerminator> for Error {
    fn from(_: widestring::error::MissingNulTerminator) -> Error {
        Error::MissingNulError
    }
}
