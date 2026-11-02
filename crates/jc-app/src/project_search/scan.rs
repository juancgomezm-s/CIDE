use super::*;
use jc_text::{decode_fragment, probe_encoding};
use std::{
    fs::{self, File},
    io::{self, Read},
    path::Component,
};

pub(super) fn absolute_source_path(path: &Path) -> io::Result<PathBuf> {
    if let Ok(path) = path.canonicalize() {
        return Ok(path);
    }
    let joined = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut path = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::ParentDir => {
                path.pop();
            }
            Component::CurDir => {}
            _ => path.push(part.as_os_str()),
        }
    }
    Ok(path)
}
fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}
pub(super) fn path_included(options: &ProjectSearchOptions, root: &Path, path: &Path) -> bool {
    let path = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    options.include.iter().any(|p| glob(p, &path))
        && !options.exclude.iter().any(|p| glob(p, &path))
}
pub(super) fn read_file(root: &Path, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || reparse(&metadata) {
        return Err(io::Error::other("Links/reparse points are excluded"));
    }
    let resolved = path.canonicalize()?;
    if !resolved.starts_with(root) {
        return Err(io::Error::other("File is outside the search root"));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(&resolved)?;
    if reparse(&file.metadata()?) {
        return Err(io::Error::other("File became a reparse point"));
    }
    verify_handle(root, &resolved, &file)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(io::Error::other("File exceeds the search byte budget"));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::other("File grew beyond the search byte budget"));
    }
    Ok(bytes)
}
#[cfg(windows)]
fn verify_handle(root: &Path, _: &Path, file: &File) -> io::Result<()> {
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFinalPathNameByHandleW(
            handle: *mut std::ffi::c_void,
            path: *mut u16,
            length: u32,
            flags: u32,
        ) -> u32;
    }
    let mut buffer = vec![0u16; 32768];
    let len = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    };
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    if len >= buffer.len() as u32 {
        return Err(io::Error::other("Resolved path too long"));
    }
    let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len as usize]));
    if !path.starts_with(root) {
        return Err(io::Error::other("Opened handle escaped the search root"));
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn verify_handle(root: &Path, _: &Path, file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    if !fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))?.starts_with(root) {
        return Err(io::Error::other("Opened handle escaped the search root"));
    }
    Ok(())
}
#[cfg(not(any(windows, target_os = "linux")))]
fn verify_handle(_: &Path, _: &Path, _: &File) -> io::Result<()> {
    Err(io::Error::other(
        "Secure project file traversal not supported on this platform yet",
    ))
}

pub(super) fn run(
    request: &Request,
    publish: impl FnMut(Vec<ProjectSearchRow>) -> bool,
) -> ProjectSearchState {
    let mut scan = Scan {
        request,
        publish,
        batch: Vec::new(),
        rows: 0,
        charge: 0,
        examined: 0,
        entries: 0,
        limited: false,
        stopped: false,
        pattern: crate::text_search::Pattern::compile(&request.options.query, true, false)
            .expect("validated project query"),
    };
    for (path, source) in &request.open {
        if scan.included(path) {
            scan.file(path, &source.bytes, Some(source.stamp));
            scan.flush();
        }
        if scan.done() {
            break;
        }
    }
    let mut stack = Vec::new();
    match fs::read_dir(&request.root) {
        Ok(dir) => stack.push(dir),
        Err(error) => {
            scan.issue(&request.root, error.to_string());
        }
    }
    while !scan.done() {
        let Some(dir) = stack.last_mut() else {
            break;
        };
        let Some(entry) = dir.next() else {
            stack.pop();
            continue;
        };
        scan.entries += 1;
        if scan.entries > request.options.max_entries {
            scan.limited = true;
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                scan.issue(&request.root, e.to_string());
                continue;
            }
        };
        let path = entry.path();
        if path.as_os_str().len() > 4096 {
            scan.issue(&request.root, "Path exceeds 4096 units".into());
            continue;
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                scan.issue(&path, e.to_string());
                continue;
            }
        };
        let relative = scan.relative(&path);
        if request
            .options
            .exclude
            .iter()
            .any(|p| glob(p, &relative) || glob(p, &(relative.clone() + "/")))
        {
            continue;
        }
        if reparse(&metadata) {
            scan.issue(&path, "Link/reparse point skipped".into());
            continue;
        }
        if metadata.is_dir() {
            let relative = scan.relative(&path) + "/";
            if request.options.exclude.iter().any(|p| glob(p, &relative)) {
                continue;
            }
            if stack.len() >= 32 {
                scan.issue(&path, "Directory depth limit reached".into());
                scan.limited = true;
                break;
            }
            match path.canonicalize().and_then(|canonical| {
                if canonical.starts_with(&request.root) {
                    fs::read_dir(canonical)
                } else {
                    Err(io::Error::other("Directory escaped the search root"))
                }
            }) {
                Ok(dir) => stack.push(dir),
                Err(e) => {
                    scan.issue(&path, e.to_string());
                }
            }
        } else if metadata.is_file() && !request.open.contains_key(&path) && scan.included(&path) {
            let remaining = request.options.max_scan_bytes.saturating_sub(scan.examined);
            if remaining == 0 {
                scan.limited = true;
                break;
            }
            match read_file(
                &request.root,
                &path,
                request.options.max_file_bytes.min(remaining),
            ) {
                Ok(bytes) => {
                    scan.file(&path, &bytes, None);
                    scan.flush();
                }
                Err(e) => {
                    scan.issue(&path, e.to_string());
                }
            }
        }
    }
    if request.cancel.is_cancelled() {
        return ProjectSearchState::Cancelled;
    }
    if !scan.flush() {
        return ProjectSearchState::Cancelled;
    }
    if scan.limited {
        ProjectSearchState::Limited
    } else {
        ProjectSearchState::Complete
    }
}
struct Scan<'a, F> {
    request: &'a Request,
    publish: F,
    batch: Vec<ProjectSearchRow>,
    rows: usize,
    charge: usize,
    examined: usize,
    entries: usize,
    limited: bool,
    stopped: bool,
    pattern: Arc<crate::text_search::Pattern>,
}
impl<F: FnMut(Vec<ProjectSearchRow>) -> bool> Scan<'_, F> {
    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.request.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
    fn included(&self, path: &Path) -> bool {
        path_included(&self.request.options, &self.request.root, path)
    }
    fn done(&self) -> bool {
        self.limited || self.stopped || self.request.cancel.is_cancelled()
    }
    fn flush(&mut self) -> bool {
        if !self.batch.is_empty() {
            self.stopped = !(self.publish)(std::mem::take(&mut self.batch));
        }
        !self.stopped
    }
    fn emit(&mut self, row: ProjectSearchRow) -> bool {
        if self.rows >= self.request.options.max_results
            || self.charge + row.charge() > 2 * 1024 * 1024
        {
            self.limited = true;
            return false;
        }
        self.rows += 1;
        self.charge += row.charge();
        self.batch.push(row);
        if self.batch.len() >= 32 {
            self.flush();
        }
        !self.done()
    }
    fn issue(&mut self, path: &Path, mut message: String) -> bool {
        if message.len() > 512 {
            let mut end = 512;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
        }
        self.emit(ProjectSearchRow::Issue {
            path: path.to_owned(),
            message,
        })
    }
    fn file(&mut self, path: &Path, bytes: &[u8], stamp: Option<TaskStamp>) {
        if bytes.len()
            > self
                .request
                .options
                .max_scan_bytes
                .saturating_sub(self.examined)
        {
            self.limited = true;
            return;
        }
        self.examined += bytes.len();
        if bytes.contains(&0) {
            self.issue(path, "Binary/NUL file skipped".into());
            return;
        }
        let probe = probe_encoding(
            bytes,
            stamp.map(|s| s.encoding).or(self.request.options.encoding),
        );
        let Some(encoding) = probe.encoding else {
            self.issue(
                path,
                "Unknown legacy encoding; choose a project encoding".into(),
            );
            return;
        };
        if encoding == TextEncoding::Utf8
            && !matches!(
                probe.source,
                jc_text::EncodingSource::AsciiCompatible | jc_text::EncodingSource::ValidUtf8
            )
            && std::str::from_utf8(bytes).is_err()
        {
            self.issue(
                path,
                "Invalid UTF-8; file skipped without converting bytes".into(),
            );
            return;
        }
        let origin = stamp.map_or_else(
            || SearchOrigin::Disk(ContentFingerprint::of(bytes)),
            SearchOrigin::Open,
        );
        let limits = crate::SearchLimits {
            max_results: self.request.options.max_results,
            total_time: Duration::from_secs(10),
            ..crate::SearchLimits::default()
        };
        let mut scanner =
            crate::text_search::Scanner::new(self.pattern.clone(), encoding, bytes.len());
        loop {
            if self.done() {
                return;
            }
            let step = scanner.step(limits, |range| bytes[range].to_vec());
            for hit in step.matches {
                let start = hit.range.start;
                let mut end = (start + 160).min(bytes.len());
                if encoding == TextEncoding::Utf8 {
                    while end < bytes.len() && bytes[end] & 0xc0 == 0x80 {
                        end -= 1;
                    }
                }
                let snippet = decode_fragment(&bytes[start..end], encoding, start)
                    .text
                    .chars()
                    .take_while(|c| *c != '\r' && *c != '\n')
                    .take(80)
                    .collect();
                if !self.emit(ProjectSearchRow::Match(ProjectSearchMatch {
                    path: path.to_owned(),
                    range: hit.range,
                    position: hit.position,
                    snippet,
                    encoding,
                    origin,
                    source_len: bytes.len(),
                })) {
                    return;
                }
            }
            match step.status {
                crate::SearchStatus::Complete => break,
                crate::SearchStatus::Running => {}
                _ => {
                    self.limited = true;
                    return;
                }
            }
        }
    }
}

fn glob(pattern: &str, path: &str) -> bool {
    if let Some(tail) = pattern.strip_prefix("**/") {
        if glob(tail, path) {
            return true;
        }
    }
    let path = if !pattern.contains('/') {
        path.rsplit('/').next().unwrap_or(path)
    } else {
        path
    };
    let chars: Vec<char> = path.chars().collect();
    let pattern: Vec<char> = pattern.chars().collect();
    let mut row = vec![false; chars.len() + 1];
    row[0] = true;
    let mut i = 0;
    while i < pattern.len() {
        let ch = pattern[i];
        let mut next = vec![false; chars.len() + 1];
        if ch == '*' {
            let double = i + 1 < pattern.len() && pattern[i + 1] == '*';
            if double {
                i += 1;
            }
            next[0] = row[0];
            for j in 1..=chars.len() {
                next[j] = row[j] || next[j - 1] && (double || chars[j - 1] != '/');
            }
        } else {
            for j in 1..=chars.len() {
                next[j] = row[j - 1] && (ch == chars[j - 1] || ch == '?' && chars[j - 1] != '/');
            }
        }
        row = next;
        i += 1;
    }
    row[chars.len()]
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn handle_verification_rejects_an_outside_handle_even_with_an_inside_path() {
        let root = std::env::temp_dir().join(format!("jc-project-handle-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let inside = root.join("root");
        fs::create_dir(&inside).unwrap();
        let outside = root.join("outside.txt");
        fs::write(&outside, b"private").unwrap();
        let file = File::open(&outside).unwrap();
        assert!(verify_handle(
            &inside.canonicalize().unwrap(),
            &inside.join("fake.txt"),
            &file
        )
        .is_err());
        drop(file);
        fs::remove_file(outside).unwrap();
        fs::remove_dir(inside).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
