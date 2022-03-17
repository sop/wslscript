use num_enum::{IntoPrimitive, TryFromPrimitive};
use once_cell::sync::Lazy;
use std::mem;
use std::pin::Pin;
use std::ptr;
use std::str::FromStr;
use wchar::*;
use widestring::*;
use winapi::shared::basetsd;
use winapi::shared::minwindef as win;
use winapi::shared::ntdef;
use winapi::shared::windef;
use winapi::um::commctrl;
use winapi::um::errhandlingapi;
use winapi::um::libloaderapi;
use winapi::um::wingdi;
use winapi::um::winuser::*;
use wslscript_common::error::*;
use wslscript_common::font::Font;
use wslscript_common::icon::ShellIcon;
use wslscript_common::registry;
use wslscript_common::win32;
use wslscript_common::{wcstr, wcstring};

mod listview;

/// Default extension to register.
static DEFAULT_EXTENSION: Lazy<WideCString> = Lazy::new(|| wcstring("sh"));

extern "system" {
    /// PickIconDlg() prototype.
    ///
    /// https://docs.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-pickicondlg
    pub fn PickIconDlg(
        hwnd: windef::HWND,
        pszIconPath: ntdef::PWSTR,
        cchIconPath: win::UINT,
        piIconIndex: *mut std::os::raw::c_int,
    ) -> std::os::raw::c_int;
}

/// Start WSL Script GUI app.
pub fn start_gui() -> Result<(), Error> {
    let mut title = "WSL Script".to_string();
    if let Ok(p) = std::env::current_exe() {
        if let Some(version) = wslscript_common::ver::product_version(&p) {
            title.push_str(" - ");
            title.push_str(&version);
        }
    };
    let wnd = MainWindow::new(&wcstring(&title))?;
    wnd.run()
}

pub trait WindowProc {
    /// Window procedure callback.
    ///
    /// If None is returned, underlying wrapper calls `DefWindowProcW`.
    fn window_proc(
        &mut self,
        hwnd: windef::HWND,
        msg: win::UINT,
        wparam: win::WPARAM,
        lparam: win::LPARAM,
    ) -> Option<win::LRESULT>;
}

/// Window procedure wrapper that stores struct pointer to window attributes.
///
/// Proxies messages to `window_proc()` with *self*.
extern "system" fn window_proc_wrapper<T: WindowProc>(
    hwnd: windef::HWND,
    msg: win::UINT,
    wparam: win::WPARAM,
    lparam: win::LPARAM,
) -> win::LRESULT {
    // get pointer to T from userdata
    let mut ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut T;
    // not yet set, initialize from CREATESTRUCT
    if ptr.is_null() && msg == WM_NCCREATE {
        let cs = unsafe { &*(lparam as LPCREATESTRUCTW) };
        ptr = cs.lpCreateParams as *mut T;
        unsafe { errhandlingapi::SetLastError(0) };
        if 0 == unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as *const _ as basetsd::LONG_PTR)
        } && unsafe { errhandlingapi::GetLastError() } != 0
        {
            return win::FALSE as _;
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

pub(crate) struct MainWindow {
    /// Main window handle.
    hwnd: windef::HWND,
    /// Font for captions.
    caption_font: Font,
    /// Font for filetype extension.
    ext_font: Font,
    /// Currently selected extension index in the listview.
    current_ext_idx: Option<usize>,
    /// Configuration of the currently selected extension.
    current_ext_cfg: Option<registry::ExtConfig>,
    /// List of available WSL distributions.
    distros: registry::Distros,
    /// Extensions listview.
    lv_extensions: listview::ExtensionsListView,
    /// Message to display on GUI.
    message: Option<String>,
}

impl Default for MainWindow {
    fn default() -> Self {
        Self {
            hwnd: ptr::null_mut(),
            caption_font: Default::default(),
            ext_font: Default::default(),
            current_ext_idx: None,
            current_ext_cfg: None,
            distros: registry::query_distros().unwrap_or_else(|_| registry::Distros::default()),
            lv_extensions: Default::default(),
            message: None,
        }
    }
}

/// Window control ID's.
#[derive(IntoPrimitive, TryFromPrimitive, PartialEq)]
#[repr(u16)]
pub(crate) enum Control {
    StaticMsg = 100,     // message area
    RegisterLabel,       // label for extension input
    EditExtension,       // input for extension
    BtnRegister,         // register button
    ListViewExtensions,  // listview of registered extensions
    StaticIcon,          // icon for extension
    IconLabel,           // label for icon
    HoldModeCombo,       // combo box for hold mode
    HoldModeLabel,       // label for hold mode
    InteractiveCheckbox, // label for interactive shell checkbox
    InteractiveLabel,    // checkbox for interactive shell
    DistroCombo,         // combo box for distro
    DistroLabel,         // label for distro
    BtnSave,             // Save button
}

/// Menu item ID's.
#[derive(IntoPrimitive, TryFromPrimitive, PartialEq)]
#[repr(u32)]
enum MenuItem {
    Unregister = 100,
    EditExtension,
}

/// Minimum and initial main window size.
const MIN_WINDOW_SIZE: (i32, i32) = (300, 315);

impl MainWindow {
    /// Create application window.
    fn new(title: &WideCStr) -> Result<Pin<Box<Self>>, Error> {
        let wnd = Pin::new(Box::new(Self::default()));
        let instance = unsafe { libloaderapi::GetModuleHandleW(ptr::null_mut()) };
        let class_name = wchz!("WSLScript");
        // register window class
        let wc = WNDCLASSEXW {
            cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_OWNDC | CS_HREDRAW | CS_VREDRAW,
            hbrBackground: (COLOR_WINDOW + 1) as _,
            lpfnWndProc: Some(window_proc_wrapper::<MainWindow>),
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            hIcon: unsafe { LoadIconW(instance, wchz!("app").as_ptr()) },
            hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
            ..unsafe { mem::zeroed() }
        };
        if 0 == unsafe { RegisterClassExW(&wc) } {
            return Err(win32::last_error());
        }
        // create window
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, class_name.as_ptr(), title.as_ptr(),
            WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX | WS_VISIBLE,
            CW_USEDEFAULT, CW_USEDEFAULT, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1,
            ptr::null_mut(), ptr::null_mut(), instance, &*wnd as *const Self as _) };
        if hwnd.is_null() {
            return Err(win32::last_error());
        }
        Ok(wnd)
    }

    /// Run message loop.
    fn run(&self) -> Result<(), Error> {
        loop {
            let mut msg: MSG = unsafe { mem::zeroed() };
            match unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } {
                1..=std::i32::MAX => {
                    unsafe { TranslateMessage(&msg) };
                    unsafe { DispatchMessageW(&msg) };
                }
                std::i32::MIN..=-1 => return Err(win32::last_error()),
                0 => return Ok(()),
            }
        }
    }

    /// Create window controls.
    fn create_window_controls(&mut self) -> Result<(), Error> {
        let instance = unsafe { GetWindowLongW(self.hwnd, GWL_HINSTANCE) as win::HINSTANCE };
        self.caption_font = Font::new_default_caption()?;
        self.ext_font = Font::new_caption(24)?;
        // init common controls
        let icex = commctrl::INITCOMMONCONTROLSEX {
            dwSize: mem::size_of::<commctrl::INITCOMMONCONTROLSEX>() as u32,
            dwICC: commctrl::ICC_LISTVIEW_CLASSES,
        };
        unsafe { commctrl::InitCommonControlsEx(&icex) };

        // static message area
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wchz!("STATIC").as_ptr(), ptr::null_mut(),
            SS_CENTER | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.hwnd,
            Control::StaticMsg as u16 as _, instance, ptr::null_mut(),
        ) };
        set_window_font(hwnd, &self.caption_font);

        // register button
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wchz!("BUTTON").as_ptr(), wchz!("Register").as_ptr(),
            WS_TABSTOP | WS_VISIBLE | WS_CHILD | BS_DEFPUSHBUTTON,
            0, 0, 0, 0, self.hwnd,
            Control::BtnRegister as u16 as _, instance, ptr::null_mut()
        ) };
        set_window_font(hwnd, &self.caption_font);

        // register label
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wchz!("STATIC").as_ptr(), wchz!("Extension:").as_ptr(),
            SS_CENTERIMAGE | SS_RIGHT | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.hwnd,
            Control::RegisterLabel as u16 as _, instance, ptr::null_mut(),
        ) };
        set_window_font(hwnd, &self.caption_font);

        // extension input
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wchz!("EDIT").as_ptr(), ptr::null_mut(),
            ES_LEFT | ES_LOWERCASE | WS_CHILD | WS_VISIBLE | WS_BORDER,
            0, 0, 0, 0, self.hwnd,
            Control::EditExtension as u16 as _, instance, ptr::null_mut(),
        ) };
        set_window_font(hwnd, &self.caption_font);
        let self_ptr = self as *const _;
        // use custom window proc
        unsafe {
            commctrl::SetWindowSubclass(
                hwnd,
                Some(extension_input_proc),
                0,
                self_ptr as basetsd::DWORD_PTR,
            )
        };
        // if no extensions are registered, set default value to input box
        if registry::query_registered_extensions()
            .unwrap_or_default()
            .is_empty()
        {
            unsafe { SetWindowTextW(hwnd, DEFAULT_EXTENSION.as_ptr()) };
        }

        self.lv_extensions = listview::ExtensionsListView::create(self);

        // extension icon
        #[rustfmt::skip]
        unsafe { CreateWindowExW(
            0, wchz!("STATIC").as_ptr(), ptr::null_mut(),
            SS_ICON | SS_CENTERIMAGE | SS_NOTIFY | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.hwnd,
            Control::StaticIcon as u16 as _, instance, ptr::null_mut(),
        ) };

        // icon tooltip
        self.create_control_tooltip(
            Control::StaticIcon,
            wcstr(wchz!("Double click to select an icon for the extension.")),
        );

        // icon label
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wchz!("STATIC").as_ptr(), wchz!("Icon").as_ptr(),
            SS_CENTER | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.hwnd,
            Control::IconLabel as u16 as _, instance, ptr::null_mut()
        ) };
        set_window_font(hwnd, &self.caption_font);

        // hold mode combo box
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wchz!("COMBOBOX").as_ptr(), ptr::null_mut(),
            CBS_DROPDOWNLIST | WS_VSCROLL | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.hwnd,
            Control::HoldModeCombo as u16 as _, instance, ptr::null_mut()
        ) };
        set_window_font(hwnd, &self.caption_font);
        let insert_item = |mode: registry::HoldMode, label: &[wchar_t]| {
            let idx =
                unsafe { SendMessageW(hwnd, CB_INSERTSTRING, -1_isize as _, label.as_ptr() as _) };
            let s = mode.as_wcstr();
            unsafe { SendMessageW(hwnd, CB_SETITEMDATA, idx as _, s.as_ptr() as _) };
        };
        insert_item(registry::HoldMode::Error, wchz!("Close on success"));
        insert_item(registry::HoldMode::Never, wchz!("Always close"));
        insert_item(registry::HoldMode::Always, wchz!("Keep open"));

        // hold mode label
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wchz!("STATIC").as_ptr(), wchz!("Exit behaviour").as_ptr(),
            SS_CENTER | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.hwnd,
            Control::HoldModeLabel as u16 as _, instance, ptr::null_mut()
        ) };
        set_window_font(hwnd, &self.caption_font);

        // hold more tooltip
        self.create_control_tooltip(
            Control::HoldModeCombo,
            wcstr(wchz!("Console window behaviour when the script exits.")),
        );

        // interactive shell checkbox
        #[rustfmt::skip]
        unsafe { CreateWindowExW(
            0, wchz!("BUTTON").as_ptr(), ptr::null_mut(),
            WS_TABSTOP | WS_VISIBLE | WS_CHILD | BS_AUTOCHECKBOX,
            0, 0, 0, 0, self.hwnd,
            Control::InteractiveCheckbox as u16 as _, instance, ptr::null_mut()
        ) };

        // interactive shell label
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wchz!("STATIC").as_ptr(), wchz!("Interactive").as_ptr(),
            SS_LEFT | SS_CENTERIMAGE | SS_NOTIFY | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.hwnd,
            Control::InteractiveLabel as u16 as _, instance, ptr::null_mut()
        ) };
        set_window_font(hwnd, &self.caption_font);

        // tooltip for interactive shell
        self.create_control_tooltip(
            Control::InteractiveCheckbox,
            wcstr(wchz!(
                "Run bash as an interactive shell and execute \
                profile scripts (eg. ~/.bashrc)."
            )),
        );

        // distro combo box
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wchz!("COMBOBOX").as_ptr(), ptr::null_mut(),
            CBS_DROPDOWNLIST | WS_VSCROLL | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.hwnd,
            Control::DistroCombo as u16 as _, instance, ptr::null_mut()
        ) };
        set_window_font(hwnd, &self.caption_font);
        let insert_item = |guid: Option<&registry::DistroGUID>, name: &str| {
            unsafe {
                let s = WideCString::from_str_unchecked(name);
                let idx = SendMessageW(hwnd, CB_INSERTSTRING, -1_isize as _, s.as_ptr() as _);
                if let Some(guid) = guid {
                    SendMessageW(
                        hwnd,
                        CB_SETITEMDATA,
                        idx as _,
                        guid.as_wcstr().as_ptr() as _,
                    );
                } else {
                    SendMessageW(hwnd, CB_SETITEMDATA, idx as _, 0);
                }
            };
        };
        insert_item(None, &self.get_distro_label(None));
        for (guid, name) in self.distros.sorted_pairs() {
            insert_item(Some(guid), name);
        }

        // distro label
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wchz!("STATIC").as_ptr(), wchz!("Distribution").as_ptr(),
            SS_CENTER | WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, self.hwnd,
            Control::DistroLabel as u16 as _, instance, ptr::null_mut()
        ) };
        set_window_font(hwnd, &self.caption_font);

        // distro tooltip
        self.create_control_tooltip(
            Control::DistroCombo,
            wcstr(wchz!("WSL distribution on which to run the script.")),
        );

        // save button
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            0, wchz!("BUTTON").as_ptr(), wchz!("Save").as_ptr(),
            WS_TABSTOP | WS_VISIBLE | WS_CHILD | BS_DEFPUSHBUTTON,
            0, 0, 0, 0, self.hwnd,
            Control::BtnSave as u16 as _, instance, ptr::null_mut()
        ) };
        set_window_font(hwnd, &self.caption_font);
        self.update_control_states();
        Ok(())
    }

    /// Create a tooltip and assign it to given control.
    fn create_control_tooltip(&self, control: Control, text: &WideCStr) {
        use commctrl::*;
        let instance = unsafe { GetWindowLongW(self.hwnd, GWL_HINSTANCE) as win::HINSTANCE };
        #[rustfmt::skip]
        let hwnd_tt = unsafe { CreateWindowExW(
            0, wchz!("tooltips_class32").as_ptr(), ptr::null_mut(),
            WS_POPUP | TTS_ALWAYSTIP | TTS_BALLOON,
            CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, self.hwnd,
            ptr::null_mut(), instance, ptr::null_mut()
        ) };
        let ti = TOOLINFOW {
            cbSize: mem::size_of::<TOOLINFOW>() as u32,
            hwnd: self.hwnd,
            uFlags: TTF_IDISHWND | TTF_SUBCLASS,
            uId: self.get_control_handle(control) as basetsd::UINT_PTR,
            lpszText: text.as_ptr() as ntdef::LPWSTR,
            ..unsafe { mem::zeroed() }
        };
        unsafe { SendMessageW(hwnd_tt, TTM_ADDTOOLW, 0, &ti as *const _ as _) };
        unsafe { SendMessageW(hwnd_tt, TTM_ACTIVATE, win::TRUE as _, 0) };
    }

    /// Update control states.
    fn update_control_states(&self) {
        // set message
        let hwnd = self.get_control_handle(Control::StaticMsg);
        if let Some(mut ext) = self.get_current_extension() {
            // if extension is registered for WSL, but handler is in another directory
            if !registry::is_registered_for_current_executable(&ext).unwrap_or(true) {
                let exe = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.file_name().map(|s| s.to_os_string()))
                    .and_then(|s| s.into_string().ok())
                    .unwrap_or_default();
                let s = wcstring(format!(
                    ".{} handler found in another directory!\n\
                     Did you move {}?",
                    ext, exe
                ));
                unsafe { SetWindowTextW(hwnd, s.as_ptr()) };
                set_window_font(hwnd, &self.caption_font);
            } else if let Some(msg) = &self.message {
                unsafe { SetWindowTextW(hwnd, wcstring(msg).as_ptr()) };
                set_window_font(hwnd, &self.caption_font);
            } else {
                ext.insert(0, '.');
                unsafe { SetWindowTextW(hwnd, wcstring(ext).as_ptr()) };
                set_window_font(hwnd, &self.ext_font);
            }
        } else {
            let s = wchz!(
                "Enter the extension and click \
                 Register to associate a filetype with WSL."
            );
            unsafe { SetWindowTextW(hwnd, s.as_ptr()) };
            set_window_font(hwnd, &self.caption_font);
        };
        let visible = self.current_ext_cfg.is_some();
        // hold mode label
        self.set_control_visibility(Control::HoldModeLabel, visible);
        // hold mode combo
        self.set_control_visibility(Control::HoldModeCombo, visible);
        if let Some(mode) = self.current_ext_cfg.as_ref().map(|cfg| cfg.hold_mode) {
            self.set_selected_hold_mode(mode);
        }
        // interactive shell label
        self.set_control_visibility(Control::InteractiveLabel, visible);
        // interactive shell checkbox
        self.set_control_visibility(Control::InteractiveCheckbox, visible);
        // set button state
        if let Some(state) = self.current_ext_cfg.as_ref().map(|cfg| cfg.interactive) {
            self.set_interactive_state(state);
        }
        // distro label
        self.set_control_visibility(Control::DistroLabel, visible);
        // distro combo
        self.set_control_visibility(Control::DistroCombo, visible);
        self.set_selected_distro(
            self.current_ext_cfg
                .as_ref()
                .and_then(|cfg| cfg.distro.as_ref()),
        );
        // set icon
        self.set_control_visibility(Control::StaticIcon, visible);
        let hwnd = self.get_control_handle(Control::StaticIcon);
        if let Some(icon) = self
            .current_ext_cfg
            .as_ref()
            .and_then(|cfg| cfg.icon.as_ref())
        {
            unsafe { SendMessageW(hwnd, STM_SETICON, icon.handle() as _, 0) };
        } else {
            // NOTE: DestroyIcon not needed for shared icon
            let hicon = unsafe { LoadIconW(ptr::null_mut(), IDI_WARNING) };
            unsafe { SendMessageW(hwnd, STM_SETICON, hicon as _, 0) };
        }
        // icon label
        self.set_control_visibility(Control::IconLabel, visible);
        // save button
        self.set_control_visibility(Control::BtnSave, visible);
    }

    /// Set control visibility.
    fn set_control_visibility(&self, control: Control, visible: bool) {
        let visibility = if visible { SW_SHOW } else { SW_HIDE };
        unsafe {
            ShowWindow(self.get_control_handle(control), visibility);
        }
    }

    /// Handle WM_SIZE message.
    ///
    /// * `width` - Window width
    /// * `height` - Window height
    fn on_resize(&self, width: i32, _height: i32) {
        self.move_control(Control::StaticMsg, 10, 10, width - 20, 40);
        self.move_control(Control::RegisterLabel, 10, 50, 60, 25);
        self.move_control(Control::EditExtension, 80, 50, width - 90 - 100, 25);
        self.move_control(Control::BtnRegister, width - 100, 50, 90, 25);
        self.move_control(Control::ListViewExtensions, 10, 85, width - 20, 75);
        self.move_control(Control::HoldModeLabel, 10, 170, 130, 20);
        self.move_control(Control::HoldModeCombo, 10, 190, 130, 100);
        self.move_control(Control::InteractiveLabel, 170, 190, 130, 20);
        self.move_control(Control::InteractiveCheckbox, 150, 190, 20, 20);
        self.move_control(Control::DistroLabel, 10, 220, 130, 20);
        self.move_control(Control::DistroCombo, 10, 240, 130, 100);
        self.move_control(Control::IconLabel, 150, 220, 32, 16);
        self.move_control(Control::StaticIcon, 150, 236, 32, 32);
        self.move_control(Control::BtnSave, width - 90, 240, 80, 25);
    }

    /// Move window control.
    fn move_control(&self, control: Control, x: i32, y: i32, width: i32, height: i32) {
        let hwnd = self.get_control_handle(control);
        unsafe { MoveWindow(hwnd, x, y, width, height, win::TRUE) };
    }

    /// Handle WM_COMMAND message from a control.
    ///
    /// * `hwnd` - Handle of the sending control
    /// * `control_id` - ID of the sending control
    /// * `code` - Notification code
    fn on_control(
        &mut self,
        _hwnd: windef::HWND,
        control_id: Control,
        code: win::WORD,
    ) -> Result<win::LRESULT, Error> {
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
            Control::InteractiveCheckbox => match code {
                BN_CLICKED => {
                    let state = self.get_interactive_state();
                    if let Some(cfg) = &mut self.current_ext_cfg {
                        cfg.interactive = state;
                    }
                }
                _ => {}
            },
            Control::InteractiveLabel => match code {
                // when interactive shell label is clicked
                STN_CLICKED => {
                    let state = !self.get_interactive_state();
                    if let Some(cfg) = &mut self.current_ext_cfg {
                        cfg.interactive = state;
                    }
                    self.set_interactive_state(state);
                }
                _ => {}
            },
            Control::DistroCombo => match code {
                CBN_SELCHANGE => {
                    let distro = self.get_selected_distro();
                    if let Some(cfg) = &mut self.current_ext_cfg {
                        cfg.distro = distro;
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
    fn on_register_button_clicked(&mut self) -> Result<win::LRESULT, Error> {
        let ext = self
            .get_extension_input_text()
            .trim_matches('.')
            .to_string();
        if ext.is_empty() {
            return Ok(0);
        }
        if registry::is_registered_for_other(&ext)? {
            let s = wcstring(format!(
                ".{} extension is already registered for another application.\n\
                 Register anyway?",
                ext
            ));
            let result = unsafe {
                MessageBoxW(
                    self.hwnd,
                    s.as_ptr(),
                    wchz!("Confirm extension registration.").as_ptr(),
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
            interactive: false,
            distro: None,
        };
        registry::register_extension(&config)?;
        // clear extension input
        self.set_extension_input_text(wcstr(wchz!("")));
        let idx = self.lv_extensions.find_ext(&ext).or_else(|| {
            // insert to listview
            if let Some(item) = self.lv_extensions.insert_item(0, &wcstring(&ext)) {
                let name = self.get_distro_label(None);
                self.lv_extensions
                    .set_subitem_text(item, 1, &wcstring(name));
                return Some(item);
            }
            None
        });
        self.set_current_extension(idx);
        self.message = Some(format!("Registered .{} extension.", &ext));
        self.update_control_states();
        Ok(0)
    }

    /// Handle save button click.
    fn on_save_button_clicked(&mut self) -> Result<win::LRESULT, Error> {
        if let Some(config) = self.current_ext_cfg.as_ref() {
            registry::register_extension(config)?;
            self.message = Some(format!("Saved .{} extension.", config.extension));
            self.update_control_states();
            if let Some(item) = self.current_ext_idx {
                let name = self.get_distro_label(config.distro.as_ref());
                self.lv_extensions
                    .set_subitem_text(item, 1, &wcstring(name));
            }
        }
        Ok(0)
    }

    /// Handle message from a menu.
    ///
    /// * `hmenu` - Handle to the menu
    /// * `item_id` - ID of the clicked menu item
    fn on_menucommand(&mut self, hmenu: windef::HMENU, item_id: MenuItem) -> win::LRESULT {
        match item_id {
            MenuItem::Unregister => {
                let idx: usize = self.get_menu_data(hmenu);
                if let Some(ext) = self.lv_extensions.get_item_text(idx) {
                    if let Err(e) = registry::unregister_extension(&ext) {
                        let s = wcstring(format!("Failed to unregister extension: {}", e));
                        win32::error_message(&s);
                        return 0;
                    }
                }
                self.lv_extensions.delete_item(idx);
                self.set_current_extension(None);
                self.update_control_states();
                // if there's no more registered extensions, and if extension
                // input was empty, reset to default extension
                if registry::query_registered_extensions()
                    .unwrap_or_default()
                    .is_empty()
                    && self.get_extension_input_text().is_empty()
                {
                    self.set_extension_input_text(&DEFAULT_EXTENSION);
                }
            }
            MenuItem::EditExtension => {
                let idx: usize = self.get_menu_data(hmenu);
                self.set_current_extension(Some(idx));
                self.update_control_states();
            }
        }
        0
    }

    /// Get application-defined value associated with a menu.
    fn get_menu_data<T>(&self, hmenu: windef::HMENU) -> T
    where
        T: From<winapi::shared::basetsd::ULONG_PTR>,
    {
        let mut mi = MENUINFO {
            cbSize: mem::size_of::<MENUINFO>() as u32,
            fMask: MIM_MENUDATA,
            ..unsafe { mem::zeroed() }
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
        hwnd: windef::HWND,
        control_id: Control,
        code: u32,
        lparam: *const isize,
    ) -> win::LRESULT {
        use commctrl::*;
        #[allow(clippy::single_match)]
        match control_id {
            Control::ListViewExtensions => match code {
                // when listview item is activated (eg. double clicked)
                LVN_ITEMACTIVATE => {
                    let nmia = unsafe { &*(lparam as LPNMITEMACTIVATE) };
                    if nmia.iItem < 0 {
                        return 0;
                    }
                    self.set_current_extension(Some(nmia.iItem as usize));
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
                        cbSize: mem::size_of::<MENUINFO>() as u32,
                        fMask: MIM_MENUDATA | MIM_STYLE,
                        dwStyle: MNS_NOTIFYBYPOS,
                        dwMenuData: nmia.iItem as usize,
                        ..unsafe { mem::zeroed() }
                    };
                    unsafe { SetMenuInfo(hmenu, &mi) };
                    let mut mii = MENUITEMINFOW {
                        cbSize: mem::size_of::<MENUITEMINFOW>() as u32,
                        fMask: MIIM_TYPE | MIIM_ID,
                        fType: MFT_STRING,
                        ..unsafe { mem::zeroed() }
                    };
                    mii.wID = MenuItem::EditExtension as _;
                    mii.dwTypeData = wchz!("Edit").as_ptr() as _;
                    unsafe { InsertMenuItemW(hmenu, 0, win::TRUE, &mii) };
                    mii.wID = MenuItem::Unregister as _;
                    mii.dwTypeData = wchz!("Unregister").as_ptr() as _;
                    unsafe { InsertMenuItemW(hmenu, 1, win::TRUE, &mii) };
                    let mut pos: windef::POINT = nmia.ptAction;
                    unsafe { ClientToScreen(hwnd, &mut pos) };
                    unsafe { TrackPopupMenuEx(hmenu, 0, pos.x, pos.y, self.hwnd, ptr::null_mut()) };
                }
                _ => {}
            },
            _ => {}
        }
        0
    }

    /// Get currently selected extension.
    fn get_current_extension(&self) -> Option<String> {
        self.current_ext_idx
            .and_then(|item| self.lv_extensions.get_item_text(item))
    }

    /// Get window handle to control.
    fn get_control_handle(&self, control: Control) -> windef::HWND {
        unsafe { GetDlgItem(self.hwnd, control as _) }
    }

    /// Get text from extension text input.
    fn get_extension_input_text(&self) -> String {
        let mut buf: Vec<ntdef::WCHAR> = Vec::with_capacity(32);
        unsafe {
            let len = GetDlgItemTextW(
                self.hwnd,
                Control::EditExtension as _,
                buf.as_mut_ptr(),
                buf.capacity() as i32,
            );
            buf.set_len(len as usize);
        }
        WideCString::from_vec(buf).unwrap().to_string_lossy()
    }

    fn set_extension_input_text(&self, text: &WideCStr) {
        unsafe {
            SetDlgItemTextW(self.hwnd, Control::EditExtension as _, text.as_ptr());
        }
    }

    /// Set extension that is currently selected for edit.
    fn set_current_extension(&mut self, item: Option<usize>) {
        self.current_ext_idx = item;
        self.current_ext_cfg = self
            .get_current_extension()
            .and_then(|ext| registry::get_extension_config(&ext).ok());
        self.message = None;
    }

    /// Launch icon picker dialog.
    ///
    /// Returns ShellIcon or None if no icon was selected.
    fn pick_icon_dlg(&self) -> Option<ShellIcon> {
        let mut buf = [0_u16; win::MAX_PATH];
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
                unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), buf.as_mut_ptr(), s.len()) };
                idx = si.index() as i32;
            }
        }
        let result =
            unsafe { PickIconDlg(self.hwnd, buf.as_mut_ptr(), buf.len() as u32, &mut idx) };
        if result == 0 {
            return None;
        }
        match buf.iter().position(|&c| c == 0) {
            Some(pos) => {
                let path = unsafe { WideCString::from_vec_unchecked(&buf[..=pos as usize]) };
                if let Ok(p) = win32::WinPathBuf::from(path.as_ucstr()).expand() {
                    match ShellIcon::load(p, idx as u32) {
                        Ok(icon) => Some(icon),
                        Err(e) => {
                            let s = wcstring(format!("Failed load icon: {}", e));
                            win32::error_message(&s);
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
        let data = unsafe { SendMessageW(hwnd, CB_GETITEMDATA, idx as _, 0) };
        let cs = unsafe { WideCStr::from_ptr_str(data as *const ntdef::WCHAR) };
        registry::HoldMode::from_wcstr(cs)
    }

    fn set_selected_hold_mode(&self, mode: registry::HoldMode) -> Option<usize> {
        let hwnd = self.get_control_handle(Control::HoldModeCombo);
        let count = unsafe { SendMessageW(hwnd, CB_GETCOUNT, 0, 0) as usize };
        for idx in 0..count {
            let data = unsafe { SendMessageW(hwnd, CB_GETITEMDATA, idx as _, 0) };
            let cs = unsafe { WideCStr::from_ptr_str(data as *const ntdef::WCHAR) };
            if let Some(m) = registry::HoldMode::from_wcstr(cs) {
                if m == mode {
                    unsafe { SendMessageW(hwnd, CB_SETCURSEL, idx as _, 0) };
                    return Some(idx);
                }
            }
        }
        None
    }

    /// Get the interactive shell checkbox state.
    fn get_interactive_state(&self) -> bool {
        let result = unsafe { IsDlgButtonChecked(self.hwnd, Control::InteractiveCheckbox as i32) };
        result == 1
    }

    /// Set the interactive shell checkbox state.
    fn set_interactive_state(&self, state: bool) {
        unsafe { CheckDlgButton(self.hwnd, Control::InteractiveCheckbox as i32, state as u32) };
    }

    /// Set selected distro in combo box.
    fn set_selected_distro(&self, distro: Option<&registry::DistroGUID>) -> Option<usize> {
        let hwnd = self.get_control_handle(Control::DistroCombo);
        let mut sel: usize = 0;
        if let Some(guid) = distro {
            let count = unsafe { SendMessageW(hwnd, CB_GETCOUNT, 0, 0) as usize };
            for idx in 1..count {
                let data = unsafe { SendMessageW(hwnd, CB_GETITEMDATA, idx as _, 0) };
                let guid_str = unsafe { WideCStr::from_ptr_str(data as *const ntdef::WCHAR) };
                if guid_str == guid.as_wcstr() {
                    sel = idx;
                    break;
                }
            }
        }
        unsafe { SendMessageW(hwnd, CB_SETCURSEL, sel as _, 0) };
        Some(sel)
    }

    /// Get currently selected GUID in distro combo box.
    fn get_selected_distro(&self) -> Option<registry::DistroGUID> {
        let hwnd = self.get_control_handle(Control::DistroCombo);
        let idx = unsafe { SendMessageW(hwnd, CB_GETCURSEL, 0, 0) };
        if idx == 0 || idx == CB_ERR {
            return None;
        }
        let data = unsafe { SendMessageW(hwnd, CB_GETITEMDATA, idx as _, 0) };
        let cs = unsafe { WideCStr::from_ptr_str(data as *const ntdef::WCHAR) };
        let s = cs.to_string_lossy();
        registry::DistroGUID::from_str(&s).ok()
    }

    /// Get label for distribution GUID.
    fn get_distro_label(&self, guid: Option<&registry::DistroGUID>) -> String {
        guid.and_then(|guid| self.distros.list.get(guid).map(|s| s.to_owned()))
            .or_else(|| Some(String::from("Default")))
            .unwrap_or_default()
    }
}

/// Set font to given window.
fn set_window_font(hwnd: windef::HWND, font: &Font) {
    unsafe { SendMessageW(hwnd, WM_SETFONT, font.handle as _, win::TRUE as _) };
}

impl WindowProc for MainWindow {
    fn window_proc(
        &mut self,
        hwnd: windef::HWND,
        msg: win::UINT,
        wparam: win::WPARAM,
        lparam: win::LPARAM,
    ) -> Option<win::LRESULT> {
        match msg {
            WM_NCCREATE => {
                // store main window handle
                self.hwnd = hwnd;
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
                    i32::from(win::LOWORD(lparam as u32)),
                    i32::from(win::HIWORD(lparam as u32)),
                );
                Some(0)
            }
            WM_GETMINMAXINFO => {
                let mmi = unsafe { &mut *(lparam as LPMINMAXINFO) };
                mmi.ptMinTrackSize.x = MIN_WINDOW_SIZE.0;
                mmi.ptMinTrackSize.y = MIN_WINDOW_SIZE.1;
                Some(0)
            }
            WM_CTLCOLORSTATIC => Some(unsafe { wingdi::GetStockObject(COLOR_WINDOW + 1_i32) } as _),
            WM_COMMAND => {
                // if lParam is non-zero, message is from a control
                if lparam != 0 {
                    if let Ok(id) = Control::try_from(win::LOWORD(wparam as _)) {
                        match self.on_control(lparam as _, id, win::HIWORD(wparam as _)) {
                            Err(e) => {
                                win32::error_message(&e.to_wide());
                                return Some(0);
                            }
                            Ok(l) => return Some(l),
                        }
                    }
                }
                // if lParam is zero and HIWORD of wParam is zero, message is from a menu
                else if win::HIWORD(wparam as u32) == 0 {
                    if let Ok(id) = MenuItem::try_from(wparam as u32) {
                        return Some(self.on_menucommand(ptr::null_mut(), id));
                    }
                }
                None
            }
            WM_MENUCOMMAND => {
                let hmenu = lparam as windef::HMENU;
                let item_id = unsafe { GetMenuItemID(hmenu, wparam as i32) };
                if let Ok(id) = MenuItem::try_from(item_id) {
                    return Some(self.on_menucommand(hmenu, id));
                }
                None
            }
            WM_NOTIFY => {
                let hdr = unsafe { &*(lparam as LPNMHDR) };
                if let Ok(id) = Control::try_from(hdr.idFrom as u16) {
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

/// Subclass callback for the extension input control.
extern "system" fn extension_input_proc(
    hwnd: windef::HWND,
    msg: win::UINT,
    wparam: win::WPARAM,
    lparam: win::LPARAM,
    _subclass_id: basetsd::UINT_PTR,
    data: basetsd::DWORD_PTR,
) -> win::LRESULT {
    let wnd = unsafe { &mut *(data as *mut MainWindow) };
    #[allow(clippy::single_match)]
    match msg {
        // TODO: filter dots etc.
        WM_KEYDOWN => match wparam as i32 {
            VK_RETURN => {
                if let Err(e) = wnd.on_register_button_clicked() {
                    win32::error_message(&e.to_wide());
                }
                return 0;
            }
            _ => {}
        },
        WM_CHAR => match wparam as i32 {
            VK_RETURN => {
                return 0;
            }
            _ => {
                if let Some(ch) = std::char::from_u32(wparam as u32) {
                    match ch {
                        // illegal filename characters
                        '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => return 0,
                        // space
                        ' ' => return 0,
                        // no periods in extension
                        '.' => return 0,
                        _ => {}
                    }
                }
            }
        },
        _ => {}
    }
    unsafe { commctrl::DefSubclassProc(hwnd, msg, wparam, lparam) }
}
