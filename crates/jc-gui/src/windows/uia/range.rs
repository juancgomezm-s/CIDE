//! UI Automation text ranges own a versioned identity and byte endpoints.
//! The UI thread answers text queries, while COM clients own range movement.
use super::Context;
use std::{
    cmp::Ordering,
    ffi::c_void,
    ptr::{self, null_mut},
    sync::{
        atomic::{AtomicU32, Ordering::Relaxed},
        Mutex,
    },
};

type HResult = i32;
type Com = *mut c_void;
type BStr = *mut u16;
const OK: HResult = 0;
const E_NOTIMPL: HResult = 0x8000_4001u32 as i32;
const E_NOINTERFACE: HResult = 0x8000_4002u32 as i32;
const E_POINTER: HResult = 0x8000_4003u32 as i32;
const E_FAIL: HResult = 0x8000_4005u32 as i32;
const E_OUTOFMEMORY: HResult = 0x8007_000eu32 as i32;
const E_INVALIDARG: HResult = 0x8007_0057u32 as i32;
const UNAVAILABLE: HResult = 0x8004_0201u32 as i32;
const VT_UNKNOWN: u16 = 13;
const MAX_UTF16: usize = 64 * 1024;

#[repr(C)]
#[derive(PartialEq, Eq)]
struct Guid(u32, u16, u16, [u8; 8]);

const IUNKNOWN: Guid = Guid(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
const ITEXT_RANGE: Guid = Guid(
    0x5347ad7b,
    0xc355,
    0x46f8,
    [0xaf, 0xf5, 0x90, 0x90, 0x33, 0x58, 0x2f, 0x63],
);

// VARIANT's value union includes both an eight-byte scalar and BRECORD's two
// pointers. Preserve the SDK layout for the FindAttribute by-value argument.
#[repr(C)]
union VariantValue {
    scalar: u64,
    pointers: [Com; 2],
}

#[repr(C)]
struct Variant {
    tag: u16,
    reserved: [u16; 3],
    value: VariantValue,
}

#[repr(C)]
struct VTable {
    query_interface: unsafe extern "system" fn(*mut TextRange, *const Guid, *mut Com) -> HResult,
    add_ref: unsafe extern "system" fn(*mut TextRange) -> u32,
    release: unsafe extern "system" fn(*mut TextRange) -> u32,
    clone: unsafe extern "system" fn(*mut TextRange, *mut Com) -> HResult,
    compare: unsafe extern "system" fn(*mut TextRange, Com, *mut i32) -> HResult,
    compare_endpoints:
        unsafe extern "system" fn(*mut TextRange, i32, Com, i32, *mut i32) -> HResult,
    expand: unsafe extern "system" fn(*mut TextRange, i32) -> HResult,
    find_attribute:
        unsafe extern "system" fn(*mut TextRange, i32, Variant, i32, *mut Com) -> HResult,
    find_text: unsafe extern "system" fn(*mut TextRange, BStr, i32, i32, *mut Com) -> HResult,
    attribute: unsafe extern "system" fn(*mut TextRange, i32, *mut Variant) -> HResult,
    rectangles: unsafe extern "system" fn(*mut TextRange, *mut Com) -> HResult,
    enclosing: unsafe extern "system" fn(*mut TextRange, *mut Com) -> HResult,
    text: unsafe extern "system" fn(*mut TextRange, i32, *mut BStr) -> HResult,
    move_range: unsafe extern "system" fn(*mut TextRange, i32, i32, *mut i32) -> HResult,
    move_endpoint_unit:
        unsafe extern "system" fn(*mut TextRange, i32, i32, i32, *mut i32) -> HResult,
    move_endpoint_range: unsafe extern "system" fn(*mut TextRange, i32, Com, i32) -> HResult,
    select: unsafe extern "system" fn(*mut TextRange) -> HResult,
    add_selection: unsafe extern "system" fn(*mut TextRange) -> HResult,
    remove_selection: unsafe extern "system" fn(*mut TextRange) -> HResult,
    scroll_into_view: unsafe extern "system" fn(*mut TextRange, i32) -> HResult,
    children: unsafe extern "system" fn(*mut TextRange, *mut Com) -> HResult,
}

static VTABLE: VTable = VTable {
    query_interface,
    add_ref,
    release,
    clone: clone_range,
    compare,
    compare_endpoints,
    expand,
    find_attribute,
    find_text,
    attribute,
    rectangles,
    enclosing,
    text,
    move_range,
    move_endpoint_unit,
    move_endpoint_range,
    select,
    add_selection: unsupported_selection,
    remove_selection: unsupported_selection,
    scroll_into_view,
    children,
};

#[link(name = "oleaut32")]
extern "system" {
    fn SysAllocStringLen(value: *const u16, len: u32) -> BStr;
    fn SafeArrayCreateVector(vt: u16, lower_bound: i32, count: u32) -> Com;
    fn SafeArrayPutElement(array: Com, index: *const i32, value: Com) -> HResult;
    fn SafeArrayDestroy(array: Com) -> HResult;
}

#[link(name = "uiautomationcore")]
extern "system" {
    fn UiaGetReservedNotSupportedValue(result: *mut Com) -> HResult;
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
}

impl Span {
    fn point(self, endpoint: i32) -> Result<usize, HResult> {
        match endpoint {
            0 => Ok(self.start),
            1 => Ok(self.end),
            _ => Err(E_INVALIDARG),
        }
    }

    fn with_point(self, endpoint: i32, offset: usize) -> Result<Self, HResult> {
        match endpoint {
            0 => Ok(Self {
                start: offset,
                end: self.end.max(offset),
            }),
            1 => Ok(Self {
                start: self.start.min(offset),
                end: offset,
            }),
            _ => Err(E_INVALIDARG),
        }
    }
}

#[repr(C)]
struct TextRange {
    vtable: *const VTable,
    references: AtomicU32,
    context: Context,
    span: Mutex<Span>,
}

impl TextRange {
    fn snapshot(&self) -> Result<(Span, usize), HResult> {
        let span = *self.span.lock().map_err(|_| E_FAIL)?;
        let len = self.context.len()?;
        if span.start > span.end || span.end > len {
            return Err(UNAVAILABLE);
        }
        Ok((span, len))
    }

    // An RPC to the owner may reenter the provider. Never hold a lock over
    // that boundary, and only replace the endpoints we originally read.
    fn publish(&self, expected: Span, value: Span) -> Result<(), HResult> {
        let mut current = self.span.lock().map_err(|_| E_FAIL)?;
        if *current != expected {
            return Err(E_FAIL);
        }
        *current = value;
        Ok(())
    }
}

pub(super) fn create(context: Context, start: usize, end: usize) -> Com {
    Box::into_raw(Box::new(TextRange {
        vtable: &VTABLE,
        references: AtomicU32::new(1),
        context,
        span: Mutex::new(Span { start, end }),
    }))
    .cast()
}

fn result(value: Result<(), HResult>) -> HResult {
    value.err().unwrap_or(OK)
}

fn unit(value: i32) -> Result<i32, HResult> {
    match value {
        0 => Ok(0),
        1..=3 => Ok(3),
        4..=6 => Ok(6),
        _ => Err(E_INVALIDARG),
    }
}

unsafe fn out<'a, T>(value: *mut T, initial: T) -> Result<&'a mut T, HResult> {
    if value.is_null() {
        return Err(E_POINTER);
    }
    value.write(initial);
    Ok(&mut *value)
}

// Callers may pass a different provider or a COM proxy. Only inspect our own
// object layout after verifying the interface's first word is our vtable.
unsafe fn local_range<'a>(range: Com) -> Option<&'a TextRange> {
    if range.is_null() || !ptr::eq(*range.cast::<*const VTable>(), &VTABLE) {
        return None;
    }
    Some(&*range.cast::<TextRange>())
}

unsafe extern "system" fn query_interface(
    this: *mut TextRange,
    iid: *const Guid,
    output: *mut Com,
) -> HResult {
    result((|| {
        let output = out(output, null_mut())?;
        let iid = iid.as_ref().ok_or(E_POINTER)?;
        if *iid != IUNKNOWN && *iid != ITEXT_RANGE {
            return Err(E_NOINTERFACE);
        }
        add_ref(this);
        *output = this.cast();
        Ok(())
    })())
}

unsafe extern "system" fn add_ref(this: *mut TextRange) -> u32 {
    (*this).references.fetch_add(1, Relaxed) + 1
}

unsafe extern "system" fn release(this: *mut TextRange) -> u32 {
    let count = (*this)
        .references
        .fetch_sub(1, std::sync::atomic::Ordering::Release)
        - 1;
    if count == 0 {
        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
        drop(Box::from_raw(this));
    }
    count
}

unsafe extern "system" fn clone_range(this: *mut TextRange, output: *mut Com) -> HResult {
    result((|| {
        let output = out(output, null_mut())?;
        let this = &*this;
        let (span, _) = this.snapshot()?;
        *output = create(this.context, span.start, span.end);
        Ok(())
    })())
}

unsafe extern "system" fn compare(this: *mut TextRange, other: Com, output: *mut i32) -> HResult {
    result((|| {
        let output = out(output, 0)?;
        if other.is_null() {
            return Err(E_POINTER);
        }
        let this = &*this;
        let (span, _) = this.snapshot()?;
        if let Some(other) = local_range(other) {
            if this.context.same_document(&other.context) {
                *output = i32::from(span == other.snapshot()?.0);
            }
        }
        Ok(())
    })())
}

unsafe extern "system" fn compare_endpoints(
    this: *mut TextRange,
    endpoint: i32,
    other: Com,
    other_endpoint: i32,
    output: *mut i32,
) -> HResult {
    result((|| {
        let output = out(output, 0)?;
        let this = &*this;
        let other = local_range(other).ok_or(E_INVALIDARG)?;
        if !this.context.same_document(&other.context) {
            return Err(E_INVALIDARG);
        }
        let first = this.snapshot()?.0.point(endpoint)?;
        let second = other.snapshot()?.0.point(other_endpoint)?;
        *output = match first.cmp(&second) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        };
        Ok(())
    })())
}

unsafe extern "system" fn expand(this: *mut TextRange, requested: i32) -> HResult {
    result((|| {
        let unit = unit(requested)?;
        let this = &*this;
        let (span, _) = this.snapshot()?;
        let (start, end) = this.context.unit_bounds(span.start, unit)?;
        this.publish(span, Span { start, end })
    })())
}

unsafe extern "system" fn find_attribute(
    this: *mut TextRange,
    _: i32,
    _: Variant,
    _: i32,
    output: *mut Com,
) -> HResult {
    result((|| {
        out(output, null_mut())?;
        (*this).snapshot()?;
        Ok(())
    })())
}

unsafe extern "system" fn find_text(
    _: *mut TextRange,
    _: BStr,
    _: i32,
    _: i32,
    output: *mut Com,
) -> HResult {
    result((|| {
        out(output, null_mut())?;
        Err(E_NOTIMPL)
    })())
}

unsafe extern "system" fn attribute(this: *mut TextRange, _: i32, output: *mut Variant) -> HResult {
    result((|| {
        let output = out(
            output,
            Variant {
                tag: 0,
                reserved: [0; 3],
                value: VariantValue {
                    pointers: [null_mut(); 2],
                },
            },
        )?;
        (*this).snapshot()?;
        let mut value = null_mut();
        let hr = UiaGetReservedNotSupportedValue(&mut value);
        if hr < 0 {
            return Err(hr);
        }
        output.tag = VT_UNKNOWN;
        output.value = VariantValue {
            pointers: [value, null_mut()],
        };
        Ok(())
    })())
}

unsafe extern "system" fn rectangles(this: *mut TextRange, output: *mut Com) -> HResult {
    result((|| {
        let output = out(output, null_mut())?;
        let this = &*this;
        let (span, _) = this.snapshot()?;
        let values = this.context.rectangles(span.start, span.end)?;
        let array = SafeArrayCreateVector(5, 0, values.len() as u32); // VT_R8
        if array.is_null() {
            return Err(E_OUTOFMEMORY);
        }
        for (index, value) in values.iter().enumerate() {
            let status = SafeArrayPutElement(
                array,
                &(index as i32),
                (value as *const f64).cast_mut().cast(),
            );
            if status < 0 {
                SafeArrayDestroy(array);
                return Err(status);
            }
        }
        *output = array;
        Ok(())
    })())
}

unsafe extern "system" fn enclosing(this: *mut TextRange, output: *mut Com) -> HResult {
    result((|| {
        let output = out(output, null_mut())?;
        let this = &*this;
        this.snapshot()?;
        *output = this.context.provider()?;
        Ok(())
    })())
}

unsafe extern "system" fn text(
    this: *mut TextRange,
    max_length: i32,
    output: *mut BStr,
) -> HResult {
    result((|| {
        let output = out(output, null_mut())?;
        let maximum = match max_length {
            -1 => MAX_UTF16,
            0.. => (max_length as usize).min(MAX_UTF16),
            _ => return Err(E_INVALIDARG),
        };
        let this = &*this;
        let (span, _) = this.snapshot()?;
        let units = this.context.text(span.start, span.end, maximum)?;
        if units.len() > maximum {
            return Err(E_FAIL);
        }
        let value = SysAllocStringLen(units.as_ptr(), units.len() as u32);
        if value.is_null() {
            return Err(E_OUTOFMEMORY);
        }
        *output = value;
        Ok(())
    })())
}

unsafe extern "system" fn move_range(
    this: *mut TextRange,
    requested: i32,
    count: i32,
    output: *mut i32,
) -> HResult {
    result((|| {
        let output = out(output, 0)?;
        let unit = unit(requested)?;
        let this = &*this;
        let (span, len) = this.snapshot()?;
        if count == 0 {
            return Ok(());
        }
        let degenerate = span.start == span.end;
        let base = if degenerate {
            span.start
        } else {
            this.context.unit_bounds(span.start, unit)?.0
        };
        let (mut start, mut moved) = this.context.move_endpoint(base, unit, count)?;
        if !degenerate && start == len && moved > 0 {
            let (previous, step) = this.context.move_endpoint(start, unit, -1)?;
            start = previous;
            moved += step;
        }
        if moved != 0 {
            let end = if degenerate {
                start
            } else {
                this.context.unit_bounds(start, unit)?.1
            };
            this.publish(span, Span { start, end })?;
        }
        *output = moved;
        Ok(())
    })())
}

unsafe extern "system" fn move_endpoint_unit(
    this: *mut TextRange,
    endpoint: i32,
    requested: i32,
    count: i32,
    output: *mut i32,
) -> HResult {
    result((|| {
        let output = out(output, 0)?;
        let unit = unit(requested)?;
        let this = &*this;
        let (span, _) = this.snapshot()?;
        let point = span.point(endpoint)?;
        let (offset, moved) = this.context.move_endpoint(point, unit, count)?;
        this.publish(span, span.with_point(endpoint, offset)?)?;
        *output = moved;
        Ok(())
    })())
}

unsafe extern "system" fn move_endpoint_range(
    this: *mut TextRange,
    endpoint: i32,
    other: Com,
    other_endpoint: i32,
) -> HResult {
    result((|| {
        let this = &*this;
        let other = local_range(other).ok_or(E_INVALIDARG)?;
        if !this.context.same_document(&other.context) {
            return Err(E_INVALIDARG);
        }
        let (span, _) = this.snapshot()?;
        let offset = other.snapshot()?.0.point(other_endpoint)?;
        this.publish(span, span.with_point(endpoint, offset)?)
    })())
}

unsafe extern "system" fn select(this: *mut TextRange) -> HResult {
    result((|| {
        let this = &*this;
        let (span, _) = this.snapshot()?;
        this.context.select(span.start, span.end)
    })())
}

unsafe extern "system" fn unsupported_selection(_: *mut TextRange) -> HResult {
    E_NOTIMPL
}

unsafe extern "system" fn scroll_into_view(_: *mut TextRange, _: i32) -> HResult {
    E_NOTIMPL
}

unsafe extern "system" fn children(this: *mut TextRange, output: *mut Com) -> HResult {
    result((|| {
        let output = out(output, null_mut())?;
        (*this).snapshot()?;
        let array = SafeArrayCreateVector(VT_UNKNOWN, 0, 0);
        if array.is_null() {
            return Err(E_OUTOFMEMORY);
        }
        *output = array;
        Ok(())
    })())
}
