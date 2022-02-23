use crate::error::*;
use crate::win32;
use std::mem;
use std::ptr;
use winapi::shared::minwindef as win;
use winapi::shared::windef;
use winapi::um::wingdi;
use winapi::um::winuser;

/// Logical font.
pub struct Font {
    pub handle: windef::HFONT,
}

impl Default for Font {
    fn default() -> Self {
        Self {
            handle: ptr::null_mut(),
        }
    }
}

impl Font {
    pub fn new_default_caption() -> Result<Self, Error> {
        Font::new_caption(0)
    }

    /// Get default caption font with given size.
    ///
    /// See: https://docs.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-logfonta
    pub fn new_caption(size: i32) -> Result<Self, Error> {
        use winuser::*;
        let mut metrics = NONCLIENTMETRICSW {
            cbSize: mem::size_of::<NONCLIENTMETRICSW>() as u32,
            ..unsafe { mem::zeroed() }
        };
        if win::FALSE
            == unsafe {
                SystemParametersInfoW(
                    SPI_GETNONCLIENTMETRICS,
                    metrics.cbSize,
                    &mut metrics as *mut _ as *mut _,
                    0,
                )
            }
        {
            return Err(win32::last_error());
        }
        let mut lf: wingdi::LOGFONTW = metrics.lfCaptionFont;
        if size > 0 {
            lf.lfHeight = size;
        }
        let font = unsafe { wingdi::CreateFontIndirectW(&lf) };
        if font.is_null() {
            return Err(win32::last_error());
        }
        Ok(Self { handle: font })
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { wingdi::DeleteObject(self.handle as _) };
        }
    }
}
