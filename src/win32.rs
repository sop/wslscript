use std::convert::From;
use std::ffi::{OsStr, OsString};
use std::iter::once;
use std::ops::{Deref, DerefMut};
use std::os::windows::ffi::{OsStrExt, OsStringExt};

pub fn error_message(msg: WideString) {
    use winapi::um::winuser::{MessageBoxW, MB_ICONERROR, MB_OK};
    let caption = WideString::from("Error");
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            msg.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub struct WideString {
    words: Vec<u16>,
}

impl WideString {
    pub fn to_string(&self) -> String {
        let s: OsString = OsStringExt::from_wide(self.words.as_slice());
        s.to_string_lossy().to_string()
    }
}

impl Deref for WideString {
    type Target = Vec<u16>;

    fn deref(&self) -> &Self::Target {
        &self.words
    }
}
impl DerefMut for WideString {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.words
    }
}

impl From<&str> for WideString {
    fn from(s: &str) -> Self {
        let words: Vec<u16> = OsStr::new(s).encode_wide().chain(once(0)).collect();
        Self { words }
    }
}

impl From<String> for WideString {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

impl From<&OsStr> for WideString {
    fn from(s: &OsStr) -> Self {
        let words: Vec<u16> = s.encode_wide().chain(once(0u16)).collect();
        Self { words }
    }
}

impl From<&[u16]> for WideString {
    fn from(s: &[u16]) -> Self {
        Self {
            words: Vec::from(s),
        }
    }
}
