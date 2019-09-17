use crate::ustr;
use std::fmt::{self, Display};
use widestring::U16CString;

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

    #[fail(display = "Registry error: {}", e)]
    RegistryError { e: std::io::Error },

    #[fail(display = "IO error: {}", e)]
    IOError { e: std::io::Error },

    #[fail(display = "WinAPI error: {}", s)]
    WinAPIError { s: String },

    #[fail(display = "Logic error: {}", s)]
    LogicError { s: &'static str },
}

#[derive(Debug)]
pub struct Error {
    inner: failure::Context<ErrorKind>,
}

impl Error {
    pub fn to_string(&self) -> String {
        format!("{}", self)
    }

    pub fn to_wide(&self) -> U16CString {
        ustr!(self.to_string())
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

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.inner, f)
    }
}
