use crate::error::*;
use std::convert::From;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::ptr::null_mut;
use wchar::*;
use widestring::*;
use winapi::shared::minwindef as win;
use winapi::um::winnt;

/// Convert &str to WideCString
pub fn wcstring<T: AsRef<str>>(s: T) -> WideCString {
    WideCString::from_str(s).unwrap_or_else(|e| {
        let p = e.nul_position();
        if let Some(mut v) = e.into_vec() {
            v.resize(p, 0);
            WideCString::from_vec_truncate(v)
        } else {
            WideCString::default()
        }
    })
}

/// Convert WCHAR slice _(usually from `wchz!` macro)_ to WideCStr
pub fn wcstr(s: &[wchar_t]) -> &WideCStr {
    WideCStr::from_slice_truncate(s).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_wcstring_with_null() {
        assert_eq!(wcstring("with\0null"), wcstring("with"));
    }
    #[test]
    fn test_wcstr() {
        assert_eq!(wcstr(wchz!("test")).as_slice(), &wchz!("test")[0..4]);
    }
}

/// Display error message as a message box.
pub fn error_message(msg: &WideCStr) {
    use winapi::um::winuser::{MessageBoxW, MB_ICONERROR, MB_OK};
    unsafe {
        MessageBoxW(
            null_mut(),
            msg.as_ptr(),
            wcstr(wchz!("Error")).as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// Get the last WinAPI error.
pub fn last_error() -> Error {
    use winapi::um::winbase::*;
    let mut buf: winnt::LPWSTR = null_mut();
    let errno = unsafe { winapi::um::errhandlingapi::GetLastError() };
    let res = unsafe {
        FormatMessageW(
            FORMAT_MESSAGE_FROM_SYSTEM
                | FORMAT_MESSAGE_IGNORE_INSERTS
                | FORMAT_MESSAGE_ALLOCATE_BUFFER,
            null_mut(),
            errno,
            win::DWORD::from(winnt::MAKELANGID(
                winnt::LANG_NEUTRAL,
                winnt::SUBLANG_DEFAULT,
            )),
            &mut buf as *mut winnt::LPWSTR as _,
            0,
            null_mut(),
        )
    };
    let s: String = if res == 0 {
        format!("Error code {}", errno)
    } else {
        let s = unsafe { WideCString::from_ptr_str(buf).to_string_lossy() };
        unsafe { LocalFree(buf as _) };
        s
    };
    Error::WinAPIError(s)
}

/// Path buffer with Windows semantics.
#[derive(Clone)]
pub struct WinPathBuf {
    buf: PathBuf,
}

impl WinPathBuf {
    pub fn new(buf: PathBuf) -> Self {
        Self { buf }
    }

    /// Get path as a nul terminated wide string.
    pub fn to_wide(&self) -> WideCString {
        unsafe { WideCString::from_os_str_unchecked(self.buf.as_os_str()) }
    }

    /// Canonicalize path.
    pub fn canonicalize(&self) -> Result<Self, Error> {
        Ok(Self::new(self.buf.canonicalize().map_err(Error::from)?))
    }

    /// Remove extended length path prefix (`\\?\`).
    pub fn without_extended(&self) -> Self {
        use std::ffi::OsString;
        use std::os::windows::ffi::*;
        let words = self.buf.as_os_str().encode_wide().collect::<Vec<_>>();
        let mut s = words.as_slice();
        if s.starts_with(wch!(r"\\?\")) {
            s = &s[4..];
        }
        Self::new(PathBuf::from(OsString::from_wide(s)))
    }

    /// Get the path as a doubly quoted wide string.
    pub fn quoted(&self) -> WideString {
        let mut ws = WideString::new();
        ws.push_slice(wch!(r#"""#));
        ws.push_os_str(self.buf.as_os_str());
        ws.push_slice(wch!(r#"""#));
        ws
    }

    /// Expand environment variables in a path.
    pub fn expand(&self) -> Result<Self, Error> {
        use winapi::um::fileapi::*;
        use winapi::um::processenv::*;
        let mut buf = [0_u16; 2048];
        let len = unsafe {
            ExpandEnvironmentStringsW(self.to_wide().as_ptr(), buf.as_mut_ptr(), buf.len() as _)
        };
        if len == 0 {
            return Err(last_error());
        }
        let path = unsafe { WideCString::from_ptr_unchecked(buf.as_ptr(), len as _) };
        let len = unsafe { GetLongPathNameW(path.as_ptr(), buf.as_mut_ptr(), buf.len() as _) };
        if len == 0 {
            return Err(last_error());
        }
        let path = unsafe { WideCString::from_ptr_unchecked(buf.as_ptr(), (len + 1) as _) };
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
