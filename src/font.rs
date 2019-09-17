use crate::error::*;
use crate::win32::*;
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use winapi::shared::minwindef::FALSE as W_FALSE;
use winapi::shared::windef::*;
use winapi::um::wingdi::*;
use winapi::um::winuser::*;

pub struct Font {
    pub handle: HFONT,
}

impl Font {
    pub fn new_default_caption() -> Result<Self, Error> {
        Font::new_caption(0)
    }

    pub fn new_caption(size: i32) -> Result<Self, Error> {
        let mut metrics = NONCLIENTMETRICSW {
            cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
            ..unsafe { zeroed() }
        };
        if W_FALSE
            == unsafe {
                SystemParametersInfoW(
                    SPI_GETNONCLIENTMETRICS,
                    metrics.cbSize,
                    &mut metrics as *mut _ as *mut _,
                    0,
                )
            }
        {
            Err(last_error())?
        }
        let mut lf: LOGFONTW = metrics.lfCaptionFont;
        if size > 0 {
            lf.lfHeight = size;
        }
        let font = unsafe { CreateFontIndirectW(&lf) };
        if font.is_null() {
            Err(last_error())?
        }
        Ok(Self { handle: font })
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { DeleteObject(self.handle as HGDIOBJ) };
        }
    }
}

impl Default for Font {
    fn default() -> Self {
        Self { handle: null_mut() }
    }
}
