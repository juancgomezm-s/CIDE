//! Opt-in CPU probe. Fixture creation is outside measured workload phases.
//! No editor API changes, thread tuning, external process control or lossy text.
use jc_app::{
    diff_model::{DiffLimits, DiffModel, DiffStatus},
    ProjectSearch, ProjectSearchOptions, ProjectSearchRow, ProjectSearchState, Wakeup, Workbench,
};
use jc_text::{DecodedScalars, TextEncoding};
use std::{
    error::Error,
    fs::{self, OpenOptions},
    hint::black_box,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const MIB: usize = 1024 * 1024;
const BLOCK: usize = 64 * 1024;
const PROTOCOL: &str = "cpu-workloads-v1";
const HASH_INITIAL: u64 = 0xcbf29ce484222325;
const MARKER: &[u8] = b"JC_CPU_HIT";

fn hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    hash
}

struct Config {
    samples: usize,
    suite: String,
    size: Option<usize>,
    load: String,
}
impl Config {
    fn parse() -> Result<Self> {
        let mut config = Self {
            samples: 7,
            suite: "all".into(),
            size: None,
            load: "unknown_qwen_unknown".into(),
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let value = args.next().ok_or("Expected an option value")?;
            match arg.as_str() {
                "--samples" => config.samples = value.parse()?,
                "--suite" => config.suite = value,
                "--size-mib" => config.size = Some(value.parse()?),
                "--load-label" => config.load = value,
                _ => return Err(format!("Unknown option: {arg}").into()),
            }
        }
        if !(1..=31).contains(&config.samples)
            || !matches!(config.suite.as_str(), "all" | "search" | "decoder" | "diff")
            || config.size.is_some_and(|size| ![1, 4, 16].contains(&size))
            || config.load.len() > 96
            || !config
                .load
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
            || (config.suite == "decoder" && config.size == Some(16))
        {
            return Err("Use --samples 1..31 --suite all|search|decoder|diff --size-mib 1|4|16; load label ASCII alphanumeric/_/-".into());
        }
        Ok(config)
    }
    fn includes(&self, suite: &str) -> bool {
        self.suite == "all" || self.suite == suite
    }
}

struct Cohort {
    root: PathBuf,
    paths: Vec<PathBuf>,
    encoding: TextEncoding,
    bytes: usize,
    hash: u64,
}
impl Cohort {
    fn load(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(self.bytes);
        for path in &self.paths {
            fs::File::open(path)?
                .take(MIB as u64 + 1)
                .read_to_end(&mut bytes)?;
        }
        if bytes.len() != self.bytes || hash_bytes(HASH_INITIAL, &bytes) != self.hash {
            return Err("Fixture size/hash changed".into());
        }
        Ok(bytes)
    }
}

struct Fixtures {
    workspace: PathBuf,
    base: PathBuf,
    root: PathBuf,
    files: Vec<PathBuf>,
    directories: Vec<PathBuf>,
}
fn reparse(path: &Path) -> io::Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        Ok(metadata.file_attributes() & 0x400 != 0)
    }
    #[cfg(not(windows))]
    {
        Ok(metadata.file_type().is_symlink())
    }
}
impl Fixtures {
    fn new() -> Result<Self> {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .ok_or("Workspace root unavailable")?
            .canonicalize()?;
        let work = workspace.join("work");
        if !work.exists() {
            fs::create_dir(&work)?;
        }
        if reparse(&work)? {
            return Err("Fixture work directory must not be a reparse point".into());
        }
        let work = work.canonicalize()?;
        if !work.starts_with(&workspace) || reparse(&work)? {
            return Err("Fixture work directory escaped the workspace".into());
        }
        let base = work.join("test-temp");
        if !base.exists() {
            fs::create_dir(&base)?;
        }
        if reparse(&base)? {
            return Err("Fixture base must not be a reparse point".into());
        }
        let base = base.canonicalize()?;
        if !base.starts_with(&workspace) || base == workspace || reparse(&base)? {
            return Err("Fixture base must be an ordinary directory inside the workspace".into());
        }
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = base.join(format!("cpu-workloads-{}-{stamp}", std::process::id()));
        fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        Ok(Self {
            workspace,
            base,
            root,
            files: Vec::new(),
            directories: Vec::new(),
        })
    }

    fn cohort(&mut self, size_mib: usize, encoding: TextEncoding) -> Result<Cohort> {
        let root = self.root.join(format!("{}-{size_mib}", encoding.name()));
        fs::create_dir(&root)?;
        self.directories.push(root.clone());
        let bytes = size_mib * MIB;
        let file_bytes = if size_mib == 16 { MIB } else { MIB / 4 };
        let block = corpus_block(encoding);
        let mut paths = Vec::new();
        let mut hash = HASH_INITIAL;
        for ordinal in 0..bytes / file_bytes {
            let path = root.join(format!("source-{ordinal:03}.txt"));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            self.files.push(path.clone());
            for _ in 0..file_bytes / BLOCK {
                file.write_all(&block)?;
                hash = hash_bytes(hash, &block);
            }
            paths.push(path);
        }
        Ok(Cohort {
            root,
            paths,
            encoding,
            bytes,
            hash,
        })
    }

    fn cleanup(&mut self) -> Result<()> {
        // Never recursively delete. Recheck resolved ownership before removing
        // only create_new files and the exact directories created by this probe.
        let root = self.root.canonicalize()?;
        let base = self.base.canonicalize()?;
        if base != self.base
            || !base.starts_with(&self.workspace)
            || root.parent() != Some(base.as_path())
            || root != self.root
            || reparse(&root)?
            || !root
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("cpu-workloads-"))
        {
            return Err("Refusing cleanup outside the owned workspace fixture root".into());
        }
        for path in &self.files {
            let resolved = path.canonicalize()?;
            if !resolved.starts_with(&root) || reparse(path)? {
                return Err("Refusing cleanup of a changed fixture path".into());
            }
            fs::remove_file(path)?;
        }
        self.files.clear();
        for path in self.directories.iter().rev() {
            if !path.canonicalize()?.starts_with(&root) || reparse(path)? {
                return Err("Refusing cleanup of a changed fixture directory".into());
            }
            fs::remove_dir(path)?;
        }
        self.directories.clear();
        fs::remove_dir(&root)?;
        Ok(())
    }
}
impl Drop for Fixtures {
    fn drop(&mut self) {
        if self.root.exists() {
            if let Err(error) = self.cleanup() {
                eprintln!(
                    "fixture_cleanup_failed={error}; retained={}",
                    self.root.display()
                );
            }
        }
    }
}

fn corpus_block(encoding: TextEncoding) -> Vec<u8> {
    let mut block = vec![b'x'; BLOCK];
    let prefix: &[u8] = match encoding {
        TextEncoding::Utf8 => "café Straße α 🙂 ".as_bytes(),
        TextEncoding::Windows1252 => b"caf\xe9 Stra\xdfe \x80 ",
        _ => unreachable!("fixed probe encodings"),
    };
    for line in block.chunks_exact_mut(256) {
        line[..prefix.len()].copy_from_slice(prefix);
        line[254..].copy_from_slice(b"\r\n");
    }
    block[..MARKER.len()].copy_from_slice(MARKER);
    block
}

#[derive(Clone, Copy, Default)]
struct Point {
    user_ns: u64,
    kernel_ns: u64,
    private: u64,
    working: u64,
    lifetime_working: u64,
    lifetime_pagefile: u64,
}
#[cfg(windows)]
fn point() -> io::Result<Point> {
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[repr(C)]
    struct Counters {
        cb: u32,
        faults: u32,
        peak_working: usize,
        working: usize,
        peak_paged: usize,
        paged: usize,
        peak_nonpaged: usize,
        nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn GetProcessTimes(
            process: *mut std::ffi::c_void,
            created: *mut FileTime,
            exited: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }
    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            size: u32,
        ) -> i32;
    }
    let mut created = FileTime::default();
    let mut exited = FileTime::default();
    let mut kernel = FileTime::default();
    let mut user = FileTime::default();
    // OS fills repr(C) POD buffers. Pseudo-handle refers only to this process,
    // needs no CloseHandle and never opens another process.
    let mut counters: Counters = unsafe { std::mem::zeroed() };
    counters.cb = std::mem::size_of::<Counters>() as u32;
    let process = unsafe { GetCurrentProcess() };
    if unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) } == 0
        || unsafe { GetProcessMemoryInfo(process, &mut counters, counters.cb) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let ns = |time: FileTime| ((u64::from(time.high) << 32) | u64::from(time.low)) * 100;
    Ok(Point {
        user_ns: ns(user),
        kernel_ns: ns(kernel),
        private: counters.private as u64,
        working: counters.working as u64,
        lifetime_working: counters.peak_working as u64,
        lifetime_pagefile: counters.peak_pagefile as u64,
    })
}
#[cfg(not(windows))]
fn point() -> io::Result<Point> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "This CPU protocol requires Windows process counters",
    ))
}

#[derive(Default)]
struct Peaks {
    private: u64,
    working: u64,
    cpu_pct: f64,
    samples: usize,
}
impl Peaks {
    fn observe(&mut self, value: Point) {
        self.private = self.private.max(value.private);
        self.working = self.working.max(value.working);
        self.samples += 1;
    }
}
struct Monitor {
    stop: Arc<(Mutex<bool>, Condvar)>,
    worker: Option<JoinHandle<io::Result<Peaks>>>,
}
impl Monitor {
    fn start() -> Result<Self> {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let stopping = stop.clone();
        let (started, ready) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("cpu-probe-own-process".into())
            .spawn(move || {
                let mut previous = point()?;
                let mut at = Instant::now();
                let mut peaks = Peaks::default();
                peaks.observe(previous);
                let _ = started.send(());
                loop {
                    let guard = stopping.0.lock().unwrap_or_else(|error| error.into_inner());
                    let (guard, _) = stopping
                        .1
                        .wait_timeout_while(guard, Duration::from_millis(10), |stop| !*stop)
                        .unwrap_or_else(|error| error.into_inner());
                    if *guard {
                        break;
                    }
                    drop(guard);
                    let current = point()?;
                    let elapsed = at.elapsed().as_nanos() as f64;
                    let cpu = current.user_ns.saturating_sub(previous.user_ns)
                        + current.kernel_ns.saturating_sub(previous.kernel_ns);
                    if elapsed > 0.0 {
                        peaks.cpu_pct = peaks.cpu_pct.max(cpu as f64 * 100.0 / elapsed);
                    }
                    peaks.observe(current);
                    previous = current;
                    at = Instant::now();
                }
                Ok(peaks)
            })?;
        let mut monitor = Self {
            stop,
            worker: Some(worker),
        };
        if ready.recv_timeout(Duration::from_secs(3)).is_err() {
            let _ = monitor.finish();
            return Err("Process sampler failed to initialize".into());
        }
        Ok(monitor)
    }
    fn finish(&mut self) -> Result<Peaks> {
        *self
            .stop
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = true;
        self.stop.1.notify_all();
        Ok(self
            .worker
            .take()
            .ok_or("Sampler already joined")?
            .join()
            .map_err(|_| "Process sampler panicked")??)
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let _ = self.finish();
        }
    }
}

#[derive(Clone)]
struct Metrics {
    // All fields emitted as independent raw samples, then per-column quantiles.
    values: [f64; 15],
}
fn measure<T>(operation: impl FnOnce() -> Result<T>) -> Result<(Metrics, T)> {
    let mut monitor = Monitor::start()?;
    let before = point()?;
    let at = Instant::now();
    let output = operation()?;
    let wall = at.elapsed().as_nanos() as f64;
    let after = point()?;
    let mut peaks = monitor.finish()?;
    let cpu_intervals = peaks.samples.saturating_sub(1);
    peaks.observe(before);
    peaks.observe(after);
    let user = after.user_ns.saturating_sub(before.user_ns) as f64;
    let kernel = after.kernel_ns.saturating_sub(before.kernel_ns) as f64;
    Ok((
        Metrics {
            values: [
                wall,
                user,
                kernel,
                (user + kernel) * 100.0 / wall.max(1.0),
                before.private as f64,
                after.private as f64,
                peaks.private as f64,
                before.working as f64,
                after.working as f64,
                peaks.working as f64,
                peaks.cpu_pct,
                after.lifetime_working as f64,
                after.lifetime_pagefile as f64,
                peaks.samples as f64,
                cpu_intervals as f64,
            ],
        },
        output,
    ))
}

#[derive(Clone)]
struct Outcome {
    state: String,
    units: usize,
    rows: usize,
    issues: usize,
    checksum: u64,
    work: usize,
    compared: usize,
    metadata: usize,
    throughput_bytes: usize,
}
impl Outcome {
    fn new(state: impl Into<String>) -> Self {
        Self {
            state: state.into(),
            units: 0,
            rows: 0,
            issues: 0,
            checksum: 0,
            work: 0,
            compared: 0,
            metadata: 0,
            throughput_bytes: 0,
        }
    }
}
struct Case<'a> {
    name: &'a str,
    encoding: &'a str,
    bytes: usize,
    files: usize,
    hash: u64,
    batch: usize,
}
fn emit(kind: &str, sample: usize, case: &Case<'_>, metrics: &Metrics, out: &Outcome, load: &str) {
    print!(
        "{PROTOCOL},{kind},{sample},{},{},{},{},{:016x},{},{load},{},{},{},{},{:016x},{},{},{},{}",
        case.name,
        case.encoding,
        case.bytes,
        case.files,
        case.hash,
        case.batch,
        out.state,
        out.units,
        out.rows,
        out.issues,
        out.checksum,
        out.work,
        out.compared,
        out.metadata,
        if out.throughput_bytes == 0 {
            "unavailable"
        } else {
            "complete_eligible_corpus"
        }
    );
    for value in metrics.values {
        print!(",{value:.3}");
    }
    if out.throughput_bytes > 0 {
        print!(
            ",{:.6}",
            out.throughput_bytes as f64 / MIB as f64 * 1e9 / metrics.values[0].max(1.0)
        );
    } else {
        print!(",");
    }
    println!();
}
fn repeated(
    config: &Config,
    case: Case<'_>,
    mut operation: impl FnMut() -> Result<Outcome>,
) -> Result<()> {
    let (warm, result) = measure(&mut operation)?;
    emit("warmup", 0, &case, &warm, &result, &config.load);
    let mut samples = Vec::with_capacity(config.samples);
    let mut last = result;
    for sample in 0..config.samples {
        let (metrics, result) = measure(&mut operation)?;
        emit("sample", sample, &case, &metrics, &result, &config.load);
        samples.push(metrics);
        last = result;
    }
    for (kind, numerator) in [("p50", 50usize), ("p95", 95)] {
        let mut summary = Metrics { values: [0.0; 15] };
        let rank = (config.samples * numerator).div_ceil(100).saturating_sub(1);
        for column in 0..15 {
            let mut values: Vec<_> = samples.iter().map(|sample| sample.values[column]).collect();
            values.sort_unstable_by(f64::total_cmp);
            summary.values[column] = values[rank];
        }
        emit(kind, config.samples, &case, &summary, &last, &config.load);
    }
    Ok(())
}

fn search(config: &Config, cohort: &Cohort) -> Result<()> {
    let app = Workbench::default(); // Disk-only corpus; no open-buffer snapshots.
    let wake = Wakeup::default();
    let mut search = ProjectSearch::new(wake.clone());
    for (name, query, expected) in [
        ("search_zero", "JC_CPU_ABSENT", 0),
        ("search_few", "JC_CPU_HIT", cohort.bytes / BLOCK),
    ] {
        repeated(
            config,
            Case {
                name,
                encoding: cohort.encoding.name(),
                bytes: cohort.bytes,
                files: cohort.paths.len(),
                hash: cohort.hash,
                batch: 1,
            },
            || {
                let mut options = ProjectSearchOptions::literal(query);
                options.encoding = Some(cohort.encoding);
                search.start(&app, &cohort.root, options)?;
                let deadline = Instant::now() + Duration::from_secs(15);
                while search.state == ProjectSearchState::Running {
                    let ticket = wake.ticket();
                    search.poll(&app, 4);
                    if search.state == ProjectSearchState::Running && !search.ready() {
                        let remaining = deadline
                            .checked_duration_since(Instant::now())
                            .ok_or("Project search probe exceeded its wait deadline")?;
                        wake.wait(ticket, Some(remaining.min(Duration::from_millis(100))));
                    }
                }
                let mut out = Outcome::new(format!("{:?}", search.state));
                let mut locations = std::collections::BTreeSet::new();
                for row in search.rows() {
                    match row {
                        ProjectSearchRow::Match(hit) => {
                            out.rows += 1;
                            out.checksum = out
                                .checksum
                                .wrapping_add(hit.range.start as u64)
                                .wrapping_add(hit.range.end as u64);
                            if hit.encoding != cohort.encoding {
                                return Err("Wrong search encoding".into());
                            }
                            if !cohort.paths.contains(&hit.path)
                                || hit.range.start % BLOCK != 0
                                || hit.range.end != hit.range.start + MARKER.len()
                                || hit.source_len != cohort.bytes / cohort.paths.len()
                                || !locations.insert((&hit.path, hit.range.start, hit.range.end))
                            {
                                return Err("Wrong or duplicated search source/range".into());
                            }
                        }
                        ProjectSearchRow::Issue { .. } => out.issues += 1,
                    }
                }
                if search.state != ProjectSearchState::Complete
                    || out.rows != expected
                    || out.issues != 0
                {
                    return Err(format!("Unexpected search outcome {name}: {} hits={} expected={expected} issues={}",
                    out.state, out.rows, out.issues).into());
                }
                if expected != 0 {
                    for path in &cohort.paths {
                        for start in (0..cohort.bytes / cohort.paths.len()).step_by(BLOCK) {
                            if !locations.contains(&(path, start, start + MARKER.len())) {
                                return Err("Expected per-file match missing".into());
                            }
                        }
                    }
                }
                out.units = cohort.paths.len();
                out.throughput_bytes = cohort.bytes;
                Ok(out)
            },
        )?;
    }
    Ok(())
}

fn decoder(config: &Config, cohort: &Cohort) -> Result<()> {
    let bytes = cohort.load()?; // Immutable source allocated once, outside samples.
    let batch = 8;
    repeated(
        config,
        Case {
            name: "decoder_borrowed",
            encoding: cohort.encoding.name(),
            bytes: bytes.len(),
            files: cohort.paths.len(),
            hash: cohort.hash,
            batch,
        },
        || {
            let mut out = Outcome::new("Complete");
            for _ in 0..batch {
                let mut end = 0;
                for scalar in DecodedScalars::new(&bytes, cohort.encoding, 0) {
                    if !scalar.valid || scalar.source_range.start != end {
                        return Err("Decoder source map/validity mismatch".into());
                    }
                    end = scalar.source_range.end;
                    out.units += 1;
                    out.checksum = out
                        .checksum
                        .wrapping_mul(31)
                        .wrapping_add(u64::from(scalar.character as u32))
                        .wrapping_add(end as u64);
                }
                if end != bytes.len() {
                    return Err("Decoder did not consume the fixture".into());
                }
            }
            black_box(out.checksum);
            out.throughput_bytes = bytes.len() * batch;
            Ok(out)
        },
    )
}

fn diff(config: &Config, cohort: &Cohort) -> Result<()> {
    let left = cohort.load()?;
    let batch = 4;
    for adversarial in [false, true] {
        let mut right = left.clone(); // One bounded input preparation, never per sample.
        for (line, bytes) in right.chunks_exact_mut(256).enumerate() {
            if adversarial || line % 1024 == 0 {
                bytes[0] = b'Z';
            }
        }
        let hash = hash_bytes(cohort.hash, &right);
        let name = if adversarial {
            "diff_adversarial"
        } else {
            "diff_sparse"
        };
        repeated(
            config,
            Case {
                name,
                encoding: "raw_bytes",
                bytes: left.len() + right.len(),
                files: 2,
                hash,
                batch,
            },
            || {
                let mut out = Outcome::new("");
                for _ in 0..batch {
                    let model = DiffModel::compute(&left, &right, DiffLimits::default(), || false);
                    let status = model.status();
                    if (!adversarial && status != DiffStatus::Exact)
                        || status == DiffStatus::Cancelled
                    {
                        return Err(format!("Unexpected diff outcome: {status:?}").into());
                    }
                    out.state = format!("{status:?}");
                    let stats = model.stats();
                    out.work += stats.work_units;
                    out.compared += stats.compared_bytes;
                    out.metadata = out.metadata.max(stats.peak_metadata_bytes);
                    out.rows += model.hunks().count();
                    for span in model.spans() {
                        out.checksum = out
                            .checksum
                            .wrapping_add(span.left.start.0 as u64)
                            .wrapping_add(span.right.end.0 as u64);
                    }
                    if status == DiffStatus::Exact {
                        out.throughput_bytes += left.len() + right.len();
                    }
                    out.units += 1;
                }
                black_box(out.checksum);
                Ok(out)
            },
        )?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let config = Config::parse()?;
    point()?; // Unsupported counters fail explicitly, never report invented zeros.
    eprintln!("protocol={PROTOCOL}; pid={}; samples={}; warmups=1; sampler_ms=10; concurrent_load={}; filesystem_cache=uncontrolled; hardware=record_by_integrator; worker_affinity_priority=unchanged",
        std::process::id(), config.samples, config.load);
    println!("protocol,kind,sample,case,encoding,corpus_bytes,files,corpus_hash,batch,concurrent_load,state,units,rows,issues,checksum,work_units,compared_bytes,metadata_peak_logical,throughput_basis,wall_ns,user_ns,kernel_ns,cpu_pct_one_core,private_before,private_after,private_peak_observed,working_before,working_after,working_peak_observed,cpu_peak_pct_observed,lifetime_peak_working,lifetime_peak_pagefile,memory_observations,cpu_interval_observations,throughput_from_wall_mib_s");
    let setup = Instant::now();
    let mut fixtures = Fixtures::new()?;
    let mut cohorts = Vec::new();
    for encoding in [TextEncoding::Utf8, TextEncoding::Windows1252] {
        for size in [1, 4, 16] {
            let needed = (config.includes("search")
                && config.size.is_none_or(|chosen| chosen == size))
                || (config.includes("decoder")
                    && size != 16
                    && config.size.is_none_or(|chosen| chosen == size))
                || (config.includes("diff") && encoding == TextEncoding::Utf8 && size == 4);
            if needed {
                cohorts.push(fixtures.cohort(size, encoding)?);
            }
        }
    }
    eprintln!(
        "fixture_setup_wall_ns={}; fixture_root={}",
        setup.elapsed().as_nanos(),
        fixtures.root.display()
    );
    for cohort in &cohorts {
        eprintln!(
            "corpus encoding={} bytes={} files={} fnv1a64={:016x}",
            cohort.encoding.name(),
            cohort.bytes,
            cohort.paths.len(),
            cohort.hash
        );
        if config.includes("search") && config.size.is_none_or(|size| size * MIB == cohort.bytes) {
            search(&config, cohort)?;
        }
        if config.includes("decoder")
            && cohort.bytes <= 4 * MIB
            && config.size.is_none_or(|size| size * MIB == cohort.bytes)
        {
            decoder(&config, cohort)?;
        }
        if config.includes("diff")
            && cohort.encoding == TextEncoding::Utf8
            && cohort.bytes == 4 * MIB
        {
            diff(&config, cohort)?;
        }
    }
    fixtures.cleanup()?;
    Ok(())
}
