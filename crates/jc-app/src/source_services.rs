//! Optional, namespaced internal source capabilities. No language-specific rules
//! or external runtime live in this host. All mutations return through Workbench.
use crate::{
    Cancellation, CompletedTask, JobId, JobKey, JobSystem, PaneId, SubmitError, TaskFailure,
    TaskItem, TaskOutput, TaskProvider, TaskSnapshot, TaskStamp, TaskStatus, Workbench,
};
use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const SOURCE_JOB_KEY: JobKey = JobKey(0x4a43_5352_4353_0001);
mod incoming;
pub(crate) use incoming::IncomingScan;
const OUTLINE_JOB_KEY: JobKey = JobKey(0x4a43_5352_4353_0002);
const SNAPSHOT: usize = 2 * 1024 * 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceMode {
    Symbols,
    References,
    Calls,
    Incoming,
    Dependencies,
    Completion,
    CatalogCompletion,
    CatalogDefinition,
    CatalogReferences,
    CatalogXref,
    ReviewEdit(ReviewedEdit),
    Rename,
    Definition,
    LspReferences,
    LspCompletion,
    LspDiagnostics,
}
impl SourceMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Symbols => "Símbolos",
            Self::References => "Referencias",
            Self::Calls => "Llama a",
            Self::Incoming => "Llamado por",
            Self::Dependencies => "Dependencias",
            Self::Completion => "Completado",
            Self::CatalogCompletion => "Completar desde esquema ABL",
            Self::CatalogDefinition => "Ir a definición ABL",
            Self::CatalogReferences => "Buscar referencias ABL",
            Self::CatalogXref => "Buscar referencias con XREF",
            Self::ReviewEdit(action) => action.label(),
            Self::Rename => "Renombrar símbolo ABL",
            Self::Definition => "Ir a definición (LSP)",
            Self::LspReferences => "Buscar referencias (LSP)",
            Self::LspCompletion => "Autocompletado semántico (LSP)",
            Self::LspDiagnostics => "Comprobar con LSP",
        }
    }

    pub fn is_catalog(self) -> bool {
        matches!(
            self,
            Self::CatalogCompletion
                | Self::CatalogDefinition
                | Self::CatalogReferences
                | Self::CatalogXref
                | Self::ReviewEdit(_)
        )
    }

    fn lsp_query(self) -> Option<crate::lsp::Query> {
        match self {
            Self::Definition => Some(crate::lsp::Query::Definition),
            Self::LspReferences => Some(crate::lsp::Query::References),
            Self::LspCompletion => Some(crate::lsp::Query::Completion),
            Self::LspDiagnostics => Some(crate::lsp::Query::Diagnostics),
            _ => None,
        }
    }

    pub fn is_lsp(self) -> bool {
        self.lsp_query().is_some()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewedEdit {
    OrganizeImports,
    UpdatePrototype,
    GenerateConstructors,
    GenerateOverrides,
}

impl ReviewedEdit {
    pub fn from_command(command: crate::CommandId) -> Option<Self> {
        use crate::CommandId;
        Some(match command {
            CommandId::SourceOrganizeImports => Self::OrganizeImports,
            CommandId::SourceUpdatePrototype => Self::UpdatePrototype,
            CommandId::SourceGenerateConstructors => Self::GenerateConstructors,
            CommandId::SourceGenerateOverrides => Self::GenerateOverrides,
            _ => return None,
        })
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::OrganizeImports => "Organizar USING de ABL",
            Self::UpdatePrototype => "Actualizar prototipo FORWARD",
            Self::GenerateConstructors => "Generar constructores de la clase base",
            Self::GenerateOverrides => "Generar implementaciones y sobrescrituras",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceAction {
    Navigate,
    ScopedSymbol {
        extent: Range<usize>,
    },
    RelativeFile(String),
    Unresolved(String),
    Replace(String),
    ProjectRename(ProjectRenameEdit, String),
    ReviewLine,
    LspCompletion(Vec<crate::TextReplacement>),
    Caller {
        path: PathBuf,
        offset: usize,
        relative: String,
        encoding: jc_text::TextEncoding,
        version: jc_files::ContentFingerprint,
    },
    ProjectLocation {
        path: PathBuf,
        offset: usize,
        encoding: jc_text::TextEncoding,
        version: jc_files::ContentFingerprint,
    },
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
                SourceAction::Navigate
                | SourceAction::ScopedSymbol { .. }
                | SourceAction::ReviewLine => 0,
                SourceAction::ProjectLocation { path, .. } => path.capacity(),
                SourceAction::ProjectRename(edit, name) => {
                    edit.path.capacity() + edit.before.capacity() + name.capacity()
                }
                SourceAction::LspCompletion(edits) => {
                    edits.capacity() * std::mem::size_of::<crate::TextReplacement>()
                        + edits.iter().map(|edit| edit.text.capacity()).sum::<usize>()
                }
                SourceAction::RelativeFile(s)
                | SourceAction::Unresolved(s)
                | SourceAction::Replace(s) => s.capacity(),
                SourceAction::Caller { path, relative, .. } => {
                    path.capacity() + relative.capacity()
                }
            }
    }
    fn valid_for(&self, mode: SourceMode) -> bool {
        self.label.len() <= 512
            && !self.label.chars().any(char::is_control)
            && match (&self.action, mode) {
                (SourceAction::ScopedSymbol { extent }, SourceMode::Symbols) => {
                    !extent.is_empty() && extent.contains(&self.range.start)
                }
                (
                    SourceAction::Navigate,
                    SourceMode::Symbols
                    | SourceMode::References
                    | SourceMode::Calls
                    | SourceMode::Dependencies
                    | SourceMode::Definition
                    | SourceMode::CatalogDefinition
                    | SourceMode::CatalogReferences
                    | SourceMode::CatalogXref
                    | SourceMode::LspReferences,
                ) => true,
                (
                    SourceAction::RelativeFile(path),
                    SourceMode::References | SourceMode::Calls | SourceMode::Dependencies,
                ) => path.len() <= 4096 && !path.is_empty() && !path.chars().any(char::is_control),
                (
                    SourceAction::Unresolved(reason),
                    SourceMode::References | SourceMode::Calls | SourceMode::Dependencies,
                ) => reason.len() <= 512,
                (
                    SourceAction::Replace(text),
                    SourceMode::Completion | SourceMode::CatalogCompletion,
                ) => text.len() <= 4096,
                (SourceAction::LspCompletion(edits), SourceMode::LspCompletion) => {
                    (1..=9).contains(&edits.len())
                }
                (SourceAction::Replace(text), SourceMode::Rename) => text.len() <= 64,
                (SourceAction::ProjectRename(edit, name), SourceMode::Rename) => {
                    edit.path.is_absolute()
                        && edit.path.as_os_str().len() <= 4096
                        && !edit.range.is_empty()
                        && edit.range.len() <= 64
                        && edit.before.len() <= 64
                        && name.len() <= 64
                }
                (SourceAction::Replace(text), SourceMode::ReviewEdit(_)) => text.len() <= 4096,
                (SourceAction::ReviewLine, SourceMode::ReviewEdit(_)) => true,
                (SourceAction::Caller { path, .. }, SourceMode::Incoming)
                | (
                    SourceAction::ProjectLocation { path, .. },
                    SourceMode::Symbols
                    | SourceMode::Definition
                    | SourceMode::LspReferences
                    | SourceMode::CatalogDefinition
                    | SourceMode::CatalogReferences
                    | SourceMode::CatalogXref,
                ) => path.is_absolute() && path.as_os_str().len() <= 4096,
                _ => false,
            }
    }
}
#[derive(Clone, Copy)]
pub enum FileResolution {
    ProjectCandidates,
    DocumentRelative,
    RelativeModule,
    ProjectImports,
    OrderedProjectPath,
}

pub type RenameProvider =
    fn(&[u8], jc_text::TextEncoding, usize, &str) -> Result<Vec<Range<usize>>, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRenameEdit {
    pub path: PathBuf,
    pub version: jc_files::ContentFingerprint,
    pub encoding: jc_text::TextEncoding,
    pub range: Range<usize>,
    pub line: usize,
    pub before: String,
}

pub type ProjectRenameProvider = fn(
    &Path,
    &Path,
    &[u8],
    jc_text::TextEncoding,
    usize,
    &str,
    &Cancellation,
) -> Result<Option<Vec<ProjectRenameEdit>>, String>;

#[derive(Clone)]
struct RenameInput {
    name: String,
    project: Option<ProjectRenameProvider>,
    confirmed: Option<Vec<SourceItem>>,
}

fn project_buffers_clean(app: &Workbench, root: &Path) -> Result<(), String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let active = app.active_document().map(|document| document.id);
    for document in app.open_documents() {
        let path = document.session.path();
        let abl = jc_languages::detect(&path.to_string_lossy()).id == "abl"
            || path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("abl"));
        if Some(document.id) != active
            && abl
            && !document.session.is_new()
            && document.session.document().is_modified()
            && (path.starts_with(&root)
                || path
                    .canonicalize()
                    .is_ok_and(|path| path.starts_with(&root)))
        {
            return Err(format!(
                "Guarda los cambios ABL de {} antes de revisar el proyecto",
                path.display()
            ));
        }
    }
    Ok(())
}

pub(crate) struct ProjectRenameJob {
    provider: ProjectRenameProvider,
    root: PathBuf,
    path: PathBuf,
}

#[derive(Clone, Copy)]
pub enum CatalogQuery {
    Symbols,
    Complete,
    Definition { cursor: usize },
    References { cursor: usize },
    Xref { cursor: usize },
    ReviewEdit { action: ReviewedEdit, cursor: usize },
}

/// A project service answers against an immutable snapshot and project context.
pub trait CatalogSource: Send + Sync {
    fn is_current(&self) -> bool;
    fn query(
        &self,
        query: CatalogQuery,
        snapshot: &TaskSnapshot,
        cancel: &Cancellation,
        output: &mut TaskOutput,
    ) -> Result<(), TaskFailure>;
}

pub fn publish_review(
    snapshot: &TaskSnapshot,
    mut edits: Vec<crate::TextReplacement>,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    edits.sort_unstable_by_key(|edit| edit.range.start);
    for pair in edits.windows(2) {
        if pair[0].range.end > pair[1].range.start || pair[0].range.start == pair[1].range.start {
            return Err(TaskFailure::InvalidRange);
        }
    }
    if edits
        .iter()
        .any(|edit| edit.range.len() > 4096 || edit.text.len() > 4096)
    {
        return Err(TaskFailure::LimitExceeded);
    }
    if edits
        .iter()
        .any(|edit| jc_text::encode(&edit.text, snapshot.stamp.encoding).is_err())
    {
        return output.push(TaskItem::Message(
            "La propuesta contiene caracteres que no admite la codificación del documento".into(),
        ));
    }
    let mut previous = 0;
    let mut line = 1;
    for edit in edits {
        let before = snapshot
            .bytes()
            .get(edit.range.clone())
            .ok_or(TaskFailure::InvalidRange)?;
        let decoded = jc_text::decode_fragment(before, snapshot.stamp.encoding, edit.range.start);
        if !decoded.issues.is_empty() {
            return Err(TaskFailure::InvalidRange);
        }
        line += snapshot.bytes()[previous..edit.range.start]
            .iter()
            .filter(|&&byte| byte == b'\n')
            .count();
        previous = edit.range.start;
        for (sign, text) in [('-', decoded.text.as_str()), ('+', edit.text.as_str())] {
            let mut push_line = |chunk: &str| {
                output.push(TaskItem::Source(SourceItem {
                    range: edit.range.start..edit.range.start,
                    label: format!("{sign} {chunk}"),
                    action: SourceAction::ReviewLine,
                }))
            };
            for text_line in text.lines() {
                let mut chunk = String::new();
                for ch in text_line.chars() {
                    match ch {
                        '\t' => chunk.push_str("⇥ "),
                        ch if ch.is_control() => chunk.extend(ch.escape_default()),
                        ch => chunk.push(ch),
                    }
                    if chunk.len() >= 52 {
                        push_line(&chunk)?;
                        chunk.clear();
                    }
                }
                if !chunk.is_empty() || text_line.is_empty() {
                    push_line(&chunk)?;
                }
            }
        }
        output.push(TaskItem::Source(SourceItem {
            range: edit.range,
            label: format!("Línea {line} · cambio completo arriba"),
            action: SourceAction::Replace(edit.text),
        }))?;
    }
    Ok(())
}

pub(crate) enum SourceJob {
    Plain(TaskProvider),
    Catalog(Arc<dyn CatalogSource>, CatalogQuery),
    Lsp(crate::lsp::Work),
    Incoming(IncomingScan),
    Rename {
        provider: RenameProvider,
        cursor: usize,
        name: String,
        project: Option<ProjectRenameJob>,
    },
}

impl SourceJob {
    pub(crate) fn timeout(&self, default: std::time::Duration) -> std::time::Duration {
        match self {
            Self::Lsp(_) => crate::lsp::TIMEOUT,
            Self::Catalog(_, CatalogQuery::Xref { .. }) => {
                default.max(std::time::Duration::from_secs(45))
            }
            Self::Catalog(_, CatalogQuery::References { .. }) => {
                default.max(std::time::Duration::from_secs(10))
            }
            Self::Rename {
                project: Some(_), ..
            } => default.max(std::time::Duration::from_secs(30)),
            _ => default,
        }
    }

    pub(crate) fn run(
        &self,
        snapshot: &TaskSnapshot,
        cancel: &Cancellation,
        output: &mut TaskOutput,
    ) -> Result<(), TaskFailure> {
        match self {
            Self::Plain(provider) => provider(snapshot, cancel, output),
            Self::Catalog(provider, query) => provider.query(*query, snapshot, cancel, output),
            Self::Lsp(work) => work.run(snapshot, cancel, output),
            Self::Incoming(scan) => scan.run(snapshot.stamp, cancel, output),
            Self::Rename {
                provider,
                cursor,
                name,
                project,
            } => {
                if cancel.is_cancelled() {
                    return Err(TaskFailure::Cancelled);
                }
                if let Some(project) = project {
                    match (project.provider)(
                        &project.root,
                        &project.path,
                        snapshot.bytes(),
                        snapshot.stamp.encoding,
                        *cursor,
                        name,
                        cancel,
                    ) {
                        Ok(Some(mut edits)) => {
                            if edits.is_empty() || edits.len() > 128 {
                                return Err(TaskFailure::LimitExceeded);
                            }
                            edits.sort_unstable_by(|left, right| {
                                left.path
                                    .cmp(&right.path)
                                    .then(left.range.start.cmp(&right.range.start))
                            });
                            for edit in edits {
                                let display = display_ui_label(
                                    &edit
                                        .path
                                        .strip_prefix(&project.root)
                                        .unwrap_or(&edit.path)
                                        .to_string_lossy(),
                                    66,
                                );
                                let previous = display_ui_label(&edit.before, 32);
                                output.push(TaskItem::Source(SourceItem {
                                    range: 0..0,
                                    label: format!("{display}:{} · {previous} → {name}", edit.line),
                                    action: SourceAction::ProjectRename(edit, name.clone()),
                                }))?;
                            }
                            return Ok(());
                        }
                        Ok(None) => {}
                        Err(error) => return output.push(TaskItem::Message(error)),
                    }
                }
                let mut ranges =
                    match provider(snapshot.bytes(), snapshot.stamp.encoding, *cursor, name) {
                        Ok(ranges) => ranges,
                        Err(error) => return output.push(TaskItem::Message(error)),
                    };
                if ranges.len() > 64 {
                    return Err(TaskFailure::LimitExceeded);
                }
                ranges.sort_unstable_by_key(|range| range.start);
                let mut previous = 0;
                let mut line = 1;
                let mut line_start = 0;
                for range in ranges {
                    if cancel.is_cancelled() {
                        return Err(TaskFailure::Cancelled);
                    }
                    if range.is_empty() || range.start < previous {
                        return Err(TaskFailure::InvalidRange);
                    }
                    let bytes = snapshot.bytes();
                    let old = bytes.get(range.clone()).ok_or(TaskFailure::InvalidRange)?;
                    let old = jc_text::decode_fragment(old, snapshot.stamp.encoding, range.start);
                    if !old.issues.is_empty()
                        || old.text.len() > 128
                        || old.text.chars().any(char::is_control)
                    {
                        return Err(TaskFailure::InvalidRange);
                    }
                    for (delta, byte) in bytes[previous..range.start].iter().enumerate() {
                        let index = previous + delta;
                        if *byte == b'\n' || *byte == b'\r' && bytes.get(index + 1) != Some(&b'\n')
                        {
                            line += 1;
                            line_start = index + 1;
                        }
                    }
                    previous = range.end;
                    let column = range.start - line_start + 1;
                    output.push(TaskItem::Source(SourceItem {
                        range,
                        label: format!("Línea {line}, byte {column}  {} → {name}", old.text),
                        action: SourceAction::Replace(name.clone()),
                    }))?;
                }
                Ok(())
            }
        }
    }
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
    pub rename: Option<RenameProvider>,
    pub snippets: &'static [crate::snippets::SnippetTemplate],
}
impl SourceProviderDescriptor {
    fn provider(self, mode: SourceMode) -> Option<TaskProvider> {
        match mode {
            SourceMode::Symbols => self.symbols,
            SourceMode::References => self.references,
            SourceMode::Calls => self.calls,
            SourceMode::Incoming => self.dependencies.and(self.references),
            SourceMode::Dependencies => self.dependencies,
            SourceMode::Completion => self.completion,
            SourceMode::Rename
            | SourceMode::ReviewEdit(_)
            | SourceMode::CatalogCompletion
            | SourceMode::CatalogDefinition
            | SourceMode::CatalogReferences
            | SourceMode::CatalogXref
            | SourceMode::Definition
            | SourceMode::LspReferences
            | SourceMode::LspCompletion
            | SourceMode::LspDiagnostics => None,
        }
    }
    fn supports(self, mode: SourceMode) -> bool {
        if mode == SourceMode::Rename {
            self.rename.is_some()
        } else {
            self.provider(mode).is_some()
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
                crate::javascript_navigation::descriptor(false),
                crate::javascript_navigation::descriptor(true),
                SourceProviderDescriptor {
                    id: "core.words",
                    language: "*",
                    symbols: None,
                    references: None,
                    calls: None,
                    dependencies: None,
                    file_resolution: FileResolution::ProjectCandidates,
                    completion: Some(local_words),
                    rename: None,
                    snippets: &[],
                },
            ]
            .into_iter()
            .chain(crate::code_navigation::descriptors())
            .collect(),
        }
    }
}
impl SourceRegistry {
    pub fn supports_lsp(&self, app: &Workbench, root: Option<&Path>) -> bool {
        root.is_some()
            && app.active_document().is_some_and(|doc| {
                let path = doc.session.path();
                !doc.session.is_new()
                    && doc.session.encoding().is_some()
                    && crate::lsp::language_id(
                        jc_languages::detect(&path.to_string_lossy()).id,
                        path,
                    )
                    .is_some()
            })
    }

    pub fn supports_incoming(&self, app: &Workbench, root: Option<&Path>) -> bool {
        root.is_some()
            && app
                .active_document()
                .is_some_and(|doc| !doc.session.is_new() && doc.session.encoding().is_some())
            && self.supports_document(app, SourceMode::Incoming)
    }
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
                && descriptor.rename.is_none()
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
            .find(|p| p.language == language && p.supports(mode))
            .or_else(|| {
                self.providers
                    .iter()
                    .find(|p| p.language == "*" && p.supports(mode))
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
                (!matches!(mode, SourceMode::Completion | SourceMode::Rename)
                    && !mode.is_catalog()
                    && (!mode.is_lsp() || mode == SourceMode::LspDiagnostics))
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
    provider: Option<SourceProviderDescriptor>,
    input: SourceInput,
}
#[derive(Clone)]
enum SourceInput {
    None,
    Rename(RenameInput),
    Catalog(Arc<dyn CatalogSource>),
}

impl Request {
    fn current(&self, app: &Workbench, root: Option<&Path>) -> bool {
        self.origin.current(app, root, self.mode)
            && match &self.input {
                SourceInput::Catalog(provider) => provider.is_current(),
                _ => true,
            }
    }
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
    lsp: crate::lsp::Client,
    project_rename_applied: bool,
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
    /// Outline results are revision-pinned; cursor movement only selects
    /// from the admitted extents and never restarts the provider.
    pub fn symbol_path(
        &self,
        app: &Workbench,
        root: Option<&Path>,
    ) -> impl DoubleEndedIterator<Item = (usize, &str)> + '_ {
        let cursor = (self.mode == Some(SourceMode::Symbols) && self.ready(app, root))
            .then(|| app.active_editor().map(|editor| editor.cursor))
            .flatten();
        self.items
            .iter()
            .enumerate()
            .filter_map(move |(index, item)| {
                let SourceAction::ScopedSymbol { extent } = &item.action else {
                    return None;
                };
                cursor.filter(|cursor| extent.contains(cursor)).map(|_| {
                    let name = item
                        .label
                        .rsplit_once(" · ")
                        .map_or(item.label.as_str(), |(name, _)| name);
                    (index, name)
                })
            })
    }

    pub fn active_symbol(&self, app: &Workbench, root: Option<&Path>) -> Option<(usize, &str)> {
        self.symbol_path(app, root).next_back()
    }
    pub fn ready(&self, app: &Workbench, root: Option<&Path>) -> bool {
        self.state == SourceState::Ready
            && !self.items.is_empty()
            && self.accepted.as_ref().is_some_and(|r| r.current(app, root))
    }
    pub fn side_ready(&self, app: &Workbench, root: Option<&Path>) -> bool {
        self.ready(app, root)
            && self
                .items
                .get(self.selected)
                .is_some_and(|item| match item.action {
                    SourceAction::Navigate | SourceAction::ScopedSymbol { .. } => true,
                    SourceAction::ProjectLocation { .. } => root.is_some(),
                    SourceAction::RelativeFile(_) | SourceAction::Caller { .. } => self
                        .accepted
                        .as_ref()
                        .is_some_and(|request| reference_root(root, request).is_ok()),
                    _ => false,
                })
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
                    Some(SourceMode::Incoming) => "Llamado por (fuentes en disco)",
                    Some(SourceMode::Rename) => "Vista previa del renombrado",
                    Some(SourceMode::Dependencies) => "Dependencias",
                    Some(mode) if mode.is_lsp() || mode.is_catalog() => mode.label(),
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
        self.request_named(mode, SourceInput::None, registry, app, root, jobs)
    }
    pub fn request_rename(
        &mut self,
        name: &str,
        registry: &SourceRegistry,
        app: &Workbench,
        root: Option<&Path>,
        jobs: &mut JobSystem,
    ) -> Result<(), String> {
        self.request_rename_with_project(name, None, registry, app, root, jobs)
    }

    pub fn request_rename_with_project(
        &mut self,
        name: &str,
        project: Option<ProjectRenameProvider>,
        registry: &SourceRegistry,
        app: &Workbench,
        root: Option<&Path>,
        jobs: &mut JobSystem,
    ) -> Result<(), String> {
        if name.is_empty() || name.len() > 64 || name.chars().any(char::is_control) {
            return Err("Escribe un identificador de hasta 64 bytes".into());
        }
        let project = project.filter(|_| {
            root.is_some()
                && app
                    .active_document()
                    .is_some_and(|document| !document.session.is_new())
        });
        self.request_named(
            SourceMode::Rename,
            SourceInput::Rename(RenameInput {
                name: name.into(),
                project,
                confirmed: None,
            }),
            registry,
            app,
            root,
            jobs,
        )
    }

    pub fn confirm_project_rename(
        &mut self,
        app: &Workbench,
        root: Option<&Path>,
        jobs: &mut JobSystem,
    ) -> Result<bool, String> {
        if !self
            .items
            .iter()
            .any(|item| matches!(item.action, SourceAction::ProjectRename(..)))
        {
            return Ok(false);
        }
        let mut request = self
            .accepted
            .as_ref()
            .filter(|request| request.current(app, root))
            .cloned()
            .ok_or("La vista previa del proyecto ya no está vigente")?;
        if self.generation == u64::MAX {
            return Err("Se agotó el historial de revisiones del renombrado".into());
        }
        project_buffers_clean(app, root.ok_or("Abre la carpeta del proyecto")?)?;
        let SourceInput::Rename(input) = &mut request.input else {
            return Err("Confirmación de renombrado inválida".into());
        };
        input.confirmed = Some(std::mem::take(&mut self.items));
        self.invalidate(jobs);
        request.generation = self.generation;
        self.pending = Some(request);
        self.status(
            SourceState::Queued,
            "Verificando de nuevo el renombrado del proyecto…",
        );
        self.tick(app, root, jobs);
        if matches!(self.state, SourceState::Limited | SourceState::Failed) {
            Err(self.message.clone())
        } else {
            Ok(true)
        }
    }

    pub fn take_project_rename_applied(&mut self) -> bool {
        std::mem::take(&mut self.project_rename_applied)
    }
    pub fn request_catalog(
        &mut self,
        mode: SourceMode,
        provider: Arc<dyn CatalogSource>,
        registry: &SourceRegistry,
        app: &Workbench,
        root: Option<&Path>,
        jobs: &mut JobSystem,
    ) -> Result<(), String> {
        if mode != SourceMode::Symbols && !mode.is_catalog() {
            return Err("Capacidad de catálogo no disponible".into());
        }
        self.request_named(
            mode,
            SourceInput::Catalog(provider),
            registry,
            app,
            root,
            jobs,
        )
    }
    fn request_named(
        &mut self,
        mode: SourceMode,
        input: SourceInput,
        registry: &SourceRegistry,
        app: &Workbench,
        root: Option<&Path>,
        jobs: &mut JobSystem,
    ) -> Result<(), String> {
        if mode == SourceMode::Rename && !matches!(input, SourceInput::Rename(_)) {
            return Err("Escribe primero el nombre nuevo".into());
        }
        if self.outline
            && (matches!(mode, SourceMode::Completion | SourceMode::Rename)
                || mode.is_catalog()
                || mode.is_lsp())
        {
            return Err("Usa el comando de navegación en la vista de código".into());
        }
        if self.generation == u64::MAX {
            return Err("Se agotó la generación de código".into());
        }
        self.invalidate(jobs);
        self.mode = Some(mode);
        self.status(SourceState::Failed, "Solicitud de código no disponible");
        if matches!(
            mode,
            SourceMode::Completion
                | SourceMode::CatalogCompletion
                | SourceMode::Rename
                | SourceMode::LspCompletion
                | SourceMode::ReviewEdit(_)
        ) && (app.active_editor().is_some_and(|e| e.selection().is_some())
            || app.rectangle_for_pane(app.panes().active()).is_some())
        {
            return Err("Coloca el cursor en el texto, sin selección".into());
        }
        let origin = Origin::capture(app, root)?;
        if mode == SourceMode::Incoming
            && (root.is_none()
                || app
                    .active_document()
                    .is_some_and(|doc| doc.session.is_new()))
        {
            return Err(
                "Llamado por requiere un archivo guardado y una carpeta de proyecto".into(),
            );
        }
        if self.outline {
            self.outline_attempt = Some(origin.clone());
        }
        let provider = if let SourceInput::Catalog(provider) = &input {
            if !provider.is_current() {
                return Err("El contexto del catálogo ABL cambió".into());
            }
            None
        } else if mode.is_catalog() {
            return Err("Actualiza primero el catálogo del proyecto ABL".into());
        } else if mode.lsp_query().is_some() {
            if !registry.supports_lsp(app, root) {
                return Err("LSP requiere un archivo guardado de un lenguaje admitido y una carpeta de proyecto".into());
            }
            None
        } else {
            Some(
                registry
                    .resolve(&origin.language, mode)
                    .ok_or("No hay proveedor para esta capacidad de código")?,
            )
        };
        self.pending = Some(Request {
            generation: self.generation,
            origin,
            mode,
            provider,
            input,
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
            .is_some_and(|r| !r.current(app, root))
            || self.pending.as_ref().is_some_and(|r| !r.current(app, root))
            || self
                .flight
                .as_ref()
                .is_some_and(|f| !f.discard && !f.request.current(app, root));
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
        let len = if r.mode == SourceMode::Incoming {
            0
        } else if matches!(
            r.mode,
            SourceMode::Completion | SourceMode::CatalogCompletion
        ) {
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
        let mode = r.mode;
        let lsp = mode.lsp_query();
        let work =
            if let SourceInput::Catalog(provider) = &r.input {
                let cursor = r.origin.cursor;
                let query = match mode {
                    SourceMode::Symbols => CatalogQuery::Symbols,
                    SourceMode::CatalogDefinition => CatalogQuery::Definition { cursor },
                    SourceMode::CatalogReferences => CatalogQuery::References { cursor },
                    SourceMode::CatalogXref => CatalogQuery::Xref { cursor },
                    SourceMode::ReviewEdit(action) => CatalogQuery::ReviewEdit { action, cursor },
                    _ => CatalogQuery::Complete,
                };
                SourceJob::Catalog(provider.clone(), query)
            } else if let Some(query) = lsp {
                SourceJob::Lsp(crate::lsp::Work {
                    client: self.lsp.clone(),
                    root: r.origin.root.clone().expect("admitted project root"),
                    path: r.origin.path.clone(),
                    profile: r.origin.language.clone(),
                    cursor: r.origin.cursor,
                    query,
                })
            } else {
                let provider = r.provider.expect("admitted source provider");
                match r.mode {
                    SourceMode::Incoming => SourceJob::Incoming(IncomingScan {
                        root: r.origin.root.clone().unwrap(),
                        target: r.origin.path.clone(),
                        provider,
                    }),
                    SourceMode::Rename => SourceJob::Rename {
                        provider: provider.rename.unwrap(),
                        cursor: r.origin.cursor,
                        name: match &r.input {
                            SourceInput::Rename(input) => input.name.clone(),
                            _ => unreachable!("admitted rename input"),
                        },
                        project: match &r.input {
                            SourceInput::Rename(input) => input
                                .project
                                .zip(r.origin.root.clone())
                                .map(|(provider, root)| ProjectRenameJob {
                                    provider,
                                    root,
                                    path: r.origin.path.clone(),
                                }),
                            _ => None,
                        },
                    },
                    mode => SourceJob::Plain(provider.provider(mode).unwrap()),
                }
            };
        match jobs.submit_source(app, self.job_key(), r.origin.stamp, len, work) {
            Ok(id) => {
                let request = self.pending.take().unwrap();
                self.flight = Some(Flight {
                    id,
                    request,
                    discard: false,
                });
                changed |= self.status(
                    SourceState::Running,
                    if lsp.is_some() {
                        "Consultando servidor LSP"
                    } else if mode == SourceMode::CatalogXref {
                        "Compilando referencias XREF del proyecto"
                    } else if mode.is_catalog() {
                        "Analizando el código ABL del proyecto"
                    } else {
                        "Leyendo captura acotada del fuente; solo evidencia léxica"
                    },
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
        if !request.current(app, root) || !task.current || task.path != request.origin.path {
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
        if request.mode == SourceMode::LspDiagnostics {
            let mut count = 0;
            let mut notice = None;
            for item in task.output.items() {
                match item {
                    TaskItem::Diagnostic { .. } => count += 1,
                    TaskItem::Message(message) => notice = Some(message),
                    item => {
                        let error = if let TaskItem::Problem(error) = item {
                            error
                        } else {
                            "Informe LSP inválido"
                        };
                        self.status(SourceState::Failed, error);
                        return true;
                    }
                }
            }
            let mut message = format!("LSP: {count} diagnósticos");
            if let Some(notice) = notice {
                message.push_str(&format!(" · {notice}"));
            }
            self.accepted = Some(request);
            self.status(SourceState::Ready, message);
            return true;
        }
        let document = app.document_by_id(request.origin.stamp.document).unwrap();
        let tab = document.tab_id;
        let source_len = document.session.document().len();
        let mut items = Vec::new();
        let is_lsp = request.mode.lsp_query().is_some();
        let catalog_symbols = request.mode == SourceMode::Symbols
            && matches!(&request.input, SourceInput::Catalog(_));
        let mut notice = None;
        for item in task.output.take_items() {
            if is_lsp
                || catalog_symbols
                || matches!(
                    request.mode,
                    SourceMode::CatalogDefinition
                        | SourceMode::CatalogReferences
                        | SourceMode::CatalogXref
                )
            {
                match item {
                    TaskItem::Problem(message) => {
                        self.status(SourceState::Failed, message);
                        return true;
                    }
                    TaskItem::Message(message) => {
                        notice = Some(message);
                        continue;
                    }
                    _ => (),
                }
            }
            if let TaskItem::Message(message) = &item {
                if matches!(request.mode, SourceMode::Rename | SourceMode::ReviewEdit(_)) {
                    self.status(SourceState::Failed, message.clone());
                    return true;
                }
            }
            let TaskItem::Source(item) = item else {
                self.status(
                    SourceState::Failed,
                    "El proveedor devolvió un tipo de resultado ajeno",
                );
                return true;
            };
            if !item.valid_for(request.mode)
                || matches!(&item.action, SourceAction::ScopedSymbol { extent }
                    if extent.end > source_len)
                || request.mode == SourceMode::LspCompletion
                    && !(item.range.start..=item.range.end).contains(&request.origin.cursor)
                || matches!(
                    request.mode,
                    SourceMode::Completion | SourceMode::CatalogCompletion
                ) && (item.range.end != request.origin.cursor || item.range.len() > 512)
                || !matches!(
                    item.action,
                    SourceAction::Caller { .. }
                        | SourceAction::ProjectLocation { .. }
                        | SourceAction::ProjectRename(..)
                ) && (app.position_for_tab(tab, item.range.start).is_none()
                    || app.position_for_tab(tab, item.range.end).is_none())
            {
                self.status(
                    SourceState::Failed,
                    "El proveedor devolvió fronteras/acción inválidas",
                );
                return true;
            }
            items.push(item);
        }
        if items
            .iter()
            .any(|item| matches!(item.action, SourceAction::ProjectRename(..)))
        {
            let result = root
                .ok_or_else(|| "Falta la raíz del proyecto".to_owned())
                .and_then(|root| project_buffers_clean(app, root));
            if let Err(error) = result {
                self.status(SourceState::Failed, error);
                return true;
            }
        }
        self.selected = if request
            .provider
            .is_some_and(|provider| provider.id == crate::document_links::ID)
        {
            items
                .iter()
                .position(|item| item.range.contains(&request.origin.cursor))
                .unwrap_or(0)
        } else {
            0
        };
        self.items = items;
        let message = if catalog_symbols {
            let mut message = format!(
                "ABL: {} símbolos del archivo y sus inclusiones",
                self.items.len()
            );
            if let Some(notice) = notice {
                message.push_str(&format!(" · {notice}"));
            }
            message
        } else if self
            .items
            .iter()
            .any(|item| matches!(item.action, SourceAction::ProjectRename(..)))
        {
            let files = self
                .items
                .iter()
                .filter_map(|item| match &item.action {
                    SourceAction::ProjectRename(edit, _) => Some(&edit.path),
                    _ => None,
                })
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            if files > crate::WorkspaceFileEdit::MAX_FILES {
                self.items.clear();
                self.status(
                    SourceState::Limited,
                    format!(
                        "El renombrado afecta {files} archivos y supera el límite de {}",
                        crate::WorkspaceFileEdit::MAX_FILES
                    ),
                );
                return true;
            }
            format!(
                "{files} archivos · {} cambios · F9 o Ctrl+Intro confirma",
                self.items.len()
            )
        } else if request.mode == SourceMode::CatalogCompletion {
            format!("Esquema ABL: {} sugerencias", self.items.len())
        } else if matches!(request.mode, SourceMode::ReviewEdit(_)) {
            if self.items.is_empty() {
                "No hay cambios para aplicar".into()
            } else {
                "Revisa los cambios antes de confirmar".into()
            }
        } else if request.mode == SourceMode::CatalogDefinition {
            if self.items.is_empty() {
                notice.unwrap_or_else(|| "Definición ABL sin resolver".into())
            } else {
                "Definición ABL localizada · Intro abre · Ctrl+Intro al lado".into()
            }
        } else if matches!(
            request.mode,
            SourceMode::CatalogReferences | SourceMode::CatalogXref
        ) {
            let provider = if request.mode == SourceMode::CatalogXref {
                "XREF"
            } else {
                "ABL"
            };
            let mut message = format!("{provider}: {} referencias", self.items.len());
            if let Some(notice) = notice {
                message.push_str(&format!(" · {notice}"));
            }
            message
        } else if let Some(provider) = request.provider {
            format!(
                "{}: {} resultados léxicos (sin verificación del compilador)",
                provider.id,
                self.items.len()
            )
        } else {
            let mut message = format!("LSP: {} resultados", self.items.len());
            if let Some(notice) = notice {
                message.push_str(&format!(" · {notice}"));
            }
            message
        };
        self.accepted = Some(request);
        self.status(SourceState::Ready, message);
        true
    }
    pub fn accept_editor(
        &mut self,
        app: &mut Workbench,
        root: Option<&Path>,
        task: &mut CompletedTask,
    ) -> bool {
        let diagnostics = self.flight.as_ref().is_some_and(|flight| {
            flight.id == task.id
                && !flight.discard
                && flight.request.generation == self.generation
                && flight.request.mode == SourceMode::LspDiagnostics
        });
        let consumed = self.accept(app, root, task);
        if consumed && diagnostics && self.state == SourceState::Ready {
            app.replace_lsp_diagnostics(task);
        }
        if consumed && self.state == SourceState::Ready && self.accepted.as_ref().is_some_and(|request| {
            matches!(&request.input, SourceInput::Rename(input) if input.confirmed.is_some())
        }) {
            let request = self.accepted.take().unwrap();
            let SourceInput::Rename(input) = &request.input else { unreachable!() };
            if input.confirmed.as_ref() != Some(&self.items) {
                self.status(SourceState::Stale, "Cambió la resolución del proyecto. Vuelve a revisar el renombrado");
            } else {
                match self.apply_project_rename(app, &request) {
                    Ok(()) => {
                        self.project_rename_applied = true;
                        self.status(SourceState::Ready, "Renombrado del proyecto aplicado · un solo Deshacer");
                    }
                    Err(error) => { self.status(SourceState::Failed, error); }
                }
            }
        }
        consumed
    }

    fn apply_project_rename(&self, app: &mut Workbench, request: &Request) -> Result<(), String> {
        let root = request
            .origin
            .root
            .as_deref()
            .ok_or("Falta la raíz del proyecto")?;
        project_buffers_clean(app, root)?;
        let mut files = std::collections::BTreeMap::<PathBuf, crate::WorkspaceFileEdit>::new();
        for item in &self.items {
            let SourceAction::ProjectRename(edit, name) = &item.action else {
                return Err("Vista previa de proyecto inválida".into());
            };
            let origin = crate::SearchOrigin::Disk(edit.version);
            let file = files
                .entry(edit.path.clone())
                .or_insert_with(|| crate::WorkspaceFileEdit {
                    path: edit.path.clone(),
                    origin,
                    encoding: edit.encoding,
                    edits: Vec::new(),
                });
            if file.origin != origin || file.encoding != edit.encoding {
                return Err("Versiones inconsistentes en la vista previa".into());
            }
            file.edits.push(crate::TextReplacement {
                range: edit.range.clone(),
                text: name.clone(),
            });
        }
        app.apply_workspace_edits(root, files.into_values().collect())
            .map_err(|error| error.to_string())
    }

    pub fn activate(
        &mut self,
        app: &mut Workbench,
        root: Option<&Path>,
        to_side: bool,
    ) -> Result<Option<Vec<PathBuf>>, String> {
        self.activate_at_index(app, root, self.selected, to_side)
    }
    pub fn confirm_review(&self, app: &mut Workbench, root: Option<&Path>) -> Result<(), String> {
        let request = self
            .accepted
            .as_ref()
            .filter(|request| matches!(request.mode, SourceMode::ReviewEdit(_)))
            .filter(|request| request.current(app, root))
            .ok_or("La vista previa aún no está lista o cambió el documento")?;
        self.apply_replacements(app, request)
    }

    fn apply_replacements(&self, app: &mut Workbench, request: &Request) -> Result<(), String> {
        let edits = self
            .items
            .iter()
            .filter_map(|item| match &item.action {
                SourceAction::ReviewLine => None,
                SourceAction::Replace(text) => Some(Ok(crate::TextReplacement {
                    range: item.range.clone(),
                    text: text.clone(),
                })),
                _ => Some(Err("La propuesta de edición no es válida".to_owned())),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if edits.is_empty() {
            return Err("No hay una propuesta de edición para aplicar".into());
        }
        app.apply_text_edits(
            request.origin.stamp.document,
            request.origin.stamp.revision,
            edits,
        )
        .map_err(|error| error.to_string())
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
        if !request.current(app, root) {
            return Err("Cambió el resultado del código; actualiza".into());
        }
        if matches!(request.mode, SourceMode::ReviewEdit(_)) {
            return Err("Confirma los cambios con F9 o Ctrl+Intro".into());
        }
        if request.mode == SourceMode::Rename {
            if to_side || self.items.is_empty() {
                return Err("No hay una propuesta de renombrado para aplicar".into());
            }
            if self
                .items
                .iter()
                .any(|item| matches!(item.action, SourceAction::ProjectRename(..)))
            {
                return Err("Confirma la vista previa del proyecto con F9 o Ctrl+Intro".into());
            }
            self.apply_replacements(app, request)?;
            return Ok(None);
        }
        let item = self
            .items
            .get(index)
            .ok_or("No hay elemento de código seleccionado")?;
        match &item.action {
            SourceAction::ProjectRename(..) => {
                Err("Confirma el renombrado completo del proyecto".into())
            }
            SourceAction::ReviewLine => Err("Esta fila solo muestra la vista previa".into()),
            SourceAction::Caller {
                path,
                offset,
                relative,
                encoding,
                version,
            } => {
                let root = root.ok_or("Abre una carpeta de proyecto")?;
                let canonical_root = root.canonicalize().map_err(|error| error.to_string())?;
                let target = request
                    .origin
                    .path
                    .canonicalize()
                    .map_err(|error| error.to_string())?;
                let paths = resolve_from(
                    &canonical_root,
                    path,
                    relative,
                    request.provider.expect("incoming provider").file_resolution,
                    SourceMode::Incoming,
                )?;
                if !paths
                    .iter()
                    .any(|candidate| candidate.canonicalize().is_ok_and(|p| p == target))
                {
                    return Err(
                        "Cambió la resolución de la referencia; actualiza la búsqueda".into(),
                    );
                }
                let tab = app
                    .open_project_file_at_version(
                        &canonical_root,
                        path.clone(),
                        Some(*encoding),
                        to_side,
                        SNAPSHOT,
                        *version,
                    )
                    .map_err(|error| error.to_string())?;
                if !app.jump_to_pane_location(app.panes().active(), tab, *offset) {
                    return Err("El destino de la referencia no es una posición válida".into());
                }
                Ok(None)
            }
            SourceAction::ProjectLocation {
                path,
                offset,
                encoding,
                version,
            } => {
                let root = root.ok_or("Abre una carpeta de proyecto")?;
                let tab = app
                    .open_project_file_at_version(
                        root,
                        path.clone(),
                        Some(*encoding),
                        to_side,
                        SNAPSHOT,
                        *version,
                    )
                    .map_err(|error| error.to_string())?;
                if !app.jump_to_pane_location(app.panes().active(), tab, *offset) {
                    return Err("El destino de código no es una posición válida".into());
                }
                Ok(None)
            }
            SourceAction::Unresolved(reason) => {
                Err(format!("Referencia léxica sin resolver: {reason}"))
            }
            SourceAction::Navigate | SourceAction::ScopedSymbol { .. } => {
                let tab = app
                    .document_by_id(request.origin.stamp.document)
                    .unwrap()
                    .tab_id;
                let pane = if to_side {
                    if !app.open_active_to_side() {
                        return Err("No se pudo abrir el símbolo al lado".into());
                    }
                    app.panes().active()
                } else {
                    request.origin.pane
                };
                if !app.jump_to_pane_location(pane, tab, item.range.start) {
                    return Err("Destino de código inválido".into());
                }
                Ok(None)
            }
            SourceAction::LspCompletion(edits) => {
                if to_side {
                    return Err("El autocompletado se aplica al documento activo".into());
                }
                app.apply_text_edits(
                    request.origin.stamp.document,
                    request.origin.stamp.revision,
                    edits.clone(),
                )
                .map_err(|error| error.to_string())?;
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
        if !request.current(app, root) {
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
        if request.provider.is_some_and(|provider| {
            matches!(
                provider.file_resolution,
                FileResolution::DocumentRelative | FileResolution::RelativeModule
            )
        }) && request.origin.path.is_absolute()
        {
            request.origin.path.parent()
        } else {
            None
        }
    })
    .ok_or_else(|| "Abre una carpeta de proyecto para resolver referencias".into())
}

fn resolve_file(root: &Path, request: &Request, relative: &str) -> Result<Vec<PathBuf>, String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    resolve_from(
        &root,
        &request.origin.path,
        relative,
        request
            .provider
            .ok_or("No hay proveedor de referencias léxicas")?
            .file_resolution,
        request.mode,
    )
}

fn resolve_from(
    root: &Path,
    source: &Path,
    relative: &str,
    resolution: FileResolution,
    mode: SourceMode,
) -> Result<Vec<PathBuf>, String> {
    match resolution {
        FileResolution::ProjectCandidates => {
            find_reference_candidates(root, source, relative, true)
        }
        FileResolution::DocumentRelative => {
            find_reference_candidates(root, source, relative, false)
        }
        FileResolution::RelativeModule => resolve_module(root, source, relative),
        FileResolution::ProjectImports => {
            let parents = load_source_roots(root)?;
            resolve_import(root, relative, &parents)
        }
        FileResolution::OrderedProjectPath => {
            let propath = load_propath(root)?;
            resolve_ordered(
                root,
                source,
                relative,
                propath.as_deref(),
                mode == SourceMode::References,
            )
        }
    }
}

fn resolve_ordered(
    root: &Path,
    source: &Path,
    relative: &str,
    propath: Option<&[PathBuf]>,
    allow_ambiguity: bool,
) -> Result<Vec<PathBuf>, String> {
    if let Some(parents) = propath {
        return candidates_from(root, relative, parents.iter().map(PathBuf::as_path), true);
    }
    let mut paths = find_reference_candidates(root, source, relative, true)?;
    if !allow_ambiguity {
        paths.truncate(1);
    }
    Ok(paths)
}

/// Reads the ordered project directories shared by static navigation and the AVM.
pub fn load_propath(root: &Path) -> Result<Option<Vec<PathBuf>>, String> {
    load_project_paths(root, "propath", "PROPATH")
}

fn load_project_paths(
    root: &Path,
    file: &str,
    label: &str,
) -> Result<Option<Vec<PathBuf>>, String> {
    let config = root.join(".cide").join(file);
    let bytes = match jc_files::read_in_root(root, &config, 8192) {
        Ok((_, bytes)) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("No se pudo leer {label}: {error}")),
    };
    let config = std::str::from_utf8(&bytes).map_err(|_| format!("{label} debe estar en UTF-8"))?;
    let mut parents = Vec::new();
    for entry in config.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if entry.is_empty() || entry.starts_with('#') {
            continue;
        }
        if parents.len() == 32
            || !project_relative(Path::new(entry))
            || entry.chars().any(char::is_control)
            || entry.contains(',')
        {
            return Err(format!(
                "{label} admite hasta 32 directorios relativos al proyecto, sin comas"
            ));
        }
        parents.push(root.join(entry));
    }
    Ok(Some(parents))
}

fn load_source_roots(root: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(
        load_project_paths(root, "source-roots", "Raíces de código")?.unwrap_or_else(|| {
            [".", "src/main/java", "src/test/java", "src"]
                .map(|path| root.join(path))
                .to_vec()
        }),
    )
}

fn resolve_import(
    root: &Path,
    relative: &str,
    parents: &[PathBuf],
) -> Result<Vec<PathBuf>, String> {
    candidates_from(root, relative, parents.iter().map(PathBuf::as_path), false)
}

fn resolve_module(root: &Path, source: &Path, relative: &str) -> Result<Vec<PathBuf>, String> {
    let typescript = jc_languages::detect(&source.to_string_lossy()).id == "typescript";
    let mut candidates = Vec::with_capacity(10);
    let path = Path::new(relative);
    if typescript {
        match path.extension().and_then(|part| part.to_str()) {
            Some("js") => {
                candidates.extend([path.with_extension("ts"), path.with_extension("tsx")])
            }
            Some("mjs") => candidates.push(path.with_extension("mts")),
            Some("cjs") => candidates.push(path.with_extension("cts")),
            _ => {}
        }
    }
    candidates.push(path.to_owned());
    if path.extension().is_none() {
        let extensions: &[&str] = if typescript {
            &["ts", "tsx", "js", "json"]
        } else {
            &["js", "json"]
        };
        if !relative.ends_with('/') {
            candidates.extend(extensions.iter().map(|ext| path.with_extension(ext)));
        }
        candidates.extend(
            extensions
                .iter()
                .map(|ext| path.join("index").with_extension(ext)),
        );
    }
    for candidate in candidates {
        let Some(candidate) = candidate.to_str() else {
            continue;
        };
        let found = find_reference_candidates(root, source, candidate, false)?;
        if !found.is_empty() {
            return Ok(found);
        }
    }
    Ok(Vec::new())
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
                label: format!("{word} (palabra local)"),
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
