use std::path::{Path, PathBuf};
use widestring::UCStr;
use winapi::shared::ntdef;
use winapi::shared::winerror;
use winapi::um::objidl;
use winapi::um::unknwnbase::IUnknown;
use winapi::um::winbase;
use winapi::um::winuser;
use wslscript_common::error::*;
use wslscript_common::win32;
use wslscript_common::wsl;

mod interface;
mod types;

/// Handle files dropped to registered filetype.
///
/// See: https://docs.microsoft.com/en-us/windows/win32/api/oleidl/nf-oleidl-idroptarget-drop
fn handle_dropped_files(target: &PathBuf, data_obj: &objidl::IDataObject) -> Result<(), Error> {
    log::debug!("Dropped to {}", target.to_string_lossy());
    let opts = get_wsl_options(target)?;
    let mut paths = vec![target.clone()];
    let mut args = get_paths_from_data_obj(data_obj)?;
    log::debug!("{} files received", args.len());
    paths.append(&mut args);
    let paths = wsl::paths_to_wsl(&paths, &opts)?;
    if paths.len() < 2 {
        return Err(Error::from(ErrorKind::LogicError {
            s: "Not enough arguments",
        }));
    }
    // TODO: run in a thread, now explorer shows loading icon while wsl is running
    // if temporary file removal is being waited
    wsl::run_wsl(&paths[0], &paths[1..], &opts)
}

/// Get WSL options from registry based on given filename's extension.
fn get_wsl_options(path: &Path) -> Result<wsl::WSLOptions, Error> {
    path.extension()
        .ok_or_else(|| {
            Error::from(ErrorKind::DropHandlerError {
                s: "No filename extension".to_owned(),
            })
        })
        .and_then(|s| {
            wsl::WSLOptions::from_ext(&s.to_string_lossy()).ok_or_else(|| {
                Error::from(ErrorKind::DropHandlerError {
                    s: format!("Extension {} not registered.", s.to_string_lossy()),
                })
            })
        })
}

/// Query IDataObject for dropped file names.
fn get_paths_from_data_obj(obj: &objidl::IDataObject) -> Result<Vec<PathBuf>, Error> {
    let format = objidl::FORMATETC {
        // https://docs.microsoft.com/en-us/windows/win32/shell/clipboard#cf_hdrop
        cfFormat: winuser::CF_HDROP as _,
        ptd: std::ptr::null(),
        dwAspect: winapi::shared::wtypes::DVASPECT_CONTENT,
        lindex: -1,
        tymed: objidl::TYMED_HGLOBAL,
    };
    let mut medium = unsafe { std::mem::zeroed::<types::STGMEDIUM>() };
    // https://docs.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-idataobject-getdata
    let rv = unsafe { obj.GetData(&format, &mut medium as *mut _ as *mut _) };
    if rv != winerror::S_OK {
        return Err(Error::from(ErrorKind::DropHandlerError {
            s: format!("IDataObject::GetData returned 0x{:X}.", rv),
        }));
    }
    if medium.tymed != objidl::TYMED_HGLOBAL {
        return Err(Error::from(ErrorKind::DropHandlerError {
            s: format!(
                "IDataObject::GetData returned unexpected medium type {}.",
                medium.tymed
            ),
        }));
    }
    let ptr = unsafe { *medium.u.hGlobal() };
    let dropfiles = unsafe { &*(ptr as *const types::DROPFILES) as &types::DROPFILES };
    if dropfiles.fWide == 0 {
        return Err(Error::from(ErrorKind::DropHandlerError {
            s: format!("ANSI not supported."),
        }));
    }
    // file name array follows the DROPFILES structure
    let farray = unsafe { ptr.cast::<u8>().offset(dropfiles.pFiles as _) };
    let paths = parse_filename_array(farray as *const ntdef::WCHAR);
    if medium.pUnkForRelease == std::ptr::null_mut() {
        log::debug!("No release interface, calling GlobalFree()");
        let rv = unsafe { winbase::GlobalFree(ptr) };
        if rv != std::ptr::null_mut() {
            log::debug!("GlobalFree failed: {}", win32::last_error());
        }
    } else {
        log::debug!("Calling IUnknown::Release()");
        unsafe {
            let unk: &IUnknown = &*(medium.pUnkForRelease);
            unk.Release();
        }
    }
    Ok(paths)
}

/// Parse file name array to list of paths.
///
/// See: https://docs.microsoft.com/en-us/windows/win32/shell/clipboard#cf_hdrop
fn parse_filename_array(mut ptr: *const ntdef::WCHAR) -> Vec<PathBuf> {
    let mut paths = Vec::<PathBuf>::new();
    loop {
        let s = unsafe { UCStr::from_ptr_str(ptr) };
        if s.is_empty() {
            break;
        }
        ptr = unsafe { ptr.offset(s.len() as isize + 1) };
        paths.push(PathBuf::from(s.to_os_string()));
    }
    paths
}
