//! HWND-backed UI Automation text provider for the active editor group.
//! COM owns references to this adapter. The editor thread owns document data.
use super::{query, range, Snapshot, Window};
use std::{
    ffi::c_void,
    mem::offset_of,
    ptr::null_mut,
    sync::atomic::{AtomicU32, Ordering},
};

type Interface = *mut c_void;
type ResultCode = i32;

const OK: ResultCode = 0;
const NO_INTERFACE: ResultCode = 0x8000_4002u32 as i32;
const POINTER: ResultCode = 0x8000_4003u32 as i32;
const OUT_OF_MEMORY: ResultCode = 0x8007_000eu32 as i32;
const INVALID_ARGUMENT: ResultCode = 0x8007_0057u32 as i32;
const TEXT_PATTERN: i32 = 10014;
const TEXT_PATTERN2: i32 = 10024;
const VT_I4: u16 = 3;
const VT_BSTR: u16 = 8;
const VT_BOOL: u16 = 11;
const VT_UNKNOWN: u16 = 13;

#[repr(C)]
#[derive(PartialEq, Eq)]
struct Guid(u32, u16, u16, [u8; 8]);

const IUNKNOWN: Guid = Guid(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
const SIMPLE_PROVIDER: Guid = Guid(
    0xd6dd_68d1,
    0x86fd,
    0x4332,
    [0x86, 0x66, 0x9a, 0xbe, 0xde, 0xa2, 0xd2, 0x4c],
);
const TEXT_PROVIDER: Guid = Guid(
    0x3589_c92c,
    0x63f3,
    0x4367,
    [0x99, 0xbb, 0xad, 0xa6, 0x53, 0xb7, 0x7c, 0xf2],
);
const TEXT_PROVIDER2: Guid = Guid(
    0x0dc5_e6ed,
    0x3e16,
    0x4bf1,
    [0x8f, 0x9a, 0xa9, 0x79, 0x87, 0x8b, 0xc1, 0x95],
);

// VARIANT's union can hold a two-pointer record and a 64-bit scalar.
#[repr(C)]
union VariantData {
    integer: i32,
    boolean: i16,
    string: *mut u16,
    record: [usize; 2],
    aligned_scalar: i64,
}

#[repr(C)]
struct Variant {
    kind: u16,
    reserved: [u16; 3],
    data: VariantData,
}

impl Variant {
    fn empty() -> Self {
        Self {
            kind: 0,
            reserved: [0; 3],
            data: VariantData { record: [0; 2] },
        }
    }

    fn integer(&mut self, value: i32) {
        self.kind = VT_I4;
        self.data.integer = value;
    }

    fn boolean(&mut self, value: bool) {
        self.kind = VT_BOOL;
        self.data.boolean = if value { -1 } else { 0 };
    }

    fn string(&mut self, value: &str) -> ResultCode {
        let utf16: Vec<u16> = value.encode_utf16().collect();
        let string = unsafe { SysAllocStringLen(utf16.as_ptr(), utf16.len() as u32) };
        if string.is_null() {
            return OUT_OF_MEMORY;
        }
        self.kind = VT_BSTR;
        self.data.string = string;
        OK
    }
}

#[repr(C)]
struct UnknownVtable {
    query: unsafe extern "system" fn(Interface, *const Guid, *mut Interface) -> ResultCode,
    add_ref: unsafe extern "system" fn(Interface) -> u32,
    release: unsafe extern "system" fn(Interface) -> u32,
}

#[repr(C)]
struct SimpleVtable {
    base: UnknownVtable,
    options: unsafe extern "system" fn(Interface, *mut i32) -> ResultCode,
    pattern: unsafe extern "system" fn(Interface, i32, *mut Interface) -> ResultCode,
    property: unsafe extern "system" fn(Interface, i32, *mut Variant) -> ResultCode,
    host: unsafe extern "system" fn(Interface, *mut Interface) -> ResultCode,
}

#[repr(C)]
struct Point {
    x: f64,
    y: f64,
}

#[repr(C)]
struct TextVtable {
    base: UnknownVtable,
    selection: unsafe extern "system" fn(Interface, *mut Interface) -> ResultCode,
    visible: unsafe extern "system" fn(Interface, *mut Interface) -> ResultCode,
    child: unsafe extern "system" fn(Interface, Interface, *mut Interface) -> ResultCode,
    point: unsafe extern "system" fn(Interface, Point, *mut Interface) -> ResultCode,
    document: unsafe extern "system" fn(Interface, *mut Interface) -> ResultCode,
    selection_kind: unsafe extern "system" fn(Interface, *mut i32) -> ResultCode,
    annotation: unsafe extern "system" fn(Interface, Interface, *mut Interface) -> ResultCode,
    caret: unsafe extern "system" fn(Interface, *mut i32, *mut Interface) -> ResultCode,
}

#[repr(C)]
struct Provider {
    simple: &'static SimpleVtable,
    text: &'static TextVtable,
    references: AtomicU32,
    window: Window,
}

#[link(name = "oleaut32")]
extern "system" {
    fn SysAllocStringLen(value: *const u16, len: u32) -> *mut u16;
    fn SafeArrayCreateVector(kind: u16, lower: i32, count: u32) -> Interface;
    fn SafeArrayPutElement(array: Interface, index: *const i32, value: Interface) -> ResultCode;
    fn SafeArrayDestroy(array: Interface) -> ResultCode;
}

#[link(name = "uiautomationcore")]
extern "system" {
    fn UiaHostProviderFromHwnd(hwnd: Interface, result: *mut Interface) -> ResultCode;
    fn UiaClientsAreListening() -> i32;
    fn UiaRaiseAutomationEvent(provider: Interface, event: i32) -> ResultCode;
}

pub(super) fn create(window: Window) -> Interface {
    Box::into_raw(Box::new(Provider {
        simple: &SIMPLE,
        text: &TEXT,
        references: AtomicU32::new(1),
        window,
    }))
    .cast()
}

/// Release the owned reference of any COM interface whose vtable starts with IUnknown.
pub(super) unsafe fn release(interface: Interface) {
    let table = *interface.cast::<*const UnknownVtable>();
    ((*table).release)(interface);
}

pub(super) fn raise(window: Window, event: i32) {
    unsafe {
        if UiaClientsAreListening() == 0 {
            return;
        }
        let provider = create(window);
        UiaRaiseAutomationEvent(provider, event);
        release(provider);
    }
}

unsafe fn from_text(interface: Interface) -> *mut Provider {
    interface
        .cast::<u8>()
        .sub(offset_of!(Provider, text))
        .cast()
}

unsafe extern "system" fn add_simple(interface: Interface) -> u32 {
    (*interface.cast::<Provider>())
        .references
        .fetch_add(1, Ordering::Relaxed)
        + 1
}

unsafe extern "system" fn release_simple(interface: Interface) -> u32 {
    let provider = interface.cast::<Provider>();
    let count = (*provider).references.fetch_sub(1, Ordering::AcqRel) - 1;
    if count == 0 {
        drop(Box::from_raw(provider));
    }
    count
}

unsafe extern "system" fn add_text(interface: Interface) -> u32 {
    add_simple(from_text(interface).cast())
}

unsafe extern "system" fn release_text(interface: Interface) -> u32 {
    release_simple(from_text(interface).cast())
}

unsafe extern "system" fn query_simple(
    interface: Interface,
    iid: *const Guid,
    result: *mut Interface,
) -> ResultCode {
    if result.is_null() {
        return POINTER;
    }
    *result = null_mut();
    let Some(iid) = iid.as_ref() else {
        return POINTER;
    };
    let provider = interface.cast::<Provider>();
    *result = if *iid == IUNKNOWN || *iid == SIMPLE_PROVIDER {
        interface
    } else if *iid == TEXT_PROVIDER || *iid == TEXT_PROVIDER2 {
        std::ptr::addr_of_mut!((*provider).text).cast()
    } else {
        return NO_INTERFACE;
    };
    add_simple(interface);
    OK
}

unsafe extern "system" fn query_text(
    interface: Interface,
    iid: *const Guid,
    result: *mut Interface,
) -> ResultCode {
    query_simple(from_text(interface).cast(), iid, result)
}

unsafe extern "system" fn options(_: Interface, result: *mut i32) -> ResultCode {
    if result.is_null() {
        return POINTER;
    }
    *result = 2; // ProviderOptions_ServerSideProvider
    OK
}

unsafe extern "system" fn pattern(
    interface: Interface,
    id: i32,
    result: *mut Interface,
) -> ResultCode {
    if result.is_null() {
        return POINTER;
    }
    *result = null_mut();
    let provider = interface.cast::<Provider>();
    if matches!(id, TEXT_PATTERN | TEXT_PATTERN2) && query((*provider).window).is_ok() {
        *result = std::ptr::addr_of_mut!((*provider).text).cast();
        add_simple(interface);
    }
    OK
}

unsafe extern "system" fn property(
    interface: Interface,
    id: i32,
    result: *mut Variant,
) -> ResultCode {
    if result.is_null() {
        return POINTER;
    }
    result.write(Variant::empty());
    let value = &mut *result;
    let window = (*interface.cast::<Provider>()).window;
    match id {
        30003 => value.integer(50030), // Document control type
        30005 => {
            return match query(window) {
                Ok(snapshot) => value.string(&snapshot.name),
                Err(_) => value.string("Editor de CIDE"),
            };
        }
        30008 => value.boolean(query(window).is_ok_and(|snapshot| snapshot.focused)),
        30009 | 30040 | 30119 => value.boolean(query(window).is_ok()),
        30011 => return value.string("CIDE.Editor"),
        30016 | 30017 => value.boolean(true), // control and content element
        30024 => return value.string("Win32"),
        _ => {}
    }
    OK
}

unsafe extern "system" fn host(interface: Interface, result: *mut Interface) -> ResultCode {
    if result.is_null() {
        return POINTER;
    }
    *result = null_mut();
    let window = (*interface.cast::<Provider>()).window;
    UiaHostProviderFromHwnd(window.hwnd as Interface, result)
}

unsafe fn text_snapshot(interface: Interface) -> Result<Snapshot, ResultCode> {
    query((*from_text(interface)).window)
}

unsafe fn range_array(
    snapshot: &Snapshot,
    ranges: &[(usize, usize)],
    result: *mut Interface,
) -> ResultCode {
    let Ok(count) = i32::try_from(ranges.len()) else {
        return INVALID_ARGUMENT;
    };
    let array = SafeArrayCreateVector(VT_UNKNOWN, 0, count as u32);
    if array.is_null() {
        return OUT_OF_MEMORY;
    }
    for (index, &(start, end)) in ranges.iter().enumerate() {
        let item = range::create(snapshot.context, start, end);
        // VT_UNKNOWN expects the interface pointer itself. The array retains
        // its own reference on success.
        let status = SafeArrayPutElement(array, &(index as i32), item);
        release(item);
        if status < 0 {
            SafeArrayDestroy(array);
            return status;
        }
    }
    *result = array;
    OK
}

unsafe extern "system" fn selection(interface: Interface, result: *mut Interface) -> ResultCode {
    if result.is_null() {
        return POINTER;
    }
    *result = null_mut();
    let snapshot = match text_snapshot(interface) {
        Ok(snapshot) => snapshot,
        Err(status) => return status,
    };
    if snapshot.selection.is_empty() {
        range_array(&snapshot, &[(snapshot.caret, snapshot.caret)], result)
    } else {
        range_array(&snapshot, &snapshot.selection, result)
    }
}

unsafe extern "system" fn visible(interface: Interface, result: *mut Interface) -> ResultCode {
    if result.is_null() {
        return POINTER;
    }
    *result = null_mut();
    let snapshot = match text_snapshot(interface) {
        Ok(snapshot) => snapshot,
        Err(status) => return status,
    };
    range_array(&snapshot, &[snapshot.visible], result)
}

unsafe extern "system" fn child(_: Interface, _: Interface, result: *mut Interface) -> ResultCode {
    if result.is_null() {
        return POINTER;
    }
    *result = null_mut();
    INVALID_ARGUMENT // This provider has no embedded child elements.
}

unsafe extern "system" fn point(
    interface: Interface,
    position: Point,
    result: *mut Interface,
) -> ResultCode {
    if result.is_null() {
        return POINTER;
    }
    *result = null_mut();
    match super::point((*from_text(interface)).window, position.x, position.y) {
        Ok((context, offset)) => {
            *result = range::create(context, offset, offset);
            OK
        }
        Err(error) => error,
    }
}

unsafe extern "system" fn annotation(
    _: Interface,
    _: Interface,
    result: *mut Interface,
) -> ResultCode {
    if result.is_null() {
        return POINTER;
    }
    *result = null_mut();
    INVALID_ARGUMENT // No annotation elements are exposed.
}

unsafe extern "system" fn document(interface: Interface, result: *mut Interface) -> ResultCode {
    if result.is_null() {
        return POINTER;
    }
    *result = null_mut();
    match text_snapshot(interface) {
        Ok(snapshot) => {
            let start = snapshot.context.start;
            *result = range::create(snapshot.context, start, snapshot.len);
            OK
        }
        Err(status) => status,
    }
}

unsafe extern "system" fn selection_kind(interface: Interface, result: *mut i32) -> ResultCode {
    if result.is_null() {
        return POINTER;
    }
    *result = 0;
    match text_snapshot(interface) {
        Ok(_) => {
            *result = 1; // SupportedTextSelection_Single: the provider exposes only linear selection.
            OK
        }
        Err(status) => status,
    }
}

unsafe extern "system" fn caret(
    interface: Interface,
    active: *mut i32,
    result: *mut Interface,
) -> ResultCode {
    if !active.is_null() {
        *active = 0;
    }
    if !result.is_null() {
        *result = null_mut();
    }
    if active.is_null() || result.is_null() {
        return POINTER;
    }
    match text_snapshot(interface) {
        Ok(snapshot) => {
            *active = i32::from(snapshot.focused);
            *result = range::create(snapshot.context, snapshot.caret, snapshot.caret);
            OK
        }
        Err(status) => status,
    }
}

static SIMPLE: SimpleVtable = SimpleVtable {
    base: UnknownVtable {
        query: query_simple,
        add_ref: add_simple,
        release: release_simple,
    },
    options,
    pattern,
    property,
    host,
};

static TEXT: TextVtable = TextVtable {
    base: UnknownVtable {
        query: query_text,
        add_ref: add_text,
        release: release_text,
    },
    selection,
    visible,
    child,
    point,
    document,
    selection_kind,
    annotation,
    caret,
};
