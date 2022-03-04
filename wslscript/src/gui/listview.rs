use crate::gui;
use std::mem;
use std::ptr;
use wchar::*;
use widestring::*;
use winapi::shared::ntdef;
use winapi::shared::windef;
use winapi::um::commctrl;
use winapi::um::libloaderapi;
use winapi::um::winuser;
use wslscript_common::registry;
use wslscript_common::wcstring;
use wslscript_common::win32;

pub(crate) struct ExtensionsListView {
    hwnd: windef::HWND,
}

impl Default for ExtensionsListView {
    fn default() -> Self {
        Self {
            hwnd: ptr::null_mut(),
        }
    }
}

impl ExtensionsListView {
    pub fn create(main: &gui::MainWindow) -> Self {
        use commctrl::*;
        use winuser::*;
        #[rustfmt::skip]
        let hwnd = unsafe { CreateWindowExW(
            LVS_EX_FULLROWSELECT | LVS_EX_GRIDLINES,
            wcstring(WC_LISTVIEW).as_ptr(), ptr::null_mut(),
            WS_CHILD | WS_VISIBLE | WS_BORDER | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
            0, 0, 0, 0, main.hwnd,
            gui::Control::ListViewExtensions as u16 as _,
            libloaderapi::GetModuleHandleW(ptr::null_mut()), ptr::null_mut(),
        ) };
        let lv = Self { hwnd };
        gui::set_window_font(hwnd, &main.caption_font);
        unsafe {
            SendMessageW(
                hwnd,
                LVM_SETEXTENDEDLISTVIEWSTYLE,
                LVS_EX_FULLROWSELECT as _,
                LVS_EX_FULLROWSELECT as _,
            )
        };
        // insert columns
        let mut col = LV_COLUMNW {
            mask: LVCF_FMT | LVCF_WIDTH | LVCF_TEXT,
            fmt: LVCFMT_LEFT,
            cx: 80,
            pszText: wchz!("Filetype").as_ptr() as _,
            ..unsafe { mem::zeroed() }
        };
        unsafe { SendMessageW(hwnd, LVM_INSERTCOLUMNW, 0, &col as *const _ as _) };
        col.pszText = wchz!("Distribution").as_ptr() as _;
        col.cx = 130;
        unsafe { SendMessageW(hwnd, LVM_INSERTCOLUMNW, 1, &col as *const _ as _) };
        // insert items
        match registry::query_registered_extensions().map(|exts| {
            exts.iter()
                .filter_map(|ext| registry::get_extension_config(ext).ok())
                .collect::<Vec<_>>()
        }) {
            Ok(configs) => {
                for (i, cfg) in configs.iter().enumerate() {
                    if let Some(item) = lv.insert_item(i, &wcstring(&cfg.extension)) {
                        let name = main.get_distro_label(cfg.distro.as_ref());
                        lv.set_subitem_text(item, 1, &wcstring(name));
                    }
                }
            }
            Err(e) => {
                let s = wcstring(format!("Failed to query registry: {}", e));
                win32::error_message(&s);
            }
        }
        lv
    }

    /// Insert item to listview.
    ///
    /// Returns the index of the new item.
    ///
    /// * `idx` - Index at which the the new item is inserted
    /// * `label` - Item label
    pub fn insert_item(&self, idx: usize, label: &WideCStr) -> Option<usize> {
        let lvi = commctrl::LV_ITEMW {
            mask: commctrl::LVIF_TEXT,
            iItem: idx as _,
            pszText: label.as_ptr() as _,
            ..unsafe { mem::zeroed() }
        };
        let rv = unsafe {
            winuser::SendMessageW(
                self.hwnd,
                commctrl::LVM_INSERTITEMW,
                0,
                &lvi as *const _ as _,
            )
        };
        match rv {
            -1 => None,
            _ => Some(rv as usize),
        }
    }

    /// Delete item from listview.
    pub fn delete_item(&self, idx: usize) {
        unsafe { winuser::SendMessageW(self.hwnd, commctrl::LVM_DELETEITEM, idx, 0) };
    }

    /// Set text to subitem.
    ///
    /// * `idx` - Item index
    /// * `sub_idx` - Subitem index
    /// * `label` - Text to insert
    pub fn set_subitem_text(&self, idx: usize, sub_idx: usize, label: &WideCStr) {
        let lvi = commctrl::LV_ITEMW {
            mask: commctrl::LVIF_TEXT,
            iItem: idx as _,
            iSubItem: sub_idx as _,
            pszText: label.as_ptr() as _,
            ..unsafe { mem::zeroed() }
        };
        unsafe {
            winuser::SendMessageW(self.hwnd, commctrl::LVM_SETITEMW, 0, &lvi as *const _ as _)
        };
    }

    /// Find extension from listview.
    ///
    /// Returns listview index or None if extension wasn't found.
    pub fn find_ext(&self, ext: &str) -> Option<usize> {
        let s = wcstring(ext);
        let lvf = commctrl::LVFINDINFOW {
            flags: commctrl::LVFI_STRING,
            psz: s.as_ptr(),
            ..unsafe { mem::zeroed() }
        };
        let idx = unsafe {
            winuser::SendMessageW(
                self.hwnd,
                commctrl::LVM_FINDITEMW,
                -1_isize as usize,
                &lvf as *const _ as _,
            )
        };
        match idx {
            -1 => None,
            _ => Some(idx as usize),
        }
    }

    /// Get listview text by index.
    pub fn get_item_text(&self, idx: usize) -> Option<String> {
        let mut buf: Vec<ntdef::WCHAR> = Vec::with_capacity(32);
        let lvi = commctrl::LV_ITEMW {
            pszText: buf.as_mut_ptr(),
            cchTextMax: buf.capacity() as _,
            ..unsafe { mem::zeroed() }
        };
        unsafe {
            let len = winuser::SendMessageW(
                self.hwnd,
                commctrl::LVM_GETITEMTEXTW,
                idx,
                &lvi as *const _ as _,
            );
            buf.set_len(len as usize);
        };
        WideCString::from_vec(buf).ok().map(|u| u.to_string_lossy())
    }
}
