//! All the nitty gritty details regarding COM interface for the shell extension
//! are defined here.
//!
//! See: https://docs.microsoft.com/en-us/windows/win32/shell/handlers#implementing-shell-extension-handlers

use com::sys::HRESULT;
use guid_win::Guid;
use once_cell::sync::Lazy;
use std::cell::RefCell;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use wchar::*;
use widestring::WideCStr;
use winapi::shared::guiddef;
use winapi::shared::minwindef as win;
use winapi::shared::windef;
use winapi::shared::winerror;
use winapi::shared::wtypesbase;
use winapi::um::objidl;
use winapi::um::oleidl;
use winapi::um::winnt;
use winapi::um::winuser;
use wslscript_common::error::*;
use wslscript_common::wcstring;

/// IClassFactory GUID.
///
/// See: https://docs.microsoft.com/en-us/windows/win32/api/unknwn/nn-unknwn-iclassfactory
///
/// Windows requests this interface via `DllGetClassObject` to further query
/// relevant COM interfaces. _com-rs_ crate implements IClassFactory automatically
/// for all interfaces (?), so we don't need to worry about details.
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
                        let text = wcstring(format!(
                            "Failed to set up logging to {}",
                            path.to_string_lossy()
                        ));
                        MessageBoxW(
                            std::ptr::null_mut(),
                            text.as_ptr(),
                            wchz!("Error").as_ptr(),
                            MB_OK | MB_ICONERROR | MB_SERVICE_NOTIFICATION,
                        );
                    }
                }
            }
            log::debug!("DLL_PROCESS_ATTACH");
            return win::TRUE;
        }
        winnt::DLL_PROCESS_DETACH => {}
        winnt::DLL_THREAD_ATTACH => {}
        winnt::DLL_THREAD_DETACH => {
            log::debug!("DLL_THREAD_DETACH");
        }
        _ => {}
    }
    win::FALSE
}

/// Called to check whether DLL can be unloaded from memory.
///
/// See: https://docs.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-dllcanunloadnow
#[no_mangle]
extern "system" fn DllCanUnloadNow() -> HRESULT {
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
) -> HRESULT {
    let class_guid = guid_from_ref(class_id);
    let interface_guid = guid_from_ref(iid);
    // expect our registered class ID
    if wslscript_common::DROP_HANDLER_CLSID.eq(&class_guid) {
        // expect IClassFactory interface to be requested
        if !CLASS_FACTORY_CLSID.eq(&interface_guid) {
            log::warn!("Expected IClassFactory, got {}", interface_guid);
        }
        use com::production::Class as COMClass;
        let cls = <Handler as COMClass>::Factory::allocate();
        let rv = unsafe { cls.QueryInterface(iid as _, result as _) };
        log::debug!(
            "QueryInterface for {} returned {}, address={:p}",
            interface_guid,
            rv,
            result
        );
        return rv;
    } else {
        log::warn!("Unsupported class: {}", class_guid);
    }
    winerror::CLASS_E_CLASSNOTAVAILABLE
}

/// Add in-process server keys into registry.
///
/// See: https://docs.microsoft.com/en-us/windows/win32/api/olectl/nf-olectl-dllregisterserver
#[no_mangle]
extern "system" fn DllRegisterServer() -> HRESULT {
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
extern "system" fn DllUnregisterServer() -> HRESULT {
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
    pub struct KeyState: win::DWORD {
        const MK_CONTROL = winuser::MK_CONTROL as win::DWORD;
        const MK_SHIFT = winuser::MK_SHIFT as win::DWORD;
        const MK_ALT = oleidl::MK_ALT as win::DWORD;
        const MK_LBUTTON = winuser::MK_LBUTTON as win::DWORD;
        const MK_MBUTTON = winuser::MK_MBUTTON as win::DWORD;
        const MK_RBUTTON = winuser::MK_RBUTTON as win::DWORD;
    }
}

// COM interface declarations.
//
// Note that methods must be in exact order!
//
// See https://www.magnumdb.com/ for interface GUID's.
// See https://docs.microsoft.com/en-us/windows/win32/shell/handlers for
// required interfaces.
com::interfaces! {
    // NOTE: class! macro generates IClassFactory interface automatically,
    // so we must directly inherit from IUnknown.
    #[uuid("81521ebe-a2d4-450b-9bf8-5c23ed8730d0")]
    pub unsafe interface IHandler : com::interfaces::IUnknown {}

    #[uuid("0000010b-0000-0000-c000-000000000046")]
    pub unsafe interface IPersistFile : IPersist {
        fn IsDirty(&self) -> HRESULT;

        fn Load(
            &self,
            pszFileName: wtypesbase::LPCOLESTR,
            dwMode: win::DWORD,
        ) -> HRESULT;

        fn Save(
            &self,
            pszFileName: wtypesbase::LPCOLESTR,
            fRemember: win::BOOL,
        ) -> HRESULT;

        fn SaveCompleted(
            &self,
            pszFileName: wtypesbase::LPCOLESTR,
        ) -> HRESULT;

        fn GetCurFile(
            &self,
            ppszFileName: *mut wtypesbase::LPOLESTR,
        ) -> HRESULT;
    }

    #[uuid("0000010c-0000-0000-c000-000000000046")]
    pub unsafe interface IPersist : com::interfaces::IUnknown {
        fn GetClassID(
            &self,
            pClassID: *mut guiddef::CLSID,
        ) -> HRESULT;
    }

    #[uuid("00000122-0000-0000-c000-000000000046")]
    pub unsafe interface IDropTarget: com::interfaces::IUnknown {
        fn DragEnter(
            &self,
            pDataObj: *const objidl::IDataObject,
            grfKeyState: win::DWORD,
            pt: *const windef::POINTL,
            pdwEffect: *mut win::DWORD,
        ) -> HRESULT;

        fn DragOver(
            &self,
            grfKeyState: win::DWORD,
            pt: *const windef::POINTL,
            pdwEffect: *mut win::DWORD,
        ) -> HRESULT;

        fn DragLeave(&self) -> HRESULT;

        fn Drop(
            &self,
            pDataObj: *const objidl::IDataObject,
            grfKeyState: win::DWORD,
            pt: *const windef::POINTL,
            pdwEffect: *mut win::DWORD,
        ) -> HRESULT;
    }
}

com::class! {
    pub class Handler: IHandler, IPersistFile(IPersist), IDropTarget {
        // File that is receiving the drop.
        target: RefCell<PathBuf>
    }

    impl IHandler for Handler {
    }

    // See: https://docs.microsoft.com/en-us/windows/win32/api/objidl/nn-objidl-ipersistfile
    impl IPersistFile for Handler {
        /// See: https://docs.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-ipersistfile-isdirty
        fn IsDirty(&self) -> HRESULT {
            log::debug!("IPersistFile::IsDirty");
            winerror::S_FALSE
        }

        /// See: https://docs.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-ipersistfile-load
        fn Load(
            &self,
            pszFileName: wtypesbase::LPCOLESTR,
            _dwMode: win::DWORD,
        ) -> HRESULT {
            // path to the file that is being dragged over, ie. the registered script file
            let filename = unsafe { WideCStr::from_ptr_str(pszFileName) };
            let path = PathBuf::from(filename.to_os_string());
            log::debug!("IPersistFile::Load {}", path.to_string_lossy());
            if let Ok(mut target) = self.target.try_borrow_mut() {
                *target = path;
            } else {
                return winerror::E_FAIL;
            }
            winerror::S_OK
        }

        /// See: https://docs.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-ipersistfile-save
        fn Save(
            &self,
            _pszFileName: wtypesbase::LPCOLESTR,
            _fRemember: win::BOOL,
        ) -> HRESULT {
            log::debug!("IPersistFile::Save");
            winerror::S_FALSE
        }

        /// See: https://docs.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-ipersistfile-savecompleted
        fn SaveCompleted(
            &self,
            _pszFileName: wtypesbase::LPCOLESTR,
        ) -> HRESULT {
            log::debug!("IPersistFile::SaveCompleted");
            winerror::S_OK
        }

        /// See: https://docs.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-ipersistfile-getcurfile
        fn GetCurFile(
            &self,
            _ppszFileName: *mut wtypesbase::LPOLESTR,
        ) -> HRESULT {
            log::debug!("IPersistFile::GetCurFile");
            winerror::E_FAIL
        }
    }

    // See: https://docs.microsoft.com/en-us/windows/win32/api/objidl/nn-objidl-ipersist
    impl IPersist for Handler {
        /// See: https://docs.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-ipersist-getclassid
        fn GetClassID(
            &self,
            pClassID: *mut guiddef::CLSID,
        ) -> HRESULT {
            log::debug!("IPersist::GetClassID");
            let guid = wslscript_common::DROP_HANDLER_CLSID.0;
            unsafe { *pClassID = guid }
            winerror::S_OK
        }
    }

    // See: https://docs.microsoft.com/en-us/windows/win32/api/oleidl/nn-oleidl-idroptarget
    impl IDropTarget for Handler {
        /// See: https://docs.microsoft.com/en-us/windows/win32/api/oleidl/nf-oleidl-idroptarget-dragenter
        fn DragEnter(
            &self,
            _pDataObj: *const objidl::IDataObject,
            _grfKeyState: win::DWORD,
            _pt: *const windef::POINTL,
            _pdwEffect: *mut win::DWORD,
        ) -> HRESULT {
            log::debug!("IDropTarget::DragEnter");
            winerror::S_OK
        }

        /// See: https://docs.microsoft.com/en-us/windows/win32/api/oleidl/nf-oleidl-idroptarget-dragover
        fn DragOver(
            &self,
            _grfKeyState: win::DWORD,
            _pt: *const windef::POINTL,
            _pdwEffect: *mut win::DWORD,
        ) -> HRESULT {
            log::debug!("IDropTarget::DragOver");
            log::debug!("Keys {:?}", KeyState::from_bits_truncate(_grfKeyState));
            winerror::S_OK
        }

        /// See: https://docs.microsoft.com/en-us/windows/win32/api/oleidl/nf-oleidl-idroptarget-dragleave
        fn DragLeave(&self) -> HRESULT {
            log::debug!("IDropTarget::DragLeave");
            winerror::S_OK
        }

        /// See: https://docs.microsoft.com/en-us/windows/win32/api/oleidl/nf-oleidl-idroptarget-drop
        fn Drop(
            &self,
            pDataObj: *const objidl::IDataObject,
            grfKeyState: win::DWORD,
            _pt: *const windef::POINTL,
            pdwEffect: *mut win::DWORD,
        ) -> HRESULT {
            log::debug!("IDropTarget::Drop");
            let target = if let Ok(target) = self.target.try_borrow() {
                target.clone()
            } else {
                return winerror::E_UNEXPECTED;
            };
            let obj = unsafe { &*pDataObj };
            let keys = KeyState::from_bits_truncate(grfKeyState);
            super::handle_dropped_files(&target, obj, keys).and_then(|_| {
                unsafe { *pdwEffect = oleidl::DROPEFFECT_COPY; }
                Ok(winerror::S_OK)
            }).unwrap_or_else(|e| {
                log::debug!("Drop failed: {}", e);
                winerror::E_UNEXPECTED
            })
        }
    }
}
