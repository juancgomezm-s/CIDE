//! Repository discovery follows the loaded explorer and the active document's
//! ancestors. Only the owner thread publishes observations. Rendering is read-only.
use super::{git_panel::GitIntent, State};
use jc_app::{
    git_read::GitBlobSource,
    git_status::{GitStatusChange, GitStatusSnapshot, GitStatusState},
    git_status_tasks::{GitStatusController, GitStatusOptions, GitStatusTaskState},
    source_services::display_ui_label,
    Wakeup,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::ErrorKind,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
};

const MAX_DECORATED: usize = 32;
const MAX_CACHED_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Marker {
    None,
    Git,
    Unavailable,
}

#[derive(Clone, PartialEq, Eq)]
struct ProbeKey {
    workspace: PathBuf,
    catalog: u64,
    anchor: PathBuf,
}

struct Probe {
    key: ProbeKey,
    receive: mpsc::Receiver<Vec<(PathBuf, Marker)>>,
    cancel: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

#[derive(Clone, Copy)]
pub(super) enum Pending {
    Panel(GitIntent),
    Code(GitBlobSource),
    CodeHistory,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Mark {
    Changed,
    Removed,
    Added,
    Untracked,
    Conflict,
}

impl Mark {
    fn from_state(state: GitStatusState) -> Option<Self> {
        match state {
            GitStatusState::Conflict(_) => Some(Self::Conflict),
            GitStatusState::Untracked => Some(Self::Untracked),
            GitStatusState::Ignored => None,
            GitStatusState::Tracked { index, worktree } => {
                let state = if worktree == GitStatusChange::Unchanged {
                    index
                } else {
                    worktree
                };
                match state {
                    GitStatusChange::Unchanged => None,
                    GitStatusChange::Deleted => Some(Self::Removed),
                    GitStatusChange::Added => Some(Self::Added),
                    _ => Some(Self::Changed),
                }
            }
        }
    }

    fn appearance(self) -> (&'static str, u32) {
        match self {
            Self::Changed => ("M", 0x007dc6ee),
            Self::Removed => ("E", 0x008d90ed),
            Self::Added => ("A", 0x00a4d69b),
            Self::Untracked => ("?", 0x00d1c28d),
            Self::Conflict => ("C", 0x00ada4ff),
        }
    }
}

pub(super) struct Decoration {
    pub label: Option<String>,
    pub badge: String,
    pub color: u32,
    pub repository: bool,
    pub changed: bool,
}

struct Summary {
    branch: Option<String>,
    entries: BTreeMap<PathBuf, Mark>,
    count: usize,
    failed: bool,
    bytes: usize,
}

impl Summary {
    fn from_snapshot(snapshot: &GitStatusSnapshot) -> Self {
        let mut entries = BTreeMap::<PathBuf, Mark>::new();
        let mut count = 0;
        for change in snapshot.status().entries() {
            let Some(mark) = Mark::from_state(change.state()) else {
                continue;
            };
            count += 1;
            for path in [Some(change.path()), change.original_path()]
                .into_iter()
                .flatten()
            {
                let Ok(text) = std::str::from_utf8(path) else {
                    continue;
                };
                let path = Path::new(text.trim_end_matches('/'));
                if path.as_os_str().is_empty()
                    || !path
                        .components()
                        .all(|part| matches!(part, Component::Normal(_)))
                {
                    continue;
                }
                entries
                    .entry(path.to_owned())
                    .and_modify(|previous| *previous = (*previous).max(mark))
                    .or_insert(mark);
            }
        }
        let branch = snapshot.info().and_then(|info| {
            info.branch
                .as_deref()
                .map(|branch| display_ui_label(&String::from_utf8_lossy(branch), 28))
                .or_else(|| {
                    info.head
                        .as_ref()
                        .map(|head| head.chars().take(8).collect())
                })
        });
        let bytes = entries.keys().map(|path| path.capacity() + 80).sum();
        Self {
            branch,
            entries,
            count,
            bytes,
            failed: false,
        }
    }

    fn failed() -> Self {
        Self {
            branch: None,
            entries: BTreeMap::new(),
            count: 0,
            bytes: 0,
            failed: true,
        }
    }

    fn marks_directory(&self, relative: &Path) -> bool {
        self.entries
            .range(relative.to_owned()..)
            .next()
            .is_some_and(|(path, _)| path.starts_with(relative))
    }
}

pub(super) struct Repositories {
    workspace: Option<PathBuf>,
    directory: Option<(Option<PathBuf>, PathBuf)>,
    checked: BTreeMap<PathBuf, Marker>,
    submitted: Option<ProbeKey>,
    probe: Option<Probe>,
    refresh_probe: bool,
    observed_root: Option<PathBuf>,
    pending: Option<(PathBuf, Pending)>,
    statuses: BTreeMap<PathBuf, Summary>,
    status_refresh: BTreeSet<PathBuf>,
    status: GitStatusController,
    status_options: Option<GitStatusOptions>,
    wakeup: Wakeup,
}

impl Repositories {
    pub fn new(wakeup: Wakeup) -> Self {
        Self {
            workspace: None,
            directory: None,
            checked: BTreeMap::new(),
            submitted: None,
            probe: None,
            refresh_probe: false,
            observed_root: None,
            pending: None,
            statuses: BTreeMap::new(),
            status_refresh: BTreeSet::new(),
            status: GitStatusController::new(wakeup.clone()),
            status_options: None,
            wakeup,
        }
    }

    pub(super) fn reset(&mut self, workspace: &Path) {
        self.workspace = Some(workspace.to_owned());
        self.directory = None;
        self.checked.clear();
        self.submitted = None;
        if let Some(probe) = &self.probe {
            probe.cancel.store(true, Ordering::Relaxed);
        }
        self.refresh_probe = false;
        self.observed_root = None;
        self.pending = None;
        self.statuses.clear();
        self.status_refresh.clear();
        self.status.cancel();
        self.status_options = None;
    }

    fn root_for<'a>(&self, workspace: &'a Path, anchor: &'a Path) -> Option<&'a Path> {
        if self.workspace.as_deref() != Some(workspace) {
            return (anchor == workspace).then_some(workspace);
        }
        for folder in anchor
            .ancestors()
            .take_while(|path| path.starts_with(workspace))
            .take(64)
        {
            match self.checked.get(folder) {
                Some(Marker::Git) => return Some(folder),
                Some(Marker::None) => (),
                Some(Marker::Unavailable) => return None,
                None => return (anchor == workspace).then_some(workspace),
            }
        }
        None
    }

    fn awaiting(&self, workspace: &Path, anchor: &Path) -> bool {
        anchor != workspace
            && self.root_for(workspace, anchor).is_none()
            && anchor
                .ancestors()
                .take_while(|path| path.starts_with(workspace))
                .take(64)
                .any(|path| !self.checked.contains_key(path))
    }

    fn remember(&mut self, root: PathBuf, summary: Summary) {
        if self.checked.get(&root) == Some(&Marker::None) {
            return;
        }
        self.statuses.remove(&root);
        let current = self.observed_root.as_ref();
        while self.statuses.len() >= MAX_DECORATED
            || self
                .statuses
                .values()
                .map(|summary| summary.bytes)
                .sum::<usize>()
                + summary.bytes
                > MAX_CACHED_BYTES
        {
            let Some(old) = self
                .statuses
                .keys()
                .find(|path| Some(*path) != current)
                .cloned()
            else {
                return;
            };
            self.statuses.remove(&old);
        }
        self.statuses.insert(root, summary);
    }

    fn poll_status(&mut self) -> bool {
        if let Some(options) = &self.status_options {
            if !self.status.poll(options) {
                return false;
            }
            if busy(self.status.state()) {
                return true;
            }
            let path = options.root.clone();
            let summary = self
                .status
                .result()
                .filter(|result| result.repository().root == path)
                .map_or_else(Summary::failed, Summary::from_snapshot);
            if self.checked.get(&path) == Some(&Marker::Git)
                && self.observed_root.as_ref() != Some(&path)
            {
                self.remember(path, summary);
            }
            self.status_options = None;
        }
        let next = self.status_refresh.pop_first();
        if let Some(root) = next {
            let options = GitStatusOptions {
                root,
                limits: Default::default(),
            };
            if self.status.request(options.clone()).is_ok() {
                self.status_options = Some(options);
            } else {
                self.wakeup.signal();
            }
        }
        true
    }

    pub fn decoration(&self, path: &Path, directory: bool) -> Option<Decoration> {
        let repo = path
            .ancestors()
            .find(|folder| self.checked.get(*folder) == Some(&Marker::Git));
        if repo == Some(path) {
            let summary = self.statuses.get(path);
            let count = summary.map_or(0, |summary| summary.count);
            let badge = match summary {
                Some(summary) if summary.failed => "!".to_owned(),
                _ if count > 99 => "99+".to_owned(),
                _ if count > 0 => count.to_string(),
                _ => String::new(),
            };
            return Some(Decoration {
                label: Some(
                    summary
                        .and_then(|summary| summary.branch.clone())
                        .unwrap_or_else(|| "Git".into()),
                ),
                badge,
                color: if count > 0 {
                    Mark::Changed.appearance().1
                } else {
                    super::shell::ACCENT
                },
                repository: true,
                changed: count > 0,
            });
        }
        let mark = if let Some((relative, summary)) =
            repo.and_then(|root| Some((path.strip_prefix(root).ok()?, self.statuses.get(root)?)))
        {
            if directory && summary.marks_directory(relative) {
                Some(("•", Mark::Changed.appearance().1))
            } else {
                summary.entries.get(relative).map(|mark| mark.appearance())
            }
        } else if directory
            && self
                .statuses
                .range(path.to_owned()..)
                .take_while(|(root, _)| root.starts_with(path))
                .any(|(_, summary)| summary.count > 0)
        {
            Some(("•", Mark::Changed.appearance().1))
        } else {
            None
        }?;
        Some(Decoration {
            label: None,
            badge: mark.0.into(),
            color: mark.1,
            repository: false,
            changed: true,
        })
    }
}

impl Drop for Repositories {
    fn drop(&mut self) {
        if let Some(probe) = self.probe.take() {
            probe.cancel.store(true, Ordering::Relaxed);
            let _ = probe.handle.join();
        }
    }
}

fn busy(state: GitStatusTaskState) -> bool {
    matches!(
        state,
        GitStatusTaskState::Queued | GitStatusTaskState::Running
    )
}

fn inspect(folder: &Path) -> Marker {
    match fs::symlink_metadata(folder.join(".git")) {
        Ok(_) if fs::canonicalize(folder).is_ok_and(|target| target == folder) => Marker::Git,
        Ok(_) => Marker::Unavailable,
        Err(error) if error.kind() == ErrorKind::NotFound => Marker::None,
        Err(_) => Marker::Unavailable,
    }
}

impl State {
    fn git_anchor<'a>(&'a self, workspace: &'a Path) -> &'a Path {
        let active = self.workbench.active_path();
        self.git_repositories
            .directory
            .as_ref()
            .filter(|(document, directory)| {
                document.as_deref() == active && directory.starts_with(workspace)
            })
            .map(|(_, directory)| directory.as_path())
            .or_else(|| {
                active
                    .and_then(Path::parent)
                    .filter(|directory| directory.starts_with(workspace))
            })
            .unwrap_or(workspace)
    }

    pub(super) fn git_root(&self) -> Option<&Path> {
        let workspace = self.project_root.as_deref()?;
        self.git_repositories
            .root_for(workspace, self.git_anchor(workspace))
    }

    pub(super) fn select_git_directory(&mut self, path: PathBuf) {
        self.git_repositories.directory =
            Some((self.workbench.active_path().map(Path::to_owned), path));
        self.schedule_git_discovery();
    }

    pub(super) fn git_follows_document(&mut self) {
        self.git_repositories.directory = None;
        self.schedule_git_discovery();
    }

    pub(super) fn schedule_git_discovery(&self) {
        if let Some(workspace) = &self.project_root {
            let anchor = self.git_anchor(workspace);
            let repos = &self.git_repositories;
            if repos.probe.is_none()
                && (repos.refresh_probe
                    || repos.submitted.as_ref().is_none_or(|key| {
                        key.workspace != *workspace
                            || key.anchor != anchor
                            || key.catalog != self.explorer.revision()
                    }))
            {
                self.bridge.signal();
            }
        }
    }

    pub(super) fn refresh_git_repositories(&mut self) {
        if self.project_root.is_some() {
            self.git_repositories.refresh_probe = true;
            self.bridge.signal();
        }
    }

    pub(super) fn defer_git_until_discovered(&mut self, intent: Pending) -> bool {
        let Some(workspace) = &self.project_root else {
            return false;
        };
        let anchor = self.git_anchor(workspace);
        if !self.git_repositories.awaiting(workspace, anchor) {
            return false;
        }
        self.git_repositories.pending = Some((anchor.to_owned(), intent));
        self.schedule_git_discovery();
        self.status = "Identificando el repositorio Git…".into();
        true
    }

    pub(super) fn remember_git_status(&mut self) {
        let Some(root) = self.git_status.root().map(Path::to_owned) else {
            return;
        };
        if self.git_root() != Some(root.as_path()) || busy(self.git_status.state()) {
            return;
        }
        let summary = self
            .git_status
            .result()
            .filter(|result| result.repository().root == root)
            .map_or_else(Summary::failed, Summary::from_snapshot);
        if self.git_repositories.checked.get(&root) == Some(&Marker::Git) {
            self.git_repositories.remember(root, summary);
        }
    }

    pub(super) fn poll_git_repositories(&mut self) -> bool {
        let Some(workspace) = self.project_root.clone() else {
            return false;
        };
        let mut changed = false;
        let anchor = self.git_anchor(&workspace).to_owned();
        let repos = &mut self.git_repositories;
        if repos.workspace.as_deref() != Some(workspace.as_path()) {
            repos.reset(&workspace);
        }
        let refresh = std::mem::take(&mut repos.refresh_probe);
        if refresh {
            repos.submitted = None;
        }
        if let Some(probe) = &repos.probe {
            if refresh {
                probe.cancel.store(true, Ordering::Relaxed);
            }
            match probe.receive.try_recv() {
                Err(mpsc::TryRecvError::Empty) => (),
                result => {
                    let probe = repos.probe.take().unwrap();
                    let _ = probe.handle.join();
                    if probe.key.workspace == workspace && !probe.cancel.load(Ordering::Relaxed) {
                        if let Ok(checks) = result {
                            for (path, marker) in checks {
                                if marker != Marker::Git {
                                    repos.statuses.remove(&path);
                                    repos.status_refresh.remove(&path);
                                }
                                repos.checked.insert(path, marker);
                            }
                            changed = true;
                        }
                    }
                }
            }
        }
        if repos.probe.is_none() {
            let key = ProbeKey {
                workspace: workspace.clone(),
                anchor: anchor.clone(),
                catalog: self.explorer.revision(),
            };
            if repos.submitted.as_ref() != Some(&key) {
                let all = repos.submitted.is_none();
                repos.submitted = Some(key.clone());
                let mut candidates = BTreeSet::new();
                candidates.insert(workspace.clone());
                candidates.extend(
                    anchor
                        .ancestors()
                        .take_while(|dir| dir.starts_with(&workspace))
                        .take(64)
                        .map(Path::to_owned),
                );
                if self.explorer.root() == Some(workspace.as_path()) {
                    candidates.extend(
                        self.explorer
                            .visible_nodes()
                            .filter(|node| node.is_dir())
                            .map(|node| workspace.join(node.relative_path())),
                    );
                }
                if all {
                    candidates.extend(repos.statuses.keys().cloned());
                }
                // The explorer and the anchor bound discovery. Forget probes for branches
                // that have left the directory catalog, without forgetting known repositories.
                repos.checked.retain(|path, marker| {
                    *marker == Marker::Git && repos.statuses.contains_key(path)
                        || candidates.contains(path)
                });
                let pending = if all {
                    candidates.into_iter().collect::<Vec<_>>()
                } else {
                    candidates
                        .into_iter()
                        .filter(|path| !repos.checked.contains_key(path))
                        .collect()
                };
                if !pending.is_empty() {
                    let (send, receive) = mpsc::sync_channel(1);
                    let cancel = Arc::new(AtomicBool::new(false));
                    let stop = cancel.clone();
                    let wakeup = repos.wakeup.clone();
                    if let Ok(handle) = thread::Builder::new()
                        .name("cide-git-repositories".into())
                        .spawn(move || {
                            let checks = pending
                                .into_iter()
                                .take_while(|_| !stop.load(Ordering::Relaxed))
                                .map(|path| {
                                    let marker = inspect(&path);
                                    (path, marker)
                                })
                                .collect();
                            let _ = send.send(checks);
                            wakeup.signal();
                        })
                    {
                        repos.probe = Some(Probe {
                            key,
                            receive,
                            cancel,
                            handle,
                        });
                    }
                }
            }
        }
        let root = self.git_root().map(Path::to_owned);
        let repos = &mut self.git_repositories;
        let switched = repos.observed_root != root;
        if switched {
            repos.observed_root.clone_from(&root);
        }
        if changed || refresh || switched {
            repos.status_refresh.extend(
                repos
                    .checked
                    .iter()
                    .filter(|(path, marker)| {
                        **marker == Marker::Git && Some(*path) != root.as_ref()
                    })
                    .filter(|(path, _)| refresh || !repos.statuses.contains_key(*path))
                    .take(MAX_DECORATED)
                    .map(|(path, _)| path.clone()),
            );
        }
        repos
            .status_refresh
            .retain(|path| Some(path) != root.as_ref());
        if repos.status_options.is_some() || !repos.status_refresh.is_empty() {
            changed |= repos.poll_status();
        }
        if switched {
            self.git_status_due = root.is_some();
        }
        if changed && self.git_status.state() == GitStatusTaskState::Ready {
            self.remember_git_status();
        }
        if let Some((target, intent)) = self.git_repositories.pending.take() {
            if target == anchor {
                if root.is_some() {
                    self.validate_git_context();
                    match intent {
                        Pending::Panel(intent) => {
                            self.request_git(intent);
                        }
                        Pending::Code(source) => {
                            self.open_git_code(source);
                        }
                        Pending::CodeHistory => {
                            self.open_document_git_history();
                        }
                    }
                } else if self.git_repositories.awaiting(&workspace, &anchor) {
                    self.git_repositories.pending = Some((target, intent));
                } else {
                    self.status =
                        "La carpeta seleccionada no pertenece a un repositorio Git".into();
                }
                changed = true;
            }
        }
        changed || switched
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GuiOptions;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap()
                .join("work/test-temp");
            fs::create_dir_all(&dir).unwrap();
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = dir
                .canonicalize()
                .unwrap()
                .join(format!("gui-repos-{}-{nonce}", std::process::id()));
            fs::create_dir(&dir).unwrap();
            Self(dir)
        }

        fn repository(&self, name: &str, branch: &str) -> PathBuf {
            let root = self.0.join(name);
            fs::create_dir_all(root.join(".git/objects")).unwrap();
            fs::create_dir_all(root.join(".git/refs/heads")).unwrap();
            fs::write(
                root.join(".git/HEAD"),
                format!("ref: refs/heads/{branch}\n"),
            )
            .unwrap();
            fs::write(
                root.join(".git/config"),
                b"[core]\nrepositoryformatversion=0\nfilemode=false\nbare=false\n",
            )
            .unwrap();
            root
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if self.0.canonicalize().is_ok_and(|path| path == self.0) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }

    fn wait(state: &mut State, ready: impl Fn(&State) -> bool) {
        let until = Instant::now() + Duration::from_secs(8);
        while !ready(state) {
            state.poll_tasks();
            assert!(Instant::now() < until, "{}", state.status);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn git_repositories_keep_sibling_status_and_explorer_badges_separate() {
        let fixture = Fixture::new();
        let first = fixture.repository("Alfa", "desarrollo");
        let second = fixture.repository("Beta", "maestra");
        let other = fixture.0.join("Documentos");
        fs::create_dir(&other).unwrap();
        let file_a = first.join("nota.txt");
        let file_b = second.join("nota.txt");
        fs::write(&file_a, b"uno\n").unwrap();
        fs::write(&file_b, b"dos\n").unwrap();
        fs::write(second.join("codigo.p"), b"RETURN.\n").unwrap();
        let mut state = State::new(&GuiOptions {
            project_root: Some(fixture.0.clone()),
            encoding: Some(jc_text::TextEncoding::Utf8),
            ..GuiOptions::default()
        })
        .unwrap();
        state.width = 1400;
        state.height = 900;
        state.explorer_visible = true;

        wait(&mut state, |state| {
            state.git_root().is_none()
                && state.git_repositories.statuses.contains_key(&first)
                && state.git_repositories.statuses.contains_key(&second)
        });
        assert_eq!(state.git_summary(), "Git: elige repositorio");
        let frame = state.explorer_presentation().unwrap();
        let badge = |name| {
            let i = frame
                .presentation
                .rows
                .iter()
                .position(|row| row.label == name)
                .unwrap();
            frame.git[i].as_ref()
        };
        assert_eq!(badge("Alfa").unwrap().badge, "1");
        assert_eq!(badge("Alfa").unwrap().label.as_deref(), Some("desarrollo"));
        assert_eq!(badge("Beta").unwrap().badge, "2");
        assert_eq!(badge("Beta").unwrap().label.as_deref(), Some("maestra"));
        assert!(badge("Documentos").is_none());

        let a = state
            .explorer
            .nodes()
            .iter()
            .find(|node| node.relative_path() == Path::new("Alfa"))
            .unwrap()
            .id();
        state.explorer_view.select(a);
        state.activate_explorer(false, true);
        state.request_git(GitIntent::Status);
        wait(&mut state, |state| {
            state
                .git_status
                .result()
                .is_some_and(|status| status.repository().root == first)
        });
        assert_eq!(state.git_root(), Some(first.as_path()));
        assert_eq!(
            state.git_status.result().unwrap().status().entries().len(),
            1
        );
        wait(&mut state, |state| {
            state
                .explorer
                .nodes()
                .iter()
                .any(|node| node.relative_path() == Path::new("Alfa").join("nota.txt"))
        });
        let frame = state.explorer_presentation().unwrap();
        let i = frame
            .presentation
            .rows
            .iter()
            .position(|row| row.label == "nota.txt")
            .unwrap();
        assert_eq!(frame.git[i].as_ref().unwrap().badge, "?");

        state
            .workbench
            .open_file(&file_b, Some(jc_text::TextEncoding::Utf8))
            .unwrap();
        state.request_git(GitIntent::Status);
        wait(&mut state, |state| {
            state
                .git_status
                .result()
                .is_some_and(|status| status.repository().root == second)
        });
        assert_eq!(state.project_root.as_deref(), Some(fixture.0.as_path()));
        assert_eq!(
            state.git_status.result().unwrap().status().entries().len(),
            2
        );
        assert_eq!(
            state
                .git_repositories
                .decoration(&file_a, false)
                .unwrap()
                .badge,
            "?"
        );
        assert_eq!(state.workbench.active_bytes().unwrap(), b"dos\n");
    }

    #[test]
    fn git_repositories_discover_the_nearest_repo_for_a_file_outside_the_loaded_tree() {
        let fixture = Fixture::new();
        let outer = fixture.repository("Exterior", "exterior");
        let inner = fixture.repository("Exterior/librerias/Interior", "interior");
        let source = inner.join("src/programa.p");
        fs::create_dir(source.parent().unwrap()).unwrap();
        fs::write(&source, b"RETURN.\n").unwrap();
        let mut state = State::new(&GuiOptions {
            project_root: Some(fixture.0.clone()),
            paths: vec![source.clone()],
            encoding: Some(jc_text::TextEncoding::Utf8),
            ..GuiOptions::default()
        })
        .unwrap();
        assert!(state.git_root().is_none());
        state.request_git(GitIntent::Status);
        assert!(state.git_repositories.pending.is_some());
        wait(&mut state, |state| {
            state
                .git_status
                .result()
                .is_some_and(|status| status.repository().root == inner)
        });
        assert_eq!(state.git_context.as_deref(), Some(inner.as_path()));
        assert_ne!(state.git_root(), Some(outer.as_path()));
        assert!(state.git_panel.is_some());
        assert!(state.git_repositories.pending.is_none());
        assert_eq!(state.workbench.active_path(), Some(source.as_path()));
        state.select_git_directory(outer.clone());
        assert_eq!(state.git_root(), Some(outer.as_path()));
        state.open_git_code(GitBlobSource::Head);
        assert_eq!(state.git_context.as_deref(), Some(inner.as_path()));
    }

    #[test]
    fn git_repositories_project_file_badges_and_parent_changes() {
        let root = PathBuf::from(r"C:\prueba\repositorio");
        let status = jc_app::git_status::parse_porcelain_v1(
            b" M src/modificado.p\0A  src/preparado.p\0?? otro/nuevo.p\0UU src/conflicto.p\0",
            Default::default(),
            || false,
        )
        .unwrap();
        let snapshot = GitStatusSnapshot::from_parts(
            jc_app::git_read::GitRepositoryIdentity {
                root: root.clone(),
                git_dir: root.join(".git"),
            },
            status,
        );
        let mut repositories = Repositories::new(Wakeup::default());
        repositories.checked.insert(root.clone(), Marker::Git);
        repositories.remember(root.clone(), Summary::from_snapshot(&snapshot));
        for (path, badge) in [
            ("src/modificado.p", "M"),
            ("src/preparado.p", "A"),
            ("otro/nuevo.p", "?"),
            ("src/conflicto.p", "C"),
        ] {
            assert_eq!(
                repositories
                    .decoration(&root.join(path), false)
                    .unwrap()
                    .badge,
                badge
            );
        }
        assert_eq!(
            repositories
                .decoration(&root.join("src"), true)
                .unwrap()
                .badge,
            "•"
        );
        assert!(repositories.decoration(&root.join("sr"), true).is_none());
        assert!(repositories
            .decoration(&root.join("src/limpio.p"), false)
            .is_none());
    }
}
