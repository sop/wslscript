use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use winapi::shared::minwindef;
use winapi::shared::windef;
use winapi::um::wingdi;
use winapi::um::winuser;
use wslscript_common::error::*;
use wslscript_common::win32;

pub struct Font {
    pub handle: windef::HFONT,
}

impl Font {
    pub fn new_default_caption() -> Result<Self, Error> {
        Font::new_caption(0)
    }

    pub fn new_caption(size: i32) -> Result<Self, Error> {
        let mut metrics = winuser::NONCLIENTMETRICSW {
            cbSize: size_of::<winuser::NONCLIENTMETRICSW>() as u32,
            ..unsafe { zeroed() }
        };
        if minwindef::FALSE
            == unsafe {
                winuser::SystemParametersInfoW(
                    winuser::SPI_GETNONCLIENTMETRICS,
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
            unsafe { wingdi::DeleteObject(self.handle as windef::HGDIOBJ) };
        }
    }
}

impl Default for Font {
    fn default() -> Self {
        Self { handle: null_mut() }
    }
}
