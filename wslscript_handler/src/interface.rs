use com::sys::HRESULT;
use guid_win::Guid;
use once_cell::sync::Lazy;
use std::cell::RefCell;
use std::path::PathBuf;
use std::str::FromStr;
use wchar::*;
use widestring::WideCString;
use winapi::shared::guiddef;
use winapi::shared::minwindef as win;
use winapi::shared::windef;
use winapi::shared::winerror;
use winapi::shared::wtypesbase;
use winapi::um::objidl;
use winapi::um::oleidl;
use winapi::um::winnt;
use wslscript_common::wcstring;

/// Our shell extension GUID: {81521ebe-a2d4-450b-9bf8-5c23ed8730d0}
static HANDLER_CLSID: Lazy<Guid> =
    Lazy::new(|| Guid::from_str("81521ebe-a2d4-450b-9bf8-5c23ed8730d0").unwrap());

/// IClassFactory GUID
static CLASS_FACTORY_CLSID: Lazy<Guid> =
    Lazy::new(|| Guid::from_str("00000001-0000-0000-c000-000000000046").unwrap());

// https://docs.microsoft.com/en-us/windows/win32/dlls/dllmain
#[no_mangle]
extern "system" fn DllMain(
    _hinstance: win::HINSTANCE,
    reason: win::DWORD,
    _reserved: win::LPVOID,
) -> win::BOOL {
    match reason {
        winnt::DLL_PROCESS_ATTACH => {
            // set up logging
            #[cfg(feature = "debug")]
            {
                if let Some(mut path) = get_module_directory(_hinstance) {
                    path.push("wslscript_handler.log");
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

// https://docs.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-dllcanunloadnow
#[no_mangle]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    log::debug!("DllCanUnloadNow");
    winerror::S_OK
}

// https://docs.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-dllgetclassobject
#[no_mangle]
extern "system" fn DllGetClassObject(
    class_id: guiddef::REFCLSID,
    iid: guiddef::REFIID,
    result: *mut win::LPVOID,
) -> HRESULT {
    let class_guid = guid_from_ref(class_id);
    let interface_guid = guid_from_ref(iid);
    // expect our registered class ID
    if HANDLER_CLSID.eq(&class_guid) {
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

/// Convert Win32 GUID pointer to Guid struct.
const fn guid_from_ref(clsid: guiddef::REFCLSID) -> Guid {
    Guid {
        0: unsafe { *(clsid as *const guiddef::GUID) },
    }
}

/// Get directory for the module instance.
#[cfg(feature = "debug")]
fn get_module_directory(hinstance: win::HINSTANCE) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use winapi::shared::ntdef;
    use winapi::um::libloaderapi::GetModuleFileNameW as GetModuleFileName;
    let mut buf: Vec<ntdef::WCHAR> = Vec::with_capacity(win::MAX_PATH);
    unsafe {
        let len = GetModuleFileName(hinstance, buf.as_mut_ptr(), buf.capacity() as _);
        buf.set_len(len as _);
    }
    let mut path = PathBuf::from(OsString::from_wide(&buf));
    path.pop();
    Some(path)
}

// See https://www.magnumdb.com/ for interface GUID's.
// https://docs.microsoft.com/en-us/windows/win32/shell/handlers
com::interfaces! {
    // NOTE: class! macro generates IClassFactory interface automatically,
    // so we must directly inherit from IUnknown.
    #[uuid("81521ebe-a2d4-450b-9bf8-5c23ed8730d0")]
    pub unsafe interface IHandler : com::interfaces::IUnknown {

    }

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

    // https://docs.microsoft.com/en-us/windows/win32/api/objidl/nn-objidl-ipersistfile
    impl IPersistFile for Handler {
        /// https://docs.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-ipersistfile-isdirty
        fn IsDirty(&self) -> HRESULT {
            log::debug!("IPersistFile::IsDirty");
            winerror::S_FALSE
        }

        /// https://docs.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-ipersistfile-load
        fn Load(
            &self,
            pszFileName: wtypesbase::LPCOLESTR,
            _dwMode: win::DWORD,
        ) -> HRESULT {
            // path to the file that received the drop, ie. the script file
            let path = unsafe {
                PathBuf::from(WideCString::from_ptr_str(pszFileName).to_os_string())
            };
            log::debug!("IPersistFile::Load {}", path.to_string_lossy());
            if let Ok(mut target) = self.target.try_borrow_mut() {
                *target = path;
            } else {
                return winerror::E_FAIL;
            }
            winerror::S_OK
        }

        /// https://docs.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-ipersistfile-save
        fn Save(
            &self,
            _pszFileName: wtypesbase::LPCOLESTR,
            _fRemember: win::BOOL,
        ) -> HRESULT {
            log::debug!("IPersistFile::Save");
            winerror::S_FALSE
        }

        /// https://docs.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-ipersistfile-savecompleted
        fn SaveCompleted(
            &self,
            _pszFileName: wtypesbase::LPCOLESTR,
        ) -> HRESULT {
            log::debug!("IPersistFile::SaveCompleted");
            winerror::S_OK
        }

        /// https://docs.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-ipersistfile-getcurfile
        fn GetCurFile(
            &self,
            _ppszFileName: *mut wtypesbase::LPOLESTR,
        ) -> HRESULT {
            log::debug!("IPersistFile::GetCurFile");
            winerror::E_FAIL
        }
    }

    // https://docs.microsoft.com/en-us/windows/win32/api/objidl/nn-objidl-ipersist
    impl IPersist for Handler {
        /// https://docs.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-ipersist-getclassid
        fn GetClassID(
            &self,
            pClassID: *mut guiddef::CLSID,
        ) -> HRESULT {
            log::debug!("IPersist::GetClassID");
            let guid = HANDLER_CLSID.0;
            unsafe { *pClassID = guid }
            winerror::S_OK
        }
    }

    // https://docs.microsoft.com/en-us/windows/win32/api/oleidl/nn-oleidl-idroptarget
    impl IDropTarget for Handler {
        /// https://docs.microsoft.com/en-us/windows/win32/api/oleidl/nf-oleidl-idroptarget-dragenter
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

        /// https://docs.microsoft.com/en-us/windows/win32/api/oleidl/nf-oleidl-idroptarget-dragover
        fn DragOver(
            &self,
            _grfKeyState: win::DWORD,
            _pt: *const windef::POINTL,
            _pdwEffect: *mut win::DWORD,
        ) -> HRESULT {
            log::debug!("IDropTarget::DragOver");
            winerror::S_OK
        }

        /// https://docs.microsoft.com/en-us/windows/win32/api/oleidl/nf-oleidl-idroptarget-dragleave
        fn DragLeave(&self) -> HRESULT {
            log::debug!("IDropTarget::DragLeave");
            winerror::S_OK
        }

        /// https://docs.microsoft.com/en-us/windows/win32/api/oleidl/nf-oleidl-idroptarget-drop
        fn Drop(
            &self,
            pDataObj: *const objidl::IDataObject,
            _grfKeyState: win::DWORD,
            _pt: *const windef::POINTL,
            pdwEffect: *mut win::DWORD,
        ) -> HRESULT {
            log::debug!("IDropTarget::Drop");
            if let Ok(target) = self.target.try_borrow() {
                let obj = unsafe { &*pDataObj };
                super::handle_dropped_files(&target, obj).and_then(|_| {
                    unsafe { *pdwEffect = oleidl::DROPEFFECT_COPY; }
                    Ok(winerror::S_OK)
                }).unwrap_or_else(|e| {
                    log::debug!("Drop failed: {}", e);
                    winerror::E_UNEXPECTED
                })
            } else {
                winerror::E_UNEXPECTED
            }
        }
    }
}
