use crate::error::*;
use crate::win32::*;
use std::ptr::null_mut;
use std::str::FromStr;
use wchar::*;
use widestring::*;
use winapi::shared::windef;
use winapi::um::libloaderapi;
use winapi::um::shellapi;
use winapi::um::winuser;

/// The Old New Thing - How the shell converts an icon location into an icon
/// https://devblogs.microsoft.com/oldnewthing/20100505-00/?p=14153

#[derive(Clone)]
pub struct ShellIcon {
    /// Handle to loaded icon.
    handle: windef::HICON,
    /// Path to file containing icon.
    path: WinPathBuf,
    /// Icon index in a file.
    index: u32,
}

impl ShellIcon {
    pub fn load(path: WinPathBuf, index: u32) -> Result<Self, Error> {
        let s = path.to_wide();
        let handle = unsafe {
            shellapi::ExtractIconW(
                libloaderapi::GetModuleHandleW(null_mut()),
                s.as_ptr(),
                index,
            )
        };
        if handle.is_null() {
            return Err(Error::from(ErrorKind::WinAPIError {
                s: String::from("No icon found from the file."),
            }));
        }
        if handle == 1 as _ {
            return Err(Error::from(ErrorKind::WinAPIError {
                s: String::from("File not found."),
            }));
        }
        Ok(Self {
            handle,
            path,
            index,
        })
    }

    /// Load default icon.
    pub fn load_default() -> Result<Self, Error> {
        use std::os::windows::ffi::OsStrExt;
        let s: Vec<WideChar> = std::env::current_exe()?
            .canonicalize()?
            .as_os_str()
            .encode_wide()
            .collect();
        // remove UNC prefix
        let ws = if &s[0..4] == wch!(r"\\?\") {
            WideStr::from_slice(&s[4..])
        } else {
            WideStr::from_slice(&s)
        };
        Self::load(WinPathBuf::from(ws), 0)
    }

    pub fn handle(&self) -> windef::HICON {
        self.handle
    }

    pub fn path(&self) -> WinPathBuf {
        self.path.clone()
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn shell_path(&self) -> WideCString {
        let mut p = self.path.to_wide().to_os_string();
        p.push(format!(",{}", self.index));
        unsafe { WideCString::from_os_str_unchecked(p) }
    }
}

impl Drop for ShellIcon {
    fn drop(&mut self) {
        unsafe { winuser::DestroyIcon(self.handle) };
    }
}

impl FromStr for ShellIcon {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let path: String;
        let index: u32;
        if let Some(i) = s.rfind(',') {
            path = s[0..i].to_string();
            index = s[i + 1..].parse::<u32>().unwrap_or(0);
        } else {
            path = s.to_owned();
            index = 0;
        }
        Self::load(WinPathBuf::from(path.as_str()), index)
    }
}
