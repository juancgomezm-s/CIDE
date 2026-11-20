use jc_app::{
    data_connections::{Engine as ProfileEngine, ProfileStore},
    Cancellation, Wakeup,
};
use jc_platform::{
    sql::{Engine, Session},
    sql_worker,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

pub(super) struct Context {
    output: mpsc::Receiver<Vec<Value>>,
    cancel: Cancellation,
}

struct Slot(Arc<AtomicBool>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Context {
    pub(super) fn start(
        root: PathBuf,
        references: Vec<(Option<String>, String)>,
        busy: &Arc<AtomicBool>,
        wakeup: Wakeup,
    ) -> Option<Self> {
        if references.is_empty() || busy.swap(true, Ordering::AcqRel) {
            return None;
        }
        let slot = Slot(busy.clone());
        let cancel = Cancellation::new(Duration::from_secs(3));
        let worker_cancel = cancel.clone();
        let (sender, output) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("cide-contexto-sql".into())
            .spawn(move || {
                let symbols =
                    symbols(&root, &references, &worker_cancel).unwrap_or_else(|_error| {
                        #[cfg(test)]
                        eprintln!("Contexto SQL de prueba: {_error}");
                        Vec::new()
                    });
                drop(slot);
                let _ = sender.send(symbols);
                wakeup.signal();
            })
            .ok()?;
        Some(Self { output, cancel })
    }

    pub(super) fn poll(&self) -> Option<Vec<Value>> {
        match self.output.try_recv() {
            Ok(symbols) => Some(symbols),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Vec::new()),
        }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

fn symbols(
    root: &Path,
    references: &[(Option<String>, String)],
    cancel: &Cancellation,
) -> Result<Vec<Value>, String> {
    let store = ProfileStore::open(root)?;
    let mut profiles = store
        .profiles()
        .iter()
        .filter(|p| p.engine() != ProfileEngine::OpenEdge);
    let Some(profile) = profiles.next() else {
        return Ok(Vec::new());
    };
    if profiles.next().is_some() || cancel.is_cancelled() {
        return Ok(Vec::new());
    }
    let (engine, provider) = match profile.engine() {
        ProfileEngine::PostgreSql => (Engine::PostgreSql, "PostgreSQL"),
        ProfileEngine::MySql => (Engine::MySql, "MySQL"),
        ProfileEngine::OpenEdge => unreachable!("filtered SQL connections"),
    };
    let options = sql_worker::options(engine, std::array::from_fn(|i| profile.value(i)))?;
    let session = Session::connect(&options)?;
    let catalog = session.catalog(|| cancel.is_cancelled())?;
    let mut symbols = Vec::new();
    for (schema, name) in references {
        let mut matches = catalog.iter().filter(|relation| {
            relation.name().eq_ignore_ascii_case(name)
                && schema
                    .as_deref()
                    .is_none_or(|schema| relation.schema().eq_ignore_ascii_case(schema))
        });
        let Some(relation) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        let columns = session.column_types(relation, || cancel.is_cancelled())?;
        for column in columns.into_iter().take(12 - symbols.len()) {
            use jc_app::source_services::display_ui_label;
            symbols.push(json!({
                "provider": provider,
                "symbol": display_ui_label(&format!("{}.{}.{}", relation.schema(), relation.name(), column.name), 92),
                "typeOrSignature": display_ui_label(&column.data_type, 100),
            }));
        }
        if symbols.len() == 12 {
            break;
        }
    }
    Ok(symbols)
}

#[cfg(test)]
mod native_tests {
    use crate::windows::{ffi::*, owner, paint, run_with_smoke, smoke};
    use crate::{GuiOptions, SmokeOptions};
    use jc_app::{CommandId, Modifiers};
    use std::{
        ffi::{c_char, c_void, CString},
        fs,
        os::windows::ffi::OsStrExt,
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };

    const SOURCE: &str = "SELECT CASE WHEN total > 0 THEN \r\n    ELSE 0 END AS billed FROM invoice;\r\nSELECT 'neighbor_sql_statement';\r\n";
    const INSERTION: &str = "CAST(total\r\n        AS DECIMAL(12,2))";

    type Hkey = *mut c_void;
    const HKCU: Hkey = 0x8000_0001_u32 as i32 as isize as Hkey;
    const HKLM: Hkey = 0x8000_0002_u32 as i32 as isize as Hkey;

    #[link(name = "advapi32")]
    extern "system" {
        fn RegOpenKeyExW(
            key: Hkey,
            path: *const u16,
            options: u32,
            access: u32,
            out: *mut Hkey,
        ) -> i32;
        fn RegOverridePredefKey(key: Hkey, replacement: Hkey) -> i32;
        fn RegCloseKey(key: Hkey) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryExW(path: *const u16, file: Handle, flags: u32) -> Handle;
        fn GetProcAddress(module: Handle, symbol: *const u8) -> Handle;
        fn FreeLibrary(module: Handle) -> i32;
    }

    struct TestLibpq(Handle);

    impl TestLibpq {
        fn prepare() -> Self {
            let library = std::env::var_os("CIDE_OMNICORE_ODBC_TEST_LIBPQ").unwrap();
            let library: Vec<u16> = library.encode_wide().chain(Some(0)).collect();
            let port: u16 = std::env::var("CIDE_OMNICORE_ODBC_TEST_PGPORT")
                .unwrap()
                .parse()
                .unwrap();
            let options = CString::new(format!(
                "host=127.0.0.1 port={port} dbname=cide_test user=cide_reader sslmode=disable connect_timeout=3"
            ))
            .unwrap();
            unsafe {
                // Warm up libpq and Windows networking before hiding the real HKLM.
                let module = LoadLibraryExW(library.as_ptr(), std::ptr::null_mut(), 0x1100);
                assert!(!module.is_null(), "no se pudo cargar libpq de prueba");
                let library = Self(module);
                let connect = GetProcAddress(module, c"PQconnectdb".as_ptr().cast());
                let status = GetProcAddress(module, c"PQstatus".as_ptr().cast());
                let finish = GetProcAddress(module, c"PQfinish".as_ptr().cast());
                assert!(!connect.is_null() && !status.is_null() && !finish.is_null());
                let connect: unsafe extern "C" fn(*const c_char) -> Handle =
                    std::mem::transmute(connect);
                let status: unsafe extern "C" fn(Handle) -> i32 = std::mem::transmute(status);
                let finish: unsafe extern "C" fn(Handle) = std::mem::transmute(finish);
                let connection = connect(options.as_ptr());
                assert!(!connection.is_null());
                let result = status(connection);
                finish(connection);
                assert_eq!(result, 0, "PostgreSQL de prueba no está disponible");
                library
            }
        }
    }

    impl Drop for TestLibpq {
        fn drop(&mut self) {
            unsafe { FreeLibrary(self.0) };
        }
    }

    struct TestRegistry {
        key: Hkey,
        _library: TestLibpq,
    }

    impl TestRegistry {
        fn open() -> Option<Self> {
            let name = std::env::var("CIDE_OMNICORE_ODBC_TEST_REGISTRY").ok()?;
            let suffix = name
                .strip_prefix(r"Software\CIDE-PruebaODBC-")
                .expect("rama privada del controlador de prueba");
            assert!(suffix.len() == 32 && suffix.bytes().all(|b| b.is_ascii_hexdigit()));
            let library = TestLibpq::prepare();
            let path: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            let mut key = std::ptr::null_mut();
            unsafe {
                assert_eq!(
                    RegOpenKeyExW(HKCU, path.as_ptr(), 0, 0x000f003f, &mut key),
                    0
                );
                let registry = Self {
                    key,
                    _library: library,
                };
                assert_eq!(RegOverridePredefKey(HKLM, key), 0);
                Some(registry)
            }
        }
    }

    impl Drop for TestRegistry {
        fn drop(&mut self) {
            unsafe {
                RegOverridePredefKey(HKLM, std::ptr::null_mut());
                RegCloseKey(self.key);
            }
        }
    }

    #[test]
    #[ignore = "requiere OmniCore, un proveedor local y una ventana de prueba Win32"]
    fn omnicore_sql_native_window() {
        let _registry = TestRegistry::open();
        let root = PathBuf::from(std::env::var_os("CIDE_OMNICORE_SEMANTIC_FIXTURE").unwrap());
        let fixture = root.join("window.txt");
        fs::write(&fixture, b"alpha\r\nbeta\r\n").unwrap();
        fs::write(root.join("factura.sql"), SOURCE).unwrap();
        let bitmap = std::env::var_os("CIDE_OMNICORE_SQL_BITMAP").map(PathBuf::from);
        let report = root.join("window.txt.report");
        run_with_smoke(
            GuiOptions {
                project_root: Some(root),
                smoke: Some(SmokeOptions {
                    fixture,
                    report,
                    ..SmokeOptions::default()
                }),
                ..GuiOptions::default()
            },
            exercise,
        )
        .unwrap();
        if let Some(bitmap) = bitmap {
            assert!(bitmap.is_file());
        }
    }

    unsafe fn exercise(hwnd: Hwnd, options: &SmokeOptions) -> Result<(), String> {
        let gui = owner(hwnd).ok_or("No se creó la ventana nativa de CIDE")?;
        let cursor = SOURCE.find(" \r\n").unwrap() + 1;
        {
            let mut state = gui.state.borrow_mut();
            state
                .workbench
                .open_file(
                    options.fixture.with_file_name("factura.sql"),
                    Some(jc_text::TextEncoding::Utf8),
                )
                .map_err(|err| err.to_string())?;
            // This owned HWND exercises the real transport as well as the editor.
            state.smoke = false;
            state.toggle_omnicore();
        }
        wait(hwnd, |state| {
            state.omnicore.can_compose() && state.omnicore.input_capacity().is_ok()
        })?;
        smoke::key(hwnd, 0x1b, Modifiers::NONE)?;
        SendMessageW(hwnd, WM_SETFOCUS, 0, 0);
        {
            let mut state = gui.state.borrow_mut();
            state.blur_omnicore();
            state
                .workbench
                .set_linear_selection_bytes(cursor, cursor)
                .map_err(|err| err.to_string())?;
        }
        smoke::command(hwnd, CommandId::EditCompleteBlockWithAi)?;
        wait(hwnd, |state| {
            state.omnicore_selection_review_visible() || state.omnicore.completion.is_none()
        })?;
        if !gui.state.borrow().omnicore_selection_review_visible() {
            return Err(gui.state.borrow().status.clone());
        }
        smoke::expected(hwnd, SOURCE.as_bytes())?;
        // The review gate publishes only an ordinary painted frame.
        ShowWindow(hwnd, 4); // SW_SHOWNOACTIVATE
        InvalidateRect(hwnd, std::ptr::null(), 0);
        UpdateWindow(hwnd);
        let bitmap = std::env::var_os("CIDE_OMNICORE_SQL_BITMAP").map_or_else(
            || options.fixture.with_file_name("revision-sql.bmp"),
            PathBuf::from,
        );
        let frame = paint::prepare(&mut gui.state.borrow_mut());
        paint::bitmap(hwnd, &frame, &bitmap)?;
        smoke::key(hwnd, 0x0d, Modifiers::CTRL)?;
        let mut expected = SOURCE.to_owned();
        expected.insert_str(cursor, INSERTION);
        if gui.state.borrow().workbench.active_bytes().as_deref() != Some(expected.as_bytes()) {
            return Err(format!(
                "El diff de SQL no aplicó la inserción exacta: {}",
                gui.state.borrow().status
            ));
        }
        smoke::key(hwnd, b'Z' as usize, Modifiers::CTRL)?;
        smoke::expected(hwnd, SOURCE.as_bytes())?;
        Ok(())
    }

    unsafe fn wait(
        hwnd: Hwnd,
        predicate: impl Fn(&crate::windows::State) -> bool,
    ) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(35);
        loop {
            let mut message: Msg = std::mem::zeroed();
            while PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
                DispatchMessageW(&message);
            }
            let window = owner(hwnd).ok_or("La ventana de prueba se cerró")?;
            let state = window.state.borrow();
            if predicate(&state) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(state.status.clone());
            }
            drop(state);
            thread::sleep(Duration::from_millis(15));
        }
    }
}
