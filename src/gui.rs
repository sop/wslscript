use crate::error::*;
use crate::font::Font;
use crate::icon::ShellIcon;
use crate::registry;
use crate::wcstr;
use crate::win32::*;
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::{FromPrimitive, ToPrimitive};
use std::mem::{size_of, zeroed};
use std::pin::Pin;
use std::ptr::null_mut;
use wchar::*;
use widestring::*;
use winapi::shared::basetsd::*;
use winapi::shared::minwindef::{self as win, *};
use winapi::shared::ntdef::*;
use winapi::shared::windef::*;
use winapi::um::commctrl::*;
use winapi::um::errhandlingapi::{GetLastError, SetLastError};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::wingdi::*;
use winapi::um::winuser::*;

extern "system" {
    /// PickIconDlg() prototype
    /// https://docs.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-pickicondlg
    pub fn PickIconDlg(
        hwnd: HWND,
        pszIconPath: PWSTR,
        cchIconPath: UINT,
        piIconIndex: *mut std::os::raw::c_int,
    ) -> std::os::raw::c_int;
}

pub fn start_gui() -> Result<(), Error> {
    let wnd = MainWindow::new(wch_c!("WSL Script"))?;
    wnd.run()
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
extern "system" fn window_proc_wrapper<T: WindowProc>(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // get pointer to T from userdata
    let mut ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut T;
    // not yet set, initialize from CREATESTRUCT
    if ptr.is_null() && msg == WM_NCCREATE {
        let cs = unsafe { &*(lparam as LPCREATESTRUCTW) };
        ptr = cs.lpCreateParams as *mut T;
        unsafe { SetLastError(0) };
        if 0 == unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as *const _ as LONG_PTR) }
            && unsafe { GetLastError() } != 0
        {
            return win::FALSE as LRESULT;
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

struct MainWindow {
    window: HWND,
    caption_font: Font,
    ext_font: Font,
    current_ext_idx: isize,
    current_ext_cfg: Option<registry::ExtConfig>,
}
impl Default for MainWindow {
    fn default() -> Self {
        Self {
            window: null_mut(),
            caption_font: Default::default(),
            ext_font: Default::default(),
            current_ext_idx: -1,
            current_ext_cfg: None,
        }
    }
}

#[derive(FromPrimitive, ToPrimitive, PartialEq)]
enum Control {
    StaticMsg = 100,    // message area
    RegisterLabel,      // label for extension input
    EditExtension,      // input for extension
    BtnRegister,        // register button
    ListViewExtensions, // listview of registered extensions
    StaticIcon,         // icon for extension
    IconLabel,          // label for icon
    HoldModeCombo,      // combo box for hold mode
    HoldModeLabel,      // label for hold mode
    BtnSave,            // Save button
}

#[derive(FromPrimitive, ToPrimitive, PartialEq)]
enum MenuItem {
    Unregister = 100,
    EditExtension,
}

impl MainWindow {
    /// Create application window.
    ///
    fn new(title: &[WCHAR]) -> Result<Pin<Box<Self>>, Error> {
        let wnd = Pin::new(Box::new(Self::default()));
        let instance = unsafe { GetModuleHandleW(null_mut()) };
        let class_name = wch_c!("WSLScript");
        // register window class
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_OWNDC | CS_HREDRAW | CS_VREDRAW,
            hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
            lpfnWndProc: Some(window_proc_wrapper::<MainWindow>),
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            hIcon: unsafe { LoadIconW(instance, wch_c!("app").as_ptr()) },
            hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
            ..unsafe { zeroed() }
        };
        if 0 == unsafe { RegisterClassExW(&wc) } {
            return Err(last_error());
        }
        // create window
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, class_name.as_ptr(), title.as_ptr(),
            WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX | WS_VISIBLE,
            CW_USEDEFAULT, CW_USEDEFAULT, 300, 300,
            null_mut(), null_mut(), instance, &*wnd as *const Self as LPVOID) };
        if hwnd.is_null() {
            return Err(last_error());
        }
        Ok(wnd)
    }

    /// Run message loop.
    ///
    fn run(&self) -> Result<(), Error> {
        loop {
            let mut msg: MSG = unsafe { zeroed() };
            match unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) } {
                1..=std::i32::MAX => {
                    unsafe { TranslateMessage(&msg) };
                    unsafe { DispatchMessageW(&msg) };
                }
                std::i32::MIN..=-1 => return Err(last_error()),
                0 => break,
            }
        }
        Ok(())
    }

    /// Create window controls.
    ///
    fn create_window_controls(&mut self) -> Result<(), Error> {
        let instance = unsafe { GetWindowLongW(self.window, GWL_HINSTANCE) as HINSTANCE };
        self.caption_font = Font::new_default_caption()?;
        self.ext_font = Font::new_caption(24)?;
        // init common controls
        let icex = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES,
        };
        unsafe { InitCommonControlsEx(&icex) };

        // static message area
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wch_c!("STATIC").as_ptr(), null_mut(),
            SS_CENTER | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.window,
            Control::StaticMsg.to_u16().unwrap() as HMENU, instance, null_mut(),
        ) };
        Self::set_window_font(hwnd, &self.caption_font);

        // register button
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wch_c!("BUTTON").as_ptr(), wch_c!("Register").as_ptr(),
            WS_TABSTOP | WS_VISIBLE | WS_CHILD | BS_DEFPUSHBUTTON,
            0, 0, 0, 0, self.window,
            Control::BtnRegister.to_u16().unwrap() as HMENU, instance, null_mut()
        ) };
        Self::set_window_font(hwnd, &self.caption_font);

        // register label
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wch_c!("STATIC").as_ptr(), wch_c!("Extension:").as_ptr(),
            SS_CENTERIMAGE | SS_RIGHT | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.window,
            Control::RegisterLabel.to_u16().unwrap() as HMENU, instance, null_mut(),
        ) };
        Self::set_window_font(hwnd, &self.caption_font);

        // extension input
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            WS_EX_CLIENTEDGE, wch_c!("EDIT").as_ptr(), null_mut(),
            ES_LEFT | ES_LOWERCASE | WS_CHILD | WS_VISIBLE | WS_BORDER,
            0, 0, 0, 0, self.window,
            Control::EditExtension.to_u16().unwrap() as HMENU, instance, null_mut(),
        ) };
        Self::set_window_font(hwnd, &self.caption_font);
        let self_ptr = self as *const _;
        unsafe { SetWindowSubclass(hwnd, Some(extension_input_proc), 0, self_ptr as DWORD_PTR) };
        // if no extensions are registered, set default value to input box
        if registry::query_registered_extensions()
            .unwrap_or_else(|_| vec![])
            .is_empty()
        {
            unsafe { SetWindowTextW(hwnd, wch_c!("sh").as_ptr()) };
        }

        // listview of registered extensions
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            LVS_EX_FULLROWSELECT | LVS_EX_GRIDLINES,
            wcstr!(WC_LISTVIEW).as_ptr(), null_mut(),
            WS_CHILD | WS_VISIBLE | WS_BORDER | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
            0, 0, 0, 0, self.window,
            Control::ListViewExtensions.to_u16().unwrap() as HMENU, instance, null_mut(),
        ) };
        Self::set_window_font(hwnd, &self.caption_font);
        // insert columns
        let col = LV_COLUMNW {
            mask: LVCF_FMT | LVCF_WIDTH | LVCF_TEXT,
            fmt: LVCFMT_LEFT,
            cx: 50,
            pszText: wch_c!("Ext").as_ptr() as _,
            ..unsafe { zeroed() }
        };
        unsafe { SendMessageW(hwnd, LVM_INSERTCOLUMNW, 0, &col as *const _ as LPARAM) };
        // insert items
        match registry::query_registered_extensions() {
            Ok(exts) => {
                for (i, ext) in exts.iter().enumerate() {
                    let s = wcstr!(ext);
                    let lvi = LV_ITEMW {
                        mask: LVIF_TEXT,
                        iItem: i as i32,
                        pszText: s.into_raw(),
                        ..unsafe { zeroed() }
                    };
                    unsafe { SendMessageW(hwnd, LVM_INSERTITEMW, 0, &lvi as *const _ as LPARAM) };
                }
            }
            Err(e) => {
                let s = wcstr!(format!("Failed to query registry: {}", e));
                error_message(&s);
            }
        }

        // extension icon
        #[rustfmt::skip]
        unsafe { CreateWindowExW(
            0, wch_c!("STATIC").as_ptr(), null_mut(),
            SS_ICON | SS_CENTERIMAGE | SS_NOTIFY | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.window,
            Control::StaticIcon.to_u16().unwrap() as HMENU, instance, null_mut(),
        ) };

        // icon label
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wch_c!("STATIC").as_ptr(), wch_c!("Icon").as_ptr(),
            SS_CENTER | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.window,
            Control::IconLabel.to_u16().unwrap() as HMENU, instance, null_mut()
        ) };
        Self::set_window_font(hwnd, &self.caption_font);

        // hold mode combo box
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wch_c!("COMBOBOX").as_ptr(), null_mut(),
            CBS_DROPDOWNLIST | WS_VSCROLL | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.window,
            Control::HoldModeCombo.to_u16().unwrap() as HMENU, instance, null_mut()
        ) };
        Self::set_window_font(hwnd, &self.caption_font);
        let insert_item = |mode: registry::HoldMode, label: &WideCStr| {
            let idx = unsafe {
                SendMessageW(
                    hwnd,
                    CB_INSERTSTRING,
                    -1_isize as WPARAM,
                    label.as_ptr() as LPARAM,
                )
            };
            let s = mode.as_wcstr();
            unsafe { SendMessageW(hwnd, CB_SETITEMDATA, idx as WPARAM, s.as_ptr() as LPARAM) };
        };
        insert_item(
            registry::HoldMode::Error,
            WideCStr::from_slice_with_nul(wch_c!("Close on success"))?,
        );
        insert_item(
            registry::HoldMode::Never,
            WideCStr::from_slice_with_nul(wch_c!("Always close"))?,
        );
        insert_item(
            registry::HoldMode::Always,
            WideCStr::from_slice_with_nul(wch_c!("Keep open"))?,
        );

        // hold mode label
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wch_c!("STATIC").as_ptr(), wch_c!("Exit behaviour").as_ptr(),
            SS_CENTER | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.window,
            Control::HoldModeLabel.to_u16().unwrap() as HMENU, instance, null_mut()
        ) };
        Self::set_window_font(hwnd, &self.caption_font);

        // save button
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wch_c!("BUTTON").as_ptr(), wch_c!("Save").as_ptr(),
            WS_TABSTOP | WS_VISIBLE | WS_CHILD | BS_DEFPUSHBUTTON,
            0, 0, 0, 0, self.window,
            Control::BtnSave.to_u16().unwrap() as HMENU, instance, null_mut()
        ) };
        Self::set_window_font(hwnd, &self.caption_font);
        self.update_control_states();
        Ok(())
    }

    fn set_window_font(hwnd: HWND, font: &Font) {
        unsafe { SendMessageW(hwnd, WM_SETFONT, font.handle as WPARAM, win::TRUE as LPARAM) };
    }

    /// Update control states.
    ///
    fn update_control_states(&self) {
        // set message
        let hwnd = self.get_control_handle(Control::StaticMsg);
        if let Some(mut ext) = self.get_current_extension() {
            // if extension is registered for WSL, but handler is in another directory
            if !registry::is_registered_for_current_executable(&ext).unwrap_or(true) {
                let exe_os = std::env::current_exe().unwrap_or_default();
                let exe_name = exe_os.file_name().unwrap_or_default().to_string_lossy();
                let s = wcstr!(format!(
                    ".{} handler found from another directory!\n\
                     Did you move {}?",
                    ext, exe_name
                ));
                unsafe { SetWindowTextW(hwnd, s.as_ptr()) };
                Self::set_window_font(hwnd, &self.caption_font);
            } else {
                ext.insert_str(0, ".");
                unsafe { SetWindowTextW(hwnd, wcstr!(ext).as_ptr()) };
                Self::set_window_font(hwnd, &self.ext_font);
            }
        } else {
            let s = wch_c!(
                "Enter the extension and click \
                 Register to associate a filetype with WSL."
            );
            unsafe { SetWindowTextW(hwnd, s.as_ptr()) };
            Self::set_window_font(hwnd, &self.caption_font);
        };
        let visibility = if self.current_ext_cfg.is_some() {
            SW_SHOW
        } else {
            SW_HIDE
        };
        // hold mode label
        unsafe { ShowWindow(self.get_control_handle(Control::HoldModeLabel), visibility) };
        // hold mode combo
        unsafe { ShowWindow(self.get_control_handle(Control::HoldModeCombo), visibility) };
        if let Some(mode) = self
            .current_ext_cfg
            .as_ref()
            .and_then(|cfg| Some(cfg.hold_mode))
        {
            self.set_selected_hold_mode(mode);
        }
        // set icon
        let hwnd = self.get_control_handle(Control::StaticIcon);
        unsafe { ShowWindow(hwnd, visibility) };
        if let Some(icon) = self
            .current_ext_cfg
            .as_ref()
            .and_then(|cfg| cfg.icon.as_ref())
        {
            unsafe { SendMessageW(hwnd, STM_SETICON, icon.handle() as WPARAM, 0) };
        } else {
            unsafe { SendMessageW(hwnd, STM_SETICON, 0, 0) };
        }
        // icon label
        unsafe { ShowWindow(self.get_control_handle(Control::IconLabel), visibility) };
        // save button
        unsafe { ShowWindow(self.get_control_handle(Control::BtnSave), visibility) };
    }

    /// Handle WM_SIZE message.
    ///
    /// * `width` - Window width
    /// * `height` - Window height
    fn on_resize(&self, width: i32, _height: i32) {
        // static message
        let hwnd = self.get_control_handle(Control::StaticMsg);
        unsafe { MoveWindow(hwnd, 10, 10, width - 20, 40, win::TRUE) };
        // register label
        let hwnd = self.get_control_handle(Control::RegisterLabel);
        unsafe { MoveWindow(hwnd, 10, 50, 60, 25, win::TRUE) };
        // register input
        let hwnd = self.get_control_handle(Control::EditExtension);
        unsafe { MoveWindow(hwnd, 80, 50, width - 90 - 100, 25, win::TRUE) };
        // register button
        let hwnd = self.get_control_handle(Control::BtnRegister);
        unsafe { MoveWindow(hwnd, width - 100, 50, 90, 25, win::TRUE) };
        // extensions listview
        let hwnd = self.get_control_handle(Control::ListViewExtensions);
        unsafe { MoveWindow(hwnd, 10, 85, width - 20, 75, win::TRUE) };
        // hold mode label
        let hwnd = self.get_control_handle(Control::HoldModeLabel);
        unsafe { MoveWindow(hwnd, 10, 170, 130, 20, win::TRUE) };
        // hold mode combo box
        let hwnd = self.get_control_handle(Control::HoldModeCombo);
        unsafe { MoveWindow(hwnd, 10, 190, 130, 100, win::TRUE) };
        // icon label
        let hwnd = self.get_control_handle(Control::IconLabel);
        unsafe { MoveWindow(hwnd, 150, 170, 32, 16, win::TRUE) };
        // static icon
        let hwnd = self.get_control_handle(Control::StaticIcon);
        unsafe { MoveWindow(hwnd, 150, 186, 32, 32, win::TRUE) };
        // save button
        let hwnd = self.get_control_handle(Control::BtnSave);
        unsafe { MoveWindow(hwnd, width - 90, 188, 80, 25, win::TRUE) };
    }

    /// Handle WM_COMMAND message from a control.
    ///
    /// * `hwnd` - Handle of the sending control
    /// * `control_id` - ID of the sending control
    /// * `code` - Notification code
    fn on_control(
        &mut self,
        _hwnd: HWND,
        control_id: Control,
        code: WORD,
    ) -> Result<LRESULT, Error> {
        #[allow(clippy::single_match)]
        match control_id {
            Control::BtnRegister => match code {
                BN_CLICKED => return self.on_register_button_clicked(),
                _ => {}
            },
            Control::HoldModeCombo => match code {
                CBN_SELCHANGE => {
                    if let Some(mode) = self.get_selected_hold_mode() {
                        if let Some(cfg) = &mut self.current_ext_cfg {
                            cfg.hold_mode = mode;
                        }
                    }
                }
                _ => {}
            },
            Control::StaticIcon => match code {
                STN_DBLCLK => {
                    if let Some(icon) = self.pick_icon_dlg() {
                        if let Some(cfg) = &mut self.current_ext_cfg {
                            cfg.icon = Some(icon);
                        }
                        self.update_control_states();
                    }
                }
                _ => {}
            },
            Control::BtnSave => match code {
                BN_CLICKED => return self.on_save_button_clicked(),
                _ => {}
            },
            _ => {}
        }
        Ok(0)
    }

    /// Handle register button click.
    ///
    fn on_register_button_clicked(&mut self) -> Result<LRESULT, Error> {
        let ext = self
            .get_extension_input_text()
            .trim_matches('.')
            .to_string();
        if ext.is_empty() {
            return Ok(0);
        }
        if registry::is_registered_for_other(&ext)? {
            let s = wcstr!(format!(
                ".{} extension is already registered for another application.\n\
                 Register anyway?",
                ext
            ));
            let result = unsafe {
                MessageBoxW(
                    self.window,
                    s.as_ptr(),
                    wch_c!("Confirm extension registration.").as_ptr(),
                    MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
                )
            };
            if result == IDNO {
                return Ok(0);
            }
        }
        let icon = ShellIcon::load_default()?;
        let config = registry::ExtConfig {
            extension: ext.clone(),
            icon: Some(icon),
            hold_mode: registry::HoldMode::Error,
        };
        registry::register_extension(&config)?;
        // clear extension input
        unsafe {
            SetDlgItemTextW(
                self.window,
                Control::EditExtension.to_i32().unwrap(),
                wch_c!("").as_ptr(),
            )
        };
        let idx = self.listview_find_ext(&ext).or_else(|| {
            // insert to listview
            let hwnd = self.get_control_handle(Control::ListViewExtensions);
            let s = wcstr!(ext);
            let lvi = LV_ITEMW {
                mask: LVIF_TEXT,
                iItem: 0,
                pszText: s.as_ptr() as _,
                ..unsafe { zeroed() }
            };
            let result =
                unsafe { SendMessageW(hwnd, LVM_INSERTITEMW, 0, &lvi as *const _ as LPARAM) };
            Some(result as usize)
        });
        let i = match idx {
            Some(i) => i as isize,
            None => -1,
        };
        self.set_current_extension(i);
        self.update_control_states();
        Ok(0)
    }

    /// Handle save button click.
    ///
    fn on_save_button_clicked(&mut self) -> Result<LRESULT, Error> {
        if let Some(config) = self.current_ext_cfg.as_ref() {
            registry::register_extension(config)?
        }
        Ok(0)
    }

    /// Handle message from a menu.
    ///
    /// * `hmenu` - Handle to the menu
    /// * `item_id` - ID of the clicked menu item
    fn on_menucommand(&mut self, hmenu: HMENU, item_id: MenuItem) -> LRESULT {
        match item_id {
            MenuItem::Unregister => {
                let idx: usize = self.get_menu_data(hmenu);
                if let Some(ext) = self.get_listview_item_text(idx) {
                    if let Err(e) = registry::unregister_extension(&ext) {
                        let s = wcstr!(format!("Failed to unregister extension: {}", e));
                        error_message(&s);
                        return 0;
                    }
                }
                let hwnd = self.get_control_handle(Control::ListViewExtensions);
                unsafe { SendMessageW(hwnd, LVM_DELETEITEM, idx, 0) };
                self.set_current_extension(-1);
                self.update_control_states();
            }
            MenuItem::EditExtension => {
                let idx: usize = self.get_menu_data(hmenu);
                self.set_current_extension(idx as isize);
                self.update_control_states();
            }
        }
        0
    }

    /// Get application-defined value associated with a menu
    ///
    fn get_menu_data<T>(&self, hmenu: HMENU) -> T
    where
        T: From<winapi::shared::basetsd::ULONG_PTR>,
    {
        let mut mi = MENUINFO {
            cbSize: size_of::<MENUINFO>() as u32,
            fMask: MIM_MENUDATA,
            ..unsafe { zeroed() }
        };
        unsafe { GetMenuInfo(hmenu, &mut mi) };
        T::from(mi.dwMenuData)
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
        #[allow(clippy::single_match)]
        match control_id {
            Control::ListViewExtensions => match code {
                // when listview item is activated (eg. double clicked)
                LVN_ITEMACTIVATE => {
                    let nmia = unsafe { &*(lparam as LPNMITEMACTIVATE) };
                    if nmia.iItem < 0 {
                        return 0;
                    }
                    self.set_current_extension(nmia.iItem as isize);
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
                    mii.dwTypeData = wch_c!("Edit").as_ptr() as _;
                    unsafe { InsertMenuItemW(hmenu, 0, win::TRUE, &mii) };
                    mii.wID = MenuItem::Unregister.to_u32().unwrap();
                    mii.dwTypeData = wch_c!("Unregister").as_ptr() as _;
                    unsafe { InsertMenuItemW(hmenu, 1, win::TRUE, &mii) };
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

    /// Get currently selected extension.
    ///
    fn get_current_extension(&self) -> Option<String> {
        if self.current_ext_idx == -1 {
            return None;
        }
        self.get_listview_item_text(self.current_ext_idx as usize)
    }

    /// Get listview text by index.
    ///
    fn get_listview_item_text(&self, index: usize) -> Option<String> {
        let mut buf: Vec<WCHAR> = Vec::with_capacity(32);
        let lvi = LV_ITEMW {
            pszText: buf.as_mut_ptr(),
            cchTextMax: buf.capacity() as i32,
            ..unsafe { zeroed() }
        };
        let hwnd = self.get_control_handle(Control::ListViewExtensions);
        unsafe {
            let len = SendMessageW(hwnd, LVM_GETITEMTEXTW, index, &lvi as *const _ as LPARAM);
            buf.set_len(len as usize);
        };
        WideCString::new(buf).ok().map(|u| u.to_string_lossy())
    }

    /// Find extension from listview.
    ///
    /// Returns listview index or None if extension wasn't found.
    fn listview_find_ext(&self, ext: &str) -> Option<usize> {
        let s = wcstr!(ext);
        let lvf = LVFINDINFOW {
            flags: LVFI_STRING,
            psz: s.as_ptr(),
            ..unsafe { zeroed() }
        };
        let hwnd = self.get_control_handle(Control::ListViewExtensions);
        let idx = unsafe {
            SendMessageW(
                hwnd,
                LVM_FINDITEMW,
                -1_isize as usize,
                &lvf as *const _ as LPARAM,
            )
        };
        match idx {
            -1 => None,
            _ => Some(idx as usize),
        }
    }

    /// Get window handle to control.
    ///
    fn get_control_handle(&self, control: Control) -> HWND {
        unsafe { GetDlgItem(self.window, control.to_i32().unwrap()) }
    }

    /// Get text from extension text input.
    ///
    fn get_extension_input_text(&self) -> String {
        let mut buf: Vec<WCHAR> = Vec::with_capacity(32);
        unsafe {
            let len = GetDlgItemTextW(
                self.window,
                Control::EditExtension.to_i32().unwrap(),
                buf.as_mut_ptr(),
                buf.capacity() as i32,
            );
            buf.set_len(len as usize);
        }
        WideCString::new(buf).unwrap().to_string_lossy()
    }

    /// Set extension that is currently selected for edit.
    ///
    fn set_current_extension(&mut self, listview_idx: isize) {
        self.current_ext_idx = listview_idx;
        self.current_ext_cfg = self
            .get_current_extension()
            .and_then(|ext| registry::get_extension_config(&ext).ok());
    }

    /// Launch icon picker dialog.
    ///
    /// Returns ShellIcon or None if no icon was selected.
    fn pick_icon_dlg(&self) -> Option<ShellIcon> {
        let mut buf = [0 as WCHAR; MAX_PATH];
        let mut idx: std::os::raw::c_int = 0;
        if let Some(si) = self
            .current_ext_cfg
            .as_ref()
            .and_then(|cfg| cfg.icon.as_ref())
        {
            let mut path = si.path();
            if let Ok(p) = path.expand() {
                path = p;
            }
            let s = path.to_wide();
            if s.len() < buf.len() {
                for (i, &c) in s.as_slice().iter().enumerate() {
                    buf[i] = c;
                }
                idx = si.index() as i32;
            }
        }
        let result =
            unsafe { PickIconDlg(self.window, buf.as_mut_ptr(), buf.len() as u32, &mut idx) };
        if result == 0 {
            return None;
        }
        match buf.iter().position(|&c| c == 0) {
            Some(pos) => {
                let path =
                    unsafe { WideCString::from_vec_with_nul_unchecked(&buf[..=pos as usize]) };
                if let Ok(p) = WinPathBuf::from(path.as_ucstr()).expand() {
                    match ShellIcon::load(p, idx as u32) {
                        Ok(icon) => Some(icon),
                        Err(e) => {
                            error_message(&e.to_wide());
                            None
                        }
                    }
                } else {
                    None
                }
            }
            None => None,
        }
    }

    fn get_selected_hold_mode(&self) -> Option<registry::HoldMode> {
        let hwnd = self.get_control_handle(Control::HoldModeCombo);
        let idx = unsafe { SendMessageW(hwnd, CB_GETCURSEL, 0, 0) };
        let data = unsafe { SendMessageW(hwnd, CB_GETITEMDATA, idx as WPARAM, 0) };
        let cs = unsafe { WideCStr::from_ptr_str(data as *const WCHAR) };
        registry::HoldMode::from_wcstr(cs)
    }

    fn set_selected_hold_mode(&self, mode: registry::HoldMode) -> Option<usize> {
        let hwnd = self.get_control_handle(Control::HoldModeCombo);
        let count = unsafe { SendMessageW(hwnd, CB_GETCOUNT, 0, 0) as usize };
        for idx in 0..count {
            let data = unsafe { SendMessageW(hwnd, CB_GETITEMDATA, idx as WPARAM, 0) };
            let cs = unsafe { WideCStr::from_ptr_str(data as *const WCHAR) };
            if let Some(m) = registry::HoldMode::from_wcstr(cs) {
                if m == mode {
                    unsafe { SendMessageW(hwnd, CB_SETCURSEL, idx as WPARAM, 0) };
                    return Some(idx);
                }
            }
        }
        None
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
                // WM_NCCREATE must be passed to DefWindowProc
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
                        match self.on_control(lparam as HWND, id, HIWORD(wparam as u32)) {
                            Err(e) => {
                                error_message(&e.to_wide());
                                return Some(0);
                            }
                            Ok(l) => return Some(l),
                        }
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

extern "system" fn extension_input_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: UINT_PTR,
    data: DWORD_PTR,
) -> LRESULT {
    let wnd = unsafe { &mut *(data as *mut MainWindow) };
    #[allow(clippy::single_match)]
    match msg {
        // TODO: filter dots etc.
        WM_KEYDOWN => match wparam as i32 {
            VK_RETURN => {
                if let Err(e) = wnd.on_register_button_clicked() {
                    error_message(&e.to_wide());
                }
                return 0;
            }
            _ => {}
        },
        WM_CHAR => match wparam as i32 {
            VK_RETURN => {
                return 0;
            }
            _ => {}
        },
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
