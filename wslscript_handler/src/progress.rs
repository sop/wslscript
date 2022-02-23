use num_enum::IntoPrimitive;
use once_cell::sync::Lazy;
use std::sync::mpsc::Sender;
use std::{mem, pin::Pin, ptr};
use wchar::*;
use widestring::*;
use winapi::shared::basetsd;
use winapi::shared::minwindef as win;
use winapi::shared::windef::*;
use winapi::um::commctrl;
use winapi::um::errhandlingapi;
use winapi::um::libloaderapi;
use winapi::um::wingdi;
use winapi::um::winuser;
use wslscript_common::error::*;
use wslscript_common::font::Font;
use wslscript_common::wcstring;
use wslscript_common::win32;

pub struct ProgressWindow {
    /// Maximum value for progress.
    high_limit: usize,
    /// Sender to signal for cancellation.
    cancel_sender: Option<Sender<()>>,
    /// Window handle.
    hwnd: HWND,
    /// Default font.
    font: Font,
}

impl Default for ProgressWindow {
    fn default() -> Self {
        Self {
            high_limit: 0,
            cancel_sender: None,
            hwnd: ptr::null_mut(),
            font: Font::default(),
        }
    }
}

/// Progress window class name.
static WND_CLASS: Lazy<WideCString> = Lazy::new(|| wcstring("WSLScriptProgress"));

/// Window message for progress update.
pub const WM_PROGRESS: win::UINT = winuser::WM_USER + 1;

/// Child window identifiers.
#[derive(IntoPrimitive, PartialEq)]
#[repr(u16)]
enum Control {
    ProgressBar = 100,
    Message,
    Title,
}

/// Minimum and initial main window size as a (width, height) tuple.
const MIN_WINDOW_SIZE: (i32, i32) = (300, 150);

impl ProgressWindow {
    pub fn new(high_limit: usize, cancel_sender: Sender<()>) -> Result<Pin<Box<Self>>, Error> {
        use winuser::*;
        // register window class
        if !Self::is_window_class_registered() {
            Self::register_window_class()?;
        }
        let mut wnd = Pin::new(Box::new(Self::default()));
        wnd.high_limit = high_limit;
        wnd.cancel_sender = Some(cancel_sender);
        let instance = unsafe { libloaderapi::GetModuleHandleW(ptr::null_mut()) };
        let title = wchz!("WSL Script");
        // create window
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST, WND_CLASS.as_ptr(), title.as_ptr(),
            WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX | WS_VISIBLE,
            CW_USEDEFAULT, CW_USEDEFAULT, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1,
            ptr::null_mut(), ptr::null_mut(), instance,
            // self as a `CREATESTRUCT`'s `lpCreateParams`
            &*wnd as *const Self as win::LPVOID)
        };
        if hwnd.is_null() {
            return Err(win32::last_error());
        }
        Ok(wnd)
    }

    /// Get handle to main window.
    pub fn handle(&self) -> HWND {
        self.hwnd
    }

    /// Run message loop.
    pub fn run(&self) -> Result<(), Error> {
        log::debug!("Starting message loop");
        loop {
            let mut msg: winuser::MSG = unsafe { mem::zeroed() };
            match unsafe { winuser::GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } {
                1..=std::i32::MAX => unsafe {
                    winuser::TranslateMessage(&msg);
                    winuser::DispatchMessageW(&msg);
                },
                std::i32::MIN..=-1 => return Err(win32::last_error()),
                0 => {
                    log::debug!("Received WM_QUIT");
                    return Ok(());
                }
            }
        }
    }

    /// Signal that progress should be cancelled.
    pub fn cancel(&self) {
        if let Some(tx) = &self.cancel_sender {
            tx.send(()).unwrap_or_else(|_| {
                log::error!("Failed to send cancel signal");
            });
        }
    }

    /// Close main window.
    pub fn close(&self) {
        unsafe { winuser::PostMessageW(self.hwnd, winuser::WM_CLOSE, 0, 0) };
    }

    /// Create child control windows.
    fn create_window_controls(&mut self) -> Result<(), Error> {
        use winuser::*;
        let instance = unsafe { GetWindowLongPtrW(self.hwnd, GWLP_HINSTANCE) as win::HINSTANCE };
        self.font = Font::new_caption(20)?;
        // init common controls
        let icex = commctrl::INITCOMMONCONTROLSEX {
            dwSize: mem::size_of::<commctrl::INITCOMMONCONTROLSEX>() as u32,
            dwICC: commctrl::ICC_PROGRESS_CLASS,
        };
        unsafe { commctrl::InitCommonControlsEx(&icex) };
        // progress bar
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wcstring(commctrl::PROGRESS_CLASS).as_ptr(), ptr::null_mut(),
            WS_CHILD | WS_VISIBLE | commctrl::PBS_MARQUEE,
            0, 0, 0, 0, self.hwnd,
            Control::ProgressBar as u16 as _, instance, ptr::null_mut(),
        ) };
        unsafe { SendMessageW(hwnd, commctrl::PBM_SETRANGE32, 0, self.high_limit as _) };
        unsafe { SendMessageW(hwnd, commctrl::PBM_SETMARQUEE, 1, 0) };
        // static message area
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wchz!("STATIC").as_ptr(), ptr::null_mut(),
            SS_CENTER | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.hwnd,
            Control::Message as u16 as _, instance, ptr::null_mut(),
        ) };
        Self::set_window_font(hwnd, &self.font);
        // static title
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wchz!("STATIC").as_ptr(), ptr::null_mut(),
            SS_CENTER | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.hwnd,
            Control::Title as u16 as _, instance, ptr::null_mut(),
        ) };
        Self::set_window_font(hwnd, &self.font);
        unsafe { SetWindowTextW(hwnd, wchz!("Converting paths...").as_ptr()) };
        Ok(())
    }

    /// Called when client was resized.
    fn on_resize(&self, width: i32, _height: i32) {
        self.move_control(Control::Title, 10, 10, width - 20, 20);
        self.move_control(Control::ProgressBar, 10, 40, width - 20, 30);
        self.move_control(Control::Message, 10, 80, width - 20, 20);
    }

    /// Move control relative to main window.
    fn move_control(&self, control: Control, x: i32, y: i32, width: i32, height: i32) {
        let hwnd = self.get_control_handle(control);
        unsafe { winuser::MoveWindow(hwnd, x, y, width, height, win::TRUE) };
    }

    /// Get window handle of given control.
    fn get_control_handle(&self, control: Control) -> HWND {
        unsafe { winuser::GetDlgItem(self.hwnd, control as i32) }
    }

    /// Set font to given window.
    fn set_window_font(hwnd: HWND, font: &Font) {
        unsafe {
            winuser::SendMessageW(hwnd, winuser::WM_SETFONT, font.handle as _, win::TRUE as _)
        };
    }

    /// Update controls to display given progress.
    fn update_progress(&mut self, current: usize, max: usize) {
        use commctrl::*;
        use winuser::*;
        log::debug!("Progress update: {}/{}", current, max);
        let msg = format!("{} / {}", current, max);
        unsafe {
            SetWindowTextW(
                self.get_control_handle(Control::Message),
                wcstring(msg).as_ptr(),
            )
        };
        if self.is_marquee_progress() {
            self.set_progress_to_range_mode();
        }
        let hwnd = self.get_control_handle(Control::ProgressBar);
        unsafe { SendMessageW(hwnd, PBM_SETPOS, current, 0) };
        // if done, close cancellation channel
        if current == max {
            self.cancel_sender.take();
        }
    }

    /// Check whether progress bar is in marquee mode.
    fn is_marquee_progress(&self) -> bool {
        let style = unsafe {
            winuser::GetWindowLongW(
                self.get_control_handle(Control::ProgressBar),
                winuser::GWL_STYLE,
            )
        } as u32;
        style & commctrl::PBS_MARQUEE != 0
    }

    /// Set progress bar to range mode.
    fn set_progress_to_range_mode(&self) {
        use commctrl::*;
        use winuser::*;
        let hwnd = self.get_control_handle(Control::ProgressBar);
        let mut style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
        style &= !PBS_MARQUEE;
        style |= PBS_SMOOTH;
        unsafe { SetWindowLongW(hwnd, GWL_STYLE, style as _) };
        unsafe { SendMessageW(hwnd, PBM_SETMARQUEE, 0, 0) };
    }
}

impl ProgressWindow {
    /// Check whether window class is registered.
    pub fn is_window_class_registered() -> bool {
        unsafe {
            let instance = libloaderapi::GetModuleHandleW(ptr::null_mut());
            let mut wc: winuser::WNDCLASSEXW = mem::zeroed();
            winuser::GetClassInfoExW(instance, WND_CLASS.as_ptr(), &mut wc) != 0
        }
    }

    /// Register window class.
    pub fn register_window_class() -> Result<(), Error> {
        use winuser::*;
        log::debug!("Registering {} window class", WND_CLASS.to_string_lossy());
        let instance = unsafe { libloaderapi::GetModuleHandleW(ptr::null_mut()) };
        let wc = WNDCLASSEXW {
            cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_OWNDC | CS_HREDRAW | CS_VREDRAW,
            hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
            lpfnWndProc: Some(window_proc_wrapper::<ProgressWindow>),
            hInstance: instance,
            lpszClassName: WND_CLASS.as_ptr(),
            hIcon: ptr::null_mut(),
            hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
            ..unsafe { mem::zeroed() }
        };
        if 0 == unsafe { RegisterClassExW(&wc) } {
            Err(win32::last_error())
        } else {
            Ok(())
        }
    }

    /// Unregister window class.
    pub fn unregister_window_class() {
        log::debug!("Unregistering {} window class", WND_CLASS.to_string_lossy());
        unsafe {
            let instance = libloaderapi::GetModuleHandleW(ptr::null_mut());
            winuser::UnregisterClassW(WND_CLASS.as_ptr(), instance);
        }
    }
}

trait WindowProc {
    /// Window procedure callback.
    ///
    /// If None is returned, underlying wrapper calls `DefWindowProcW`.
    fn window_proc(
        &mut self,
        hwnd: HWND,
        msg: win::UINT,
        wparam: win::WPARAM,
        lparam: win::LPARAM,
    ) -> Option<win::LRESULT>;
}

/// Window proc wrapper that manages the `&self` pointer to `ProgressWindow` object.
///
/// Must be `extern "system"` because the function is called by Windows.
extern "system" fn window_proc_wrapper<T: WindowProc>(
    hwnd: HWND,
    msg: win::UINT,
    wparam: win::WPARAM,
    lparam: win::LPARAM,
) -> win::LRESULT {
    use winuser::*;
    // get pointer to T from userdata
    let mut ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut T;
    // not yet set, initialize from CREATESTRUCT
    if ptr.is_null() && msg == WM_NCCREATE {
        let cs = unsafe { &*(lparam as LPCREATESTRUCTW) };
        ptr = cs.lpCreateParams as *mut T;
        log::debug!("Initialize window pointer {:p}", ptr);
        unsafe { errhandlingapi::SetLastError(0) };
        if 0 == unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as *const _ as basetsd::LONG_PTR)
        } && unsafe { errhandlingapi::GetLastError() } != 0
        {
            return win::FALSE as win::LRESULT;
        }
    }
    // call wrapped window proc
    if !ptr.is_null() {
        let this = unsafe { &mut *(ptr as *mut T) };
        if let Some(result) = this.window_proc(hwnd, msg, wparam, lparam) {
            return result;
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

impl WindowProc for ProgressWindow {
    fn window_proc(
        &mut self,
        hwnd: HWND,
        msg: win::UINT,
        wparam: win::WPARAM,
        lparam: win::LPARAM,
    ) -> Option<win::LRESULT> {
        use winuser::*;
        match msg {
            // https://docs.microsoft.com/en-us/windows/win32/winmsg/wm-nccreate
            WM_NCCREATE => {
                // store main window handle
                self.hwnd = hwnd;
                // WM_NCCREATE must be passed to DefWindowProc
                None
            }
            // https://docs.microsoft.com/en-us/windows/win32/winmsg/wm-create
            WM_CREATE => match self.create_window_controls() {
                Err(e) => {
                    log::error!("Failed to create window controls: {}", e);
                    Some(-1)
                }
                Ok(()) => Some(0),
            },
            // https://docs.microsoft.com/en-us/windows/win32/winmsg/wm-size
            WM_SIZE => {
                self.on_resize(
                    i32::from(win::LOWORD(lparam as u32)),
                    i32::from(win::HIWORD(lparam as u32)),
                );
                Some(0)
            }
            // https://docs.microsoft.com/en-us/windows/win32/winmsg/wm-getminmaxinfo
            WM_GETMINMAXINFO => {
                let mmi = unsafe { &mut *(lparam as LPMINMAXINFO) };
                mmi.ptMinTrackSize.x = MIN_WINDOW_SIZE.0;
                mmi.ptMinTrackSize.y = MIN_WINDOW_SIZE.1;
                Some(0)
            }
            // https://docs.microsoft.com/en-us/windows/win32/controls/wm-ctlcolorstatic
            WM_CTLCOLORSTATIC => {
                Some(unsafe { wingdi::GetStockObject(COLOR_WINDOW + 1) } as win::LPARAM)
            }
            // https://docs.microsoft.com/en-us/windows/win32/winmsg/wm-close
            WM_CLOSE => {
                self.cancel();
                unsafe { DestroyWindow(hwnd) };
                Some(0)
            }
            // https://docs.microsoft.com/en-us/windows/win32/winmsg/wm-destroy
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                Some(0)
            }
            WM_PROGRESS => {
                self.update_progress(wparam, lparam as _);
                Some(0)
            }
            _ => None,
        }
    }
}
