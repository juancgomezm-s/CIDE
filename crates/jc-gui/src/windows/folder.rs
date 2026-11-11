//! Windows Common Item Dialog, called outside any Workbench borrow.
use super::{
    ffi::{Handle, Hwnd},
    wide,
};
use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, PathBuf, Prefix},
    ptr::{null_mut, NonNull},
};

#[repr(C)]
struct Guid(u32, u16, u16, [u8; 8]);

const fn guid(value: u128) -> Guid {
    Guid(
        (value >> 96) as u32,
        (value >> 80) as u16,
        (value >> 64) as u16,
        (value as u64).to_be_bytes(),
    )
}

const FILE_OPEN_DIALOG: Guid = guid(0xdc1c5a9c_e88a_4dde_a5a1_60f82a20aef7);
const FILE_DIALOG_IID: Guid = guid(0x42f85136_db7e_439c_85f1_e4075d135fc8);
const SHELL_ITEM_IID: Guid = guid(0x43826d1e_e718_42ee_bc55_a1e261c37bfe);
const CANCELLED: i32 = 0x800704c7_u32 as i32;
const PICK_FOLDERS: u32 = 0x20;
const FORCE_FILESYSTEM: u32 = 0x40;
const NO_CHANGE_DIR: u32 = 0x08;
const PATH_MUST_EXIST: u32 = 0x800;
const FILE_MUST_EXIST: u32 = 0x1000;

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: Handle, flags: u32) -> i32;
    fn CoUninitialize();
    fn CoCreateInstance(
        class: *const Guid,
        outer: Handle,
        context: u32,
        iid: *const Guid,
        result: *mut Handle,
    ) -> i32;
    fn CoTaskMemFree(memory: Handle);
}

#[link(name = "shell32")]
extern "system" {
    fn SHCreateItemFromParsingName(
        path: *const u16,
        context: Handle,
        iid: *const Guid,
        result: *mut Handle,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn lstrlenW(value: *const u16) -> i32;
}

#[repr(C)]
struct Unknown {
    _query_interface: usize,
    _add_ref: usize,
    release: unsafe extern "system" fn(Handle) -> u32,
}

// Vtable order follows IModalWindow/IFileDialog in shobjidl_core.h.
#[repr(C)]
struct FileDialog {
    _unknown: Unknown,
    show: unsafe extern "system" fn(Handle, Hwnd) -> i32,
    _file_types_and_events: [usize; 5],
    set_options: unsafe extern "system" fn(Handle, u32) -> i32,
    get_options: unsafe extern "system" fn(Handle, *mut u32) -> i32,
    _set_default_folder: usize,
    set_folder: unsafe extern "system" fn(Handle, Handle) -> i32,
    _get_folder: unsafe extern "system" fn(Handle, *mut Handle) -> i32,
    _selection_and_filename: [usize; 3],
    set_title: unsafe extern "system" fn(Handle, *const u16) -> i32,
    _button_and_filename_labels: [usize; 2],
    get_result: unsafe extern "system" fn(Handle, *mut Handle) -> i32,
}

#[repr(C)]
struct ShellItem {
    _unknown: Unknown,
    _bind_to_handler: usize,
    _get_parent: usize,
    get_display_name: unsafe extern "system" fn(Handle, u32, *mut *mut u16) -> i32,
}

fn check(result: i32, operation: &str) -> Result<(), String> {
    if result >= 0 {
        Ok(())
    } else {
        Err(format!("{operation} (0x{:08X})", result as u32))
    }
}

struct Apartment;
impl Apartment {
    fn initialize() -> Result<Self, String> {
        check(
            unsafe { CoInitializeEx(null_mut(), 2) },
            "No se pudo iniciar el diálogo de Windows",
        )?;
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}

struct Com<V>(NonNull<*const V>);
impl<V> Com<V> {
    fn new(call: impl FnOnce(*mut Handle) -> i32, operation: &str) -> Result<Self, String> {
        let mut object = null_mut();
        check(call(&mut object), operation)?;
        NonNull::new(object.cast())
            .map(Self)
            .ok_or_else(|| operation.to_owned())
    }

    fn raw(&self) -> Handle {
        self.0.as_ptr().cast()
    }

    unsafe fn methods(&self) -> &V {
        &**self.0.as_ptr()
    }
}
impl<V> Drop for Com<V> {
    fn drop(&mut self) {
        unsafe {
            ((**self.0.as_ptr().cast::<*const Unknown>()).release)(self.raw());
        }
    }
}

impl Com<ShellItem> {
    unsafe fn from_path(path: &Path) -> Result<Self, String> {
        let mut units: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let parse = |units: &[u16]| {
            Self::new(
                |out| SHCreateItemFromParsingName(units.as_ptr(), null_mut(), &SHELL_ITEM_IID, out),
                "Windows no pudo localizar la ruta",
            )
        };
        // The filesystem uses verbatim paths after canonicalization. Shell parsing
        // also accepts conventional drive and UNC paths for those locations.
        parse(&units).or_else(|error| {
            let Some(Component::Prefix(prefix)) = path.components().next() else {
                return Err(error);
            };
            match prefix.kind() {
                Prefix::VerbatimDisk(_) => units.drain(..4),
                Prefix::VerbatimUNC(..) => units.drain(2..8),
                _ => return Err(error),
            };
            parse(&units)
        })
    }

    unsafe fn path(&self) -> Result<PathBuf, String> {
        const FILESYSTEM_PATH: u32 = 0x80058000;
        let mut value = null_mut();
        let result = (self.methods().get_display_name)(self.raw(), FILESYSTEM_PATH, &mut value);
        let path = if result >= 0 && !value.is_null() {
            Some(PathBuf::from(OsString::from_wide(
                std::slice::from_raw_parts(value, lstrlenW(value) as usize),
            )))
        } else {
            None
        };
        CoTaskMemFree(value.cast());
        check(result, "Windows no devolvió una ruta de archivo o carpeta")?;
        path.ok_or_else(|| "Windows devolvió una ruta vacía".into())
    }
}

unsafe fn dialog(initial: Option<&Path>, folder: bool) -> Result<Com<FileDialog>, String> {
    let dialog = Com::<FileDialog>::new(
        |out| CoCreateInstance(&FILE_OPEN_DIALOG, null_mut(), 1, &FILE_DIALOG_IID, out),
        "No se pudo crear el diálogo de Windows",
    )?;
    let v = dialog.methods();
    let raw = dialog.raw();
    let mut options = 0;
    check(
        (v.get_options)(raw, &mut options),
        "No se pudo consultar el diálogo de Windows",
    )?;
    options |= FORCE_FILESYSTEM | NO_CHANGE_DIR | PATH_MUST_EXIST;
    options |= if folder {
        PICK_FOLDERS
    } else {
        FILE_MUST_EXIST
    };
    check(
        (v.set_options)(raw, options),
        "No se pudo configurar el diálogo de Windows",
    )?;
    let title = wide(if folder {
        "Seleccionar carpeta"
    } else {
        "Abrir archivo"
    });
    check(
        (v.set_title)(raw, title.as_ptr()),
        "No se pudo establecer el título del diálogo",
    )?;
    // A stale or inaccessible project path leaves Windows free to use its usual location.
    if let Some(item) = initial.and_then(|path| Com::<ShellItem>::from_path(path).ok()) {
        (v.set_folder)(raw, item.raw());
    }
    Ok(dialog)
}

unsafe fn choose(
    hwnd: Hwnd,
    initial: Option<&Path>,
    folder: bool,
) -> Result<Option<PathBuf>, String> {
    let _apartment = Apartment::initialize()?;
    let dialog = dialog(initial, folder)?;
    let v = dialog.methods();
    let result = (v.show)(dialog.raw(), hwnd);
    if result == CANCELLED {
        return Ok(None);
    }
    check(result, "No se pudo abrir el diálogo de Windows")?;
    let item = Com::<ShellItem>::new(
        |out| (v.get_result)(dialog.raw(), out),
        "No se pudo obtener la selección de Windows",
    )?;
    item.path().map(Some)
}

pub(super) unsafe fn choose_folder(
    hwnd: Hwnd,
    initial: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    choose(hwnd, initial, true)
}

pub(super) unsafe fn choose_file(
    hwnd: Hwnd,
    initial: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    choose(hwnd, initial, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_dialog_configures_both_modes_at_the_project_path() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        let _apartment = Apartment::initialize().unwrap();
        unsafe {
            let initial = Com::<ShellItem>::from_path(&root).unwrap();
            assert_eq!(initial.path().unwrap().canonicalize().unwrap(), root);
            for folder in [false, true] {
                let dialog = dialog(Some(&root), folder).unwrap();
                let v = dialog.methods();
                let mut flags = 0;
                check((v.get_options)(dialog.raw(), &mut flags), "Opciones").unwrap();
                assert_eq!(flags & PICK_FOLDERS != 0, folder);
                assert_ne!(flags & FORCE_FILESYSTEM, 0);
                let item = Com::<ShellItem>::new(
                    |out| (v._get_folder)(dialog.raw(), out),
                    "Carpeta inicial",
                )
                .unwrap();
                assert_eq!(item.path().unwrap().canonicalize().unwrap(), root);
            }
            let file = root.join("Cargo.toml");
            let item = Com::<ShellItem>::from_path(&file).unwrap();
            assert_eq!(item.path().unwrap().canonicalize().unwrap(), file);
        }
    }
}
