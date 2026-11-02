//! On-demand, bounded Git blob snapshots. No worktree/index mutation or UI.
//!
//! The caller supplies a trusted installed Git executable and schedules this
//! synchronous operation outside the input/render path. Windows 10+ containment
//! is required; unsupported systems fail explicitly rather than launching freely.
use std::{
    ffi::OsString,
    fmt, io,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitBlobSource {
    Head,
    Index,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitObjectId(String);
impl GitObjectId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    fn parse(bytes: &[u8]) -> Result<Self, GitReadError> {
        if ![40, 64].contains(&bytes.len())
            || !bytes.iter().all(u8::is_ascii_hexdigit)
            || bytes.iter().all(|&b| b == b'0')
        {
            return Err(GitReadError::Protocol);
        }
        Ok(Self(
            std::str::from_utf8(bytes)
                .map_err(|_| GitReadError::Protocol)?
                .to_ascii_lowercase(),
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitFileMode {
    Regular,
    Executable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepositoryIdentity {
    pub root: PathBuf,
    pub git_dir: PathBuf,
}

/// Identity of the selected object, not a promise that mutable HEAD/index still
/// select it after delivery. Index identity is path/blob/mode, not a whole-index CAS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitBlobIdentity {
    pub repository: GitRepositoryIdentity,
    pub path: String,
    pub source: GitBlobSource,
    pub commit: Option<GitObjectId>,
    pub blob: GitObjectId,
    pub mode: GitFileMode,
}

pub struct GitBlobSnapshot {
    identity: GitBlobIdentity,
    bytes: Vec<u8>,
}
impl GitBlobSnapshot {
    pub fn identity(&self) -> &GitBlobIdentity {
        &self.identity
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
impl fmt::Debug for GitBlobSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitBlobSnapshot")
            .field("identity", &self.identity)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GitReadLimits {
    pub max_blob_bytes: usize,
    pub max_metadata_bytes: usize,
    pub max_stderr_bytes: usize,
    pub timeout: Duration,
}
impl Default for GitReadLimits {
    fn default() -> Self {
        Self {
            max_blob_bytes: 2 * 1024 * 1024,
            max_metadata_bytes: 64 * 1024,
            max_stderr_bytes: 16 * 1024,
            timeout: Duration::from_secs(5),
        }
    }
}
impl GitReadLimits {
    fn valid(self) -> bool {
        self.max_blob_bytes <= 16 * 1024 * 1024
            && (1..=1024 * 1024).contains(&self.max_metadata_bytes)
            && (1..=64 * 1024).contains(&self.max_stderr_bytes)
            && !self.timeout.is_zero()
            && self.timeout <= Duration::from_secs(30)
    }
    fn metadata(self) -> OutputQuota {
        OutputQuota {
            stdout_cap: self.max_metadata_bytes,
            stdout_limit: GitOutputLimit::Metadata,
            stderr_cap: self.max_stderr_bytes,
        }
    }
}
#[derive(Clone, Copy)]
struct OutputQuota {
    stdout_cap: usize,
    stdout_limit: GitOutputLimit,
    stderr_cap: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitOutputLimit {
    Metadata,
    Blob,
    Stderr,
}

#[derive(Debug)]
pub enum GitReadError {
    UnsupportedPlatform,
    InvalidLimits,
    InvalidPath,
    UnsupportedPathEncoding,
    NotRepositoryRoot,
    Missing,
    Unmerged,
    UnsupportedMode,
    Protocol,
    Cancelled,
    TimedOut,
    OutputLimit(GitOutputLimit),
    Io(io::Error),
    Containment(io::Error),
    CommandFailed { code: u32, stderr: Vec<u8> },
}
impl fmt::Display for GitReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "Git read I/O: {error}"),
            Self::Containment(error) => write!(f, "Git process containment: {error}"),
            Self::CommandFailed { code, .. } => write!(f, "Git read command failed ({code})"),
            other => write!(f, "Git read: {other:?}"),
        }
    }
}
impl std::error::Error for GitReadError {}
impl From<io::Error> for GitReadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug)]
pub struct GitReader {
    executable: PathBuf,
}
impl GitReader {
    /// No PATH search, download or execution here. The caller chooses a trusted
    /// installed executable. This validates its absolute existing file path.
    pub fn new(executable: impl Into<PathBuf>) -> Result<Self, GitReadError> {
        let executable = executable.into();
        if !executable.is_absolute() {
            return Err(GitReadError::InvalidPath);
        }
        let executable = executable.canonicalize()?;
        if !executable.is_file() {
            return Err(GitReadError::InvalidPath);
        }
        Ok(Self { executable })
    }
    pub fn read_blob(
        &self,
        repository_root: &Path,
        source: GitBlobSource,
        path: &Path,
        limits: GitReadLimits,
        cancelled: impl FnMut() -> bool,
    ) -> Result<GitBlobSnapshot, GitReadError> {
        if !cfg!(windows) {
            return Err(GitReadError::UnsupportedPlatform);
        }
        if !limits.valid() {
            return Err(GitReadError::InvalidLimits);
        }
        let mut control = Control {
            deadline: Instant::now() + limits.timeout,
            cancelled,
        };
        control.check()?;
        let path = validate_path(path)?;
        if !repository_root.is_absolute() {
            return Err(GitReadError::InvalidPath);
        }
        let root = repository_root.canonicalize()?;
        validate_metadata_path(&root)?;
        if !root.is_dir() {
            return Err(GitReadError::InvalidPath);
        }
        let discovered = self.run(
            &root,
            None,
            &[
                "rev-parse".into(),
                "--path-format=absolute".into(),
                "--show-toplevel".into(),
                "--absolute-git-dir".into(),
            ],
            limits.metadata(),
            &mut control,
        )?;
        let lines = metadata_lines(&discovered)?;
        if lines.len() != 2 {
            return Err(GitReadError::Protocol);
        }
        let reported_root = PathBuf::from(lines[0]).canonicalize()?;
        let git_dir = PathBuf::from(lines[1]).canonicalize()?;
        validate_metadata_path(&git_dir)?;
        if root != reported_root || !git_dir.is_dir() {
            return Err(GitReadError::NotRepositoryRoot);
        }
        let repository = GitRepositoryIdentity { root, git_dir };
        let commit = if source == GitBlobSource::Head {
            let out = self.run(
                &repository.root,
                Some(&repository.git_dir),
                &[
                    "rev-parse".into(),
                    "--verify".into(),
                    "--end-of-options".into(),
                    "HEAD^{commit}".into(),
                ],
                limits.metadata(),
                &mut control,
            )?;
            Some(GitObjectId::parse(trim_line(&out)?)?)
        } else {
            None
        };
        let args = if let Some(commit) = &commit {
            vec![
                "ls-tree".into(),
                "-z".into(),
                "--full-tree".into(),
                commit.as_str().into(),
                "--".into(),
                path.clone().into(),
            ]
        } else {
            vec![
                "ls-files".into(),
                "--cached".into(),
                "--stage".into(),
                "--full-name".into(),
                "-z".into(),
                "--".into(),
                path.clone().into(),
            ]
        };
        let listed = self.run(
            &repository.root,
            Some(&repository.git_dir),
            &args,
            limits.metadata(),
            &mut control,
        )?;
        let (blob, mode) = parse_entry(&listed, path.as_bytes(), source)?;
        let size = self.run(
            &repository.root,
            Some(&repository.git_dir),
            &["cat-file".into(), "-s".into(), blob.as_str().into()],
            OutputQuota {
                stdout_cap: 128.min(limits.max_metadata_bytes),
                ..limits.metadata()
            },
            &mut control,
        )?;
        let size = std::str::from_utf8(trim_line(&size)?)
            .map_err(|_| GitReadError::Protocol)?
            .parse::<usize>()
            .map_err(|_| GitReadError::Protocol)?;
        if size > limits.max_blob_bytes {
            return Err(GitReadError::OutputLimit(GitOutputLimit::Blob));
        }
        let bytes = self.run(
            &repository.root,
            Some(&repository.git_dir),
            &["cat-file".into(), "blob".into(), blob.as_str().into()],
            OutputQuota {
                stdout_cap: size,
                stdout_limit: GitOutputLimit::Blob,
                stderr_cap: limits.max_stderr_bytes,
            },
            &mut control,
        )?;
        control.check()?;
        if bytes.len() != size {
            return Err(GitReadError::Protocol);
        }
        Ok(GitBlobSnapshot {
            identity: GitBlobIdentity {
                repository,
                path,
                source,
                commit,
                blob,
                mode,
            },
            bytes,
        })
    }
    fn run(
        &self,
        root: &Path,
        git_dir: Option<&Path>,
        args: &[OsString],
        quota: OutputQuota,
        control: &mut Control<impl FnMut() -> bool>,
    ) -> Result<Vec<u8>, GitReadError> {
        control.check()?;
        let mut command: Vec<OsString> = [
            "--no-pager",
            "--no-replace-objects",
            "--no-optional-locks",
            "--no-lazy-fetch",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "core.hooksPath=NUL",
            "-c",
            "gc.auto=0",
            "-c",
            "maintenance.auto=false",
            "-c",
            "credential.helper=",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        if let Some(git_dir) = git_dir {
            command.push("--git-dir".into());
            command.push(git_argument_path(git_dir));
            command.push("--work-tree".into());
            command.push(git_argument_path(root));
        } else {
            command.push("-C".into());
            command.push(git_argument_path(root));
        }
        command.extend_from_slice(args);
        platform::capture(
            &self.executable,
            root,
            &command,
            quota.stdout_cap,
            quota.stdout_limit,
            quota.stderr_cap,
            control,
        )
    }
}

fn git_argument_path(path: &Path) -> OsString {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let units: Vec<_> = path.as_os_str().encode_wide().collect();
        // Rust canonical paths use the Win32 verbatim namespace. Ordinary drive
        // and UNC spellings are the Git CLI boundary, while identity retains the
        // canonical Rust path. No filename/content decoding is involved.
        if units.starts_with(&[92, 92, 63, 92]) {
            let tail = &units[4..];
            if tail.len() >= 3
                && tail[1] == 58
                && tail[2] == 92
                && (tail[0] as u8).is_ascii_alphabetic()
            {
                return OsString::from_wide(tail);
            }
            if tail.starts_with(&[85, 78, 67, 92]) {
                let mut unc = vec![92, 92];
                unc.extend_from_slice(&tail[4..]);
                return OsString::from_wide(&unc);
            }
        }
    }
    path.as_os_str().to_owned()
}

struct Control<F> {
    deadline: Instant,
    cancelled: F,
}
impl<F: FnMut() -> bool> Control<F> {
    fn check(&mut self) -> Result<(), GitReadError> {
        if (self.cancelled)() {
            Err(GitReadError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(GitReadError::TimedOut)
        } else {
            Ok(())
        }
    }
}
fn validate_metadata_path(path: &Path) -> Result<(), GitReadError> {
    let value = path.to_str().ok_or(GitReadError::UnsupportedPathEncoding)?;
    if value.contains(['\0', '\r', '\n']) {
        return Err(GitReadError::InvalidPath);
    }
    Ok(())
}
fn validate_path(path: &Path) -> Result<String, GitReadError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(GitReadError::InvalidPath);
    }
    let raw = path.to_str().ok_or(GitReadError::UnsupportedPathEncoding)?;
    if raw
        .split(['/', '\\'])
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(GitReadError::InvalidPath);
    }
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(value) = component else {
            return Err(GitReadError::InvalidPath);
        };
        let value = value
            .to_str()
            .ok_or(GitReadError::UnsupportedPathEncoding)?;
        let device = value.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || device
                .strip_prefix("COM")
                .or_else(|| device.strip_prefix("LPT"))
                .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"));
        if value.is_empty()
            || value.eq_ignore_ascii_case(".git")
            || reserved
            || value.contains(['\0', '\r', '\n', ':', '<', '>', '"', '|', '?', '*'])
            || value.ends_with([' ', '.'])
        {
            return Err(GitReadError::InvalidPath);
        }
        parts.push(value);
    }
    let result = parts.join("/");
    if result.is_empty() || result.len() > 4096 {
        return Err(GitReadError::InvalidPath);
    }
    Ok(result)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;
    use std::{
        process::{Command, Stdio},
        sync::atomic::{AtomicU64, Ordering},
        thread,
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn executable() -> PathBuf {
        if let Some(path) = std::env::var_os("JC_GIT_TEST_EXE") {
            return PathBuf::from(path).canonicalize().unwrap();
        }
        std::env::split_paths(&std::env::var_os("PATH").expect("Git must already be installed"))
            .map(|p| p.join("git.exe"))
            .find(|p| p.is_file())
            .expect("Installed Git executable required; no download is attempted")
            .canonicalize()
            .unwrap()
    }
    struct Fixture {
        root: PathBuf,
        base: PathBuf,
        git: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap();
            let base = workspace.join("work/test-temp");
            fs::create_dir_all(&base).unwrap();
            let base = base.canonicalize().unwrap();
            let root = base.join(format!(
                "jc git read {}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let result = Self {
                root: root.canonicalize().unwrap(),
                base,
                git: executable(),
            };
            result.git(
                &["init", "--quiet", "--template=", "--initial-branch=main"],
                None,
            );
            result
        }
        // Fixture preparation only. These commands create synthetic objects/index
        // in this exclusively owned repository, never in the user's repository.
        fn git(&self, args: &[&str], input: Option<&Path>) -> Vec<u8> {
            let mut command = Command::new(&self.git);
            command.env_clear();
            for key in ["SYSTEMROOT", "WINDIR", "TEMP", "TMP"] {
                if let Some(value) = std::env::var_os(key) {
                    command.env(key, value);
                }
            }
            command
                .current_dir(&self.root)
                .args([
                    "--no-pager",
                    "-c",
                    "core.hooksPath=NUL",
                    "-c",
                    "core.fsmonitor=false",
                    "-c",
                    "core.autocrlf=false",
                ])
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_SYSTEM", "NUL")
                .env("GIT_CONFIG_GLOBAL", "NUL")
                .env("GIT_CONFIG_COUNT", "0")
                .env("GIT_AUTHOR_NAME", "JC fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_NAME", "JC fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
                .stdin(
                    input
                        .map(|p| Stdio::from(fs::File::open(p).unwrap()))
                        .unwrap_or(Stdio::null()),
                );
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "Fixture Git failed: {:?}",
                output.stderr
            );
            output.stdout
        }
        fn blob(&self, bytes: &[u8]) -> String {
            let path = self.root.join("fixture-object");
            fs::write(&path, bytes).unwrap();
            let output = self.git(
                &["hash-object", "-w", "--no-filters", "--", "fixture-object"],
                None,
            );
            String::from_utf8(output).unwrap().trim().to_owned()
        }
        fn index(&self, path: &str, oid: &str, mode: &str) {
            self.git(
                &[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("{mode},{oid},{path}"),
                ],
                None,
            );
        }
        fn head(&self) -> String {
            let tree = String::from_utf8(self.git(&["write-tree"], None)).unwrap();
            let commit = String::from_utf8(self.git(
                &["commit-tree", tree.trim(), "-m", "synthetic fixture"],
                None,
            ))
            .unwrap();
            self.git(&["update-ref", "HEAD", commit.trim()], None);
            commit.trim().to_owned()
        }
        fn reader(&self) -> GitReader {
            GitReader::new(&self.git).unwrap()
        }
        fn read(&self, source: GitBlobSource, path: &str) -> Result<GitBlobSnapshot, GitReadError> {
            self.reader().read_blob(
                &self.root,
                source,
                Path::new(path),
                GitReadLimits::default(),
                || false,
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let target = self.root.canonicalize().unwrap();
            assert_eq!(target, self.root);
            assert_eq!(target.parent(), Some(self.base.as_path()));
            assert!(target
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("jc git read "));
            fs::remove_dir_all(&target).unwrap();
        }
    }
    #[test]
    fn head_and_index_are_distinct_immutable_raw_blobs_without_worktree_or_index_writes() {
        let fixture = Fixture::new();
        let path = "dir with space/niño[1].p";
        let original = b"\xef\xbb\xbfni\xf1o\r\nno-final";
        let staged = b"\xffINDEX\r\nlast\n";
        let old = fixture.blob(original);
        fixture.index(path, &old, "100644");
        let commit = fixture.head();
        let new = fixture.blob(staged);
        fixture.index(path, &new, "100755");
        fs::create_dir(fixture.root.join("dir with space")).unwrap();
        fs::write(fixture.root.join(path), b"working bytes\r\n").unwrap();
        fixture.git(
            &["config", "core.fsmonitor", "this-provider-must-not-run"],
            None,
        );
        fixture.git(
            &["config", "diff.external", "this-provider-must-not-run"],
            None,
        );
        fixture.git(
            &[
                "config",
                "diff.fixture.textconv",
                "this-provider-must-not-run",
            ],
            None,
        );
        fixture.git(
            &[
                "config",
                "filter.fixture.smudge",
                "this-provider-must-not-run",
            ],
            None,
        );
        fs::write(
            fixture.root.join(".gitattributes"),
            b"* filter=fixture diff=fixture\n",
        )
        .unwrap();
        let index = fs::read(fixture.root.join(".git/index")).unwrap();
        let config = fs::read(fixture.root.join(".git/config")).unwrap();
        let head = fixture.read(GitBlobSource::Head, path).unwrap();
        let selected = fixture.read(GitBlobSource::Index, path).unwrap();
        assert_eq!(head.bytes(), original);
        assert_eq!(selected.bytes(), staged);
        assert_eq!(head.identity().commit.as_ref().unwrap().as_str(), commit);
        assert_eq!(head.identity().blob.as_str(), old);
        assert_eq!(selected.identity().blob.as_str(), new);
        assert_eq!(selected.identity().commit, None);
        assert_eq!(selected.identity().mode, GitFileMode::Executable);
        let diff = crate::diff_model::DiffModel::compute(
            head.bytes(),
            selected.bytes(),
            crate::diff_model::DiffLimits::default(),
            || false,
        );
        assert_eq!(diff.status(), crate::diff_model::DiffStatus::Exact);
        let hunks: Vec<_> = diff.hunks().collect();
        assert_eq!(hunks.len(), 1);
        assert_eq!(
            hunks[0].left,
            crate::diff_model::DiffLine(0)..crate::diff_model::DiffLine(2)
        );
        assert_eq!(
            hunks[0].right,
            crate::diff_model::DiffLine(0)..crate::diff_model::DiffLine(2)
        );
        assert_eq!(head.identity().repository.root, fixture.root);
        assert_eq!(head.identity().path, path);
        assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
        assert_eq!(fs::read(fixture.root.join(".git/config")).unwrap(), config);
        assert_eq!(
            fs::read(fixture.root.join(path)).unwrap(),
            b"working bytes\r\n"
        );
        fixture.index(path, &old, "100644");
        assert_eq!(selected.bytes(), staged);
        assert_eq!(selected.identity().blob.as_str(), new);
    }
    #[test]
    fn missing_unmerged_and_non_regular_paths_are_explicit() {
        let fixture = Fixture::new();
        let oid = fixture.blob(b"payload\r\n");
        fixture.index("file.p", &oid, "100644");
        fixture.head();
        assert!(matches!(
            fixture.read(GitBlobSource::Head, "missing.p"),
            Err(GitReadError::Missing)
        ));
        fixture.index("link.p", &oid, "120000");
        assert!(matches!(
            fixture.read(GitBlobSource::Index, "link.p"),
            Err(GitReadError::UnsupportedMode)
        ));
        fixture.git(&["update-index", "--force-remove", "--", "file.p"], None);
        let input = fixture.root.join("index-stages");
        fs::write(
            &input,
            format!("100644 {oid} 1\tfile.p\n100644 {oid} 2\tfile.p\n100644 {oid} 3\tfile.p\n"),
        )
        .unwrap();
        fixture.git(&["update-index", "--index-info"], Some(&input));
        assert!(matches!(
            fixture.read(GitBlobSource::Index, "file.p"),
            Err(GitReadError::Unmerged)
        ));
        let empty = fixture.blob(b"");
        fixture.index("empty.p", &empty, "100644");
        let snapshot = fixture
            .reader()
            .read_blob(
                &fixture.root,
                GitBlobSource::Index,
                Path::new("empty.p"),
                GitReadLimits {
                    max_blob_bytes: 0,
                    ..GitReadLimits::default()
                },
                || false,
            )
            .unwrap();
        assert!(snapshot.bytes().is_empty());
    }
    #[test]
    fn traversal_ambiguous_paths_and_parent_repository_discovery_are_rejected() {
        for path in [
            "",
            ".",
            "../a",
            "a/../b",
            "a/./b",
            "a//b",
            ".git/config",
            "x/.GIT/HEAD",
            "a.",
            "a ",
            "NUL.txt",
            "COM1",
            "a:b",
            "/outside",
            "C:\\outside",
        ] {
            assert!(
                matches!(
                    validate_path(Path::new(path)),
                    Err(GitReadError::InvalidPath)
                ),
                "{path}"
            );
        }
        assert_eq!(
            validate_path(Path::new("dir with space/niño[1].p")).unwrap(),
            "dir with space/niño[1].p"
        );
        let fixture = Fixture::new();
        let nested = fixture.root.join("nested");
        fs::create_dir(&nested).unwrap();
        let result = fixture.reader().read_blob(
            &nested,
            GitBlobSource::Index,
            Path::new("file.p"),
            GitReadLimits::default(),
            || false,
        );
        assert!(matches!(result, Err(GitReadError::NotRepositoryRoot)));
    }
    #[test]
    fn blob_metadata_and_stderr_output_limits_are_enforced() {
        let fixture = Fixture::new();
        let oid = fixture.blob(&vec![b'x'; 64 * 1024]);
        fixture.index("large.p", &oid, "100644");
        let result = fixture.reader().read_blob(
            &fixture.root,
            GitBlobSource::Index,
            Path::new("large.p"),
            GitReadLimits {
                max_blob_bytes: 8,
                ..GitReadLimits::default()
            },
            || false,
        );
        assert!(matches!(
            result,
            Err(GitReadError::OutputLimit(GitOutputLimit::Blob))
        ));
        let result = fixture.reader().read_blob(
            &fixture.root,
            GitBlobSource::Index,
            Path::new("large.p"),
            GitReadLimits {
                max_metadata_bytes: 1,
                ..GitReadLimits::default()
            },
            || false,
        );
        assert!(matches!(
            result,
            Err(GitReadError::OutputLimit(GitOutputLimit::Metadata))
        ));
        let mut control = Control {
            deadline: Instant::now() + Duration::from_secs(5),
            cancelled: || false,
        };
        let result = platform::capture(
            &std::env::current_exe().unwrap(),
            &fixture.root,
            &["--definitely-invalid-test-option".into()],
            64 * 1024,
            GitOutputLimit::Metadata,
            1,
            &mut control,
        );
        assert!(matches!(
            result,
            Err(GitReadError::OutputLimit(GitOutputLimit::Stderr))
        ));
        let mut control = Control {
            deadline: Instant::now() + Duration::from_secs(5),
            cancelled: || false,
        };
        let result = platform::capture(
            &std::env::current_exe().unwrap(),
            &fixture.root,
            &["--list".into()],
            8,
            GitOutputLimit::Metadata,
            4096,
            &mut control,
        );
        assert!(matches!(
            result,
            Err(GitReadError::OutputLimit(GitOutputLimit::Metadata))
        ));
    }
    #[test]
    fn real_process_timeout_and_cancellation_return_and_reap_the_job() {
        let fixture = Fixture::new();
        for cancel in [false, true] {
            platform::fixture_environment(vec![("JC_GIT_PROCESS_FIXTURE".into(), "1".into())]);
            let start = Instant::now();
            let mut control = Control {
                deadline: start + Duration::from_millis(if cancel { 5000 } else { 40 }),
                cancelled: || cancel && start.elapsed() > Duration::from_millis(40),
            };
            let args = [
                "--exact".into(),
                "git_read::tests::fixture_sleep_child".into(),
                "--ignored".into(),
                "--nocapture".into(),
            ];
            let result = platform::capture(
                &std::env::current_exe().unwrap(),
                &fixture.root,
                &args,
                4096,
                GitOutputLimit::Metadata,
                4096,
                &mut control,
            );
            platform::fixture_environment(Vec::new());
            assert!(if cancel {
                matches!(result, Err(GitReadError::Cancelled))
            } else {
                matches!(result, Err(GitReadError::TimedOut))
            });
            assert!(start.elapsed() < Duration::from_secs(2));
        }
    }
    fn assert_created_process_finished() {
        use std::os::windows::io::AsRawHandle;
        #[link(name = "kernel32")]
        extern "system" {
            fn WaitForSingleObject(process: *mut std::ffi::c_void, ms: u32) -> u32;
        }
        let process = platform::fixture_take_process()
            .expect("Created process observed by independent OS handle");
        assert_eq!(
            unsafe { WaitForSingleObject(process.as_raw_handle(), 0) },
            0
        );
    }
    #[test]
    fn reader_startup_failures_and_callback_unwind_reap_the_created_process() {
        let fixture = Fixture::new();
        let executable = std::env::current_exe().unwrap();
        let args = [
            "--exact".into(),
            "git_read::tests::fixture_sleep_child".into(),
            "--ignored".into(),
            "--nocapture".into(),
        ];
        platform::fixture_environment(vec![("JC_GIT_PROCESS_FIXTURE".into(), "1".into())]);
        for failed_reader in [0, 1] {
            platform::fixture_observe_process(true);
            platform::fixture_reader_failure(Some(failed_reader));
            let mut control = Control {
                deadline: Instant::now() + Duration::from_secs(5),
                cancelled: || false,
            };
            let result = platform::capture(
                &executable,
                &fixture.root,
                &args,
                4096,
                GitOutputLimit::Metadata,
                4096,
                &mut control,
            );
            platform::fixture_reader_failure(None);
            assert!(matches!(result, Err(GitReadError::Io(_))));
            assert_created_process_finished();
        }
        platform::fixture_observe_process(true);
        let mut checks = 0;
        let mut control = Control {
            deadline: Instant::now() + Duration::from_secs(5),
            cancelled: || {
                checks += 1;
                assert_ne!(checks, 2, "Injected callback panic after CreateProcess");
                false
            },
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            platform::capture(
                &executable,
                &fixture.root,
                &args,
                4096,
                GitOutputLimit::Metadata,
                4096,
                &mut control,
            )
        }));
        assert!(result.is_err());
        assert_created_process_finished();
        platform::fixture_observe_process(false);
        platform::fixture_environment(Vec::new());
    }
    #[test]
    fn successful_capture_drains_the_complete_blob_after_process_exit() {
        let fixture = Fixture::new();
        let bytes: Vec<_> = (0..1024 * 1024 + 31).map(|i| (i % 256) as u8).collect();
        let oid = fixture.blob(&bytes);
        fixture.index("full.p", &oid, "100644");
        let snapshot = fixture.read(GitBlobSource::Index, "full.p").unwrap();
        assert_eq!(snapshot.bytes(), bytes);
    }
    #[test]
    #[ignore = "Owned subprocess fixture, invoked explicitly by the process-control test"]
    fn fixture_sleep_child() {
        assert_eq!(std::env::var("JC_GIT_PROCESS_FIXTURE").unwrap(), "1");
        thread::sleep(Duration::from_secs(10));
    }

    #[test]
    fn cancellation_reaps_a_descendant_that_inherits_output_pipes() {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        #[link(name = "kernel32")]
        extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn WaitForSingleObject(handle: *mut std::ffi::c_void, ms: u32) -> u32;
        }
        let fixture = Fixture::new();
        let pid_file = fixture.root.join("owned-descendant.pid");
        platform::fixture_environment(vec![
            ("JC_GIT_PROCESS_FIXTURE".into(), "1".into()),
            ("JC_GIT_FIXTURE_PID".into(), pid_file.as_os_str().to_owned()),
        ]);
        let mut descendant: Option<OwnedHandle> = None;
        let mut control = Control {
            deadline: Instant::now() + Duration::from_secs(5),
            cancelled: || {
                let Some(pid) = fs::read_to_string(&pid_file)
                    .ok()
                    .and_then(|v| v.parse::<u32>().ok())
                else {
                    return false;
                };
                let raw = unsafe { OpenProcess(0x0010_0000, 0, pid) };
                if raw.is_null() {
                    return false;
                }
                descendant = Some(unsafe { OwnedHandle::from_raw_handle(raw) });
                true
            },
        };
        let args = [
            "--exact".into(),
            "git_read::tests::fixture_descendant_child".into(),
            "--ignored".into(),
            "--nocapture".into(),
        ];
        let result = platform::capture(
            &std::env::current_exe().unwrap(),
            &fixture.root,
            &args,
            4096,
            GitOutputLimit::Metadata,
            4096,
            &mut control,
        );
        platform::fixture_environment(Vec::new());
        assert!(matches!(result, Err(GitReadError::Cancelled)));
        let descendant = descendant.expect("Owned descendant observed before cancellation");
        assert_eq!(
            unsafe { WaitForSingleObject(descendant.as_raw_handle(), 5000) },
            0
        );
    }
    #[test]
    #[ignore = "Owned subprocess fixture, invoked explicitly by the descendant cleanup test"]
    fn fixture_descendant_child() {
        assert_eq!(std::env::var("JC_GIT_PROCESS_FIXTURE").unwrap(), "1");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "git_read::tests::fixture_sleep_child",
                "--ignored",
                "--nocapture",
            ])
            .env("JC_GIT_PROCESS_FIXTURE", "1")
            .spawn()
            .unwrap();
        fs::write(
            std::env::var_os("JC_GIT_FIXTURE_PID").unwrap(),
            child.id().to_string(),
        )
        .unwrap();
        let _ = child.wait();
    }
}
fn metadata_lines(bytes: &[u8]) -> Result<Vec<&str>, GitReadError> {
    let text = std::str::from_utf8(bytes).map_err(|_| GitReadError::UnsupportedPathEncoding)?;
    let text = text.strip_suffix('\n').ok_or(GitReadError::Protocol)?;
    Ok(text
        .split('\n')
        .map(|s| s.strip_suffix('\r').unwrap_or(s))
        .collect())
}
fn trim_line(bytes: &[u8]) -> Result<&[u8], GitReadError> {
    let line = bytes.strip_suffix(b"\n").ok_or(GitReadError::Protocol)?;
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.contains(&b'\n') || line.contains(&b'\r') {
        return Err(GitReadError::Protocol);
    }
    Ok(line)
}
fn parse_entry(
    bytes: &[u8],
    path: &[u8],
    source: GitBlobSource,
) -> Result<(GitObjectId, GitFileMode), GitReadError> {
    if bytes.is_empty() {
        return Err(GitReadError::Missing);
    }
    if bytes.last() != Some(&0) {
        return Err(GitReadError::Protocol);
    }
    let mut found = None;
    for entry in bytes[..bytes.len() - 1].split(|&b| b == 0) {
        let split = entry
            .iter()
            .position(|&b| b == b'\t')
            .ok_or(GitReadError::Protocol)?;
        if &entry[split + 1..] != path {
            return Err(GitReadError::Protocol);
        }
        let fields: Vec<_> = entry[..split].split(|&b| b == b' ').collect();
        if fields.len() != 3 {
            return Err(GitReadError::Protocol);
        }
        if source == GitBlobSource::Index && fields[2] != b"0" {
            return Err(GitReadError::Unmerged);
        }
        if source == GitBlobSource::Head && fields[1] != b"blob" {
            return Err(GitReadError::UnsupportedMode);
        }
        let mode = match fields[0] {
            b"100644" => GitFileMode::Regular,
            b"100755" => GitFileMode::Executable,
            _ => return Err(GitReadError::UnsupportedMode),
        };
        let oid = GitObjectId::parse(fields[if source == GitBlobSource::Head { 2 } else { 1 }])?;
        if found.replace((oid, mode)).is_some() {
            return Err(GitReadError::Protocol);
        }
    }
    found.ok_or(GitReadError::Missing)
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    pub(super) fn capture(
        _executable: &Path,
        _root: &Path,
        _args: &[OsString],
        _stdout_cap: usize,
        _limit: GitOutputLimit,
        _stderr_cap: usize,
        _control: &mut Control<impl FnMut() -> bool>,
    ) -> Result<Vec<u8>, GitReadError> {
        Err(GitReadError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        collections::BTreeMap,
        ffi::c_void,
        fs::File,
        io::Read,
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        ptr,
        sync::{
            atomic::{AtomicBool, AtomicU8, Ordering},
            Arc,
        },
        thread,
    };
    type Handle = *mut c_void;
    #[cfg(test)]
    thread_local! {static FIXTURE_ENV:std::cell::RefCell<Vec<(OsString,OsString)>>=const {std::cell::RefCell::new(Vec::new())};}
    #[cfg(test)]
    thread_local! {
        static READER_FAILURE:std::cell::Cell<Option<usize>>=const {std::cell::Cell::new(None)};
        static OBSERVE_PROCESS:std::cell::Cell<bool>=const {std::cell::Cell::new(false)};
        static OBSERVED_PROCESS:std::cell::RefCell<Option<OwnedHandle>>=const {std::cell::RefCell::new(None)};
    }
    #[cfg(test)]
    pub(super) fn fixture_environment(values: Vec<(OsString, OsString)>) {
        FIXTURE_ENV.with(|env| *env.borrow_mut() = values);
    }
    #[cfg(test)]
    pub(super) fn fixture_reader_failure(index: Option<usize>) {
        READER_FAILURE.with(|failure| failure.set(index));
    }
    #[cfg(test)]
    pub(super) fn fixture_observe_process(enabled: bool) {
        OBSERVE_PROCESS.with(|observe| observe.set(enabled));
        if enabled {
            OBSERVED_PROCESS.with(|process| *process.borrow_mut() = None);
        }
    }
    #[cfg(test)]
    pub(super) fn fixture_take_process() -> Option<OwnedHandle> {
        OBSERVED_PROCESS.with(|process| process.borrow_mut().take())
    }
    #[repr(C)]
    struct Security {
        size: u32,
        descriptor: Handle,
        inherit: i32,
    }
    #[repr(C)]
    struct Startup {
        size: u32,
        reserved: *mut u16,
        desktop: *mut u16,
        title: *mut u16,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        chars_x: u32,
        chars_y: u32,
        fill: u32,
        flags: u32,
        show: u16,
        reserved_bytes: u16,
        reserved_ptr: *mut u8,
        input: Handle,
        output: Handle,
        error: Handle,
    }
    #[repr(C)]
    struct StartupEx {
        startup: Startup,
        attributes: Handle,
    }
    #[repr(C)]
    struct ProcessInfo {
        process: Handle,
        thread: Handle,
        pid: u32,
        tid: u32,
    }
    #[repr(C)]
    struct BasicLimits {
        process_time: i64,
        job_time: i64,
        flags: u32,
        min_working: usize,
        max_working: usize,
        active: u32,
        affinity: usize,
        priority: u32,
        scheduling: u32,
    }
    #[repr(C)]
    struct ExtendedLimits {
        basic: BasicLimits,
        io: [u64; 6],
        process_memory: usize,
        job_memory: usize,
        peak_process: usize,
        peak_job: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreatePipe(
            read: *mut Handle,
            write: *mut Handle,
            security: *mut Security,
            size: u32,
        ) -> i32;
        fn SetHandleInformation(handle: Handle, mask: u32, flags: u32) -> i32;
        fn CreateJobObjectW(security: Handle, name: *const u16) -> Handle;
        fn SetInformationJobObject(job: Handle, class: u32, info: Handle, size: u32) -> i32;
        fn InitializeProcThreadAttributeList(
            list: Handle,
            count: u32,
            flags: u32,
            bytes: *mut usize,
        ) -> i32;
        fn UpdateProcThreadAttribute(
            list: Handle,
            flags: u32,
            attribute: usize,
            value: Handle,
            size: usize,
            previous: Handle,
            returned: *mut usize,
        ) -> i32;
        fn DeleteProcThreadAttributeList(list: Handle);
        fn CreateProcessW(
            application: *const u16,
            command: *mut u16,
            process_security: Handle,
            thread_security: Handle,
            inherit: i32,
            flags: u32,
            environment: Handle,
            directory: *const u16,
            startup: *mut StartupEx,
            info: *mut ProcessInfo,
        ) -> i32;
        fn WaitForSingleObject(handle: Handle, ms: u32) -> u32;
        fn GetExitCodeProcess(process: Handle, code: *mut u32) -> i32;
        fn CancelSynchronousIo(thread: Handle) -> i32;
        #[cfg(test)]
        fn GetCurrentProcess() -> Handle;
        #[cfg(test)]
        fn DuplicateHandle(
            source_process: Handle,
            source: Handle,
            target_process: Handle,
            target: *mut Handle,
            access: u32,
            inherit: i32,
            options: u32,
        ) -> i32;
    }
    fn check(value: i32) -> io::Result<()> {
        if value == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    unsafe fn owned(handle: Handle) -> io::Result<OwnedHandle> {
        if handle.is_null() || handle == usize::MAX as Handle {
            Err(io::Error::last_os_error())
        } else {
            Ok(OwnedHandle::from_raw_handle(handle))
        }
    }
    fn wide(value: &std::ffi::OsStr) -> Result<Vec<u16>, GitReadError> {
        let mut result: Vec<_> = value.encode_wide().collect();
        if result.contains(&0) {
            return Err(GitReadError::InvalidPath);
        }
        result.push(0);
        Ok(result)
    }
    // Windows CRT quoting, applied once at the process API boundary. No shell.
    fn quote(value: &std::ffi::OsStr) -> Result<Vec<u16>, GitReadError> {
        let value = wide(value)?;
        let mut result = vec![b'"' as u16];
        let mut slashes = 0;
        for &ch in &value[..value.len() - 1] {
            if ch == b'\\' as u16 {
                slashes += 1;
                continue;
            }
            if ch == b'"' as u16 {
                result.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
            } else {
                result.extend(std::iter::repeat_n(b'\\' as u16, slashes));
            }
            result.push(ch);
            slashes = 0;
        }
        result.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
        result.push(b'"' as u16);
        Ok(result)
    }
    fn environment() -> Vec<u16> {
        let mut values: BTreeMap<OsString, OsString> = BTreeMap::new();
        for key in ["SYSTEMROOT", "WINDIR", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(key) {
                values.insert(key.into(), value);
            }
        }
        for (key, value) in [
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_SYSTEM", "NUL"),
            ("GIT_CONFIG_GLOBAL", "NUL"),
            ("GIT_CONFIG_COUNT", "0"),
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_OPTIONAL_LOCKS", "0"),
            ("GIT_NO_REPLACE_OBJECTS", "1"),
            ("GIT_NO_LAZY_FETCH", "1"),
            ("GIT_LITERAL_PATHSPECS", "1"),
        ] {
            values.insert(key.into(), value.into());
        }
        #[cfg(test)]
        FIXTURE_ENV.with(|env| {
            for (key, value) in env.borrow().iter() {
                values.insert(key.clone(), value.clone());
            }
        });
        let mut result = Vec::new();
        for (key, value) in values {
            result.extend(key.encode_wide());
            result.push(b'=' as u16);
            result.extend(value.encode_wide());
            result.push(0);
        }
        result.push(0);
        result
    }
    fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
        let mut read = ptr::null_mut();
        let mut write = ptr::null_mut();
        let mut security = Security {
            size: std::mem::size_of::<Security>() as u32,
            descriptor: ptr::null_mut(),
            inherit: 1,
        };
        check(unsafe { CreatePipe(&mut read, &mut write, &mut security, 0) })?;
        Ok((unsafe { owned(read)? }, unsafe { owned(write)? }))
    }
    struct Attributes {
        storage: Vec<usize>,
        initialized: bool,
    }
    impl Attributes {
        fn new() -> io::Result<Self> {
            let mut size = 0;
            unsafe {
                InitializeProcThreadAttributeList(ptr::null_mut(), 2, 0, &mut size);
            }
            if size == 0 || size > 64 * 1024 {
                return Err(io::Error::other("Invalid process attribute size"));
            }
            let mut result = Self {
                storage: vec![0; size.div_ceil(std::mem::size_of::<usize>())],
                initialized: false,
            };
            check(unsafe { InitializeProcThreadAttributeList(result.ptr(), 2, 0, &mut size) })?;
            result.initialized = true;
            Ok(result)
        }
        fn ptr(&mut self) -> Handle {
            self.storage.as_mut_ptr().cast()
        }
        fn update(&mut self, attribute: usize, value: Handle, size: usize) -> io::Result<()> {
            check(unsafe {
                UpdateProcThreadAttribute(
                    self.ptr(),
                    0,
                    attribute,
                    value,
                    size,
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            })
        }
    }
    impl Drop for Attributes {
        fn drop(&mut self) {
            if self.initialized {
                unsafe { DeleteProcThreadAttributeList(self.ptr()) };
            }
        }
    }
    struct Child {
        job: OwnedHandle,
        process: OwnedHandle,
        stdout: File,
        stderr: File,
    }
    fn spawn(executable: &Path, root: &Path, args: &[OsString]) -> Result<Child, GitReadError> {
        let job = unsafe { owned(CreateJobObjectW(ptr::null_mut(), ptr::null())) }
            .map_err(GitReadError::Containment)?;
        let mut limits: ExtendedLimits = unsafe { std::mem::zeroed() };
        limits.basic.flags = 0x2000;
        check(unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                9,
                (&mut limits as *mut ExtendedLimits).cast(),
                std::mem::size_of::<ExtendedLimits>() as u32,
            )
        })
        .map_err(GitReadError::Containment)?;
        let (input, input_write) = pipe()?;
        let (stdout, output) = pipe()?;
        let (stderr, error) = pipe()?;
        check(unsafe { SetHandleInformation(stdout.as_raw_handle(), 1, 0) })?;
        check(unsafe { SetHandleInformation(stderr.as_raw_handle(), 1, 0) })?;
        drop(input_write); // Child stdin is EOF; no prompts or batch input.
        let mut attributes = Attributes::new().map_err(GitReadError::Containment)?;
        let mut handles = [
            input.as_raw_handle(),
            output.as_raw_handle(),
            error.as_raw_handle(),
        ];
        let mut jobs = [job.as_raw_handle()];
        attributes
            .update(
                0x0002_0002,
                handles.as_mut_ptr().cast(),
                std::mem::size_of_val(&handles),
            )
            .map_err(GitReadError::Containment)?;
        attributes
            .update(
                0x0002_000d,
                jobs.as_mut_ptr().cast(),
                std::mem::size_of_val(&jobs),
            )
            .map_err(GitReadError::Containment)?;
        let mut startup: StartupEx = unsafe { std::mem::zeroed() };
        startup.startup.size = std::mem::size_of::<StartupEx>() as u32;
        startup.startup.flags = 0x100;
        startup.startup.input = input.as_raw_handle();
        startup.startup.output = output.as_raw_handle();
        startup.startup.error = error.as_raw_handle();
        startup.attributes = attributes.ptr();
        let executable_wide = wide(executable.as_os_str())?;
        let root = wide(root.as_os_str())?;
        let mut command = quote(&git_argument_path(executable))?;
        for arg in args {
            command.push(b' ' as u16);
            command.extend(quote(arg)?);
        }
        if command.len() >= 32767 {
            return Err(GitReadError::InvalidPath);
        }
        command.push(0);
        let mut environment = environment();
        let mut info: ProcessInfo = unsafe { std::mem::zeroed() };
        check(unsafe {
            CreateProcessW(
                executable_wide.as_ptr(),
                command.as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                1,
                0x0008_0000 | 0x0800_0000 | 0x400,
                environment.as_mut_ptr().cast(),
                root.as_ptr(),
                &mut startup,
                &mut info,
            )
        })
        .map_err(GitReadError::Containment)?;
        let process = unsafe { owned(info.process)? };
        let thread = unsafe { owned(info.thread)? };
        drop(thread);
        drop(attributes);
        drop(input);
        drop(output);
        drop(error);
        Ok(Child {
            job,
            process,
            stdout: File::from(stdout),
            stderr: File::from(stderr),
        })
    }
    fn read_bounded(
        mut pipe: File,
        cap: usize,
        which: u8,
        limit: GitOutputLimit,
        failed: Arc<AtomicU8>,
        stop: Arc<AtomicBool>,
    ) -> Result<Vec<u8>, GitReadError> {
        let mut result = Vec::with_capacity(cap);
        let mut block = [0u8; 4096];
        loop {
            if stop.load(Ordering::Acquire) {
                return Err(GitReadError::Cancelled);
            }
            let size = match pipe.read(&mut block) {
                Ok(size) => size,
                Err(error) => {
                    if stop.load(Ordering::Acquire) {
                        return Err(GitReadError::Cancelled);
                    }
                    let _ = failed.compare_exchange(0, 3, Ordering::Release, Ordering::Relaxed);
                    return Err(error.into());
                }
            };
            if size == 0 {
                return Ok(result);
            }
            if size > cap.saturating_sub(result.len()) {
                let _ = failed.compare_exchange(0, which, Ordering::Release, Ordering::Relaxed);
                return Err(GitReadError::OutputLimit(limit));
            }
            result.extend_from_slice(&block[..size]);
        }
    }
    type ReaderResult = Result<Vec<u8>, GitReadError>;
    type Reader = thread::JoinHandle<ReaderResult>;
    /// Owns every resource from CreateProcess onward, including unwinding from a
    /// caller callback. Cleanup never detaches a pending reader to meet a timer.
    struct CaptureGuard {
        job: Option<OwnedHandle>,
        process: OwnedHandle,
        readers: [Option<Reader>; 2],
        results: [Option<ReaderResult>; 2],
        failed: Arc<AtomicU8>,
        stop: Arc<AtomicBool>,
        cleaned: bool,
    }
    impl CaptureGuard {
        fn new(job: OwnedHandle, process: OwnedHandle) -> Self {
            Self {
                job: Some(job),
                process,
                readers: [None, None],
                results: [None, None],
                failed: Arc::new(AtomicU8::new(0)),
                stop: Arc::new(AtomicBool::new(false)),
                cleaned: false,
            }
        }
        #[cfg(test)]
        fn observe(&self) -> Result<(), GitReadError> {
            if OBSERVE_PROCESS.with(std::cell::Cell::get) {
                let mut target = ptr::null_mut();
                let current = unsafe { GetCurrentProcess() };
                check(unsafe {
                    DuplicateHandle(
                        current,
                        self.process.as_raw_handle(),
                        current,
                        &mut target,
                        0x0010_0000,
                        0,
                        0,
                    )
                })?;
                let target = unsafe { owned(target)? };
                OBSERVED_PROCESS.with(|observed| *observed.borrow_mut() = Some(target));
            }
            Ok(())
        }
        fn reader(
            &mut self,
            index: usize,
            name: &str,
            pipe: File,
            cap: usize,
            limit: GitOutputLimit,
        ) -> io::Result<()> {
            #[cfg(test)]
            if READER_FAILURE.with(|failure| failure.get() == Some(index)) {
                return Err(io::Error::other("Injected reader startup failure"));
            }
            let failed = self.failed.clone();
            let stop = self.stop.clone();
            self.readers[index] =
                Some(thread::Builder::new().name(name.into()).spawn(move || {
                    read_bounded(pipe, cap, index as u8 + 1, limit, failed, stop)
                })?);
            Ok(())
        }
        fn finish(&mut self, abort: bool) -> Result<(), GitReadError> {
            if self.cleaned {
                return Ok(());
            }
            // This is deliberately first: kill members/descendants before any
            // attempt to wait for or cancel the readers holding their pipes.
            drop(self.job.take());
            let drain_deadline = Instant::now() + Duration::from_secs(5);
            let mut cancel = abort;
            let mut issue = None;
            loop {
                let mut all_done = true;
                for reader in self.readers.iter().flatten() {
                    let state = unsafe { WaitForSingleObject(reader.as_raw_handle(), 0) };
                    if state == 0 {
                        continue;
                    }
                    if state != 258 {
                        issue.get_or_insert_with(|| {
                            GitReadError::Containment(io::Error::last_os_error())
                        });
                        // Keep ownership and wait for the Rust thread even if
                        // the native observation failed; never detach it.
                        if reader.is_finished() {
                            continue;
                        }
                    }
                    all_done = false;
                }
                if all_done {
                    break;
                }
                if !cancel && Instant::now() >= drain_deadline {
                    cancel = true;
                    issue.get_or_insert_with(|| {
                        GitReadError::Containment(io::Error::other(
                            "Git output did not drain during cleanup",
                        ))
                    });
                }
                if cancel {
                    self.stop.store(true, Ordering::Release);
                    for reader in self.readers.iter().flatten() {
                        if unsafe { WaitForSingleObject(reader.as_raw_handle(), 0) } == 0 {
                            continue;
                        }
                        if unsafe { CancelSynchronousIo(reader.as_raw_handle()) } == 0 {
                            let error = io::Error::last_os_error();
                            // The reader can be between reads. The stop flag
                            // prevents its next read, and another loop retries
                            // cancellation if it raced the preceding check.
                            if error.raw_os_error() != Some(1168) {
                                issue.get_or_insert(GitReadError::Containment(error));
                            }
                        }
                    }
                }
                for reader in self.readers.iter().flatten() {
                    if unsafe { WaitForSingleObject(reader.as_raw_handle(), 0) } != 0 {
                        unsafe {
                            WaitForSingleObject(reader.as_raw_handle(), 5);
                        }
                    }
                }
            }
            for (index, reader) in self.readers.iter_mut().enumerate() {
                if let Some(reader) = reader.take() {
                    self.results[index] = Some(reader.join().unwrap_or_else(|_| {
                        Err(GitReadError::Io(io::Error::other(
                            "Git output reader panicked",
                        )))
                    }));
                }
            }
            // A deadline requests cancellation; it cannot turn kernel teardown
            // into a hard real-time guarantee. Retain the process handle until
            // termination is observed instead of dropping it after a timer.
            if unsafe { WaitForSingleObject(self.process.as_raw_handle(), u32::MAX) } != 0 {
                issue.get_or_insert_with(|| GitReadError::Containment(io::Error::last_os_error()));
            }
            self.cleaned = true;
            issue.map_or(Ok(()), Err)
        }
    }
    impl Drop for CaptureGuard {
        fn drop(&mut self) {
            if !self.cleaned {
                let _ = self.finish(true);
            }
        }
    }
    pub(super) fn capture(
        executable: &Path,
        root: &Path,
        args: &[OsString],
        stdout_cap: usize,
        stdout_limit: GitOutputLimit,
        stderr_cap: usize,
        control: &mut Control<impl FnMut() -> bool>,
    ) -> Result<Vec<u8>, GitReadError> {
        control.check()?;
        let Child {
            job,
            process,
            stdout,
            stderr,
        } = spawn(executable, root, args)?;
        let mut running = CaptureGuard::new(job, process);
        #[cfg(test)]
        running.observe()?;
        if let Err(error) = running.reader(0, "jc-git-stdout", stdout, stdout_cap, stdout_limit) {
            running.finish(true)?;
            return Err(error.into());
        }
        if let Err(error) = running.reader(
            1,
            "jc-git-stderr",
            stderr,
            stderr_cap,
            GitOutputLimit::Stderr,
        ) {
            running.finish(true)?;
            return Err(error.into());
        }
        let mut code = 0;
        let failure = loop {
            if let Err(error) = control.check() {
                break Some(error);
            }
            match running.failed.load(Ordering::Acquire) {
                1 => break Some(GitReadError::OutputLimit(stdout_limit)),
                2 => break Some(GitReadError::OutputLimit(GitOutputLimit::Stderr)),
                3 => break Some(GitReadError::Io(io::Error::other("Git output pipe failed"))),
                _ => {}
            }
            match unsafe { WaitForSingleObject(running.process.as_raw_handle(), 5) } {
                0 => {
                    if let Err(error) = check(unsafe {
                        GetExitCodeProcess(running.process.as_raw_handle(), &mut code)
                    }) {
                        break Some(error.into());
                    }
                    break None;
                }
                258 => {}
                _ => break Some(GitReadError::Io(io::Error::last_os_error())),
            }
        };
        running.finish(failure.is_some())?;
        if let Some(error) = failure {
            return Err(error);
        }
        let output = running.results[0].take().ok_or(GitReadError::Protocol)??;
        let errors = running.results[1].take().ok_or(GitReadError::Protocol)??;
        if code != 0 {
            return Err(GitReadError::CommandFailed {
                code,
                stderr: errors,
            });
        }
        Ok(output)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::windows::ffi::OsStringExt;
        #[link(name = "shell32")]
        extern "system" {
            fn CommandLineToArgvW(command: *const u16, count: *mut i32) -> *mut *mut u16;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn LocalFree(memory: Handle) -> Handle;
        }
        #[test]
        fn structured_windows_arguments_roundtrip_without_shell_expansion() {
            let args: Vec<OsString> = [
                "C:\\Program Files\\Git\\git.exe",
                "",
                "dir with space/niño[1].p",
                "a\"b",
                r"C:\tail with space\",
                r"literal;$(&value)",
                "--",
            ]
            .into_iter()
            .map(Into::into)
            .collect();
            let mut command = Vec::new();
            for arg in &args {
                if !command.is_empty() {
                    command.push(b' ' as u16);
                }
                command.extend(quote(arg).unwrap());
            }
            command.push(0);
            let mut count = 0;
            let parsed = unsafe { CommandLineToArgvW(command.as_ptr(), &mut count) };
            assert!(!parsed.is_null());
            let mut actual = Vec::new();
            for i in 0..count as usize {
                let arg = unsafe { *parsed.add(i) };
                let mut len = 0;
                while unsafe { *arg.add(len) } != 0 {
                    len += 1;
                }
                actual.push(OsString::from_wide(unsafe {
                    std::slice::from_raw_parts(arg, len)
                }));
            }
            unsafe {
                LocalFree(parsed.cast());
            }
            assert_eq!(actual, args);
        }
    }
}
