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
fn read_retry(file: &mut File, buffer: &mut [u8]) -> io::Result<usize> {
    loop {
        match file.read(buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
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
    let initial = usize::try_from(file.metadata()?.len())
        .unwrap_or(limit)
        .min(limit);
    let mut file = file;
    let mut bytes = vec![0; initial];
    let mut filled = 0;
    loop {
        while filled < bytes.len() {
            let count = read_retry(&mut file, &mut bytes[filled..])?;
            if count == 0 {
                bytes.truncate(filled);
                return Ok(bytes);
            }
            filled += count;
        }
        // Probe growth in a stack byte: a full Vec must not double just to read
        // the sentinel beyond its admitted payload budget.
        let mut guard = [0u8; 1];
        if read_retry(&mut file, &mut guard)? == 0 {
            return Ok(bytes);
        }
        if filled == limit {
            return Err(io::Error::other("File grew beyond the search byte budget"));
        }
        let target = bytes.len().saturating_mul(2).max(filled + 1).min(limit);
        bytes.reserve_exact(target - bytes.len());
        bytes.resize(target, 0);
        if bytes.capacity() > limit {
            return Err(io::Error::other(
                "File buffer exceeds the search byte budget",
            ));
        }
        bytes[filled] = guard[0];
        filled += 1;
    }
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

#[derive(Clone, Copy)]
pub(super) enum Source {
    Open,
    Disk { limit: usize },
}
pub(super) struct FileTask {
    pub path: PathBuf,
    pub source: Source,
    pub reservation: usize,
}
pub(super) enum WalkItem {
    Open(PathBuf),
    Disk(PathBuf, u64),
    Issue(PathBuf, String),
    Limited(Option<(PathBuf, String)>),
}
pub(super) struct Walker<'a> {
    request: &'a Request,
    open: std::collections::btree_map::Iter<'a, PathBuf, OpenSource>,
    stack: Vec<fs::ReadDir>,
    entries: usize,
    disk_started: bool,
    finished: bool,
}
impl<'a> Walker<'a> {
    pub fn new(request: &'a Request) -> Self {
        Self {
            request,
            open: request.open.iter(),
            stack: Vec::new(),
            entries: 0,
            disk_started: false,
            finished: false,
        }
    }
    pub fn next(&mut self) -> Option<WalkItem> {
        if self.finished || self.request.cancel.is_cancelled() {
            return None;
        }
        for (path, _) in self.open.by_ref() {
            if path_included(&self.request.options, &self.request.root, path) {
                return Some(WalkItem::Open(path.clone()));
            }
        }
        if !self.disk_started {
            self.disk_started = true;
            match fs::read_dir(&self.request.root) {
                Ok(dir) => self.stack.push(dir),
                Err(error) => {
                    self.finished = true;
                    return Some(WalkItem::Issue(
                        self.request.root.clone(),
                        error.to_string(),
                    ));
                }
            }
        }
        loop {
            if self.request.cancel.is_cancelled() {
                return None;
            }
            let dir = self.stack.last_mut()?;
            let Some(entry) = dir.next() else {
                self.stack.pop();
                continue;
            };
            self.entries += 1;
            if self.entries > self.request.options.max_entries {
                self.finished = true;
                return Some(WalkItem::Limited(None));
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    return Some(WalkItem::Issue(
                        self.request.root.clone(),
                        error.to_string(),
                    ))
                }
            };
            let path = entry.path();
            if path.as_os_str().len() > 4096 {
                return Some(WalkItem::Issue(
                    self.request.root.clone(),
                    "Path exceeds 4096 units".into(),
                ));
            }
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => return Some(WalkItem::Issue(path, error.to_string())),
            };
            let relative = path
                .strip_prefix(&self.request.root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if self
                .request
                .options
                .exclude
                .iter()
                .any(|p| glob(p, &relative) || glob(p, &(relative.clone() + "/")))
            {
                continue;
            }
            if reparse(&metadata) {
                return Some(WalkItem::Issue(path, "Link/reparse point skipped".into()));
            }
            if metadata.is_dir() {
                if self.stack.len() >= 32 {
                    self.finished = true;
                    return Some(WalkItem::Limited(Some((
                        path,
                        "Directory depth limit reached".into(),
                    ))));
                }
                match path.canonicalize().and_then(|canonical| {
                    if canonical.starts_with(&self.request.root) {
                        fs::read_dir(canonical)
                    } else {
                        Err(io::Error::other("Directory escaped the search root"))
                    }
                }) {
                    Ok(dir) => self.stack.push(dir),
                    Err(error) => return Some(WalkItem::Issue(path, error.to_string())),
                }
            } else if metadata.is_file()
                && !self.request.open.contains_key(&path)
                && path_included(&self.request.options, &self.request.root, &path)
            {
                return Some(WalkItem::Disk(path, metadata.len()));
            }
        }
    }
}

pub(super) fn issue(path: &Path, mut message: String) -> ProjectSearchRow {
    if message.len() > 512 {
        let mut end = 512;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
    }
    ProjectSearchRow::Issue {
        path: path.to_owned(),
        message,
    }
}
#[derive(Debug, Clone, Copy)]
pub(super) struct FileReport {
    pub charged: usize,
    pub limited: bool,
}

/// The only file scanner, used by scanner0 and helpers without splitting text.
pub(super) fn file(
    request: &Request,
    task: &FileTask,
    pattern: Arc<crate::text_search::Pattern>,
    mut stopped: impl FnMut() -> bool,
    mut emit: impl FnMut(ProjectSearchRow) -> bool,
) -> FileReport {
    let mut report = FileReport {
        charged: 0,
        limited: false,
    };
    if stopped() || request.cancel.is_cancelled() {
        return report;
    }
    let disk;
    let (bytes, stamp) = match task.source {
        Source::Open => {
            let source = request.open.get(&task.path).expect("admitted open source");
            (source.bytes.as_slice(), Some(source.stamp))
        }
        Source::Disk { limit } => match read_file(&request.root, &task.path, limit) {
            Ok(bytes) => {
                disk = bytes;
                (disk.as_slice(), None)
            }
            Err(error) => {
                emit(issue(&task.path, error.to_string()));
                return report;
            }
        },
    };
    report.charged = bytes.len();
    if stopped() || request.cancel.is_cancelled() {
        return report;
    }
    if bytes.contains(&0) {
        emit(issue(&task.path, "Binary/NUL file skipped".into()));
        return report;
    }
    let probe = probe_encoding(
        bytes,
        stamp.map(|s| s.encoding).or(request.options.encoding),
    );
    let Some(encoding) = probe.encoding else {
        emit(issue(
            &task.path,
            "Unknown legacy encoding; choose a project encoding".into(),
        ));
        return report;
    };
    if encoding == TextEncoding::Utf8
        && !matches!(
            probe.source,
            jc_text::EncodingSource::AsciiCompatible | jc_text::EncodingSource::ValidUtf8
        )
        && std::str::from_utf8(bytes).is_err()
    {
        emit(issue(
            &task.path,
            "Invalid UTF-8; file skipped without converting bytes".into(),
        ));
        return report;
    }
    let origin = stamp.map_or_else(
        || SearchOrigin::Disk(ContentFingerprint::of(bytes)),
        SearchOrigin::Open,
    );
    let limits = crate::SearchLimits {
        max_results: request.options.max_results,
        total_time: Duration::from_secs(10),
        ..crate::SearchLimits::default()
    };
    let mut scanner = crate::text_search::Scanner::new(pattern, encoding, bytes.len());
    loop {
        if stopped() || request.cancel.is_cancelled() {
            return report;
        }
        let step = scanner.step(limits, |range, buffer| {
            buffer.extend_from_slice(&bytes[range])
        });
        for hit in step.matches {
            if stopped() || request.cancel.is_cancelled() {
                return report;
            }
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
            if !emit(ProjectSearchRow::Match(ProjectSearchMatch {
                path: task.path.clone(),
                range: hit.range,
                position: hit.position,
                snippet,
                encoding,
                origin,
                source_len: bytes.len(),
            })) {
                return report;
            }
        }
        match step.status {
            crate::SearchStatus::Complete => return report,
            crate::SearchStatus::Running => {}
            _ => {
                report.limited = true;
                return report;
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
