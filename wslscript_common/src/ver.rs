use crate::error::*;
use crate::win32::*;
use std::path::Path;
use std::ptr;
use widestring::WideCStr;
use widestring::WideChar;
use winapi::shared::minwindef as win;
use winapi::um::winver;

/// Get version string from file.
pub fn product_version(path: &Path) -> Option<String> {
    let filever = FileVersion::try_new(path).ok()?;
    let translations = filever
        .query::<LANGANDCODEPAGE>(r"\VarFileInfo\Translation")
        .ok()?;
    for translation in translations {
        let sub_block = format!(
            r"\StringFileInfo\{:04x}{:04x}\ProductVersion",
            translation.lang, translation.cp
        );
        if let Ok(s) = filever.query::<WideChar>(&sub_block) {
            let version = WideCStr::from_slice_truncate(s).unwrap_or_default();
            return Some(version.to_string_lossy());
        }
    }
    None
}

#[repr(C)]
struct LANGANDCODEPAGE {
    lang: win::WORD,
    cp: win::WORD,
}

struct FileVersion {
    /// File version information.
    ///
    /// See: https://docs.microsoft.com/en-us/windows/win32/api/winver/nf-winver-getfileversioninfow
    data: Vec<u8>,
}

impl FileVersion {
    pub fn try_new(path: &Path) -> Result<Self, Error> {
        let path_c = WinPathBuf::new(path.to_owned()).to_wide();
        let size = unsafe { winver::GetFileVersionInfoSizeW(path_c.as_ptr(), ptr::null_mut()) };
        if size == 0 {
            return Err(last_error());
        }
        let mut data = Vec::<u8>::with_capacity(size as _);
        let rv = unsafe {
            winver::GetFileVersionInfoW(path_c.as_ptr(), 0, size, data.as_mut_ptr() as _)
        };
        if rv == 0 {
            return Err(last_error());
        }
        unsafe { data.set_len(size as _) };
        Ok(Self { data })
    }

    /// Query file version value.
    ///
    /// See: https://docs.microsoft.com/en-us/windows/win32/api/winver/nf-winver-verqueryvaluew
    pub fn query<T>(&self, sub_block: &str) -> Result<&[T], Error> {
        let mut buf: win::LPVOID = ptr::null_mut();
        let mut len: win::UINT = 0;
        let rv = unsafe {
            winver::VerQueryValueW(
                self.data.as_ptr() as _,
                wcstring(sub_block).as_ptr(),
                &mut buf,
                &mut len,
            )
        };
        if rv == 0 {
            return Err(Error::GenericError("Version not found.".to_string()));
        }
        let s = unsafe { std::slice::from_raw_parts::<T>(buf as _, len as _) };
        Ok(s)
    }
}
