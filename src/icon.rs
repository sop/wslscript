use crate::error::*;
use crate::win32::*;
use std::ptr::null_mut;
use std::str::FromStr;
use widestring::*;
use winapi::shared::windef::*;
use winapi::um::libloaderapi::*;
use winapi::um::shellapi::*;
use winapi::um::winuser::*;

/// The Old New Thing - How the shell converts an icon location into an icon
/// https://devblogs.microsoft.com/oldnewthing/20100505-00/?p=14153

#[derive(Clone)]
pub struct ShellIcon {
    handle: HICON,    // handle to loaded icon
    path: WinPathBuf, // path to file containing icon
    index: u32,       // icon index in a file
}

impl ShellIcon {
    pub fn load(path: WinPathBuf, index: u32) -> Result<Self, Error> {
        let s = path.to_wide();
        let handle = unsafe { ExtractIconW(GetModuleHandleW(null_mut()), s.as_ptr(), index) };
        if handle.is_null() {
            Err(ErrorKind::WinAPIError {
                s: "No icon found from the file.".to_owned(),
            })?
        }
        if handle == 1 as _ {
            Err(ErrorKind::WinAPIError {
                s: "File not found.".to_owned(),
            })?
        }
        Ok(Self {
            handle,
            path,
            index,
        })
    }

    pub fn load_default() -> Result<Self, Error> {
        let exe_os = std::env::current_exe()?.canonicalize()?;
        let executable = exe_os
            .to_str()
            .ok_or_else(|| ErrorKind::StringToPathUTF8Error)?
            .trim_start_matches("\\\\?\\");
        Self::load(WinPathBuf::from(executable), 0)
    }

    pub fn handle(&self) -> HICON {
        self.handle
    }

    pub fn path(&self) -> WinPathBuf {
        self.path.clone()
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn shell_path(&self) -> U16CString {
        let mut p = self.path.to_wide().to_os_string();
        p.push(format!(",{}", self.index));
        unsafe { U16CString::from_os_str_unchecked(p) }
    }
}

impl Drop for ShellIcon {
    fn drop(&mut self) {
        unsafe { DestroyIcon(self.handle) };
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