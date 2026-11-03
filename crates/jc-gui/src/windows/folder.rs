//! Native filesystem-folder picker. Invoke on the HWND's UI thread, outside any
//! State/Workbench borrow: the Shell's modal loop can reenter window callbacks.
//!
//! ABI/lifetime references (Microsoft primary documentation):
//! https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/ns-shlobj_core-browseinfow
//! https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shbrowseforfolderw
//! https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shgetpathfromidlistex
//! https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-coinitializeex
use super::ffi::Hwnd;
use std::{
    ffi::{c_void, OsString},
    marker::PhantomData,
    os::windows::ffi::OsStringExt,
    path::PathBuf,
    ptr::{null, null_mut, NonNull},
    rc::Rc,
};

const COINIT_APARTMENTTHREADED: u32 = 0x2;
const COINIT_DISABLE_OLE1DDE: u32 = 0x4;
const RPC_E_CHANGED_MODE: u32 = 0x8001_0106;
const BIF_RETURNONLYFSDIRS: u32 = 0x1;
const BIF_EDITBOX: u32 = 0x10;
const BIF_NEWDIALOGSTYLE: u32 = 0x40;
const BIF_NONEWFOLDERBUTTON: u32 = 0x200;
const PATH_UNITS: usize = 32_768;

type BrowseCallback = unsafe extern "system" fn(Hwnd, u32, isize, isize) -> i32;
#[repr(C)]
struct BrowseInfo {
    owner: Hwnd,
    root: *const c_void,
    display_name: *mut u16,
    title: *const u16,
    flags: u32,
    callback: Option<BrowseCallback>,
    parameter: isize,
    image: i32,
}

#[link(name = "shell32")]
extern "system" {
    fn SHBrowseForFolderW(info: *mut BrowseInfo) -> *mut c_void;
    fn SHGetPathFromIDListEx(
        pidl: *const c_void,
        path: *mut u16,
        path_units: u32,
        flags: u32,
    ) -> i32;
}
#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
    fn CoUninitialize();
    fn CoTaskMemFree(memory: *mut c_void);
}

// Successful CoInitializeEx, including S_FALSE, owns precisely one matching
// CoUninitialize. !Send/!Sync prevents releasing an apartment on another thread.
struct Apartment(PhantomData<Rc<()>>);
impl Apartment {
    unsafe fn enter() -> Result<Self, String> {
        let result = CoInitializeEx(
            null_mut(),
            COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE,
        );
        if result >= 0 {
            Ok(Self(PhantomData))
        } else if result as u32 == RPC_E_CHANGED_MODE {
            Err("Folder picker requires a COM STA UI thread; this thread already has an incompatible apartment".into())
        } else {
            Err(format!(
                "COM initialization for folder picker failed (HRESULT {:#010x})",
                result as u32
            ))
        }
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}

struct SelectedFolder(NonNull<c_void>);
impl Drop for SelectedFolder {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(self.0.as_ptr()) }
    }
}

/// Returns an absolute native filesystem-directory path, retaining UTF-16 path
/// units through OsString (including units not representable by Rust String).
///
/// The caller must supply its own live HWND on this UI thread, with no model
/// borrow held across this modal operation. No worker, source read, command,
/// project mutation, or background selection is started here.
pub(super) unsafe fn choose(hwnd: Hwnd) -> Result<Option<PathBuf>, String> {
    if hwnd.is_null() {
        return Err("Folder picker requires an owning native window".into());
    }
    let _apartment = Apartment::enter()?;
    // BROWSEINFO's display-name buffer is MAX_PATH, independently of the longer
    // filesystem-path buffer used by SHGetPathFromIDListEx below.
    let mut display_name = [0u16; 260];
    let title: Vec<u16> = "Choose the project folder"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut info = BrowseInfo {
        owner: hwnd,
        root: null(),
        display_name: display_name.as_mut_ptr(),
        title: title.as_ptr(),
        flags: BIF_RETURNONLYFSDIRS | BIF_EDITBOX | BIF_NEWDIALOGSTYLE | BIF_NONEWFOLDERBUTTON,
        callback: None,
        parameter: 0,
        image: 0,
    };
    let Some(pidl) = NonNull::new(SHBrowseForFolderW(&mut info)) else {
        // This API documents NULL for cancellation; it has no HRESULT/extended
        // failure channel. Do not invent a GetLastError-based diagnosis.
        return Ok(None);
    };
    let selected = SelectedFolder(pidl);
    let mut path = vec![0u16; PATH_UNITS];
    if SHGetPathFromIDListEx(selected.0.as_ptr(), path.as_mut_ptr(), PATH_UNITS as u32, 0) == 0 {
        // In particular, a bare network-server PIDL may pass RETURNONLYFSDIRS
        // yet cannot identify a usable filesystem directory. Do not accept it.
        return Err(
            "Selected Shell item has no filesystem folder path, or exceeds the native path budget"
                .into(),
        );
    }
    let end = path
        .iter()
        .position(|unit| *unit == 0)
        .ok_or("Folder picker returned an unterminated native path")?;
    if end == 0 {
        return Err("Folder picker returned an empty filesystem path".into());
    }
    let path = PathBuf::from(OsString::from_wide(&path[..end]));
    if !path.is_absolute() {
        return Err("Folder picker returned a non-absolute filesystem path".into());
    }
    let metadata = std::fs::metadata(&path)
        .map_err(|error| format!("Selected folder is unavailable: {error}"))?;
    if !metadata.is_dir() {
        return Err("Selected filesystem item is not a directory".into());
    }
    // selected drops before _apartment on every return path: PIDL allocation is
    // freed before this call's successful COM initialization is balanced.
    Ok(Some(path))
}
