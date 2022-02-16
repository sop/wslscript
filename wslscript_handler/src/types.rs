use winapi::shared::minwindef;
use winapi::shared::windef;
use winapi::um::objidl;
use winapi::um::unknwnbase;

/// Correct STGMEDIUM structure - union is not a pointer.
/// See https://docs.microsoft.com/en-us/windows/win32/api/objidl/ns-objidl-ustgmedium-r1
/// TODO: open an issue to winapi-rs github
#[repr(C)]
#[allow(non_snake_case)]
pub struct STGMEDIUM {
    pub tymed: minwindef::DWORD,
    pub u: objidl::STGMEDIUM_u,
    pub pUnkForRelease: *mut unknwnbase::IUnknown,
}

/// https://docs.microsoft.com/en-us/windows/win32/api/shlobj_core/ns-shlobj_core-dropfiles
#[repr(C)]
#[allow(non_snake_case)]
pub struct DROPFILES {
    /// The offset of the file list from the beginning of this structure, in bytes.
    pub pFiles: minwindef::DWORD,
    pub pt: windef::POINT,
    pub fNC: minwindef::BOOL,
    pub fWide: minwindef::BOOL,
}
