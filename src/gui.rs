use crate::error::*;
use crate::registry;
use crate::win32::*;
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::{FromPrimitive, ToPrimitive};
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use winapi::shared::basetsd;
use winapi::shared::minwindef::*;
use winapi::shared::minwindef::{FALSE as W_FALSE, TRUE as W_TRUE};
use winapi::shared::ntdef::*;
use winapi::shared::windef::*;
use winapi::um::errhandlingapi::{GetLastError, SetLastError};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::winbase::*;
use winapi::um::wingdi::*;
use winapi::um::winuser::*;

pub fn start_gui() -> Result<(), Error> {
    MainWindow::new("WSL Script")?.run()
}

pub trait WindowProc {
    /// Window procedure callback.
    ///
    /// If None is returned, underlying wrapper calls `DefWindowProcW`.
    fn window_proc(
        &mut self,
        hwnd: HWND,
        msg: UINT,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT>;
}

/// Window procedure wrapper that stores struct pointer to window attributes.
///
/// Proxies messages to `window_proc()` with *self*.
unsafe extern "system" fn window_proc_wrapper<T: WindowProc>(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // get pointer to T from userdata
    let mut ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut T;
    // not yet set, initialize from CREATESTRUCT
    if ptr.is_null() && msg == WM_NCCREATE {
        let cs = &*(lparam as *const CREATESTRUCTW);
        ptr = cs.lpCreateParams as *mut T;
        SetLastError(0);
        if 0 == SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as *const _ as basetsd::LONG_PTR)
            && GetLastError() != 0
        {
            return W_FALSE as LRESULT;
        }
    }
    // call wrapped window proc
    if !ptr.is_null() {
        let this = &mut *(ptr as *mut T);
        if let Some(result) = this.window_proc(hwnd, msg, wparam, lparam) {
            return result;
        }
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

struct MainWindow {
    window: HWND,
    caption_font: Font,
}
impl Default for MainWindow {
    fn default() -> Self {
        Self {
            window: null_mut(),
            caption_font: Default::default(),
        }
    }
}

#[derive(FromPrimitive, ToPrimitive)]
enum Control {
    StaticMsg = 100,
    BtnRegister,
}

impl MainWindow {
    fn new(title: &str) -> Result<Box<Self>, Error> {
        // must be boxed to have a fixed pointer
        let wnd = Box::new(Self {
            ..Default::default()
        });
        let wnd_ptr = &*wnd as *const _;
        let instance = unsafe { GetModuleHandleW(null_mut()) };
        let class_name = WideString::from("WSLScript");
        // register window class
        unsafe {
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: CS_OWNDC | CS_HREDRAW | CS_VREDRAW,
                hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
                lpfnWndProc: Some(window_proc_wrapper::<MainWindow>),
                hInstance: instance,
                lpszClassName: class_name.as_ptr(),
                hIcon: LoadIconW(instance, WideString::from("app").as_ptr()),
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                ..zeroed()
            };
            if 0 == RegisterClassExW(&wc) {
                Err(last_error())?
            }
        }
        // create window
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                WideString::from(title).as_ptr(),
                WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                300,
                150,
                null_mut(),
                null_mut(),
                instance,
                wnd_ptr as LPVOID,
            )
        };
        if hwnd.is_null() {
            Err(last_error())?
        }
        Ok(wnd)
    }

    fn run(&self) -> Result<(), Error> {
        loop {
            unsafe {
                let mut msg: MSG = zeroed();
                match GetMessageW(&mut msg, self.window, 0, 0) {
                    x if x > 0 => {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    x if x < 0 => Err(last_error())?,
                    _ => break,
                }
            }
        }
        Ok(())
    }

    fn create_window_controls(&mut self) -> Result<(), Error> {
        let instance = unsafe { GetWindowLongW(self.window, GWL_HINSTANCE) as HINSTANCE };
        let mut main_rect: RECT = unsafe { zeroed() };
        self.caption_font = Font::new_default_caption()?;
        unsafe { GetClientRect(self.window, &mut main_rect) };
        unsafe {
            let hwnd = CreateWindowExW(
                0,
                WideString::from("BUTTON").as_ptr(),
                WideString::from("Register .sh").as_ptr(),
                WS_TABSTOP | WS_VISIBLE | WS_CHILD | BS_DEFPUSHBUTTON,
                10,
                60,
                main_rect.right - 20,
                30,
                self.window,
                Control::BtnRegister.to_u16().unwrap() as HMENU,
                instance,
                null_mut(),
            );
            SendMessageW(
                hwnd,
                WM_SETFONT,
                self.caption_font.handle as WPARAM,
                W_TRUE as LPARAM,
            );
        }
        unsafe {
            let hwnd = CreateWindowExW(
                0,
                WideString::from("STATIC").as_ptr(),
                null_mut(),
                SS_CENTER | WS_CHILD | WS_VISIBLE,
                0,
                10,
                main_rect.right,
                40,
                self.window,
                Control::StaticMsg.to_u16().unwrap() as HMENU,
                instance,
                null_mut(),
            );
            SendMessageW(
                hwnd,
                WM_SETFONT,
                self.caption_font.handle as WPARAM,
                W_TRUE as LPARAM,
            );
        }
        self.update_control_states();
        Ok(())
    }

    fn update_control_states(&self) -> Option<()> {
        let registered_extensions = registry::query_registered_extensions().ok()?;
        let hwnd_msg = unsafe { GetDlgItem(self.window, Control::StaticMsg.to_i32().unwrap()) };
        let msg: String;
        let ext = "sh";
        if registered_extensions.iter().any(|s| s == ext) {
            if !registry::is_extension_registered_for_wsl(ext).unwrap_or(false) {
                msg = format!(".{} extension is registered for another application!", ext);
            } else {
                msg = match registry::is_registered_for_current_executable(ext) {
                    Ok(true) => format!(".{} extension is registered for WSL!", ext),
                    _ => format!(
                        ".{} extension is registered for wslscript in another location!",
                        ext
                    ),
                }
            }
        } else {
            msg = format!("Click to register .{} extension.", ext);
        }
        unsafe { SetWindowTextW(hwnd_msg, WideString::from(msg).as_ptr()) };
        Some(())
    }

    fn on_resize(&self, width: i32, _height: i32) {
        unsafe {
            let hwnd = GetDlgItem(self.window, Control::StaticMsg.to_i32().unwrap());
            if !hwnd.is_null() {
                MoveWindow(hwnd, 0, 10, width, 40, W_TRUE);
            }
        }
        unsafe {
            let hwnd = GetDlgItem(self.window, Control::BtnRegister.to_i32().unwrap());
            if !hwnd.is_null() {
                MoveWindow(hwnd, 10, 60, width - 20, 30, W_TRUE);
            }
        }
    }

    fn on_control_message(&self, _hwnd: HWND, control_id: Control, _control_code: WORD) -> LRESULT {
        if let Control::BtnRegister = control_id {
            if let Err(e) = registry::register_extension("sh") {
                error_message(WideString::from(format!(
                    "Failed to register extension: {}",
                    e
                )));
            }
            self.update_control_states();
        }
        0
    }
}

impl WindowProc for MainWindow {
    fn window_proc(
        &mut self,
        hwnd: HWND,
        msg: UINT,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        match msg {
            WM_NCCREATE => {
                // store main window handle
                self.window = hwnd;
                // fall through to return None
            }
            WM_CREATE => {
                if self.create_window_controls().is_err() {
                    return Some(-1);
                }
                return Some(0);
            }
            WM_SIZE => {
                self.on_resize(
                    i32::from(LOWORD(lparam as u32)),
                    i32::from(HIWORD(lparam as u32)),
                );
                return Some(0);
            }
            WM_GETMINMAXINFO => {
                let mmi = unsafe { &mut *(lparam as *mut MINMAXINFO) };
                mmi.ptMinTrackSize.x = 300;
                mmi.ptMinTrackSize.y = 150;
                return Some(0);
            }
            WM_CTLCOLORSTATIC => {
                return Some(unsafe { GetStockObject(COLOR_WINDOW + 1 as i32) } as LPARAM);
            }
            WM_COMMAND => {
                // if lParam is non-zero, message is from a control
                if lparam != 0 {
                    if let Some(id) = FromPrimitive::from_u16(LOWORD(wparam as u32)) {
                        return Some(self.on_control_message(
                            lparam as HWND,
                            id,
                            HIWORD(wparam as u32),
                        ));
                    }
                }
            }
            WM_CLOSE => {
                unsafe { DestroyWindow(hwnd) };
                return Some(0);
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                return Some(0);
            }
            _ => {}
        }
        None
    }
}

struct Font {
    handle: HFONT,
}
impl Font {
    fn new_default_caption() -> Result<Self, Error> {
        unsafe {
            let mut metrics = NONCLIENTMETRICSW {
                cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
                ..zeroed()
            };
            if W_FALSE
                == SystemParametersInfoW(
                    SPI_GETNONCLIENTMETRICS,
                    metrics.cbSize,
                    &mut metrics as *mut _ as *mut _,
                    0,
                )
            {
                Err(last_error())?
            }
            let font = CreateFontIndirectW(&metrics.lfCaptionFont);
            if font.is_null() {
                Err(last_error())?
            }
            Ok(Self { handle: font })
        }
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

fn last_error() -> Error {
    let msg: String;
    unsafe {
        let mut buf = [0 as WCHAR; 2048];
        let errno = GetLastError();
        let res = FormatMessageW(
            FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
            null_mut(),
            errno,
            DWORD::from(MAKELANGID(LANG_NEUTRAL, SUBLANG_DEFAULT)),
            buf.as_mut_ptr(),
            buf.len() as DWORD,
            null_mut(),
        );
        if res == 0 {
            msg = format!("Error code {}", errno).to_string();
        } else {
            msg = WideString::from(&buf[..(res + 1) as usize]).to_string();
        }
    }
    Error::from(ErrorKind::WinAPIError { s: msg })
}
