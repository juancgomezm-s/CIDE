//! Optional, namespaced internal source capabilities. No language-specific rules
//! or external runtime live in this host. All mutations return through Workbench.
use crate::{
    Cancellation, CompletedTask, JobId, JobKey, JobSystem, PaneId, SubmitError, TaskFailure,
    TaskItem, TaskOutput, TaskProvider, TaskSnapshot, TaskStamp, TaskStatus, Workbench,
};
use std::{
    ops::Range,
    path::{Path, PathBuf},
};

pub const SOURCE_JOB_KEY: JobKey = JobKey(0x4a43_5352_4353_0001);
const SNAPSHOT: usize = 2 * 1024 * 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceMode {
    Symbols,
    References,
    Completion,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceAction {
    Navigate,
    RelativeFile(String),
    Unresolved(String),
    Replace(String),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceItem {
    pub range: Range<usize>,
    pub label: String,
    pub action: SourceAction,
}
impl SourceItem {
    pub(crate) fn allocation(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.label.capacity()
            + match &self.action {
                SourceAction::Navigate => 0,
                SourceAction::RelativeFile(s)
                | SourceAction::Unresolved(s)
                | SourceAction::Replace(s) => s.capacity(),
            }
    }
    fn valid_for(&self, mode: SourceMode) -> bool {
        self.label.len() <= 512
            && !self.label.chars().any(char::is_control)
            && match (&self.action, mode) {
                (SourceAction::Navigate, SourceMode::Symbols | SourceMode::References) => true,
                (SourceAction::RelativeFile(path), SourceMode::References) => {
                    path.len() <= 4096 && !path.is_empty() && !path.chars().any(char::is_control)
                }
                (SourceAction::Unresolved(reason), SourceMode::References) => reason.len() <= 512,
                (SourceAction::Replace(text), SourceMode::Completion) => text.len() <= 4096,
                _ => false,
            }
    }
}
#[derive(Clone, Copy)]
pub struct SourceProviderDescriptor {
    pub id: &'static str,
    pub language: &'static str,
    pub symbols: Option<TaskProvider>,
    pub references: Option<TaskProvider>,
    pub completion: Option<TaskProvider>,
}
impl SourceProviderDescriptor {
    fn provider(self, mode: SourceMode) -> Option<TaskProvider> {
        match mode {
            SourceMode::Symbols => self.symbols,
            SourceMode::References => self.references,
            SourceMode::Completion => self.completion,
        }
    }
}
pub struct SourceRegistry {
    providers: Vec<SourceProviderDescriptor>,
}
impl Default for SourceRegistry {
    fn default() -> Self {
        Self {
            providers: vec![SourceProviderDescriptor {
                id: "core.words",
                language: "*",
                symbols: None,
                references: None,
                completion: Some(local_words),
            }],
        }
    }
}
impl SourceRegistry {
    pub fn register(&mut self, descriptor: SourceProviderDescriptor) -> Result<(), &'static str> {
        if self.providers.len() >= 16
            || !descriptor.id.contains('.')
            || descriptor.id.len() > 64
            || descriptor.language.len() > 32
            || descriptor.language.is_empty()
            || !descriptor
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            || !descriptor
                .language
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'*'))
            || [
                descriptor.symbols,
                descriptor.references,
                descriptor.completion,
            ]
            .iter()
            .all(Option::is_none)
            || self
                .providers
                .iter()
                .any(|p| p.id == descriptor.id || p.language == descriptor.language)
        {
            return Err("Duplicate/invalid source provider or registry budget");
        }
        self.providers.push(descriptor);
        Ok(())
    }
    pub fn resolve(&self, language: &str, mode: SourceMode) -> Option<SourceProviderDescriptor> {
        self.providers
            .iter()
            .find(|p| p.language == language && p.provider(mode).is_some())
            .or_else(|| {
                self.providers
                    .iter()
                    .find(|p| p.language == "*" && p.provider(mode).is_some())
            })
            .copied()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceState {
    #[default]
    Idle,
    Queued,
    Running,
    Ready,
    Stale,
    Cancelled,
    Limited,
    Failed,
}
#[derive(Clone)]
struct Origin {
    stamp: TaskStamp,
    path: PathBuf,
    pane: PaneId,
    cursor: usize,
    anchor: Option<usize>,
    root: Option<PathBuf>,
    language: String,
}
impl Origin {
    fn capture(app: &Workbench, root: Option<&Path>) -> Result<Self, String> {
        let doc = app.active_document().ok_or("No source document")?;
        let pane = app.panes().active();
        let view = app.panes().view(pane).ok_or("No source view")?;
        let editor = app.active_editor().ok_or("No source editor")?;
        let encoding = doc
            .session
            .encoding()
            .ok_or("Choose source encoding first")?;
        if doc.session.path().as_os_str().len() > 4096
            || root.is_some_and(|r| r.as_os_str().len() > 4096)
        {
            return Err("Source metadata budget".into());
        }
        Ok(Self {
            stamp: TaskStamp {
                document: doc.id,
                revision: doc.session.document().revision(),
                encoding,
                view: Some(view.view_id().ok_or("No source ViewId")?),
            },
            path: doc.session.path().to_owned(),
            pane,
            cursor: editor.cursor,
            anchor: editor.anchor,
            root: root.map(Path::to_owned),
            language: jc_languages::detect(&doc.session.path().to_string_lossy())
                .id
                .into(),
        })
    }
    fn current(&self, app: &Workbench, root: Option<&Path>, mode: SourceMode) -> bool {
        app.task_stamp_is_current(self.stamp)
            && self.root.as_deref() == root
            && app.panes().active() == self.pane
            && app
                .panes()
                .view(self.pane)
                .is_some_and(|v| v.view_id() == self.stamp.view)
            && app
                .active_document()
                .is_some_and(|d| d.id == self.stamp.document && d.session.path() == self.path)
            && app.active_editor().is_some_and(|e| {
                mode != SourceMode::Completion
                    || e.cursor == self.cursor
                        && e.anchor == self.anchor
                        && app.rectangle_for_pane(self.pane).is_none()
            })
    }
}
#[derive(Clone)]
struct Request {
    generation: u64,
    origin: Origin,
    mode: SourceMode,
    provider: SourceProviderDescriptor,
}
struct Flight {
    id: JobId,
    request: Request,
    discard: bool,
}
#[derive(Default)]
pub struct SourceController {
    generation: u64,
    flight: Option<Flight>,
    pending: Option<Request>,
    accepted: Option<Request>,
    items: Vec<SourceItem>,
    selected: usize,
    state: SourceState,
    message: String,
    mode: Option<SourceMode>,
}
pub struct SourceRow {
    pub index: usize,
    pub label: String,
    pub selected: bool,
}
pub struct SourcePresentation {
    pub title: String,
    pub message: String,
    pub rows: Vec<SourceRow>,
}
pub fn display_label(text: &str, columns: usize) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        let token = match ch {
            '\\' => "\\\\".into(),
            ' '..='~' => ch.to_string(),
            _ => format!("\\u{{{:X}}}", ch as u32),
        };
        if out.len() + token.len() > columns.min(256) {
            break;
        }
        out.push_str(&token);
    }
    out
}
impl SourceController {
    pub fn state(&self) -> SourceState {
        self.state
    }
    pub fn mode(&self) -> Option<SourceMode> {
        self.mode
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub fn items(&self) -> &[SourceItem] {
        &self.items
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn ready(&self, app: &Workbench, root: Option<&Path>) -> bool {
        self.state == SourceState::Ready
            && !self.items.is_empty()
            && self
                .accepted
                .as_ref()
                .is_some_and(|r| r.origin.current(app, root, r.mode))
    }
    pub fn present(&self, max_rows: usize, columns: usize) -> SourcePresentation {
        let count = max_rows.min(64);
        let first = self.selected.saturating_sub(count.saturating_sub(1));
        SourcePresentation {
            title: display_label(
                match self.mode {
                    Some(SourceMode::Symbols) => "Symbols (lexical)",
                    Some(SourceMode::References) => "References (lexical)",
                    _ => "Completion (local words)",
                },
                columns,
            ),
            message: display_label(&self.message, columns),
            rows: self
                .items
                .iter()
                .enumerate()
                .skip(first)
                .take(count)
                .map(|(index, item)| SourceRow {
                    index,
                    label: display_label(&item.label, columns),
                    selected: index == self.selected,
                })
                .collect(),
        }
    }
    pub fn select_relative(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.items.len().saturating_sub(1));
    }
    pub fn first(&mut self) {
        self.selected = 0;
    }
    pub fn last(&mut self) {
        self.selected = self.items.len().saturating_sub(1);
    }
    fn status(&mut self, state: SourceState, message: impl Into<String>) -> bool {
        let message = message.into();
        let changed = self.state != state || self.message != message;
        self.state = state;
        self.message = message;
        changed
    }
    fn invalidate(&mut self, jobs: &mut JobSystem) {
        self.generation = self.generation.saturating_add(1);
        if let Some(f) = &mut self.flight {
            f.discard = true;
            jobs.cancel(SOURCE_JOB_KEY);
        }
        self.pending = None;
        self.accepted = None;
        self.items.clear();
        self.selected = 0;
    }
    pub fn cancel(&mut self, jobs: &mut JobSystem) {
        self.invalidate(jobs);
        self.status(SourceState::Cancelled, "Source service cancelled");
    }
    pub fn request(
        &mut self,
        mode: SourceMode,
        registry: &SourceRegistry,
        app: &Workbench,
        root: Option<&Path>,
        jobs: &mut JobSystem,
    ) -> Result<(), String> {
        if self.generation == u64::MAX {
            return Err("Source generation exhausted".into());
        }
        self.invalidate(jobs);
        self.mode = Some(mode);
        self.status(SourceState::Failed, "Source request unavailable");
        if mode == SourceMode::Completion
            && (app.active_editor().is_some_and(|e| e.selection().is_some())
                || app.rectangle_for_pane(app.panes().active()).is_some())
        {
            return Err("Completion requires an unselected text caret".into());
        }
        let origin = Origin::capture(app, root)?;
        let provider = registry
            .resolve(&origin.language, mode)
            .ok_or("No provider for this source capability")?;
        self.pending = Some(Request {
            generation: self.generation,
            origin,
            mode,
            provider,
        });
        self.status(SourceState::Queued, "Source capability queued");
        self.tick(app, root, jobs);
        if matches!(self.state, SourceState::Limited | SourceState::Failed) {
            Err(self.message.clone())
        } else {
            Ok(())
        }
    }
    pub fn validate_context(
        &mut self,
        app: &Workbench,
        root: Option<&Path>,
        jobs: &mut JobSystem,
    ) -> bool {
        let stale = self
            .accepted
            .as_ref()
            .is_some_and(|r| !r.origin.current(app, root, r.mode))
            || self
                .pending
                .as_ref()
                .is_some_and(|r| !r.origin.current(app, root, r.mode))
            || self.flight.as_ref().is_some_and(|f| {
                !f.discard && !f.request.origin.current(app, root, f.request.mode)
            });
        if stale {
            self.invalidate(jobs);
            return self.status(
                SourceState::Stale,
                "Source context changed; refresh explicitly",
            );
        }
        false
    }
    pub fn tick(&mut self, app: &Workbench, root: Option<&Path>, jobs: &mut JobSystem) -> bool {
        let mut changed = self.validate_context(app, root, jobs);
        if self.flight.is_some() {
            return changed;
        }
        let Some(r) = self.pending.as_ref() else {
            return changed;
        };
        let len = if r.mode == SourceMode::Completion {
            r.origin.cursor
        } else {
            app.document_by_id(r.origin.stamp.document)
                .map_or(0, |d| d.session.document().len())
        };
        if len > SNAPSHOT {
            self.pending = None;
            return self.status(SourceState::Limited, "Source service snapshot exceeds2MiB");
        }
        match jobs.submit_source(
            app,
            SOURCE_JOB_KEY,
            r.origin.stamp,
            len,
            r.provider.provider(r.mode).unwrap(),
        ) {
            Ok(id) => {
                let request = self.pending.take().unwrap();
                self.flight = Some(Flight {
                    id,
                    request,
                    discard: false,
                });
                changed |= self.status(
                    SourceState::Running,
                    "Reading bounded source snapshot; lexical evidence only",
                );
            }
            Err(SubmitError::Busy) => {
                changed |= self.status(
                    SourceState::Queued,
                    "Waiting for shared background capacity",
                )
            }
            Err(e) => {
                self.pending = None;
                changed |= self.status(
                    if e == SubmitError::SnapshotLimit {
                        SourceState::Limited
                    } else {
                        SourceState::Failed
                    },
                    format!("Source admission: {e:?}"),
                );
            }
        }
        changed
    }
    pub fn accept(
        &mut self,
        app: &Workbench,
        root: Option<&Path>,
        task: &mut CompletedTask,
    ) -> bool {
        if task.key != SOURCE_JOB_KEY {
            return false;
        }
        if self.flight.as_ref().is_none_or(|f| f.id != task.id) {
            return true;
        }
        let flight = self.flight.take().unwrap();
        let request = flight.request;
        if flight.discard || request.generation != self.generation {
            return true;
        }
        if !request.origin.current(app, root, request.mode)
            || !task.current
            || task.path != request.origin.path
        {
            self.status(SourceState::Stale, "Source result no longer current");
            return true;
        }
        if task.status != TaskStatus::Completed {
            self.status(
                if task.status == TaskStatus::LimitExceeded {
                    SourceState::Limited
                } else {
                    SourceState::Failed
                },
                format!("Source task: {:?}", task.status),
            );
            return true;
        }
        if !task.applicable {
            self.status(SourceState::Stale, "Source delivery not applicable");
            return true;
        }
        let tab = app
            .document_by_id(request.origin.stamp.document)
            .unwrap()
            .tab_id;
        let mut items = Vec::new();
        for item in task.output.take_items() {
            let TaskItem::Source(item) = item else {
                self.status(
                    SourceState::Failed,
                    "Provider returned a foreign result kind",
                );
                return true;
            };
            if !item.valid_for(request.mode)
                || request.mode == SourceMode::Completion
                    && (item.range.end != request.origin.cursor || item.range.len() > 512)
                || app.position_for_tab(tab, item.range.start).is_none()
                || app.position_for_tab(tab, item.range.end).is_none()
            {
                self.status(
                    SourceState::Failed,
                    "Provider returned invalid source boundaries/action",
                );
                return true;
            }
            items.push(item);
        }
        self.items = items;
        self.selected = 0;
        let provider = request.provider.id;
        self.accepted = Some(request);
        self.status(
            SourceState::Ready,
            format!(
                "{provider}: {} lexical results (not compiler verified)",
                self.items.len()
            ),
        );
        true
    }
    pub fn activate(
        &mut self,
        app: &mut Workbench,
        root: Option<&Path>,
        to_side: bool,
    ) -> Result<Option<Vec<PathBuf>>, String> {
        let request = self.accepted.as_ref().ok_or("No current source result")?;
        if !request.origin.current(app, root, request.mode) {
            return Err("Source result changed; refresh".into());
        }
        let item = self
            .items
            .get(self.selected)
            .ok_or("No source item selected")?;
        match &item.action {
            SourceAction::Unresolved(reason) => {
                Err(format!("Unresolved lexical reference: {reason}"))
            }
            SourceAction::Navigate => {
                let tab = app
                    .document_by_id(request.origin.stamp.document)
                    .unwrap()
                    .tab_id;
                if !app.jump_to_pane_location(request.origin.pane, tab, item.range.start) {
                    return Err("Invalid source destination".into());
                }
                Ok(None)
            }
            SourceAction::Replace(text) => {
                app.apply_text_edits(
                    request.origin.stamp.document,
                    request.origin.stamp.revision,
                    vec![crate::TextReplacement {
                        range: item.range.clone(),
                        text: text.clone(),
                    }],
                )
                .map_err(|e| e.to_string())?;
                Ok(None)
            }
            SourceAction::RelativeFile(relative) => {
                let root = root.ok_or("Open a project folder to resolve references")?;
                let paths = reference_candidates(root, &request.origin.path, relative)?;
                if paths.is_empty() {
                    return Err("No existing candidate inside the project root".into());
                }
                if paths.len() > 1 {
                    return Ok(Some(paths));
                }
                app.open_project_file(root, paths[0].clone(), None, to_side, SNAPSHOT)
                    .map_err(|e| e.to_string())?;
                Ok(None)
            }
        }
    }
    pub fn activate_candidate(
        &mut self,
        app: &mut Workbench,
        root: Option<&Path>,
        candidate: &Path,
        to_side: bool,
    ) -> Result<(), String> {
        let request = self.accepted.as_ref().ok_or("No current source result")?;
        if !request.origin.current(app, root, request.mode) {
            return Err("Source result changed; refresh".into());
        }
        let item = self
            .items
            .get(self.selected)
            .ok_or("No source item selected")?;
        let SourceAction::RelativeFile(relative) = &item.action else {
            return Err("Not a file reference".into());
        };
        let root = root.ok_or("Open a project folder to resolve references")?;
        if !reference_candidates(root, &request.origin.path, relative)?
            .iter()
            .any(|path| path == candidate)
        {
            return Err("Reference candidate changed".into());
        }
        app.open_project_file(root, candidate.to_owned(), None, to_side, SNAPSHOT)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
pub fn reference_candidates(
    root: &Path,
    source: &Path,
    relative: &str,
) -> Result<Vec<PathBuf>, String> {
    let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
    let root = canonical_root.as_path();
    let canonical_parent = source.parent().and_then(|p| p.canonicalize().ok());
    let path = Path::new(relative);
    if path.is_absolute() || relative.contains(':') || relative.contains('\0') {
        return Err("Reference must be relative to the project".into());
    }
    let mut candidates = Vec::new();
    for parent in [canonical_parent.as_deref().unwrap_or(root), root] {
        let joined = parent.join(path);
        let mut normalized = PathBuf::new();
        for component in joined.components() {
            match component {
                std::path::Component::ParentDir => {
                    normalized.pop();
                }
                std::path::Component::CurDir => {}
                c => normalized.push(c.as_os_str()),
            }
        }
        if normalized.starts_with(root)
            && normalized.is_file()
            && normalized.canonicalize().is_ok_and(|p| p.starts_with(root))
            && !candidates.contains(&normalized)
        {
            candidates.push(normalized);
        }
    }
    Ok(candidates)
}

// A universal textual fallback, independent of language grammars and services.
fn local_words(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    out: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    use jc_text::{is_search_word, DecodedScalars};
    let bytes = snapshot.bytes();
    let mut tail = bytes.len().saturating_sub(512);
    if snapshot.stamp.encoding == jc_text::TextEncoding::Utf8 {
        while tail < bytes.len() && bytes[tail] & 0xc0 == 0x80 {
            tail += 1;
        }
    }
    let mut prefix = String::new();
    let mut start = tail;
    let mut overflow = false;
    for s in DecodedScalars::new(&bytes[tail..], snapshot.stamp.encoding, tail) {
        if s.valid && is_search_word(s.character) {
            if prefix.is_empty() && !overflow {
                start = s.source_range.start;
            }
            if prefix.len() + s.character.len_utf8() <= 256 && !overflow {
                prefix.push(s.character);
            } else {
                prefix.clear();
                overflow = true;
            }
        } else {
            prefix.clear();
            overflow = false;
            start = s.source_range.end;
        }
    }
    if prefix.is_empty() || overflow {
        return Ok(());
    }
    let range = start..bytes.len();
    let mut word = String::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut count = 0;
    let emit = |word: &mut String,
                seen: &mut std::collections::BTreeSet<String>,
                out: &mut TaskOutput|
     -> Result<(), TaskFailure> {
        if word.starts_with(&prefix) && *word != prefix && seen.insert(word.clone()) {
            out.push(TaskItem::Source(SourceItem {
                range: range.clone(),
                label: format!("{} (local word)", word),
                action: SourceAction::Replace(word.clone()),
            }))?;
        }
        word.clear();
        Ok(())
    };
    overflow = false;
    for s in DecodedScalars::new(bytes, snapshot.stamp.encoding, 0) {
        count += 1;
        if count % 128 == 0 && cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        if s.valid && is_search_word(s.character) {
            if word.len() + s.character.len_utf8() <= 256 && !overflow {
                word.push(s.character);
            } else {
                word.clear();
                overflow = true;
            }
        } else {
            if !overflow {
                emit(&mut word, &mut seen, out)?;
            }
            word.clear();
            overflow = false;
        }
    }
    if !overflow {
        emit(&mut word, &mut seen, out)?;
    }
    Ok(())
}
