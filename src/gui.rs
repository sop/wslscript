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
use winapi::um::commctrl::*;
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
        let cs = &*(lparam as LPCREATESTRUCTW);
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
    current_ext_idx: i32,
}
impl Default for MainWindow {
    fn default() -> Self {
        Self {
            window: null_mut(),
            caption_font: Default::default(),
            current_ext_idx: -1,
        }
    }
}

#[derive(FromPrimitive, ToPrimitive, PartialEq)]
enum Control {
    StaticMsg = 100,    // message area
    StaticRegister,     // label for extension input
    EditExtension,      // input for extension
    BtnRegister,        // register button
    ListViewExtensions, // listview of registered extensions
}

#[derive(FromPrimitive, ToPrimitive, PartialEq)]
enum MenuItem {
    Unregister = 100,
    EditExtension,
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
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, class_name.as_ptr(), WideString::from(title).as_ptr(),
            WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX | WS_VISIBLE,
            CW_USEDEFAULT, CW_USEDEFAULT, 300, 300,
            null_mut(), null_mut(), instance, wnd_ptr as LPVOID) };
        if hwnd.is_null() {
            Err(last_error())?
        }
        Ok(wnd)
    }

    /// Run message loop.
    ///
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

    /// Create window controls.
    ///
    fn create_window_controls(&mut self) -> Result<(), Error> {
        let instance = unsafe { GetWindowLongW(self.window, GWL_HINSTANCE) as HINSTANCE };
        self.caption_font = Font::new_default_caption()?;
        // init common controls
        let icex = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES,
        };
        unsafe { InitCommonControlsEx(&icex) };
        // static message area
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, WideString::from("STATIC").as_ptr(), null_mut(),
            SS_CENTER | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.window,
            Control::StaticMsg.to_u16().unwrap() as HMENU, instance, null_mut(),
        ) };
        #[rustfmt::skip]
        unsafe { SendMessageW(hwnd, WM_SETFONT,
            self.caption_font.handle as WPARAM, W_TRUE as LPARAM) };
        // register button
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, WideString::from("BUTTON").as_ptr(),
            WideString::from("Register").as_ptr(),
            WS_TABSTOP | WS_VISIBLE | WS_CHILD | BS_DEFPUSHBUTTON,
            0, 0, 0, 0, self.window,
            Control::BtnRegister.to_u16().unwrap() as HMENU, instance, null_mut()
        ) };
        #[rustfmt::skip]
        unsafe { SendMessageW(hwnd, WM_SETFONT,
            self.caption_font.handle as WPARAM, W_TRUE as LPARAM) };
        // register label
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, WideString::from("STATIC").as_ptr(),
            WideString::from("Extension:").as_ptr(),
            SS_CENTERIMAGE | SS_RIGHT | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.window,
            Control::StaticRegister.to_u16().unwrap() as HMENU, instance, null_mut(),
        ) };
        #[rustfmt::skip]
        unsafe { SendMessageW(hwnd, WM_SETFONT,
            self.caption_font.handle as WPARAM, W_TRUE as LPARAM) };
        // extension input
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            WS_EX_CLIENTEDGE, WideString::from("EDIT").as_ptr(), null_mut(),
            ES_LEFT | ES_LOWERCASE | WS_CHILD | WS_VISIBLE | WS_BORDER,
            0, 0, 0, 0, self.window,
            Control::EditExtension.to_u16().unwrap() as HMENU, instance, null_mut(),
        ) };
        #[rustfmt::skip]
        unsafe { SendMessageW(hwnd, WM_SETFONT,
            self.caption_font.handle as WPARAM, W_TRUE as LPARAM) };
        // if no extensions are registered, set default value to input box
        if registry::query_registered_extensions()
            .unwrap_or_else(|_| vec![])
            .is_empty()
        {
            unsafe { SetWindowTextW(hwnd, WideString::from("sh").as_ptr()) };
        }
        // listview of registered extensions
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            LVS_EX_FULLROWSELECT | LVS_EX_GRIDLINES,
            WideString::from(WC_LISTVIEW).as_ptr(), null_mut(),
            WS_CHILD | WS_VISIBLE | WS_BORDER | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
            0, 0, 0, 0, self.window,
            Control::ListViewExtensions.to_u16().unwrap() as HMENU, instance, null_mut(),
        ) };
        #[rustfmt::skip]
        unsafe { SendMessageW(hwnd, WM_SETFONT,
            self.caption_font.handle as WPARAM, W_TRUE as LPARAM) };
        // insert columns
        let col = LV_COLUMNW {
            mask: LVCF_FMT | LVCF_WIDTH | LVCF_TEXT,
            fmt: LVCFMT_LEFT,
            cx: 50,
            pszText: WideString::from("Ext").as_mut_ptr(),
            ..unsafe { zeroed() }
        };
        unsafe { SendMessageW(hwnd, LVM_INSERTCOLUMNW, 0, &col as *const _ as LPARAM) };
        // insert items
        match registry::query_registered_extensions() {
            Ok(exts) => {
                for (i, ext) in exts.iter().enumerate() {
                    let lvi = LV_ITEMW {
                        mask: LVIF_TEXT,
                        iItem: i as i32,
                        pszText: WideString::from(ext.as_str()).as_mut_ptr(),
                        ..unsafe { zeroed() }
                    };
                    unsafe { SendMessageW(hwnd, LVM_INSERTITEMW, 0, &lvi as *const _ as LPARAM) };
                }
            }
            Err(e) => {
                error_message(WideString::from(format!("Failed to query registry: {}", e)));
            }
        }
        self.update_control_states();
        Ok(())
    }

    /// Update control states.
    ///
    fn update_control_states(&self) -> Option<()> {
        let hwnd_msg = self.get_control_handle(Control::StaticMsg);
        let msg: String = if self.current_ext_idx == -1 {
            "Enter the extension and click Register to associate a filetype with WSL.".to_string()
        } else {
            let hwnd = self.get_control_handle(Control::ListViewExtensions);
            let ext = Self::get_listview_item_text(hwnd, self.current_ext_idx as usize);
            format!(".{} extension is registered for WSL!", ext)
        };
        unsafe { SetWindowTextW(hwnd_msg, WideString::from(msg).as_ptr()) };
        Some(())
    }

    /// Handle WM_SIZE message.
    ///
    /// * `width` - Window width
    /// * `height` - Window height
    fn on_resize(&self, width: i32, _height: i32) {
        // static message
        let hwnd = self.get_control_handle(Control::StaticMsg);
        if !hwnd.is_null() {
            unsafe { MoveWindow(hwnd, 0, 10, width, 40, W_TRUE) };
        }
        // register button
        let hwnd = self.get_control_handle(Control::BtnRegister);
        if !hwnd.is_null() {
            unsafe { MoveWindow(hwnd, width - 100, 50, 90, 25, W_TRUE) };
        }
        // register label
        let hwnd = self.get_control_handle(Control::StaticRegister);
        if !hwnd.is_null() {
            unsafe { MoveWindow(hwnd, 10, 50, 60, 25, W_TRUE) };
        }
        // register input
        let hwnd = self.get_control_handle(Control::EditExtension);
        if !hwnd.is_null() {
            unsafe { MoveWindow(hwnd, 80, 50, width - 90 - 100, 25, W_TRUE) };
        }
        // extensions listview
        let hwnd = self.get_control_handle(Control::ListViewExtensions);
        if !hwnd.is_null() {
            unsafe { MoveWindow(hwnd, 10, 100, width - 20, 80, W_TRUE) };
        }
    }

    /// Handle WM_COMMAND message from a control.
    ///
    /// * `hwnd` - Handle of the sending control
    /// * `control_id` - ID of the sending control
    /// * `code` - Notification code
    fn on_control(&mut self, _hwnd: HWND, control_id: Control, _code: WORD) -> LRESULT {
        if let Control::BtnRegister = control_id {
            let ext = self
                .get_extension_input_text()
                .trim_matches('.')
                .to_string();
            if ext.is_empty() {
                return 0;
            }
            match registry::is_registered_for_other(&ext) {
                Err(e) => {
                    error_message(WideString::from(format!(
                        "Failed to register extension: {}",
                        e
                    )));
                    return 0;
                }
                Ok(true) => {
                    let result = unsafe {
                        MessageBoxW(
                            self.window,
                            WideString::from(format!(
                                ".{} extension is already registered for another application.\nRegister anyway?",
                                ext
                            ))
                            .as_ptr(),
                            WideString::from("Confirm extension registration.").as_ptr(),
                            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
                        )
                    };
                    if result == IDNO {
                        return 0;
                    }
                }
                Ok(false) => {}
            }
            if let Err(e) = registry::register_extension(&ext) {
                error_message(WideString::from(format!(
                    "Failed to register extension: {}",
                    e
                )));
            }
            // insert to listview
            let hwnd = self.get_control_handle(Control::ListViewExtensions);
            let lvi = LV_ITEMW {
                mask: LVIF_TEXT,
                iItem: 0,
                pszText: WideString::from(ext.as_str()).as_mut_ptr(),
                ..unsafe { zeroed() }
            };
            let idx = unsafe { SendMessageW(hwnd, LVM_INSERTITEMW, 0, &lvi as *const _ as LPARAM) };
            self.current_ext_idx = idx as i32;
            // clear extension input
            unsafe {
                SetDlgItemTextW(
                    self.window,
                    Control::EditExtension.to_i32().unwrap(),
                    WideString::from("").as_ptr(),
                )
            };
            self.update_control_states();
        }
        0
    }

    /// Handle message from a menu.
    ///
    /// * `hmenu` - Handle to the menu
    /// * `item_id` - ID of the clicked menu item
    fn on_menucommand(&mut self, hmenu: HMENU, item_id: MenuItem) -> LRESULT {
        match item_id {
            MenuItem::Unregister => {
                let mut mi = MENUINFO {
                    cbSize: size_of::<MENUINFO>() as u32,
                    fMask: MIM_MENUDATA,
                    ..unsafe { zeroed() }
                };
                unsafe { GetMenuInfo(hmenu, &mut mi) };
                let hwnd = self.get_control_handle(Control::ListViewExtensions);
                let idx = mi.dwMenuData as usize;
                let ext = Self::get_listview_item_text(hwnd, idx);
                if let Err(e) = registry::unregister_extension(&ext) {
                    error_message(WideString::from(format!(
                        "Failed to unregister extension: {}",
                        e
                    )));
                    return 0;
                }
                unsafe { SendMessageW(hwnd, LVM_DELETEITEM, idx, 0) };
                self.current_ext_idx = -1;
                self.update_control_states();
            }
            MenuItem::EditExtension => {}
        }
        0
    }

    /// Handle WM_NOTIFY message.
    ///
    /// * `hwnd` - Handle of the sending control
    /// * `control_id` - ID of the sending control
    /// * `code` - Notification code
    /// * `lparam` - Notification specific parameter
    fn on_notify(
        &mut self,
        hwnd: HWND,
        control_id: Control,
        code: u32,
        lparam: *const isize,
    ) -> LRESULT {
        #![allow(clippy::single_match)]
        match control_id {
            Control::ListViewExtensions => match code {
                // when listview item is activated (eg. double clicked)
                LVN_ITEMACTIVATE => {
                    let nmia = unsafe { &*(lparam as LPNMITEMACTIVATE) };
                    if nmia.iItem < 0 {
                        return 0;
                    }
                    self.current_ext_idx = nmia.iItem;
                    self.update_control_states();
                }
                // when listview item is right-clicked
                NM_RCLICK => {
                    let nmia = unsafe { &*(lparam as LPNMITEMACTIVATE) };
                    if nmia.iItem < 0 {
                        return 0;
                    }
                    let hmenu = unsafe { CreatePopupMenu() };
                    let mi = MENUINFO {
                        cbSize: size_of::<MENUINFO>() as u32,
                        fMask: MIM_MENUDATA | MIM_STYLE,
                        dwStyle: MNS_NOTIFYBYPOS,
                        dwMenuData: nmia.iItem as usize,
                        ..unsafe { zeroed() }
                    };
                    unsafe { SetMenuInfo(hmenu, &mi) };
                    let mut mii = MENUITEMINFOW {
                        cbSize: size_of::<MENUITEMINFOW>() as u32,
                        fMask: MIIM_TYPE | MIIM_ID,
                        fType: MFT_STRING,
                        ..unsafe { zeroed() }
                    };
                    mii.wID = MenuItem::EditExtension.to_u32().unwrap();
                    mii.dwTypeData = WideString::from("Edit").as_mut_ptr();
                    unsafe { InsertMenuItemW(hmenu, 0, W_TRUE, &mii) };
                    mii.wID = MenuItem::Unregister.to_u32().unwrap();
                    mii.dwTypeData = WideString::from("Unregister").as_mut_ptr();
                    unsafe { InsertMenuItemW(hmenu, 1, W_TRUE, &mii) };
                    let mut pos: POINT = nmia.ptAction;
                    unsafe { ClientToScreen(hwnd, &mut pos) };
                    unsafe { TrackPopupMenuEx(hmenu, 0, pos.x, pos.y, self.window, null_mut()) };
                }
                _ => {}
            },
            _ => {}
        }
        0
    }

    fn get_listview_item_text(hwnd: HWND, index: usize) -> String {
        let mut buf: Vec<u16> = Vec::with_capacity(32);
        let lvi = LV_ITEMW {
            pszText: buf.as_mut_ptr(),
            cchTextMax: buf.capacity() as i32,
            ..unsafe { zeroed() }
        };
        unsafe {
            let len = SendMessageW(hwnd, LVM_GETITEMTEXTW, index, &lvi as *const _ as isize);
            buf.set_len(len as usize);
        };
        WideString::from(buf.as_slice()).to_string()
    }

    fn get_control_handle(&self, control: Control) -> HWND {
        unsafe { GetDlgItem(self.window, control.to_i32().unwrap()) }
    }

    fn get_extension_input_text(&self) -> String {
        let mut buf: Vec<u16> = Vec::with_capacity(32);
        unsafe {
            let len = GetDlgItemTextW(
                self.window,
                Control::EditExtension.to_i32().unwrap(),
                buf.as_mut_ptr(),
                buf.capacity() as i32,
            );
            buf.set_len(len as usize);
        }
        WideString::from(buf.as_slice()).to_string()
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
                None
            }
            WM_CREATE => {
                if self.create_window_controls().is_err() {
                    return Some(-1);
                }
                Some(0)
            }
            WM_SIZE => {
                self.on_resize(
                    i32::from(LOWORD(lparam as u32)),
                    i32::from(HIWORD(lparam as u32)),
                );
                Some(0)
            }
            WM_GETMINMAXINFO => {
                let mmi = unsafe { &mut *(lparam as LPMINMAXINFO) };
                mmi.ptMinTrackSize.x = 300;
                mmi.ptMinTrackSize.y = 300;
                Some(0)
            }
            WM_CTLCOLORSTATIC => Some(unsafe { GetStockObject(COLOR_WINDOW + 1 as i32) } as LPARAM),
            WM_COMMAND => {
                // if lParam is non-zero, message is from a control
                if lparam != 0 {
                    if let Some(id) = FromPrimitive::from_u16(LOWORD(wparam as u32)) {
                        return Some(self.on_control(lparam as HWND, id, HIWORD(wparam as u32)));
                    }
                }
                // if lParam is zero and HIWORD of wParam is zero, message is from a menu
                else if HIWORD(wparam as u32) == 0 {
                    if let Some(id) = FromPrimitive::from_u16(LOWORD(wparam as u32)) {
                        return Some(self.on_menucommand(null_mut(), id));
                    }
                }
                None
            }
            WM_MENUCOMMAND => {
                let hmenu = lparam as HMENU;
                let item_id = unsafe { GetMenuItemID(hmenu, wparam as i32) };
                if let Some(id) = FromPrimitive::from_u32(item_id) {
                    return Some(self.on_menucommand(hmenu, id));
                }
                None
            }
            WM_NOTIFY => {
                let hdr = unsafe { &*(lparam as LPNMHDR) };
                if let Some(id) = FromPrimitive::from_usize(hdr.idFrom) {
                    return Some(self.on_notify(hdr.hwndFrom, id, hdr.code, lparam as *const _));
                }
                None
            }
            WM_CLOSE => {
                unsafe { DestroyWindow(hwnd) };
                Some(0)
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                Some(0)
            }
            _ => None,
        }
    }
}

struct Font {
    handle: HFONT,
}
impl Font {
    fn new_default_caption() -> Result<Self, Error> {
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
        let font = unsafe { CreateFontIndirectW(&metrics.lfCaptionFont) };
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

fn last_error() -> Error {
    let msg: String;
    let mut buf = [0 as WCHAR; 2048];
    let errno = unsafe { GetLastError() };
    let res = unsafe {
        FormatMessageW(
            FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
            null_mut(),
            errno,
            DWORD::from(MAKELANGID(LANG_NEUTRAL, SUBLANG_DEFAULT)),
            buf.as_mut_ptr(),
            buf.len() as DWORD,
            null_mut(),
        )
    };
    if res == 0 {
        msg = format!("Error code {}", errno).to_string();
    } else {
        msg = WideString::from(&buf[..(res + 1) as usize]).to_string();
    }
    Error::from(ErrorKind::WinAPIError { s: msg })
}
