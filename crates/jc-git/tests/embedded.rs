//! Real repository fixtures. Git CLI is only an oracle/preparation tool here;
//! calls into jc_git receive neither a program path nor a process runner.
use jc_git::{Change, Error, Kind, Limits, read_blob, read_status};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn limits() -> Limits {
    Limits {
        max_blob: 2 * 1024 * 1024,
        max_entries: 4096,
        max_path: 4096,
        max_bytes: 4 * 1024 * 1024,
        timeout: Duration::from_secs(10),
    }
}

fn oracle() -> PathBuf {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("JC_GIT_TEST_EXE") {
        candidates.push(PathBuf::from(path));
    }
    for key in ["ProgramW6432", "ProgramFiles"] {
        if let Some(base) = std::env::var_os(key) {
            candidates.push(PathBuf::from(base).join("Git/cmd/git.exe"));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(
            std::env::split_paths(&path)
                .filter(|p| p.is_absolute())
                .map(|p| p.join(if cfg!(windows) { "git.exe" } else { "git" })),
        );
    }
    candidates.into_iter().find(|p| p.is_file())
        .expect("Repository fixtures need a development Git oracle (JC_GIT_TEST_EXE); the embedded engine does not")
        .canonicalize().unwrap()
}

struct Fixture {
    root: PathBuf,
    git: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("work/test-temp");
        fs::create_dir_all(&base).unwrap();
        let root = base.join(format!(
            "embedded-git-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let fixture = Self {
            root: root.canonicalize().unwrap(),
            git: oracle(),
        };
        fixture.git(&[
            "init",
            "--quiet",
            "--template=",
            "--initial-branch=main",
            ".",
        ]);
        fixture.git(&["config", "user.name", "CIDE owned test fixture"]);
        fixture.git(&["config", "user.email", "fixture@example.invalid"]);
        fixture.git(&["config", "core.autocrlf", "false"]);
        fixture.git(&["config", "core.fsmonitor", "false"]);
        fixture.git(&["config", "commit.gpgsign", "false"]);
        fixture
    }
    fn command_at(&self, directory: &Path) -> Command {
        let mut command = Command::new(&self.git);
        command.current_dir(directory).env_clear();
        for key in ["SYSTEMROOT", "WINDIR"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .env("TEMP", &self.root)
            .env("TMP", &self.root)
            .env("PATH", self.git.parent().unwrap())
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env(
                "GIT_CONFIG_SYSTEM",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0");
        command.arg("--no-pager").arg("-c").arg("core.hooksPath=");
        command
    }
    fn output(&self, args: &[&str]) -> Output {
        self.command_at(&self.root).args(args).output().unwrap()
    }
    fn git(&self, args: &[&str]) -> Vec<u8> {
        let result = self.output(args);
        assert!(
            result.status.success(),
            "Git fixture preparation {args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        result.stdout
    }
    fn write(&self, name: &str, data: &[u8]) {
        let path = self.root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    }
    fn commit_all(&self) {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "--no-verify", "-m", "owned fixture"]);
    }
    fn expected_status(&self, root: &Path) -> BTreeMap<Vec<u8>, Observed> {
        let result = self
            .command_at(root)
            .args([
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--ignored=no",
                "--no-renames",
                "--ignore-submodules=all",
            ])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "Git status oracle: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        result
            .stdout
            .split(|b| *b == 0)
            .filter(|record| !record.is_empty())
            .map(|record| {
                assert!(record.len() >= 4 && record[2] == b' ');
                let pair = &record[..2];
                let kind = match pair {
                    b"??" => Kind::Untracked,
                    b"DD" => Kind::Conflict(1),
                    b"AU" => Kind::Conflict(2),
                    b"UD" => Kind::Conflict(3),
                    b"UA" => Kind::Conflict(4),
                    b"DU" => Kind::Conflict(5),
                    b"AA" => Kind::Conflict(6),
                    b"UU" => Kind::Conflict(7),
                    _ => Kind::Tracked,
                };
                let (index, worktree) = if matches!(kind, Kind::Tracked) {
                    (change(pair[0]), change(pair[1]))
                } else {
                    (Change::Unchanged, Change::Unchanged)
                };
                (
                    record[3..].to_vec(),
                    Observed {
                        index,
                        worktree,
                        kind,
                    },
                )
            })
            .collect()
    }
    fn assert_status(&self, root: &Path) -> BTreeMap<Vec<u8>, Observed> {
        let expected = self.expected_status(root);
        let before = image(&self.root);
        let status = read_status(root, limits(), || false).unwrap();
        assert_eq!(status.repository.root, root.canonicalize().unwrap());
        let actual = status
            .entries
            .into_iter()
            .map(|entry| {
                assert!(!entry.submodule);
                (
                    entry.path,
                    Observed {
                        index: entry.index,
                        worktree: entry.worktree,
                        kind: entry.kind,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(actual, expected);
        assert_eq!(
            image(&self.root),
            before,
            "An embedded read changed source or repository metadata"
        );
        actual
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Observed {
    index: Change,
    worktree: Change,
    kind: Kind,
}
fn change(value: u8) -> Change {
    match value {
        b' ' => Change::Unchanged,
        b'M' => Change::Modified,
        b'A' => Change::Added,
        b'D' => Change::Deleted,
        b'T' => Change::TypeChanged,
        other => panic!("Unsupported test-oracle status byte {other}"),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct FileImage {
    bytes: Vec<u8>,
    modified: Option<SystemTime>,
    readonly: bool,
}
fn image(root: &Path) -> BTreeMap<PathBuf, FileImage> {
    fn visit(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, FileImage>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let meta = fs::symlink_metadata(&path).unwrap();
            assert!(
                !meta.file_type().is_symlink(),
                "Owned test fixture unexpectedly contains a symlink"
            );
            if meta.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    FileImage {
                        bytes: fs::read(&path).unwrap(),
                        modified: meta.modified().ok(),
                        readonly: meta.permissions().readonly(),
                    },
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn blobs_keep_bom_legacy_bytes_eol_and_index_identity_with_loose_and_packed_objects() {
    let fixture = Fixture::new();
    let name = "src/niño[1].p";
    let head = b"\xef\xbb\xbfhead \xe9\r\nlast\r\n";
    let staged = b"\xef\xbb\xbfindex \xf1\r\nlast";
    fixture.write(name, head);
    fixture.commit_all();
    fixture.write(name, staged);
    fixture.git(&["add", "--", name]);
    fixture.write(name, b"unsaved elsewhere\0\n");
    let head_oid = String::from_utf8(fixture.git(&["rev-parse", &format!("HEAD:{name}")]))
        .unwrap()
        .trim()
        .to_owned();
    let index_oid = String::from_utf8(fixture.git(&["rev-parse", &format!(":{name}")]))
        .unwrap()
        .trim()
        .to_owned();
    for packed in [false, true] {
        if packed {
            fixture.git(&["gc", "--prune=now"]);
            assert!(
                fs::read_dir(fixture.root.join(".git/objects/pack"))
                    .unwrap()
                    .any(|entry| entry
                        .unwrap()
                        .path()
                        .extension()
                        .is_some_and(|e| e == "pack"))
            );
        }
        let before = image(&fixture.root);
        let original = read_blob(&fixture.root, false, name, limits(), || false).unwrap();
        let index = read_blob(&fixture.root, true, name, limits(), || false).unwrap();
        assert_eq!(original.bytes, head);
        assert_eq!(index.bytes, staged);
        assert_eq!(original.oid, head_oid);
        assert_eq!(index.oid, index_oid);
        assert!(original.commit.is_some());
        assert!(index.commit.is_none());
        assert_eq!(original.mode, 0o100644);
        assert_eq!(original.repository.root, fixture.root);
        assert_eq!(image(&fixture.root), before);
    }
}

#[test]
fn status_matches_oracle_for_staged_worktree_deleted_untracked_and_ignored_files() {
    let fixture = Fixture::new();
    fixture.write("staged.p", b"initial staged\n");
    fixture.write("worktree.p", b"initial worktree\n");
    fixture.write("deleted.p", b"delete me\n");
    fixture.write(".gitignore", b"ignored/\n*.cache\n");
    fixture.commit_all();
    fixture.write("staged.p", b"changed and staged\n");
    fixture.git(&["add", "--", "staged.p"]);
    fixture.write("staged.p", b"changed again after staging\n");
    fixture.write("worktree.p", b"changed only in worktree\n");
    fs::remove_file(fixture.root.join("deleted.p")).unwrap();
    fixture.write("new.txt", b"untracked\n");
    fixture.write("ignored/secret.p", b"ignored source\n");
    fixture.write("temp.cache", b"ignored cache\n");
    let actual = fixture.assert_status(&fixture.root);
    assert_eq!(actual[b"staged.p".as_slice()].index, Change::Modified);
    assert_eq!(actual[b"staged.p".as_slice()].worktree, Change::Modified);
    assert_eq!(actual[b"worktree.p".as_slice()].index, Change::Unchanged);
    assert_eq!(actual[b"new.txt".as_slice()].kind, Kind::Untracked);
    assert!(
        !actual
            .keys()
            .any(|path| path.starts_with(b"ignored/") || path.ends_with(b".cache"))
    );
}

#[test]
fn status_honors_repository_autocrlf_without_false_worktree_modifications() {
    let fixture = Fixture::new();
    fixture.git(&["config", "core.autocrlf", "true"]);
    fixture.write("windows.p", b"first\r\nsecond\r\n");
    fixture.commit_all();
    assert!(fixture.assert_status(&fixture.root).is_empty());
    fixture.write("windows.p", b"first\r\nsecond changed\r\n");
    let actual = fixture.assert_status(&fixture.root);
    assert_eq!(actual[b"windows.p".as_slice()].worktree, Change::Modified);
}

#[test]
fn unborn_head_and_linked_worktree_read_without_a_git_program_in_the_api() {
    let fixture = Fixture::new();
    fixture.write("first.p", b"unborn\n");
    fixture.git(&["add", "--", "first.p"]);
    let unborn = fixture.assert_status(&fixture.root);
    assert_eq!(unborn[b"first.p".as_slice()].index, Change::Added);
    assert!(matches!(
        read_blob(&fixture.root, false, "first.p", limits(), || false),
        Err(Error::Missing)
    ));
    assert_eq!(
        read_blob(&fixture.root, true, "first.p", limits(), || false)
            .unwrap()
            .bytes,
        b"unborn\n"
    );
    fixture.commit_all();
    let linked = fixture.root.join("linked-worktree");
    // The development Git oracle rejects Rust's verbatim Win32 prefix, even
    // though the embedded engine keeps canonical native paths for identity.
    let linked_argument = linked.to_str().unwrap();
    let linked_argument = linked_argument
        .strip_prefix(r"\\?\")
        .unwrap_or(linked_argument);
    fixture.git(&[
        "worktree",
        "add",
        "--quiet",
        "-b",
        "linked",
        linked_argument,
    ]);
    fs::write(linked.join("first.p"), b"linked worktree only\n").unwrap();
    fixture.assert_status(&linked);
    let before = image(&fixture.root);
    let blob = read_blob(&linked, false, "first.p", limits(), || false).unwrap();
    assert_eq!(blob.bytes, b"unborn\n");
    assert_eq!(blob.repository.root, linked.canonicalize().unwrap());
    assert_ne!(blob.repository.git_dir, linked.join(".git"));
    assert_eq!(image(&fixture.root), before);
}

#[test]
fn conflicts_are_typed_and_index_blob_is_rejected_without_modifying_the_index() {
    let fixture = Fixture::new();
    fixture.write("conflict.p", b"base\n");
    fixture.commit_all();
    fixture.git(&["checkout", "--quiet", "-b", "other"]);
    fixture.write("conflict.p", b"other branch\n");
    fixture.commit_all();
    fixture.git(&["checkout", "--quiet", "main"]);
    fixture.write("conflict.p", b"main branch\n");
    fixture.commit_all();
    let merge = fixture.output(&["merge", "--no-edit", "other"]);
    assert!(
        !merge.status.success(),
        "Fixture must contain a real unresolved merge"
    );
    let actual = fixture.assert_status(&fixture.root);
    assert_eq!(actual[b"conflict.p".as_slice()].kind, Kind::Conflict(7));
    let before = image(&fixture.root);
    assert!(matches!(
        read_blob(&fixture.root, true, "conflict.p", limits(), || false),
        Err(Error::Unmerged)
    ));
    assert_eq!(image(&fixture.root), before);
}

#[test]
fn limits_cancellation_and_configured_external_filters_fail_without_writes() {
    let fixture = Fixture::new();
    fixture.write("long-filename.p", b"original object payload\r\n");
    fixture.commit_all();
    fixture.write("long-filename.p", b"changed worktree payload\r\n");
    let before = image(&fixture.root);
    let mut cap = limits();
    cap.max_blob = 2;
    assert!(matches!(
        read_blob(&fixture.root, false, "long-filename.p", cap, || false),
        Err(Error::Limited)
    ));
    for cap in [
        Limits {
            max_entries: 0,
            ..limits()
        },
        Limits {
            max_path: 2,
            ..limits()
        },
        Limits {
            max_bytes: 1,
            ..limits()
        },
    ] {
        assert!(matches!(
            read_status(&fixture.root, cap, || false),
            Err(Error::Limited)
        ));
    }
    assert!(matches!(
        read_status(&fixture.root, limits(), || true),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        read_blob(&fixture.root, false, "long-filename.p", limits(), || true),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        read_blob(
            Path::new("relative-root"),
            false,
            "long-filename.p",
            limits(),
            || false
        ),
        Err(Error::InvalidRoot)
    ));
    let expired = Limits {
        timeout: Duration::from_nanos(1),
        ..limits()
    };
    assert!(matches!(
        read_status(&fixture.root, expired, || false),
        Err(Error::TimedOut)
    ));
    assert!(matches!(
        read_blob(&fixture.root, false, "long-filename.p", expired, || false),
        Err(Error::TimedOut)
    ));
    assert_eq!(image(&fixture.root), before);
    fixture.git(&[
        "config",
        "filter.cide.clean",
        "CIDE_FILTER_MUST_NOT_BE_EXECUTED",
    ]);
    let before = image(&fixture.root);
    assert!(matches!(
        read_status(&fixture.root, limits(), || false),
        Err(Error::Filter)
    ));
    assert!(matches!(
        read_blob(&fixture.root, false, "long-filename.p", limits(), || false),
        Err(Error::Filter)
    ));
    assert_eq!(image(&fixture.root), before);
}

#[test]
fn oversized_index_and_config_are_rejected_before_parsing_without_writes() {
    for name in ["index", "config"] {
        let fixture = Fixture::new();
        fixture.write("owned.p", b"original source\r\n");
        fixture.commit_all();
        let control = fixture.root.join(".git").join(name);
        let file = fs::File::create(&control).unwrap();
        file.set_len(8 * 1024 * 1024 + 1).unwrap();
        drop(file);
        let before = image(&fixture.root);
        assert!(matches!(
            read_status(&fixture.root, limits(), || false),
            Err(Error::Limited)
        ));
        assert!(matches!(
            read_blob(&fixture.root, false, "owned.p", limits(), || false),
            Err(Error::Limited)
        ));
        assert_eq!(image(&fixture.root), before);
    }
}

#[test]
fn a_validly_encoded_blob_with_the_wrong_object_hash_is_rejected_without_writes() {
    let fixture = Fixture::new();
    fixture.write("owned.p", b"expected original payload\r\n");
    fixture.commit_all();
    fixture.write("other.p", b"different original payload\r\n");
    let expected = String::from_utf8(fixture.git(&["rev-parse", "HEAD:owned.p"]))
        .unwrap()
        .trim()
        .to_owned();
    let other = String::from_utf8(fixture.git(&["hash-object", "-w", "--", "other.p"]))
        .unwrap()
        .trim()
        .to_owned();
    assert_ne!(expected, other);
    let object_path = |oid: &str| {
        fixture
            .root
            .join(".git/objects")
            .join(&oid[..2])
            .join(&oid[2..])
    };
    let corrupted = object_path(&expected);
    #[cfg(windows)]
    #[allow(clippy::permissions_set_readonly_false)]
    // Owned Windows fixture; Unix uses mode 0600 below.
    {
        let mut permission = fs::metadata(&corrupted).unwrap().permissions();
        permission.set_readonly(false);
        fs::set_permissions(&corrupted, permission).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&corrupted, fs::Permissions::from_mode(0o600)).unwrap();
    }
    fs::write(&corrupted, fs::read(object_path(&other)).unwrap()).unwrap();
    let before = image(&fixture.root);
    for from_index in [false, true] {
        let result = read_blob(&fixture.root, from_index, "owned.p", limits(), || false);
        assert!(matches!(result, Err(Error::Engine(_))));
    }
    assert_eq!(image(&fixture.root), before);
}

#[test]
fn embedded_reads_run_in_a_child_with_empty_path_and_without_oracle_configuration() {
    let fixture = Fixture::new();
    fixture.write("owned.p", b"\xef\xbb\xbfhead\r\n");
    fixture.commit_all();
    fixture.write("owned.p", b"changed\r\n");
    let before = image(&fixture.root);
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .arg("--ignored")
        .arg("--exact")
        .arg("empty_path_child_helper")
        .env("PATH", OsString::new())
        .env_remove("JC_GIT_TEST_EXE")
        .env("CIDE_EMBEDDED_GIT_CHILD_ROOT", &fixture.root);
    let result = child.output().unwrap();
    assert!(
        result.status.success(),
        "Empty-PATH child: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(image(&fixture.root), before);
}

#[test]
#[ignore = "Only invoked by its parent with an owned fixture and empty PATH"]
fn empty_path_child_helper() {
    assert!(std::env::var_os("PATH").is_some_and(|path| path.is_empty()));
    assert!(std::env::var_os("JC_GIT_TEST_EXE").is_none());
    let root = PathBuf::from(
        std::env::var_os("CIDE_EMBEDDED_GIT_CHILD_ROOT").expect("Parent fixture is required"),
    );
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("work/test-temp")
        .canonicalize()
        .unwrap();
    let root = root.canonicalize().unwrap();
    assert!(
        root.starts_with(&base)
            && root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("embedded-git-")
    );
    let head = read_blob(&root, false, "owned.p", limits(), || false).unwrap();
    assert_eq!(head.bytes, b"\xef\xbb\xbfhead\r\n");
    let status = read_status(&root, limits(), || false).unwrap();
    assert_eq!(status.entries.len(), 1);
    assert_eq!(status.entries[0].path, b"owned.p");
    assert_eq!(status.entries[0].worktree, Change::Modified);
}

#[test]
fn staged_removal_with_retained_workfile_preserves_two_records_and_shared_quotas() {
    let fixture = Fixture::new();
    fixture.write("kept.p", b"keep the original bytes\r\n");
    fixture.commit_all();
    fixture.git(&["rm", "--cached", "--", "kept.p"]);
    let oracle = fixture.git(&[
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--no-renames",
    ]);
    assert_eq!(oracle, b"D  kept.p\0?? kept.p\0");
    let before = image(&fixture.root);
    let status = read_status(&fixture.root, limits(), || false).unwrap();
    assert_eq!(status.entries.len(), 2);
    let deleted = &status.entries[0];
    assert_eq!(deleted.path, b"kept.p");
    assert_eq!(deleted.kind, Kind::Tracked);
    assert_eq!(deleted.index, Change::Deleted);
    assert_eq!(deleted.worktree, Change::Unchanged);
    let retained = &status.entries[1];
    assert_eq!(retained.path, b"kept.p");
    assert_eq!(retained.kind, Kind::Untracked);
    assert_eq!(retained.index, Change::Unchanged);
    assert_eq!(retained.worktree, Change::Unchanged);
    let cap = Limits {
        max_entries: 1,
        ..limits()
    };
    assert!(matches!(
        read_status(&fixture.root, cap, || false),
        Err(Error::Limited)
    ));
    assert_eq!(image(&fixture.root), before);
}

#[test]
fn staged_and_head_gitlinks_are_rejected_without_reporting_false_clean_or_writing() {
    let fixture = Fixture::new();
    fixture.write("owned.p", b"ordinary source\n");
    fixture.commit_all();
    let commit = String::from_utf8(fixture.git(&["rev-parse", "HEAD"])).unwrap();
    let cacheinfo = format!("160000,{},nested", commit.trim());
    fixture.git(&["update-index", "--add", "--cacheinfo", &cacheinfo]);
    let before = image(&fixture.root);
    assert!(matches!(
        read_status(&fixture.root, limits(), || false),
        Err(Error::Unsupported)
    ));
    assert!(matches!(
        read_blob(&fixture.root, true, "nested", limits(), || false),
        Err(Error::Unsupported)
    ));
    assert_eq!(image(&fixture.root), before);
    fixture.git(&["commit", "--quiet", "--no-verify", "-m", "gitlink fixture"]);
    fixture.git(&["update-index", "--force-remove", "--", "nested"]);
    let before = image(&fixture.root);
    assert!(matches!(
        read_status(&fixture.root, limits(), || false),
        Err(Error::Unsupported)
    ));
    assert!(matches!(
        read_blob(&fixture.root, false, "nested", limits(), || false),
        Err(Error::Unsupported)
    ));
    assert_eq!(image(&fixture.root), before);
}
