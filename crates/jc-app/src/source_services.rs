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
const OUTLINE_JOB_KEY: JobKey = JobKey(0x4a43_5352_4353_0002);
const SNAPSHOT: usize = 2 * 1024 * 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceMode {
    Symbols,
    References,
    Calls,
    Dependencies,
    Completion,
}
impl SourceMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Symbols => "Símbolos",
            Self::References => "Referencias",
            Self::Calls => "Llama a",
            Self::Dependencies => "Dependencias",
            Self::Completion => "Completado",
        }
    }
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
                (
                    SourceAction::Navigate,
                    SourceMode::Symbols
                    | SourceMode::References
                    | SourceMode::Calls
                    | SourceMode::Dependencies,
                ) => true,
                (
                    SourceAction::RelativeFile(path),
                    SourceMode::References | SourceMode::Calls | SourceMode::Dependencies,
                ) => path.len() <= 4096 && !path.is_empty() && !path.chars().any(char::is_control),
                (
                    SourceAction::Unresolved(reason),
                    SourceMode::References | SourceMode::Calls | SourceMode::Dependencies,
                ) => reason.len() <= 512,
                (SourceAction::Replace(text), SourceMode::Completion) => text.len() <= 4096,
                _ => false,
            }
    }
}
#[derive(Clone, Copy)]
pub enum FileResolution {
    ProjectCandidates,
    DocumentRelative,
    OrderedProjectPath,
}

#[derive(Clone, Copy)]
pub struct SourceProviderDescriptor {
    pub id: &'static str,
    pub language: &'static str,
    pub symbols: Option<TaskProvider>,
    pub references: Option<TaskProvider>,
    pub calls: Option<TaskProvider>,
    pub dependencies: Option<TaskProvider>,
    pub file_resolution: FileResolution,
    pub completion: Option<TaskProvider>,
    pub snippets: &'static [crate::snippets::SnippetTemplate],
}
impl SourceProviderDescriptor {
    fn provider(self, mode: SourceMode) -> Option<TaskProvider> {
        match mode {
            SourceMode::Symbols => self.symbols,
            SourceMode::References => self.references,
            SourceMode::Calls => self.calls,
            SourceMode::Dependencies => self.dependencies,
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
            providers: vec![
                crate::document_links::descriptor(),
                crate::javascript_navigation::descriptor(),
                SourceProviderDescriptor {
                    id: "core.words",
                    language: "*",
                    symbols: None,
                    references: None,
                    calls: None,
                    dependencies: None,
                    file_resolution: FileResolution::ProjectCandidates,
                    completion: Some(local_words),
                    snippets: &[],
                },
            ],
        }
    }
}
impl SourceRegistry {
    pub fn supports_document(&self, app: &Workbench, mode: SourceMode) -> bool {
        app.active_document().is_some_and(|document| {
            let path = document.session.path().to_string_lossy();
            self.resolve(jc_languages::detect(&path).id, mode).is_some()
        })
    }
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
                descriptor.calls,
                descriptor.dependencies,
                descriptor.completion,
            ]
            .iter()
            .all(Option::is_none)
                && descriptor.snippets.is_empty()
            || self
                .providers
                .iter()
                .any(|p| p.id == descriptor.id || p.language == descriptor.language)
        {
            return Err("Proveedor de fuente duplicado/inválido o cuota del registro");
        }
        if descriptor.snippets.len() > 16 {
            return Err("El catálogo de plantillas excede16 entradas");
        }
        for (index, template) in descriptor.snippets.iter().enumerate() {
            template
                .validate(descriptor.id)
                .map_err(|_| "Metadatos de plantilla inválidos")?;
            if descriptor.snippets[..index]
                .iter()
                .any(|other| other.id == template.id)
                || self
                    .providers
                    .iter()
                    .flat_map(|p| p.snippets)
                    .any(|other| other.id == template.id)
            {
                return Err("Identificador de plantilla duplicado");
            }
        }
        self.providers.push(descriptor);
        Ok(())
    }
    pub fn snippets(&self, language: &str) -> Vec<&'static crate::snippets::SnippetTemplate> {
        self.providers
            .iter()
            .find(|p| p.language == language && !p.snippets.is_empty())
            .or_else(|| {
                self.providers
                    .iter()
                    .find(|p| p.language == "*" && !p.snippets.is_empty())
            })
            .map_or_else(Vec::new, |p| p.snippets.iter().collect())
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
impl SourceState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Inactivo",
            Self::Queued => "En cola",
            Self::Running => "En curso",
            Self::Ready => "Listo",
            Self::Stale => "Obsoleto",
            Self::Cancelled => "Cancelado",
            Self::Limited => "Limitado",
            Self::Failed => "Fallido",
        }
    }
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
        let doc = app.active_document().ok_or("No hay documento fuente")?;
        let pane = app.panes().active();
        let view = app.panes().view(pane).ok_or("No hay vista de fuente")?;
        let editor = app.active_editor().ok_or("No hay editor de fuente")?;
        let encoding = doc
            .session
            .encoding()
            .ok_or("Elige primero la codificación del fuente")?;
        if doc.session.path().as_os_str().len() > 4096
            || root.is_some_and(|r| r.as_os_str().len() > 4096)
        {
            return Err("Cuota de metadatos de fuente".into());
        }
        Ok(Self {
            stamp: TaskStamp {
                document: doc.id,
                revision: doc.session.document().revision(),
                encoding,
                view: Some(view.view_id().ok_or("No hay ViewId de fuente")?),
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
    outline: bool,
    outline_attempt: Option<Origin>,
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
/// Locale-facing labels retain complete printable graphemes. Source/path
/// diagnostics keep using `display_label` and their established ASCII notation.
/// Control and bidi formatting cannot become terminal/native UI instructions.
pub fn display_ui_label(text: &str, columns: usize) -> String {
    use std::borrow::Cow;
    let mut out = String::new();
    let mut used = 0usize;
    for cluster in jc_text::grapheme::graphemes(text) {
        if cluster.len() > jc_text::grapheme::MAX_CLUSTER_BYTES {
            break;
        }
        let width = jc_text::grapheme::width(cluster);
        let unsafe_text = width == 0 || cluster.chars().any(|ch| {
            ch.is_control() || matches!(ch, '\u{61c}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}')
        });
        let token = if unsafe_text {
            Cow::Owned(
                cluster
                    .chars()
                    .flat_map(char::escape_default)
                    .collect::<String>(),
            )
        } else {
            Cow::Borrowed(cluster)
        };
        let width = if unsafe_text { token.len() } else { width };
        if used.saturating_add(width) > columns.min(256)
            || out.len().saturating_add(token.len()) > 2048
        {
            break;
        }
        out.push_str(&token);
        used += width;
    }
    out
}
impl SourceController {
    pub fn outline() -> Self {
        Self {
            outline: true,
            ..Self::default()
        }
    }
    fn job_key(&self) -> JobKey {
        if self.outline {
            OUTLINE_JOB_KEY
        } else {
            SOURCE_JOB_KEY
        }
    }
    /// A failed attempt is remembered until its source context changes.
    /// This checks only metadata; actual capture belongs in the task path.
    pub fn needs_outline_refresh(&self, app: &Workbench, root: Option<&Path>) -> bool {
        self.outline
            && !self
                .outline_attempt
                .as_ref()
                .is_some_and(|origin| origin.current(app, root, SourceMode::Symbols))
            && app.active_document().is_some_and(|document| {
                document.session.encoding().is_some()
                    && document.session.path().as_os_str().len() <= 4096
            })
            && root.is_none_or(|path| path.as_os_str().len() <= 4096)
            && app
                .panes()
                .view(app.panes().active())
                .and_then(|view| view.view_id())
                .is_some()
    }
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
    pub fn side_ready(&self, app: &Workbench, root: Option<&Path>) -> bool {
        self.ready(app, root)
            && self
                .accepted
                .as_ref()
                .is_some_and(|request| reference_root(root, request).is_ok())
            && self
                .items
                .get(self.selected)
                .is_some_and(|item| matches!(item.action, SourceAction::RelativeFile(_)))
    }
    pub fn present(&self, max_rows: usize, columns: usize) -> SourcePresentation {
        let count = max_rows.min(64);
        let first = self.selected.saturating_sub(count.saturating_sub(1));
        SourcePresentation {
            title: display_ui_label(
                match self.mode {
                    Some(SourceMode::Symbols) => "Símbolos (léxicos)",
                    Some(SourceMode::References) => "Referencias (léxicas)",
                    Some(SourceMode::Calls) => "Llama a",
                    Some(SourceMode::Dependencies) => "Dependencias",
                    _ => "Completado (palabras locales)",
                },
                columns,
            ),
            message: display_ui_label(&self.message, columns),
            rows: self
                .items
                .iter()
                .enumerate()
                .skip(first)
                .take(count)
                .map(|(index, item)| SourceRow {
                    index,
                    label: display_ui_label(&item.label, columns),
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
        let key = self.job_key();
        if let Some(f) = &mut self.flight {
            f.discard = true;
            jobs.cancel(key);
        }
        self.pending = None;
        self.accepted = None;
        self.items.clear();
        self.selected = 0;
    }
    pub fn cancel(&mut self, jobs: &mut JobSystem) {
        self.invalidate(jobs);
        self.outline_attempt = None;
        self.status(SourceState::Cancelled, "Servicio de código cancelado");
    }
    pub fn request(
        &mut self,
        mode: SourceMode,
        registry: &SourceRegistry,
        app: &Workbench,
        root: Option<&Path>,
        jobs: &mut JobSystem,
    ) -> Result<(), String> {
        if self.outline && mode != SourceMode::Symbols {
            return Err("El esquema solo admite símbolos".into());
        }
        if self.generation == u64::MAX {
            return Err("Se agotó la generación de código".into());
        }
        self.invalidate(jobs);
        self.mode = Some(mode);
        self.status(SourceState::Failed, "Solicitud de código no disponible");
        if mode == SourceMode::Completion
            && (app.active_editor().is_some_and(|e| e.selection().is_some())
                || app.rectangle_for_pane(app.panes().active()).is_some())
        {
            return Err("El completado requiere cursor de texto sin selección".into());
        }
        let origin = Origin::capture(app, root)?;
        if self.outline {
            self.outline_attempt = Some(origin.clone());
        }
        let provider = registry
            .resolve(&origin.language, mode)
            .ok_or("No hay proveedor para esta capacidad de código")?;
        self.pending = Some(Request {
            generation: self.generation,
            origin,
            mode,
            provider,
        });
        self.status(SourceState::Queued, "Capacidad de código en cola");
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
                "Cambió el contexto del código; actualiza explícitamente",
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
            return self.status(
                SourceState::Limited,
                "La captura del servicio de código excede2MiB",
            );
        }
        match jobs.submit_source(
            app,
            self.job_key(),
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
                    "Leyendo captura acotada del fuente; solo evidencia léxica",
                );
            }
            Err(SubmitError::Busy) => {
                changed |= self.status(
                    SourceState::Queued,
                    "Esperando capacidad de fondo compartida",
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
                    format!("Admisión de código: {e}"),
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
        if task.key != self.job_key() {
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
            self.status(
                SourceState::Stale,
                "El resultado del código ya no está vigente",
            );
            return true;
        }
        if task.status != TaskStatus::Completed {
            self.status(
                if task.status == TaskStatus::LimitExceeded {
                    SourceState::Limited
                } else {
                    SourceState::Failed
                },
                format!("Tarea de código: {}", task.status),
            );
            return true;
        }
        if !task.applicable {
            self.status(SourceState::Stale, "La entrega del código no es aplicable");
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
                    "El proveedor devolvió un tipo de resultado ajeno",
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
                    "El proveedor devolvió fronteras/acción inválidas",
                );
                return true;
            }
            items.push(item);
        }
        self.selected = if request.provider.id == crate::document_links::ID {
            items
                .iter()
                .position(|item| item.range.contains(&request.origin.cursor))
                .unwrap_or(0)
        } else {
            0
        };
        self.items = items;
        let provider = request.provider.id;
        self.accepted = Some(request);
        self.status(
            SourceState::Ready,
            format!(
                "{provider}: {} resultados léxicos (sin verificación del compilador)",
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
        self.activate_at_index(app, root, self.selected, to_side)
    }
    pub fn activate_at_index(
        &self,
        app: &mut Workbench,
        root: Option<&Path>,
        index: usize,
        to_side: bool,
    ) -> Result<Option<Vec<PathBuf>>, String> {
        let request = self
            .accepted
            .as_ref()
            .ok_or("No hay resultado de código vigente")?;
        if !request.origin.current(app, root, request.mode) {
            return Err("Cambió el resultado del código; actualiza".into());
        }
        let item = self
            .items
            .get(index)
            .ok_or("No hay elemento de código seleccionado")?;
        match &item.action {
            SourceAction::Unresolved(reason) => {
                Err(format!("Referencia léxica sin resolver: {reason}"))
            }
            SourceAction::Navigate => {
                let tab = app
                    .document_by_id(request.origin.stamp.document)
                    .unwrap()
                    .tab_id;
                if !app.jump_to_pane_location(request.origin.pane, tab, item.range.start) {
                    return Err("Destino de código inválido".into());
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
                let root = reference_root(root, request)?;
                let paths = resolve_file(root, request, relative)?;
                if paths.is_empty() {
                    return Err("No hay candidato existente dentro de la raíz del proyecto".into());
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
        let request = self
            .accepted
            .as_ref()
            .ok_or("No hay resultado de código vigente")?;
        if !request.origin.current(app, root, request.mode) {
            return Err("Cambió el resultado del código; actualiza".into());
        }
        let item = self
            .items
            .get(self.selected)
            .ok_or("No hay elemento de código seleccionado")?;
        let SourceAction::RelativeFile(relative) = &item.action else {
            return Err("No es una referencia de archivo".into());
        };
        let root = reference_root(root, request)?;
        if !resolve_file(root, request, relative)?
            .iter()
            .any(|path| path == candidate)
        {
            return Err("Cambió el candidato de referencia".into());
        }
        app.open_project_file(root, candidate.to_owned(), None, to_side, SNAPSHOT)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
fn reference_root<'a>(root: Option<&'a Path>, request: &'a Request) -> Result<&'a Path, String> {
    root.or_else(|| {
        if matches!(
            request.provider.file_resolution,
            FileResolution::DocumentRelative
        ) && request.origin.path.is_absolute()
        {
            request.origin.path.parent()
        } else {
            None
        }
    })
    .ok_or_else(|| "Abre una carpeta de proyecto para resolver referencias".into())
}

fn resolve_file(root: &Path, request: &Request, relative: &str) -> Result<Vec<PathBuf>, String> {
    let source = &request.origin.path;
    match request.provider.file_resolution {
        FileResolution::ProjectCandidates => {
            find_reference_candidates(root, source, relative, true)
        }
        FileResolution::DocumentRelative => {
            find_reference_candidates(root, source, relative, false)
        }
        FileResolution::OrderedProjectPath => {
            let root = root.canonicalize().map_err(|error| error.to_string())?;
            let config = root.join(".cide").join("propath");
            match jc_files::read_in_root(&root, &config, 8192) {
                Ok((_, bytes)) => {
                    let config =
                        std::str::from_utf8(&bytes).map_err(|_| "PROPATH debe estar en UTF-8")?;
                    let mut parents = Vec::new();
                    for entry in config.trim_start_matches('\u{feff}').lines().map(str::trim) {
                        if entry.is_empty() || entry.starts_with('#') {
                            continue;
                        }
                        if parents.len() == 32
                            || !project_relative(Path::new(entry))
                            || entry.chars().any(char::is_control)
                        {
                            return Err(
                                "PROPATH admite hasta 32 directorios relativos al proyecto".into(),
                            );
                        }
                        parents.push(root.join(entry));
                    }
                    let mut paths = candidates_from(
                        &root,
                        relative,
                        parents.iter().map(PathBuf::as_path),
                        true,
                    )?;
                    paths.truncate(1);
                    Ok(paths)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let mut paths = find_reference_candidates(&root, source, relative, true)?;
                    if request.mode != SourceMode::References {
                        paths.truncate(1);
                    }
                    Ok(paths)
                }
                Err(error) => Err(format!("No se pudo leer PROPATH: {error}")),
            }
        }
    }
}

fn project_relative(path: &Path) -> bool {
    path.components().all(|part| {
        matches!(
            part,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    })
}

pub fn reference_candidates(
    root: &Path,
    source: &Path,
    relative: &str,
) -> Result<Vec<PathBuf>, String> {
    find_reference_candidates(root, source, relative, true)
}

fn find_reference_candidates(
    root: &Path,
    source: &Path,
    relative: &str,
    project_fallback: bool,
) -> Result<Vec<PathBuf>, String> {
    let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
    let root = canonical_root.as_path();
    let canonical_parent = source.parent().and_then(|p| p.canonicalize().ok());
    let parents = [
        canonical_parent.as_deref(),
        project_fallback.then_some(root),
    ];
    candidates_from(root, relative, parents.into_iter().flatten(), false)
}

fn candidates_from<'a>(
    root: &Path,
    relative: &str,
    parents: impl Iterator<Item = &'a Path>,
    first_only: bool,
) -> Result<Vec<PathBuf>, String> {
    let path = Path::new(relative);
    if path.has_root() || relative.contains(':') || relative.contains('\0') {
        return Err("La referencia debe ser relativa al proyecto".into());
    }
    let mut candidates = Vec::new();
    for parent in parents {
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
            if first_only {
                break;
            }
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
                label: format!("{} (palabra local)", word),
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
