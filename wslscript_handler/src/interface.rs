//! All the nitty gritty details regarding COM interface for the shell extension
//! are defined here.
//!
//! See: https://docs.microsoft.com/en-us/windows/win32/shell/handlers#implementing-shell-extension-handlers

use guid_win::Guid;
use once_cell::sync::Lazy;
use std::cell::RefCell;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use widestring::WideCStr;
use winapi::shared::guiddef;
use winapi::shared::minwindef as win;
use winapi::shared::winerror;
use winapi::um::oleidl;
use winapi::um::winnt;
use winapi::um::winuser;
use windows::core as wc;
use windows::core::Interface;
use windows::Win32::{Foundation, System::Com, System::Ole, System::SystemServices};
use wslscript_common::error::*;

use crate::progress::ProgressWindow;

/// IClassFactory GUID.
///
/// See: https://docs.microsoft.com/en-us/windows/win32/api/unknwn/nn-unknwn-iclassfactory
///
/// Windows requests this interface via `DllGetClassObject` to further query
/// relevant COM interfaces.
static CLASS_FACTORY_CLSID: Lazy<Guid> =
    Lazy::new(|| Guid::from_str("00000001-0000-0000-c000-000000000046").unwrap());

/// Semaphore to keep track of running WSL threads.
///
/// DLL shall not be released if there are threads running.
pub(crate) static THREAD_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Handle to loaded DLL module.
static mut DLL_HANDLE: win::HINSTANCE = std::ptr::null_mut();

/// DLL module entry point.
///
/// See: https://docs.microsoft.com/en-us/windows/win32/dlls/dllmain
#[no_mangle]
extern "system" fn DllMain(
    hinstance: win::HINSTANCE,
    reason: win::DWORD,
    _reserved: win::LPVOID,
) -> win::BOOL {
    match reason {
        winnt::DLL_PROCESS_ATTACH => {
            // store module instance to global variable
            unsafe { DLL_HANDLE = hinstance };
            // set up logging
            #[cfg(feature = "debug")]
            if let Ok(mut path) = get_module_path(hinstance) {
                let stem = path.file_stem().map_or_else(
                    || "debug.log".to_string(),
                    |s| s.to_string_lossy().into_owned(),
                );
                path.pop();
                path.push(format!("{}.log", stem));
                if simple_logging::log_to_file(&path, log::LevelFilter::Debug).is_err() {
                    unsafe {
                        use winapi::um::winuser::*;
                        let text = wslscript_common::wcstring(format!(
                            "Failed to set up logging to {}",
                            path.to_string_lossy()
                        ));
                        MessageBoxW(
                            std::ptr::null_mut(),
                            text.as_ptr(),
                            wchar::wchz!("Error").as_ptr(),
                            MB_OK | MB_ICONERROR | MB_SERVICE_NOTIFICATION,
                        );
                    }
                }
            }
            log::debug!("DLL_PROCESS_ATTACH");
            return win::TRUE;
        }
        winnt::DLL_PROCESS_DETACH => {
            log::debug!("DLL_PROCESS_DETACH");
            ProgressWindow::unregister_window_class();
        }
        winnt::DLL_THREAD_ATTACH => {}
        winnt::DLL_THREAD_DETACH => {}
        _ => {}
    }
    win::FALSE
}

/// Called to check whether DLL can be unloaded from memory.
///
/// See: https://docs.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-dllcanunloadnow
#[no_mangle]
extern "system" fn DllCanUnloadNow() -> winnt::HRESULT {
    let n = THREAD_COUNTER.load(Ordering::SeqCst);
    if n > 0 {
        log::info!("{} WSL threads running, denying DLL unload", n);
        winerror::S_FALSE
    } else {
        log::info!("Permitting DLL unload");
        winerror::S_OK
    }
}

/// Exposes class factory.
///
/// See: https://docs.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-dllgetclassobject
#[no_mangle]
extern "system" fn DllGetClassObject(
    class_id: guiddef::REFCLSID,
    iid: guiddef::REFIID,
    result: *mut win::LPVOID,
) -> winnt::HRESULT {
    let class_guid = guid_from_ref(class_id);
    let interface_guid = guid_from_ref(iid);
    // expect our registered class ID
    if wslscript_common::DROP_HANDLER_CLSID.eq(&class_guid) {
        // expect IClassFactory interface to be requested
        if !CLASS_FACTORY_CLSID.eq(&interface_guid) {
            log::warn!("Expected IClassFactory, got {}", interface_guid);
        }
        let cls: Com::IClassFactory = Handler::default().into();
        let rv = unsafe { cls.query(iid as _, result as _) };
        log::debug!(
            "QueryInterface for {} returned {}, address={:p}",
            interface_guid,
            rv,
            result
        );
        return rv.0;
    } else {
        log::warn!("Unsupported class: {}", class_guid);
    }
    winerror::CLASS_E_CLASSNOTAVAILABLE
}

/// Add in-process server keys into registry.
///
/// See: https://docs.microsoft.com/en-us/windows/win32/api/olectl/nf-olectl-dllregisterserver
#[no_mangle]
extern "system" fn DllRegisterServer() -> winnt::HRESULT {
    let hinstance = unsafe { DLL_HANDLE };
    let path = match get_module_path(hinstance) {
        Ok(p) => p,
        Err(_) => return winerror::E_UNEXPECTED,
    };
    log::debug!("DllRegisterServer for {}", path.to_string_lossy());
    match wslscript_common::registry::add_server_to_registry(&path) {
        Ok(_) => (),
        Err(e) => {
            log::error!("Failed to register server: {}", e);
            return winerror::E_UNEXPECTED;
        }
    }
    winerror::S_OK
}

/// Remove in-process server keys from registry.
///
/// See: https://docs.microsoft.com/en-us/windows/win32/api/olectl/nf-olectl-dllunregisterserver
#[no_mangle]
extern "system" fn DllUnregisterServer() -> winnt::HRESULT {
    match wslscript_common::registry::remove_server_from_registry() {
        Ok(_) => (),
        Err(e) => {
            log::error!("Failed to unregister server: {}", e);
            return winerror::E_UNEXPECTED;
        }
    }
    winerror::S_OK
}

/// Convert Win32 GUID pointer to Guid struct.
const fn guid_from_ref(clsid: *const guiddef::GUID) -> Guid {
    Guid {
        0: unsafe { *clsid },
    }
}

/// Get path to loaded DLL file.
fn get_module_path(hinstance: win::HINSTANCE) -> Result<PathBuf, Error> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use winapi::shared::ntdef;
    use winapi::um::libloaderapi::GetModuleFileNameW as GetModuleFileName;
    let mut buf: Vec<ntdef::WCHAR> = Vec::with_capacity(win::MAX_PATH);
    let len = unsafe { GetModuleFileName(hinstance, buf.as_mut_ptr(), buf.capacity() as _) };
    if len == 0 {
        return Err(wslscript_common::win32::last_error());
    }
    unsafe { buf.set_len(len as _) };
    Ok(PathBuf::from(OsString::from_wide(&buf)))
}

bitflags::bitflags! {
    /// Key state flags.
    #[derive(Debug)]
    pub struct KeyState: win::DWORD {
        const MK_CONTROL = winuser::MK_CONTROL as win::DWORD;
        const MK_SHIFT = winuser::MK_SHIFT as win::DWORD;
        const MK_ALT = oleidl::MK_ALT as win::DWORD;
        const MK_LBUTTON = winuser::MK_LBUTTON as win::DWORD;
        const MK_MBUTTON = winuser::MK_MBUTTON as win::DWORD;
        const MK_RBUTTON = winuser::MK_RBUTTON as win::DWORD;
    }
}

#[wc::implement(Com::IClassFactory, Com::IPersistFile, Ole::IDropTarget)]
#[derive(Default)]
struct Handler {
    target: RefCell<PathBuf>,
}

/// IClassFactory interface.
///
/// https://learn.microsoft.com/en-us/windows/win32/api/unknwn/nn-unknwn-iclassfactory
impl Com::IClassFactory_Impl for Handler {
    /// https://learn.microsoft.com/en-us/windows/win32/api/unknwn/nf-unknwn-iclassfactory-createinstance
    fn CreateInstance(
        &self,
        punkouter: Option<&wc::IUnknown>,
        riid: *const wc::GUID,
        ppvobject: *mut *mut ::core::ffi::c_void,
    ) -> wc::Result<()> {
        log::debug!("IClassFactory::CreateInstance");
        if punkouter.is_some() {
            return Err(wc::Error::from(Foundation::CLASS_E_NOAGGREGATION));
        }
        unsafe { *ppvobject = core::ptr::null_mut() };
        if riid.is_null() {
            return Err(wc::Error::from(Foundation::E_INVALIDARG));
        }
        unsafe { self.cast::<wc::IUnknown>()?.query(riid, ppvobject).ok() }
    }

    /// https://learn.microsoft.com/en-us/windows/win32/api/unknwn/nf-unknwn-iclassfactory-lockserver
    fn LockServer(&self, _flock: Foundation::BOOL) -> wc::Result<()> {
        log::debug!("IClassFactory::LockServer");
        Err(wc::Error::from(Foundation::E_NOTIMPL))
    }
}

/// IPersist interface.
///
/// https://learn.microsoft.com/en-us/windows/win32/api/objidl/nn-objidl-ipersist
impl Com::IPersist_Impl for Handler {
    fn GetClassID(&self) -> wc::Result<wc::GUID> {
        log::debug!("IPersist::GetClassID");
        let guid = wslscript_common::DROP_HANDLER_CLSID.0;
        wc::Result::Ok(wc::GUID::from_values(
            guid.Data1, guid.Data2, guid.Data3, guid.Data4,
        ))
    }
}

/// IPersistFile interface.
///
/// https://learn.microsoft.com/en-us/windows/win32/api/objidl/nn-objidl-ipersistfile
impl Com::IPersistFile_Impl for Handler {
    fn IsDirty(&self) -> wc::HRESULT {
        log::debug!("IPersistFile::IsDirty");
        Foundation::S_FALSE
    }

    fn Load(&self, pszfilename: &wc::PCWSTR, _dwmode: Com::STGM) -> wc::Result<()> {
        // path to the file that is being dragged over, ie. the registered script file
        let filename = unsafe { WideCStr::from_ptr_str(pszfilename.as_ptr()) };
        let path = PathBuf::from(filename.to_os_string());
        log::debug!("IPersistFile::Load {}", path.to_string_lossy());
        if let Ok(mut target) = self.target.try_borrow_mut() {
            *target = path;
        } else {
            return Err(wc::Error::from(Foundation::E_FAIL));
        }
        Ok(())
    }

    fn Save(&self, _pszfilename: &wc::PCWSTR, _fremember: Foundation::BOOL) -> wc::Result<()> {
        log::debug!("IPersistFile::Save");
        Err(wc::Error::from(Foundation::S_FALSE))
    }

    fn SaveCompleted(&self, _pszfilename: &wc::PCWSTR) -> wc::Result<()> {
        log::debug!("IPersistFile::SaveCompleted");
        Ok(())
    }

    fn GetCurFile(&self) -> wc::Result<wc::PWSTR> {
        // TODO: return target file
        // https://learn.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-ipersistfile-getcurfile#remarks
        log::debug!("IPersistFile::GetCurFile");
        Err(wc::Error::from(Foundation::E_FAIL))
    }
}

/// IDropTarget interface.
///
/// https://learn.microsoft.com/en-us/windows/win32/api/oleidl/nn-oleidl-idroptarget
impl Ole::IDropTarget_Impl for Handler {
    fn DragEnter(
        &self,
        _pdataobj: Option<&Com::IDataObject>,
        _grfkeystate: SystemServices::MODIFIERKEYS_FLAGS,
        _pt: &Foundation::POINTL,
        _pdweffect: *mut Ole::DROPEFFECT,
    ) -> wc::Result<()> {
        log::debug!("IDropTarget::DragEnter");
        Ok(())
    }

    fn DragOver(
        &self,
        grfkeystate: SystemServices::MODIFIERKEYS_FLAGS,
        _pt: &Foundation::POINTL,
        _pdweffect: *mut Ole::DROPEFFECT,
    ) -> wc::Result<()> {
        log::debug!(
            "IDropTarget::DragOver {:?}",
            KeyState::from_bits_truncate(grfkeystate.0)
        );
        Ok(())
    }

    fn DragLeave(&self) -> wc::Result<()> {
        log::debug!("IDropTarget::DragLeave");
        Ok(())
    }

    fn Drop(
        &self,
        pdataobj: Option<&Com::IDataObject>,
        grfkeystate: SystemServices::MODIFIERKEYS_FLAGS,
        _pt: &Foundation::POINTL,
        pdweffect: *mut Ole::DROPEFFECT,
    ) -> wc::Result<()> {
        log::debug!("IDropTarget::Drop");
        let target = match self.target.try_borrow() {
            Ok(t) => t.clone(),
            Err(_) => return Err(wc::Error::from(Foundation::E_UNEXPECTED)),
        };
        let obj = match pdataobj {
            None => return Err(wc::Error::from(Foundation::E_UNEXPECTED)),
            Some(o) => unsafe { std::mem::transmute(o.as_raw()) },
        };
        let keys = KeyState::from_bits_truncate(grfkeystate.0);
        match super::handle_dropped_files(&target, obj, keys) {
            Ok(_) => {
                unsafe {
                    *pdweffect = Ole::DROPEFFECT_COPY;
                }
                Ok(())
            }
            Err(e) => {
                log::debug!("Drop failed: {}", e);
                Err(wc::Error::from(Foundation::E_UNEXPECTED))
            }
        }
    }
}
