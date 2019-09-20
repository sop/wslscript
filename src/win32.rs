use crate::error::*;
use std::convert::From;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::ptr::null_mut;
use wchar::*;
use widestring::*;
use winapi::shared::minwindef::*;
use winapi::um::winnt::*;

#[macro_export]
macro_rules! wcstr {
    ($x:expr) => {
        WideCString::from_str($x).unwrap_or_else(|e| {
            let p = e.nul_position();
            let mut v = e.into_vec();
            v.resize(p, 0);
            WideCString::new(v).unwrap()
        })
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_wcstr_with_null() {
        assert_eq!(wcstr!("with\0null"), wcstr!("with"));
    }
}

pub fn error_message(msg: &WideCStr) {
    use winapi::um::winuser::{MessageBoxW, MB_ICONERROR, MB_OK};
    unsafe {
        MessageBoxW(
            null_mut(),
            msg.as_ptr(),
            wch_c!("Error").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub fn last_error() -> Error {
    use winapi::um::winbase::*;
    let mut buf: LPWSTR = null_mut();
    let errno = unsafe { winapi::um::errhandlingapi::GetLastError() };
    let res = unsafe {
        FormatMessageW(
            FORMAT_MESSAGE_FROM_SYSTEM
                | FORMAT_MESSAGE_IGNORE_INSERTS
                | FORMAT_MESSAGE_ALLOCATE_BUFFER,
            null_mut(),
            errno,
            DWORD::from(MAKELANGID(LANG_NEUTRAL, SUBLANG_DEFAULT)),
            &mut buf as *mut LPWSTR as _,
            0,
            null_mut(),
        )
    };
    let s: String = if res == 0 {
        format!("Error code {}", errno).to_string()
    } else {
        let s = unsafe { WideCString::from_ptr_str(buf).to_string_lossy() };
        unsafe { LocalFree(buf as _) };
        s
    };
    Error::from(ErrorKind::WinAPIError { s })
}

#[derive(Clone)]
pub struct WinPathBuf {
    buf: PathBuf,
}

impl WinPathBuf {
    pub fn to_wide(&self) -> WideCString {
        unsafe { WideCString::from_os_str_unchecked(self.buf.as_os_str()) }
    }

    pub fn expand(&self) -> Result<WinPathBuf, Error> {
        let mut buf = [0 as WCHAR; 2048];
        let len = unsafe {
            winapi::um::processenv::ExpandEnvironmentStringsW(
                self.to_wide().as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as DWORD,
            )
        };
        if len == 0 {
            return Err(last_error());
        }
        let path = unsafe { WideCString::from_ptr_with_nul_unchecked(buf.as_ptr(), len as usize) };
        let len = unsafe {
            winapi::um::fileapi::GetLongPathNameW(
                path.as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as DWORD,
            )
        };
        if len == 0 {
            return Err(last_error());
        }
        let path =
            unsafe { WideCString::from_ptr_with_nul_unchecked(buf.as_ptr(), (len + 1) as usize) };
        Ok(Self::from(path.as_ucstr()))
    }
}

impl From<&WideCStr> for WinPathBuf {
    fn from(s: &WideCStr) -> Self {
        Self::from(WideStr::from_slice(s.as_slice()))
    }
}

impl From<&WideStr> for WinPathBuf {
    fn from(s: &WideStr) -> Self {
        Self {
            buf: PathBuf::from(s.to_os_string()),
        }
    }
}

impl From<&str> for WinPathBuf {
    fn from(s: &str) -> Self {
        Self {
            buf: PathBuf::from(s),
        }
    }
}

impl Deref for WinPathBuf {
    type Target = PathBuf;

    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

impl DerefMut for WinPathBuf {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buf
    }
}
