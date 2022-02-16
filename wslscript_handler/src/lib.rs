use std::path::PathBuf;
use widestring::UCStr;
use winapi::shared::ntdef;
use winapi::shared::winerror;
use winapi::um::objidl;
use winapi::um::winbase;
use winapi::um::winuser;

mod interface;
mod types;

/// Handle files dropped to registered filetype.
fn handle_dropped_files(target: &PathBuf, data_obj: &objidl::IDataObject) -> Result<(), ()> {
    log::debug!("Dropped to {}", target.to_string_lossy());
    let format = objidl::FORMATETC {
        // https://docs.microsoft.com/en-us/windows/win32/shell/clipboard#cf_hdrop
        cfFormat: winuser::CF_HDROP as _,
        ptd: std::ptr::null(),
        dwAspect: winapi::shared::wtypes::DVASPECT_CONTENT,
        lindex: -1,
        tymed: objidl::TYMED_HGLOBAL,
    };
    let mut medium = unsafe { std::mem::zeroed::<types::STGMEDIUM>() };
    let rv = unsafe { data_obj.GetData(&format, &mut medium as *mut _ as *mut _) };
    if rv != winerror::S_OK {
        log::debug!("IDataObject::GetData returned 0x{:X}", rv);
        return Err(());
    }
    if medium.tymed != objidl::TYMED_HGLOBAL {
        log::debug!(
            "IDataObject::GetData returned unexpected medium type {}",
            medium.tymed
        );
        return Err(());
    }
    let ptr = unsafe { *medium.u.hGlobal() };
    let dropfiles = unsafe { &*(ptr as *const types::DROPFILES) as &types::DROPFILES };
    // file name array follows the DROPFILES structure
    let farray = unsafe { ptr.cast::<u8>().offset(dropfiles.pFiles as _) };
    let paths = if dropfiles.fWide == 0 {
        log::debug!("ANSI not supported");
        vec![]
    } else {
        parse_filename_array(farray as *const ntdef::WCHAR)
    };
    log::debug!("{} files received", paths.len());
    if medium.pUnkForRelease == std::ptr::null_mut() {
        log::debug!("NULL pUnkForRelease, releasing memory");
        let rv = unsafe { winbase::GlobalFree(ptr) };
        if rv != std::ptr::null_mut() {
            log::debug!("GlobalFree failed");
        }
    }
    Ok(())
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
