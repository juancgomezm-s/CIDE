//! Observable allocation/byte regression for the cold file lifecycle. The
//! document is still resident today; saving/checking must not duplicate it.
use jc_files::{ContentFingerprint, ExternalState, FileError, FileSession};
use jc_text::TextEncoding;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    fs::{self, File},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct TrackedAllocator;
thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static LARGEST: Cell<usize> = const { Cell::new(0) };
}
fn record(size: usize) {
    if TRACKING.try_with(Cell::get).unwrap_or(false) {
        let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(size)));
    }
}
unsafe impl GlobalAlloc for TrackedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        System.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        System.alloc_zeroed(layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        System.realloc(ptr, layout, size)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}
#[global_allocator]
static ALLOCATOR: TrackedAllocator = TrackedAllocator;

fn measured<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            TRACKING.with(|flag| flag.set(false));
        }
    }
    LARGEST.with(|largest| largest.set(0));
    TRACKING.with(|flag| flag.set(true));
    let reset = Reset;
    let result = operation();
    drop(reset);
    (result, LARGEST.with(Cell::get))
}
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cide-stream-save-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn binary_fragmented_save_and_conflict_checks_do_not_allocate_a_source_sized_copy() {
    let workspace = Workspace::new();
    let path = workspace.0.join("origen.bin");
    let copy = workspace.0.join("copia.bin");
    let mut expected = vec![0u8; 1024 * 1024 + 257];
    for (i, byte) in expected.iter_mut().enumerate() {
        *byte = (i % 256) as u8;
    }
    expected[..3].copy_from_slice(b"\xef\xbb\xbf");
    expected[31..34].copy_from_slice(b"\r\n\r");
    fs::write(&path, &expected).unwrap();
    let mut session = FileSession::open(&path, Some(TextEncoding::Utf8)).unwrap();
    session
        .document_mut()
        .insert(65_535, b"\xff\x00\r\n")
        .unwrap();
    expected.splice(65_535..65_535, b"\xff\x00\r\n".iter().copied());
    session
        .document_mut()
        .replace(600_000..600_003, b"\x80\r")
        .unwrap();
    expected.splice(600_000..600_003, b"\x80\r".iter().copied());
    assert!(session.document().piece_count() > 1);
    let (external, largest) = measured(|| session.external_state());
    assert_eq!(external.unwrap(), ExternalState::Unchanged);
    assert!(
        largest < 128 * 1024,
        "external check allocated {largest} bytes"
    );
    let (saved, largest) = measured(|| session.save());
    saved.unwrap();
    assert!(largest < 128 * 1024, "save allocated {largest} bytes");
    assert_eq!(fs::read(&path).unwrap(), expected);
    assert!(!session.document().is_modified());
    assert_eq!(session.external_state().unwrap(), ExternalState::Unchanged);
    let (saved_as, largest) = measured(|| session.save_as(&copy));
    saved_as.unwrap();
    assert!(largest < 128 * 1024, "save-as allocated {largest} bytes");
    assert_eq!(fs::read(&copy).unwrap(), expected);
    assert_eq!(
        ContentFingerprint::of_reader(&mut File::open(&copy).unwrap()).unwrap(),
        ContentFingerprint::of(&expected)
    );
    session.document_mut().insert(0, b"local").unwrap();
    let mut external = expected.clone();
    external[800_000] ^= 0xff;
    fs::write(&copy, &external).unwrap();
    let (conflict, largest) = measured(|| session.save());
    assert!(matches!(
        conflict,
        Err(FileError::ExternalModification(ExternalState::Changed))
    ));
    assert!(
        largest < 128 * 1024,
        "conflict check allocated {largest} bytes"
    );
    assert_eq!(fs::read(&copy).unwrap(), external);
    assert!(session.document().is_modified());
}
