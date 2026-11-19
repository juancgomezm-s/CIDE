//! Native OmniCore queries and the change journal for the active workspace.
use super::{
    ffi::{Hdc, Hwnd, Rect},
    omnicore_runtime::{self, Answer, Query, LEGACY_INPUT_BYTES},
    omnicore_transport::{Catalog, ChangeKind, ChangedFile, Model, Opened, Transport, Update},
    paint, shell, Action, State,
};
use jc_app::{edit_selected_field, FieldEdit, FieldSelection, Modifiers};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    rc::Rc,
};

mod chat;
mod composer;
mod sessions;
use chat::{Attachment, Mode, Outgoing, Prompt};
use sessions::Sessions;

const ROW: i32 = 26;
const HEADER: i32 = 83;
const CHAT_FOOTER: i32 = 136;
const CHAT_ROW: i32 = 18;
const SESSIONS_TOP: i32 = HEADER + 36;
const SESSIONS_FOOTER: i32 = 39;
const MAX_PENDING: usize = 8;

#[derive(Default)]
pub(super) struct Panel {
    pub visible: bool,
    transport: Transport,
    root: Option<PathBuf>,
    executable: Option<PathBuf>,
    monitoring: bool,
    ready: bool,
    ask_supported: bool,
    edit_supported: bool,
    context_supported: bool,
    launch_pending: bool,
    session: Option<String>,
    rows: Vec<ChangedFile>,
    pending: VecDeque<Opened>,
    waiting: Option<Opened>,
    status: Option<String>,
    first: usize,
    sessions_first: usize,
    show_sessions: bool,
    collapse_folder: bool,
    show_changes: bool,
    focused: bool,
    chat_notice: Option<String>,
    sessions: Sessions,
    query: Option<PendingQuery>,
    model_catalog: Option<Result<Rc<Vec<Model>>, String>>,
    catalog_request: Option<Catalog>,
    model: Option<Model>,
    stamp: Rc<()>,
    pub hits: Option<Hits>,
}

struct PendingQuery {
    session: u64,
    child: Query,
    context: Option<Attachment>,
    edit: bool,
}

#[derive(PartialEq, Eq)]
struct EditRequest {
    session: u64,
    root: PathBuf,
    executable: PathBuf,
    model: Option<Model>,
    draft: String,
    outgoing: Outgoing,
    capacity: usize,
}

impl Panel {
    fn can_compose(&self) -> bool {
        self.ready && self.ask_supported
    }

    fn querying_here(&self) -> bool {
        self.query
            .as_ref()
            .is_some_and(|query| query.session == self.sessions.active.id)
    }

    fn input_capacity(&self) -> Result<usize, String> {
        let Some(Ok(models)) = &self.model_catalog else {
            return Err(
                "Espera a que cargue el catálogo de modelos o recárgalo desde Modelo.".into(),
            );
        };
        let tokens = if let Some(model) = &self.model {
            if !models.contains(model) {
                return Err(
                    "El modelo seleccionado ya no está disponible. Elige otro en Modelo.".into(),
                );
            }
            model.input_tokens
        } else {
            // Automatic routing may pick any configured model. Use the lowest
            // published budget unless the environment pins the runtime's model.
            let pinned = std::env::var("OMNI_MODEL").ok();
            if let Some(model) = pinned
                .as_ref()
                .and_then(|id| models.iter().find(|model| &model.id == id))
            {
                model.input_tokens
            } else {
                models.first().and_then(|_| {
                    models
                        .iter()
                        .try_fold(usize::MAX, |min, model| Some(min.min(model.input_tokens?)))
                })
            }
        }
        .ok_or("El catálogo no informó el contexto disponible. Elige un modelo que lo indique.")?;
        Ok(omnicore_runtime::input_capacity(
            self.context_supported,
            tokens,
        ))
    }

    fn draft_capacity(&self) -> usize {
        self.input_capacity().unwrap_or(LEGACY_INPUT_BYTES)
    }

    fn finish_query(&mut self, answer: Result<Answer, String>) {
        if let Some(query) = self.query.take() {
            if let Some(conversation) = self.sessions.conversation_mut(query.session) {
                if answer.is_err()
                    && conversation.draft.is_empty()
                    && conversation.chat.attachment.is_none()
                {
                    if let Some(turn) = conversation.chat.turns.back() {
                        conversation.draft.clone_from(&turn.question);
                        conversation.selection.cursor = conversation.draft.len();
                        conversation.chat.attachment = query.context;
                    }
                }
                conversation.chat.finish(answer);
            }
            self.invalidate();
        }
    }

    fn invalidate(&mut self) {
        self.stamp = Rc::new(());
        self.hits = None;
    }

    fn stop(&mut self) {
        self.transport.stop();
        self.root = None;
        self.monitoring = false;
        self.ready = false;
        self.ask_supported = false;
        self.edit_supported = false;
        self.context_supported = false;
        self.launch_pending = false;
        self.pending.clear();
        self.waiting = None;
        self.rows.clear();
        self.session = None;
        self.first = 0;
        self.focused = false;
        self.query = None;
        self.model_catalog = None;
        self.catalog_request = None;
        self.chat_notice = None;
        self.sessions = Sessions::default();
        self.show_sessions = false;
        self.collapse_folder = false;
        self.show_changes = false;
        self.sessions_first = 0;
        self.invalidate();
    }
}

#[derive(Clone)]
enum Target {
    Hide,
    Sessions,
    ToggleFolder,
    PickSession(u64),
    NewChat,
    Session,
    ChooseExecutable,
    ChooseModel,
    Retry,
    ChooseWorkspace,
    Conversation,
    Changes,
    Draft,
    Mode(Mode),
    ToggleActiveFile,
    Attach,
    RemoveAttachment,
    Ask,
    Effect(String),
}

#[derive(Clone)]
pub(super) struct Hits {
    stamp: Rc<()>,
    rect: Rect,
    buttons: Vec<(Rect, Target)>,
    draft: Vec<composer::HitRow>,
}

pub(super) struct Frame {
    hits: Hits,
    labels: Vec<(Rect, String, u32, u32)>,
    dpi: super::dpi::Dpi,
    composer_card: Option<Rect>,
    draft: Option<composer::Layout>,
    folder_open: bool,
}

impl Frame {
    pub fn hits(&self) -> Hits {
        self.hits.clone()
    }
}

fn contains(rect: Rect, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn same(a: Rect, b: Rect) -> bool {
    (a.left, a.top, a.right, a.bottom) == (b.left, b.top, b.right, b.bottom)
}

fn safe_label(text: &str) -> String {
    text.chars()
        .take(256)
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}

fn context_text(
    editor: &jc_app::EditorRead<'_>,
    start: usize,
    budget: usize,
) -> Option<(String, usize)> {
    let document = editor.session.document();
    let encoding = editor.session.encoding()?;
    let read_end = start
        .saturating_add(budget)
        .saturating_add(6) // UTF-8 BOM and the tail of a split scalar
        .min(document.len());
    let bytes = document.read_range(start..read_end).ok()?;
    let mut end = start;
    let mut remaining = budget;
    let mut text = String::new();
    for scalar in jc_text::DecodedScalars::new(&bytes, encoding, start) {
        let cost = chat::escaped_bytes(scalar.character);
        if cost > remaining {
            break;
        }
        if !scalar.valid {
            return None;
        }
        remaining -= cost;
        end = scalar.source_range.end;
        text.push(scalar.character);
    }
    Some((text, end))
}

fn display_lines(text: &str, width: usize) -> impl Iterator<Item = String> + '_ {
    text.lines().flat_map(move |line| {
        let line: Vec<char> = line
            .chars()
            .map(|ch| match ch {
                '\t' => ' ',
                ch if ch.is_control() => '�',
                ch => ch,
            })
            .collect();
        let mut rows = Vec::new();
        let mut rest = line.as_slice();
        while rest.len() > width {
            let cut = rest[..width]
                .iter()
                .rposition(|ch| ch.is_whitespace())
                .filter(|index| *index >= width / 2)
                .map_or(width, |index| index + 1);
            rows.push(rest[..cut].iter().collect());
            rest = &rest[cut..];
        }
        rows.push(rest.iter().collect());
        rows
    })
}

impl State {
    pub(super) fn omnicore_region(&self) -> Option<Rect> {
        if !self.omnicore.visible
            || self.width < self.dpi.pixels(480)
            || self.height < self.dpi.pixels(280)
            || (self.git_panel.is_some() && !self.git_code.active)
        {
            return None;
        }
        let right = self.width.clamp(1, 8192);
        let width = self.dpi.pixels(348).min(right / 2);
        Some(Rect {
            left: right - width,
            right,
            top: self.chrome_layout().top(),
            bottom: self.height.clamp(1, 8192) - self.cell_height - 8,
        })
    }

    pub(super) fn toggle_omnicore(&mut self) {
        self.omnicore.visible = !self.omnicore.visible;
        if self.omnicore.visible && !self.omnicore.monitoring {
            self.start_omnicore();
        } else if !self.omnicore.visible {
            self.omnicore.focused = false;
        }
        self.omnicore.invalidate();
        self.tab_hits = None;
        self.outline.hits = None;
        self.explorer_hits = None;
    }

    pub(super) fn reset_omnicore_project(&mut self) {
        self.omnicore.stop();
        self.omnicore.status = None;
        self.clear_omnicore_diff();
    }

    pub(super) fn connect_omnicore(&mut self, executable: PathBuf) {
        if !omnicore_runtime::valid_executable(&executable) {
            self.omnicore.status = Some("Elige el archivo omni.exe de OmniCore.".into());
            return;
        }
        self.omnicore
            .finish_query(Err("La conexión con OmniCore cambió.".into()));
        let sessions = std::mem::take(&mut self.omnicore.sessions);
        self.omnicore.stop();
        self.omnicore.sessions = sessions;
        self.omnicore.executable = Some(executable);
        self.start_omnicore();
    }

    pub(super) fn start_omnicore(&mut self) {
        if !self.omnicore.visible || self.omnicore.monitoring || self.smoke {
            return;
        }
        let root = match omnicore_runtime::workspace(self.project_root.as_deref()) {
            Ok(root) => root,
            Err(error) => {
                self.omnicore.status = Some(error);
                return;
            }
        };
        self.omnicore.root = Some(root.clone());
        let Some(executable) = self
            .omnicore
            .executable
            .clone()
            .or_else(omnicore_runtime::discover)
        else {
            self.omnicore.status = Some("OmniCore no está incluido en esta instalación.".into());
            return;
        };
        self.omnicore.executable = Some(executable.clone());
        self.omnicore.edit_supported = omnicore_runtime::edit_bridge(&executable).is_some();
        self.omnicore.context_supported = omnicore_runtime::context_bridge(&executable).is_some();
        match self
            .omnicore
            .transport
            .start(root, executable, self.wakeup.clone())
        {
            Ok(()) => {
                self.omnicore.monitoring = true;
                self.omnicore.status = Some("Conectando…".into());
            }
            Err(error) => self.omnicore.status = Some(error),
        }
    }

    pub(super) fn omnicore_executable_folder(&self) -> Option<PathBuf> {
        self.omnicore
            .executable
            .as_deref()
            .and_then(std::path::Path::parent)
            .map(std::path::Path::to_owned)
    }

    fn omnicore_accepts_open(&self) -> bool {
        self.pointer_overlays_clear()
            && self.menu_stamp.is_none()
            && self.git_panel.is_none()
            && !self.zoom.open
            && (!self.open_diff_visible || self.omnicore_diff.is_some())
            && !self.workbench.large_open_pending()
    }

    pub(super) fn schedule_omnicore(&self) {
        if (!self.omnicore.pending.is_empty() || self.omnicore.waiting.is_some())
            && self.omnicore_accepts_open()
        {
            self.bridge.signal();
        }
    }

    pub(super) fn poll_omnicore(&mut self) -> bool {
        let mut changed = false;
        if let Some(models) = self
            .omnicore
            .catalog_request
            .as_ref()
            .and_then(Catalog::poll)
        {
            self.omnicore.catalog_request = None;
            if let (Ok(choices), Some(selected)) = (&models, &mut self.omnicore.model) {
                if let Some(current) = choices
                    .iter()
                    .find(|model| model.id == selected.id && model.provider == selected.provider)
                {
                    selected.clone_from(current);
                }
            }
            self.omnicore.model_catalog = Some(models.map(Rc::new));
            self.omnicore.invalidate();
            changed = true;
        }
        if let Some(answer) = self
            .omnicore
            .query
            .as_ref()
            .and_then(|query| query.child.poll())
        {
            self.omnicore.finish_query(answer);
            changed = true;
        }
        for _ in 0..4 {
            let Some(update) = self.omnicore.transport.poll() else {
                break;
            };
            changed = true;
            match update {
                Update::Snapshot {
                    session,
                    rows,
                    opened,
                    ask_supported,
                } => {
                    self.omnicore.ready = true;
                    self.omnicore.ask_supported = ask_supported;
                    if ask_supported
                        && self.omnicore.model_catalog.is_none()
                        && self.omnicore.catalog_request.is_none()
                    {
                        self.refresh_omnicore_models();
                    }
                    if self.omnicore.session != session {
                        self.omnicore.first = 0;
                    }
                    self.omnicore.session = session;
                    self.omnicore.rows = rows;
                    self.omnicore.first = self
                        .omnicore
                        .first
                        .min(self.omnicore.rows.len().saturating_sub(1));
                    self.omnicore.status = None;
                    for effect in opened {
                        if let Some(index) = self
                            .omnicore
                            .pending
                            .iter()
                            .position(|pending| pending.path == effect.path)
                        {
                            self.omnicore.pending.remove(index);
                        }
                        if self.omnicore.pending.len() == MAX_PENDING {
                            self.omnicore.pending.pop_front();
                        }
                        self.omnicore.pending.push_back(effect);
                    }
                }
                Update::Error(error) => {
                    self.omnicore.transport.stop();
                    self.omnicore.monitoring = false;
                    self.omnicore.ready = false;
                    self.omnicore.ask_supported = false;
                    self.omnicore.status = Some(error);
                }
            }
            if self.omnicore.launch_pending && (self.omnicore.ready || !self.omnicore.monitoring) {
                self.omnicore.launch_pending = false;
                self.open_omnicore_session();
            }
            self.omnicore.invalidate();
        }
        if !self.omnicore_accepts_open() {
            return changed;
        }
        if let Some(effect) = self.omnicore.waiting.take() {
            if self.workbench.active_path() == Some(effect.path.as_path()) {
                self.show_omnicore_effect(effect);
            }
            changed = true;
        }
        if let Some(effect) = self.omnicore.pending.pop_front() {
            self.activate_omnicore_effect(effect);
            changed = true;
        }
        self.schedule_omnicore();
        changed
    }

    fn activate_omnicore_effect(&mut self, effect: Opened) {
        let Some(root) = self.omnicore.root.clone() else {
            return;
        };
        if self
            .project_root
            .as_ref()
            .is_some_and(|project| project != &root)
        {
            return;
        }
        match self.workbench.request_file_open_in_root(
            &root,
            effect.path.clone(),
            self.encoding,
            false,
            2 * 1024 * 1024,
        ) {
            Ok(Some(_)) => {
                self.clear_omnicore_diff();
                if effect.preview.is_none() {
                    self.file_opened();
                }
                self.show_omnicore_effect(effect);
            }
            Ok(None) => {
                self.clear_omnicore_diff();
                self.omnicore.waiting = Some(effect);
            }
            Err(error) => {
                self.omnicore.status = Some(format!("No se pudo abrir el archivo: {error}"))
            }
        }
    }

    fn show_omnicore_effect(&mut self, effect: Opened) {
        if let Some(preview) = effect.preview {
            self.open_omnicore_diff(effect.path, preview);
        } else if effect.kind == ChangeKind::Modified {
            self.omnicore.status = Some(effect.unavailable.unwrap_or_else(|| {
                "OmniCore no conserva una comparación para este cambio.".into()
            }));
        }
    }

    fn open_omnicore_session(&mut self) {
        let panel = &mut self.omnicore;
        if panel.monitoring && !panel.ready {
            panel.launch_pending = true;
            return;
        }
        if let (Some(executable), Some(root)) = (&panel.executable, &panel.root) {
            if let Err(error) = omnicore_runtime::open_session(executable, root) {
                panel.status = Some(error);
            }
        }
    }

    fn refresh_omnicore_models(&mut self) {
        let panel = &mut self.omnicore;
        if panel.catalog_request.is_some() {
            return;
        }
        let Some(executable) = panel.executable.clone() else {
            return;
        };
        match Catalog::start(executable, self.wakeup.clone()) {
            Ok(request) => panel.catalog_request = Some(request),
            Err(error) => panel.model_catalog = Some(Err(error)),
        }
        panel.invalidate();
    }

    fn ask_omnicore(&mut self) {
        self.send_omnicore(None);
    }

    fn submit_omnicore(&mut self) -> Action {
        if self.omnicore.sessions.active.chat.mode == Mode::Edit {
            return Action::ConfirmOmniCoreEdit;
        }
        self.ask_omnicore();
        Action::None
    }

    fn edit_request(&self) -> Result<Option<EditRequest>, String> {
        let panel = &self.omnicore;
        let conversation = &panel.sessions.active;
        if !panel.visible
            || panel.show_sessions
            || !panel.can_compose()
            || !panel.edit_supported
            || conversation.chat.mode != Mode::Edit
            || conversation.draft.trim().is_empty()
            || panel.query.is_some()
        {
            return Ok(None);
        }
        let (Some(root), Some(executable)) = (&panel.root, &panel.executable) else {
            return Ok(None);
        };
        let capacity = panel.input_capacity()?;
        Ok(Some(EditRequest {
            session: conversation.id,
            root: root.clone(),
            executable: executable.clone(),
            model: panel.model.clone(),
            draft: conversation.draft.clone(),
            outgoing: self.prepare_omnicore(capacity)?,
            capacity,
        }))
    }

    fn send_omnicore(&mut self, approval: Option<EditRequest>) {
        let edit = self.omnicore.sessions.active.chat.mode == Mode::Edit;
        if edit != approval.is_some() || (edit && self.edit_request().ok().flatten() != approval) {
            return;
        }
        let panel = &self.omnicore;
        let question = panel.sessions.active.draft.trim();
        if question.is_empty() || panel.query.is_some() {
            return;
        }
        if !panel.can_compose() {
            return;
        }
        let (Some(executable), Some(root)) = (panel.executable.clone(), panel.root.clone()) else {
            return;
        };
        let prepared = match approval {
            Some(review) => Ok((review.outgoing, review.capacity)),
            None => panel.input_capacity().and_then(|capacity| {
                self.prepare_omnicore(capacity)
                    .map(|outgoing| (outgoing, capacity))
            }),
        };
        let panel = &mut self.omnicore;
        let question = panel.sessions.active.draft.trim();
        let (outgoing, capacity) = match prepared {
            Ok(turn) => turn,
            Err(error) => {
                panel.chat_notice = Some(error);
                panel.invalidate();
                return;
            }
        };
        match Query::start(
            executable,
            root,
            panel.model.clone(),
            outgoing.input,
            capacity,
            edit,
            self.wakeup.clone(),
        ) {
            Ok(query) => {
                panel.query = Some(PendingQuery {
                    session: panel.sessions.active.id,
                    child: query,
                    context: panel.sessions.active.chat.attachment.clone(),
                    edit,
                });
                panel
                    .sessions
                    .active
                    .chat
                    .sent(question.to_owned(), outgoing.attachment_title);
                panel.sessions.active.draft.clear();
                panel.sessions.active.selection = FieldSelection::default();
                panel.sessions.active.first = None;
                panel.chat_notice = None;
            }
            Err(error) => panel.chat_notice = Some(error),
        }
        panel.invalidate();
    }

    fn prepare_omnicore(&self, capacity: usize) -> Result<Outgoing, String> {
        let conversation = &self.omnicore.sessions.active;
        let use_file = !conversation.omit_active_file
            && conversation.chat.attachment.is_none()
            && self.workbench.active_editor().is_some_and(|editor| {
                editor.session.encoding().is_some() && !editor.session.document().is_empty()
            });
        let prompt = conversation
            .chat
            .prompt(conversation.draft.trim(), capacity, use_file)?;
        let automatic = use_file.then(|| self.file_context(&prompt)).flatten();
        prompt.finish(conversation.chat.attachment.as_ref().or(automatic.as_ref()))
    }

    fn attachment_title(&self, path: &Path, scope: &str) -> String {
        let relative = self
            .project_root
            .as_deref()
            .and_then(|root| path.strip_prefix(root).ok());
        let name = relative
            .or_else(|| path.file_name().map(Path::new))
            .unwrap_or(path);
        format!("{} · {scope}", safe_label(&name.to_string_lossy()))
    }

    fn file_context(&self, prompt: &Prompt<'_>) -> Option<Attachment> {
        let editor = self.workbench.active_editor()?;
        let document = editor.session.document();
        let line = editor.scroll_line;
        let full = self.attachment_title(editor.session.path(), "archivo");
        let full_budget = prompt.context_bytes(&full);
        if document.len() <= full_budget.saturating_add(3) {
            let (text, end) = context_text(&editor, 0, full_budget)?;
            if !text.is_empty() && end == document.len() {
                return Some(Attachment { title: full, text });
            }
        }
        let title = self.attachment_title(editor.session.path(), "extracto del archivo");
        let start = self
            .workbench
            .line_range_for_tab(editor.tab_id, line)
            .map_or(editor.cursor, |(content, _)| content.start);
        let (text, _) = context_text(&editor, start, prompt.context_bytes(&title))?;
        (!text.is_empty()).then_some(Attachment { title, text })
    }

    pub(super) fn attach_current_code_to_omnicore(&mut self) {
        if !self.omnicore.visible {
            self.toggle_omnicore();
        }
        let attachment = (|| {
            let capacity = self.omnicore.input_capacity()?;
            let conversation = &self.omnicore.sessions.active;
            let prompt = conversation
                .chat
                .prompt(conversation.draft.trim(), capacity, true)?;
            let editor = self
                .workbench
                .active_editor()
                .ok_or("Abre un archivo o selecciona código para adjuntarlo.")?;
            let title = self.attachment_title(editor.session.path(), "selección");
            let selection_budget = prompt.context_bytes(&title);
            let selection = self
                .workbench
                .export_selection(selection_budget)
                .map_err(|error| error.to_string())?;
            if let Some(selection) = selection {
                if selection
                    .text
                    .chars()
                    .map(chat::escaped_bytes)
                    .sum::<usize>()
                    > selection_budget
                {
                    return Err("La selección supera el espacio disponible del modelo.".into());
                }
                Ok(Attachment {
                    title,
                    text: selection.text,
                })
            } else {
                self.file_context(&prompt).ok_or_else(|| {
                    "El archivo está vacío, no es texto válido o el modelo no tiene espacio para adjuntarlo.".into()
                })
            }
        })();
        let panel = &mut self.omnicore;
        panel.show_sessions = false;
        panel.focused = true;
        match attachment {
            Ok(attachment) => {
                panel.sessions.active.chat.attachment = Some(attachment);
                panel.chat_notice = None;
            }
            Err(error) => {
                panel.chat_notice = Some(error);
            }
        }
        panel.invalidate();
    }

    fn omnicore_field_focused(&self) -> bool {
        self.omnicore.focused
            && !self.omnicore.show_sessions
            && self.omnicore.can_compose()
            && self.omnicore_region().is_some()
            && self.pointer_overlays_clear()
            && self.git_panel.is_none()
            && !self.zoom.open
    }

    pub(super) fn blur_omnicore(&mut self) {
        if self.omnicore.focused {
            self.omnicore.focused = false;
            self.omnicore.invalidate();
        }
    }

    fn edit_omnicore(&mut self, edit: FieldEdit<'_>, select: bool) {
        let panel = &mut self.omnicore;
        let capacity = panel.draft_capacity();
        if let Err(error) = edit_selected_field(
            &mut panel.sessions.active.draft,
            &mut panel.sessions.active.selection,
            edit,
            select,
            capacity,
        ) {
            self.status = error.into();
        }
        panel.invalidate();
    }

    pub(super) fn omnicore_character(&mut self, ch: char) -> bool {
        if !self.omnicore_field_focused() {
            return false;
        }
        self.edit_omnicore(FieldEdit::Insert(ch.encode_utf8(&mut [0; 4])), false);
        true
    }

    pub(super) fn omnicore_key(&mut self, code: usize, modifiers: Modifiers) -> Option<Action> {
        if code == 0x1b
            && modifiers == Modifiers::NONE
            && self.omnicore.show_sessions
            && self.pointer_overlays_clear()
            && self.omnicore_region().is_some()
        {
            self.omnicore.show_sessions = false;
            self.omnicore.invalidate();
            return Some(Action::None);
        }
        if !self.omnicore_field_focused() {
            return None;
        }
        if modifiers == Modifiers::CTRL {
            match code {
                0x41 => {
                    let panel = &mut self.omnicore;
                    panel
                        .sessions
                        .active
                        .selection
                        .select_all(&panel.sessions.active.draft);
                    panel.invalidate();
                }
                0x56 => return Some(Action::PasteOmniCore(self.omnicore.stamp.clone())),
                _ => {}
            }
            return Some(Action::None);
        }
        let selecting = modifiers == Modifiers::SHIFT;
        if modifiers == Modifiers::NONE || selecting {
            let edit = match code {
                0x08 => Some(FieldEdit::Backspace),
                0x23 => Some(FieldEdit::End),
                0x24 => Some(FieldEdit::Home),
                0x25 => Some(FieldEdit::Left),
                0x26 => Some(FieldEdit::Up),
                0x27 => Some(FieldEdit::Right),
                0x28 => Some(FieldEdit::Down),
                0x2e => Some(FieldEdit::Delete),
                0x0d if !selecting => return Some(self.submit_omnicore()),
                0x0d => Some(FieldEdit::Insert("\n")),
                0x09 | 0x1b => {
                    self.blur_omnicore();
                    None
                }
                _ => None,
            };
            if let Some(edit) = edit {
                self.edit_omnicore(edit, selecting);
            }
        }
        Some(Action::None)
    }

    pub(super) fn omnicore_click(&mut self, x: i32, y: i32) -> Option<Action> {
        let rect = self.omnicore_region()?;
        if !contains(rect, x, y) || !self.pointer_overlays_clear() {
            return None;
        }
        let Some(target) = self.omnicore.hits.as_ref().and_then(|hits| {
            (same(hits.rect, rect) && Rc::ptr_eq(&hits.stamp, &self.omnicore.stamp))
                .then(|| hits.buttons.iter().find(|(area, _)| contains(*area, x, y)))
                .flatten()
                .map(|(_, target)| target.clone())
        }) else {
            return Some(Action::None);
        };
        match target {
            Target::Hide => self.toggle_omnicore(),
            Target::Sessions => {
                let panel = &mut self.omnicore;
                panel.show_sessions = true;
                panel.show_changes = false;
                panel.invalidate();
            }
            Target::ToggleFolder => {
                let panel = &mut self.omnicore;
                panel.collapse_folder = !panel.collapse_folder;
                panel.invalidate();
            }
            Target::PickSession(id) => {
                let panel = &mut self.omnicore;
                panel.sessions.choose(id);
                panel.sessions_first = 0;
                panel.chat_notice = None;
                panel.show_sessions = false;
                panel.focused = true;
                panel.invalidate();
            }
            Target::NewChat => {
                let panel = &mut self.omnicore;
                let protected = panel.query.as_ref().map(|query| query.session);
                panel.sessions.new_conversation(protected);
                panel.sessions_first = 0;
                panel.chat_notice = None;
                panel.show_sessions = false;
                panel.focused = true;
                panel.invalidate();
            }
            Target::Session => self.open_omnicore_session(),
            Target::ChooseExecutable => return Some(Action::ChooseOmniCore),
            Target::ChooseModel => {
                if matches!(&self.omnicore.model_catalog, Some(Ok(_))) {
                    return Some(Action::ChooseOmniCoreModel);
                }
                self.refresh_omnicore_models();
            }
            Target::Retry => {
                self.start_omnicore();
                self.omnicore.invalidate();
            }
            Target::ChooseWorkspace => return Some(Action::ChooseFolder),
            Target::Conversation => {
                self.omnicore.show_sessions = false;
                self.omnicore.focused = true;
                self.omnicore.invalidate();
            }
            Target::Changes => {
                self.omnicore.show_changes = !self.omnicore.show_changes;
                self.omnicore.invalidate();
            }
            Target::Draft => {
                if let Some(cursor) = self
                    .omnicore
                    .hits
                    .as_ref()
                    .and_then(|hits| composer::at(&hits.draft, x, y))
                {
                    let selection = &mut self.omnicore.sessions.active.selection;
                    selection.cursor = cursor;
                    selection.anchor = None;
                }
                self.omnicore.focused = true;
                self.omnicore.invalidate();
            }
            Target::Mode(mode) => {
                self.omnicore.sessions.active.chat.mode = mode;
                self.omnicore.invalidate();
            }
            Target::ToggleActiveFile => {
                let panel = &mut self.omnicore;
                panel.sessions.active.omit_active_file = !panel.sessions.active.omit_active_file;
                panel.chat_notice = None;
                panel.invalidate();
            }
            Target::Attach => self.attach_current_code_to_omnicore(),
            Target::RemoveAttachment => {
                self.omnicore.sessions.active.chat.attachment = None;
                self.omnicore.chat_notice = None;
                self.omnicore.invalidate();
            }
            Target::Ask => {
                if self.omnicore.querying_here() {
                    let pending = self.omnicore.query.as_ref().unwrap();
                    if pending.edit {
                        pending.child.cancel();
                        self.omnicore.invalidate();
                    } else {
                        self.omnicore
                            .finish_query(Err("Consulta cancelada.".into()));
                    }
                } else {
                    return Some(self.submit_omnicore());
                }
            }
            Target::Effect(id) => {
                if let Err(error) = self.omnicore.transport.request_open(&id) {
                    self.omnicore.status = Some(error);
                }
            }
        }
        Some(Action::None)
    }

    pub(super) fn omnicore_wheel(&mut self, x: i32, y: i32, delta: i32) -> bool {
        if let Some(rect) = self.omnicore_region().filter(|rect| contains(*rect, x, y)) {
            if !self.pointer_overlays_clear() {
                return false;
            }
            let movement = -(delta.signum() as isize) * 3;
            if self.omnicore.show_sessions && self.omnicore.show_changes {
                self.omnicore.first = self
                    .omnicore
                    .first
                    .saturating_add_signed(movement)
                    .min(self.omnicore.rows.len().saturating_sub(1));
            } else if self.omnicore.show_sessions {
                let page = self.omnicore_session_capacity(rect);
                self.omnicore.sessions_first = self
                    .omnicore
                    .sessions_first
                    .saturating_add_signed(movement)
                    .min(self.omnicore.sessions.len().saturating_sub(page.max(1)));
            } else {
                let page = self.omnicore_chat_capacity(rect);
                let max = self.omnicore_answer_rows(rect).count().saturating_sub(page);
                let first = self
                    .omnicore
                    .sessions
                    .active
                    .first
                    .unwrap_or(max)
                    .saturating_add_signed(movement)
                    .min(max);
                self.omnicore.sessions.active.first = (first != max).then_some(first);
            }
            self.omnicore.invalidate();
            true
        } else {
            false
        }
    }

    fn omnicore_answer_rows(&self, rect: Rect) -> impl Iterator<Item = String> + '_ {
        let columns = ((rect.right - rect.left - self.dpi.pixels(24)) / self.dpi.pixels(8).max(1))
            .max(1) as usize;
        let chat = &self.omnicore.sessions.active.chat;
        chat.turns.iter().flat_map(move |turn| {
            let (answer, tail) = match &turn.answer {
                Some(Ok(answer)) => (
                    answer.text.as_str(),
                    (!answer.complete).then_some("Respuesta incompleta."),
                ),
                Some(Err(error)) => (error.as_str(), None),
                None => ("Esperando respuesta…", None),
            };
            std::iter::once(format!("Tú · {}", turn.mode.label()))
                .chain(
                    turn.attachment
                        .as_ref()
                        .map(|title| format!("Adjunto · {title}")),
                )
                .chain(display_lines(&turn.question, columns))
                .chain(std::iter::once("OmniCore".into()))
                .chain(display_lines(answer, columns))
                .chain(tail.map(str::to_owned))
                .chain(std::iter::once(String::new()))
        })
    }

    fn omnicore_chat_capacity(&self, rect: Rect) -> usize {
        let dpi = self.dpi;
        ((rect.bottom - rect.top - dpi.pixels(HEADER) - dpi.pixels(CHAT_FOOTER + 8)).max(0)
            / dpi.pixels(CHAT_ROW).max(1))
        .min(128) as usize
    }

    fn omnicore_session_capacity(&self, rect: Rect) -> usize {
        ((rect.bottom - rect.top - self.dpi.pixels(SESSIONS_TOP + SESSIONS_FOOTER)).max(0)
            / self.dpi.pixels(ROW).max(1))
        .min(64) as usize
    }

    fn omnicore_sessions_presentation(
        &self,
        rect: Rect,
        buttons: &mut Vec<(Rect, Target)>,
        labels: &mut Vec<(Rect, String, u32, u32)>,
    ) {
        let panel = &self.omnicore;
        let dpi = self.dpi;
        let top = rect.top + dpi.pixels(HEADER);
        let group = Rect {
            left: rect.left + dpi.pixels(12),
            top,
            right: rect.right - dpi.pixels(12),
            bottom: top + dpi.pixels(27),
        };
        let footer = Rect {
            top: rect.bottom - dpi.pixels(SESSIONS_FOOTER - 5),
            bottom: rect.bottom - dpi.pixels(8),
            ..group
        };
        let width = (footer.right - footer.left) / 3;
        for (i, (text, target)) in [
            ("Carpeta…".into(), Target::ChooseWorkspace),
            (
                if panel.show_changes {
                    "Ver sesiones".into()
                } else {
                    format!("Cambios · {}", panel.rows.len())
                },
                Target::Changes,
            ),
            if panel.executable.is_some() {
                ("Terminal".into(), Target::Session)
            } else {
                ("Conectar…".into(), Target::ChooseExecutable)
            },
        ]
        .into_iter()
        .enumerate()
        {
            let left = footer.left + i as i32 * width;
            let button = Rect {
                left,
                right: left + width - dpi.pixels(3),
                ..footer
            };
            labels.push((button, text, shell::TEXT, shell::SURFACE));
            buttons.push((button, target));
        }

        let page = self.omnicore_session_capacity(rect);
        let row = |index: usize| {
            let top = rect.top + dpi.pixels(SESSIONS_TOP) + index as i32 * dpi.pixels(ROW);
            Rect {
                top,
                bottom: top + dpi.pixels(ROW),
                ..group
            }
        };
        if panel.show_changes {
            labels.push((
                group,
                "Cambios de OmniCore".into(),
                shell::TEXT,
                shell::RAISED,
            ));
            if panel.rows.is_empty() && page > 0 {
                labels.push((
                    row(0),
                    "Aún no hay archivos modificados.".into(),
                    shell::MUTED,
                    shell::SIDEBAR,
                ));
            }
            for (index, file) in panel.rows.iter().skip(panel.first).take(page).enumerate() {
                let (badge, color) = match file.kind {
                    ChangeKind::Added => ("+", 0x0089c495),
                    ChangeKind::Modified => ("~", 0x00a5bfef),
                    ChangeKind::Deleted => ("−", 0x008698ec),
                    ChangeKind::Unknown => ("·", shell::MUTED),
                };
                let path = panel
                    .root
                    .as_ref()
                    .and_then(|root| file.path.strip_prefix(root).ok())
                    .unwrap_or(&file.path);
                let area = row(index);
                labels.push((
                    area,
                    format!("{badge}  {}", safe_label(&path.to_string_lossy())),
                    color,
                    shell::SIDEBAR,
                ));
                if matches!(file.kind, ChangeKind::Added | ChangeKind::Modified) {
                    buttons.push((area, Target::Effect(file.effect_id.clone())));
                }
            }
            return;
        }

        let new_chat = Rect {
            left: group.right - dpi.pixels(62),
            ..group
        };
        let folder = Rect {
            right: new_chat.left - dpi.pixels(4),
            ..group
        };
        let workspace = self
            .project_root
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(
                || "Espacio de OmniCore".into(),
                |name| safe_label(&name.to_string_lossy()),
            );
        labels.push((folder, workspace, shell::TEXT, shell::RAISED));
        labels.push((new_chat, "+ Nueva".into(), shell::ACCENT, shell::RAISED));
        buttons.extend([(folder, Target::ToggleFolder), (new_chat, Target::NewChat)]);
        if panel.collapse_folder {
            return;
        }
        let first = panel
            .sessions_first
            .min(panel.sessions.len().saturating_sub(page.max(1)));
        for (index, session) in panel.sessions.iter().skip(first).take(page).enumerate() {
            let active = session.id == panel.sessions.active.id;
            let running = panel
                .query
                .as_ref()
                .is_some_and(|query| query.session == session.id);
            let prefix = if running {
                "…  "
            } else if active {
                "•  "
            } else {
                "    "
            };
            let area = row(index);
            labels.push((
                area,
                format!("{prefix}{}", safe_label(session.title())),
                shell::TEXT,
                if active {
                    shell::SELECTED
                } else {
                    shell::SIDEBAR
                },
            ));
            buttons.push((area, Target::PickSession(session.id)));
        }
    }

    pub(super) fn omnicore_presentation(&self) -> Option<Frame> {
        let rect = self.omnicore_region()?;
        let dpi = self.dpi;
        let side = dpi.pixels(12);
        let inset = |left, top, right, bottom| Rect {
            left: rect.left + dpi.pixels(left),
            top: rect.top + dpi.pixels(top),
            right: rect.right - dpi.pixels(right),
            bottom: (rect.top + dpi.pixels(bottom)).min(rect.bottom),
        };
        let hide = Rect {
            left: rect.right - dpi.pixels(30),
            ..inset(0, 5, 8, 29)
        };
        let split = |area: Rect, trailing: i32| {
            let right = area.right - dpi.pixels(trailing);
            (
                Rect {
                    right: right - dpi.pixels(4),
                    ..area
                },
                Rect {
                    left: right,
                    ..area
                },
            )
        };
        let panel = &self.omnicore;
        let tabs = inset(12, 33, 12, 59);
        let middle = tabs.left + (tabs.right - tabs.left) / 2;
        let conversation = Rect {
            right: middle - dpi.pixels(3),
            ..tabs
        };
        let sessions = Rect {
            left: middle + dpi.pixels(3),
            ..tabs
        };
        let mut buttons = vec![
            (hide, Target::Hide),
            (conversation, Target::Conversation),
            (sessions, Target::Sessions),
        ];
        let description = panel
            .chat_notice
            .as_deref()
            .or(panel.status.as_deref())
            .or_else(|| {
                (panel.ready && !panel.ask_supported)
                    .then_some("Esta versión de OmniCore requiere actualización.")
            })
            .or_else(|| {
                (panel.query.is_some() && !panel.querying_here())
                    .then_some("OmniCore está respondiendo en otra sesión.")
            })
            .or_else(|| match &panel.model_catalog {
                Some(Err(error)) if !panel.show_sessions => Some(error),
                _ => None,
            })
            .unwrap_or(if panel.show_sessions {
                "Conversaciones de la carpeta activa"
            } else {
                ""
            });
        let mut labels = vec![
            (
                inset(12, 6, 36, 29),
                "OmniCore".into(),
                shell::TEXT,
                shell::SIDEBAR,
            ),
            (hide, "×".into(), shell::TEXT, shell::SIDEBAR),
            (
                conversation,
                "Chat".into(),
                shell::TEXT,
                if panel.show_sessions {
                    shell::SIDEBAR
                } else {
                    shell::SELECTED
                },
            ),
            (
                sessions,
                "Sesiones".into(),
                shell::TEXT,
                if panel.show_sessions {
                    shell::SELECTED
                } else {
                    shell::SIDEBAR
                },
            ),
            (
                inset(12, 60, 12, 80),
                safe_label(description),
                shell::MUTED,
                shell::SIDEBAR,
            ),
        ];
        if !panel.show_sessions && panel.can_compose() {
            let model_button = inset(88, 4, 38, 28);
            let loading = panel.catalog_request.is_some();
            let caption = if loading {
                "Cargando modelos…".into()
            } else if matches!(&panel.model_catalog, Some(Err(_))) {
                "Recargar modelos ↻".into()
            } else if let Some(model) = &panel.model {
                format!("Modelo ▾  {}", model.id)
            } else {
                "Modelo ▾  Automático".into()
            };
            labels.push((
                model_button,
                safe_label(&caption),
                if loading { shell::MUTED } else { shell::TEXT },
                shell::SURFACE,
            ));
            if !loading {
                buttons.push((model_button, Target::ChooseModel));
            }
        }
        let top = rect.top + dpi.pixels(HEADER);
        let mut composer_card = None;
        let mut draft_layout = None;
        if !panel.show_sessions && panel.can_compose() {
            let line_height = dpi.pixels(CHAT_ROW).max(1);
            let composer = Rect {
                left: rect.left + side,
                top: rect.bottom - dpi.pixels(CHAT_FOOTER),
                right: rect.right - side,
                bottom: rect.bottom - dpi.pixels(9),
            };
            composer_card = Some(composer);
            let field = Rect {
                left: composer.left + dpi.pixels(11),
                top: composer.top + dpi.pixels(9),
                right: composer.right - dpi.pixels(11),
                bottom: composer.top + dpi.pixels(47),
            };
            let active_file = Rect {
                top: field.bottom + dpi.pixels(3),
                bottom: composer.top + dpi.pixels(72),
                ..field
            };
            let include = !panel.sessions.active.omit_active_file;
            labels.push((
                active_file,
                format!("{} Incluir archivo activo", if include { "☑" } else { "☐" }),
                shell::TEXT,
                if include {
                    shell::SELECTED
                } else {
                    shell::SURFACE
                },
            ));
            buttons.push((active_file, Target::ToggleActiveFile));
            let attachment = Rect {
                top: active_file.bottom + dpi.pixels(2),
                bottom: composer.top + dpi.pixels(96),
                ..field
            };
            if let Some(context) = &panel.sessions.active.chat.attachment {
                let (pill, remove) = split(attachment, 23);
                labels.push((
                    pill,
                    safe_label(&context.title),
                    shell::ACCENT,
                    shell::RAISED,
                ));
                labels.push((remove, "×".into(), shell::TEXT, shell::RAISED));
                buttons.push((remove, Target::RemoveAttachment));
            } else {
                labels.push((
                    attachment,
                    "＋ Adjuntar código".into(),
                    shell::TEXT,
                    shell::SURFACE,
                ));
                buttons.push((attachment, Target::Attach));
            }
            let (mode, ask) = split(
                Rect {
                    top: attachment.bottom + dpi.pixels(1),
                    bottom: composer.bottom - dpi.pixels(7),
                    ..field
                },
                75,
            );
            draft_layout = Some(composer::Layout::new(
                field,
                &panel.sessions.active.draft,
                panel.sessions.active.selection,
                self.omnicore_field_focused(),
                self.ui_font,
                dpi,
            ));
            buttons.push((field, Target::Draft));
            let rows = self.omnicore_chat_capacity(rect);
            let max = self.omnicore_answer_rows(rect).count().saturating_sub(rows);
            let first = self.omnicore.sessions.active.first.unwrap_or(max).min(max);
            for (index, line) in self
                .omnicore_answer_rows(rect)
                .skip(first)
                .take(rows)
                .enumerate()
            {
                let top = top + index as i32 * line_height;
                let text_color = if line.starts_with("Tú · ") || line == "OmniCore" {
                    shell::ACCENT
                } else {
                    shell::TEXT
                };
                labels.push((
                    Rect {
                        top,
                        bottom: top + line_height,
                        ..composer
                    },
                    line,
                    text_color,
                    shell::SIDEBAR,
                ));
            }
            let modes: &[Mode] = if panel.edit_supported {
                &[Mode::Ask, Mode::Plan, Mode::Edit]
            } else {
                &[Mode::Ask, Mode::Plan]
            };
            let width = mode.right - mode.left;
            for (index, &choice) in modes.iter().enumerate() {
                let count = modes.len() as i32;
                let index = index as i32;
                let area = Rect {
                    left: mode.left + width * index / count,
                    right: mode.left + width * (index + 1) / count - dpi.pixels(4),
                    ..mode
                };
                labels.push((
                    area,
                    choice.label().into(),
                    shell::TEXT,
                    if choice == self.omnicore.sessions.active.chat.mode {
                        shell::SELECTED
                    } else {
                        shell::SURFACE
                    },
                ));
                buttons.push((area, Target::Mode(choice)));
            }
            let cancelling = panel.query.as_ref().is_some_and(|query| {
                query.session == panel.sessions.active.id && query.child.cancelling()
            });
            let actionable = (panel.querying_here() && !cancelling)
                || (panel.query.is_none() && !panel.sessions.active.draft.trim().is_empty());
            let caption = if cancelling {
                "Cancelando"
            } else if panel.querying_here() {
                "Cancelar"
            } else if panel.query.is_some() {
                "Ocupado"
            } else {
                "Enviar"
            };
            labels.push((
                ask,
                caption.into(),
                if actionable {
                    shell::BACKGROUND
                } else {
                    shell::MUTED
                },
                if actionable {
                    shell::ACCENT
                } else {
                    shell::RAISED
                },
            ));
            if actionable {
                buttons.push((ask, Target::Ask));
            }
        } else if !panel.show_sessions && (!panel.monitoring || panel.ready) {
            let (caption, target) = if panel.executable.is_some() && !panel.monitoring {
                ("Reintentar conexión", Target::Retry)
            } else {
                ("Conectar OmniCore…", Target::ChooseExecutable)
            };
            let action = inset(12, HEADER + 8, 12, HEADER + 39);
            labels.push((action, caption.into(), shell::TEXT, shell::RAISED));
            buttons.push((action, target));
            if panel.executable.is_some() && !panel.monitoring {
                let choose = inset(12, HEADER + 45, 12, HEADER + 73);
                labels.push((
                    choose,
                    "Elegir OmniCore…".into(),
                    shell::TEXT,
                    shell::SURFACE,
                ));
                buttons.push((choose, Target::ChooseExecutable));
            }
        }
        if panel.show_sessions {
            self.omnicore_sessions_presentation(rect, &mut buttons, &mut labels);
        }
        Some(Frame {
            hits: Hits {
                stamp: self.omnicore.stamp.clone(),
                rect,
                buttons,
                draft: draft_layout
                    .as_ref()
                    .map_or_else(Vec::new, composer::Layout::hits),
            },
            labels,
            dpi,
            composer_card,
            draft: draft_layout,
            folder_open: !panel.collapse_folder,
        })
    }
}

pub(super) unsafe fn confirm_edit(hwnd: Hwnd) {
    let Some(owner) = super::owner(hwnd) else {
        return;
    };
    let prepared = owner.state.borrow().edit_request();
    let request = match prepared {
        Ok(Some(request)) => request,
        Ok(None) => return,
        Err(error) => {
            let mut state = owner.state.borrow_mut();
            state.omnicore.chat_notice = Some(error);
            state.omnicore.invalidate();
            return;
        }
    };
    let model = request
        .model
        .as_ref()
        .map_or("Automático", |model| &model.id);
    let context = request
        .outgoing
        .attachment_title
        .as_ref()
        .map_or_else(String::new, |title| format!("\nContexto adjunto: {title}"));
    let message = format!(
        "OmniCore podrá crear o modificar archivos de esta carpeta según su política.\n\nCarpeta: {}\nModelo: {}{context}\n\n{}",
        request.root.display(),
        safe_label(model),
        request.draft,
    );
    let accepted = super::message_dialog::confirm_review(
        hwnd,
        "Enviar edición a OmniCore",
        &message,
        "Enviar edición",
    );
    let Some(owner) = super::owner(hwnd) else {
        return;
    };
    let mut state = owner.state.borrow_mut();
    match accepted {
        Ok(true) => state.send_omnicore(Some(request)),
        Ok(false) => {}
        Err(error) => state.error(error),
    }
}

pub(super) unsafe fn choose_model(hwnd: Hwnd) {
    let Some(owner) = super::owner(hwnd) else {
        return;
    };
    let (models, current) = {
        let state = owner.state.borrow();
        let Some(Ok(models)) = &state.omnicore.model_catalog else {
            return;
        };
        (models.clone(), state.omnicore.model.clone())
    };
    let check = |selected| if selected { "✓ " } else { "" };
    let labels = std::iter::once(format!(
        "{}Automático · configuración de OmniCore",
        check(current.is_none())
    ))
    .chain(models.iter().map(|model| {
        safe_label(&format!(
            "{}{} · {}",
            check(current.as_ref() == Some(model)),
            model.provider,
            model.id
        ))
    }))
    .chain(std::iter::once("Actualizar lista de modelos".into()))
    .collect::<Vec<_>>();
    let selection = super::message_dialog::form::choose(
        hwnd,
        "Modelo de OmniCore",
        if models.is_empty() {
            "OmniCore no tiene modelos de texto configurados."
        } else {
            "Elige el proveedor y modelo para la próxima consulta."
        },
        &labels,
    );
    let Some(owner) = super::owner(hwnd) else {
        return;
    };
    let mut state = owner.state.borrow_mut();
    if !matches!(&state.omnicore.model_catalog, Some(Ok(active)) if Rc::ptr_eq(active, &models)) {
        return;
    }
    match selection {
        Ok(Some(index)) if index == labels.len() - 1 => state.refresh_omnicore_models(),
        Ok(Some(index)) if index < labels.len() - 1 => {
            state.set_omnicore_model(index.checked_sub(1).and_then(|index| models.get(index)));
        }
        Ok(_) => {}
        Err(error) => state.error(error),
    }
}

impl State {
    fn set_omnicore_model(&mut self, model: Option<&Model>) {
        self.omnicore.model = model.cloned();
        self.omnicore.chat_notice = None;
        self.omnicore.focused = true;
        self.omnicore.invalidate();
    }
}

pub(super) unsafe fn paste(hwnd: Hwnd, stamp: Rc<()>) {
    let Some(model) = super::owner(hwnd) else {
        return;
    };
    let capacity = {
        let state = model.state.borrow();
        if state.smoke
            || !state.omnicore_field_focused()
            || !Rc::ptr_eq(&state.omnicore.stamp, &stamp)
        {
            return;
        }
        state.omnicore.draft_capacity()
    };
    let text = super::clipboard::read_limited(hwnd, capacity);
    let Some(model) = super::owner(hwnd) else {
        return;
    };
    let mut state = model.state.borrow_mut();
    if !state.omnicore_field_focused() || !Rc::ptr_eq(&state.omnicore.stamp, &stamp) {
        return;
    }
    match text {
        Ok(Some(text)) => {
            let multiline = text.replace("\r\n", "\n").replace('\r', "\n");
            state.edit_omnicore(FieldEdit::Insert(&multiline), false);
        }
        Ok(None) => {}
        Err(error) => state.error(error),
    }
}

pub(super) unsafe fn draw(dc: Hdc, frame: &Frame) -> Result<(), String> {
    let px = |value| frame.dpi.pixels(value);
    paint::fill(dc, frame.hits.rect, shell::SIDEBAR)?;
    paint::fill(
        dc,
        Rect {
            right: frame.hits.rect.left + px(1),
            ..frame.hits.rect
        },
        shell::SURFACE,
    )?;
    if let Some(card) = frame.composer_card {
        composer::rounded(dc, card, px(12), shell::BACKGROUND)?;
    }
    for (area, label, text, background) in &frame.labels {
        let target = frame
            .hits
            .buttons
            .iter()
            .find(|(rect, _)| same(*rect, *area))
            .map(|(_, target)| target);
        if *background != shell::SIDEBAR {
            composer::rounded(dc, *area, px(6), *background)?;
        }
        let leading = if matches!(target, Some(Target::ToggleFolder)) {
            shell::chevron(
                dc,
                area.left + px(9),
                area.top + px(10),
                frame.folder_open,
                shell::MUTED,
            )?;
            shell::file_icon(
                dc,
                area.left + px(21),
                area.top + px(8),
                frame.folder_open,
                shell::ACCENT,
            )?;
            42
        } else {
            9
        };
        let interactive = target.is_some() || *background != shell::SIDEBAR;
        let label_box = Rect {
            left: area.left + if interactive { px(leading) } else { 0 },
            right: area.right - if interactive { px(4) } else { 0 },
            top: area.top
                + if interactive {
                    ((area.bottom - area.top) - px(17)).max(0) / 2
                } else {
                    0
                },
            ..*area
        };
        composer::text(dc, label_box, label, *text)?;
    }
    if let Some(draft) = &frame.draft {
        draft.draw(dc)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::open_diff_panel::OmniCorePreview;

    fn context_model(state: &mut State, tokens: usize) {
        let model = Model {
            id: "local".into(),
            provider: "prueba".into(),
            input_tokens: Some(tokens),
        };
        state.omnicore.model_catalog = Some(Ok(Rc::new(vec![model.clone()])));
        state.omnicore.model = Some(model);
        state.omnicore.context_supported = true;
    }

    fn click(state: &mut State, pick: impl Fn(&Target) -> bool) {
        let frame = state.omnicore_presentation().unwrap();
        let area = frame
            .hits
            .buttons
            .iter()
            .find(|(_, target)| pick(target))
            .unwrap()
            .0;
        state.omnicore.hits = Some(frame.hits());
        assert!(matches!(
            state.omnicore_click(area.left + 1, area.top + 1),
            Some(Action::None)
        ));
    }

    #[test]
    fn omnicore_edit_requires_confirmation_of_the_current_draft() {
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.omnicore.visible = true;
        state.omnicore.ready = true;
        state.omnicore.ask_supported = true;
        state.omnicore.edit_supported = true;
        context_model(&mut state, 4096);
        state.omnicore.root = Some(std::env::temp_dir());
        state.omnicore.executable = Some(PathBuf::from("omni.exe"));
        state
            .workbench
            .new_file("edicion.p", jc_text::TextEncoding::Utf8);
        state.workbench.insert_text("DISPLAY 1.").unwrap();
        state.omnicore.sessions.active.chat.mode = Mode::Edit;
        state.omnicore.sessions.active.draft = "Crea el ejemplo.".into();

        assert!(matches!(
            state.submit_omnicore(),
            Action::ConfirmOmniCoreEdit
        ));
        state.send_omnicore(None);
        assert!(state.omnicore.query.is_none());

        let approved = state.edit_request().unwrap().unwrap();
        assert!(approved.outgoing.input.contains("DISPLAY 1."));
        state.workbench.insert_text(" DISPLAY 2.").unwrap();
        assert!(state.edit_request().unwrap().as_ref() != Some(&approved));
        state.send_omnicore(Some(approved));
        assert!(state.omnicore.query.is_none());

        let approved = state.edit_request().unwrap().unwrap();
        state
            .omnicore
            .sessions
            .active
            .draft
            .push_str(" O cambia otro archivo.");
        state.send_omnicore(Some(approved));
        assert!(state.omnicore.query.is_none());
        assert!(state.omnicore.sessions.active.chat.turns.is_empty());
    }

    #[test]
    fn omnicore_disconnected_panel_offers_connection_instead_of_an_inert_composer() {
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.width = 1000;
        state.height = 650;
        state.omnicore.visible = true;
        state.omnicore.ready = true;
        let frame = state.omnicore_presentation().unwrap();
        assert!(frame.draft.is_none());
        assert!(frame
            .hits
            .buttons
            .iter()
            .any(|(_, target)| matches!(target, Target::ChooseExecutable)));
        assert!(frame
            .hits
            .buttons
            .iter()
            .all(|(_, target)| !matches!(target, Target::Draft | Target::Ask)));
    }

    #[test]
    fn omnicore_composer_switch_controls_the_current_unsaved_file() {
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.width = 1000;
        state.height = 650;
        state.omnicore.visible = true;
        state.omnicore.ready = true;
        state.omnicore.ask_supported = true;
        context_model(&mut state, 4096);
        let prepare = |state: &State| {
            state
                .prepare_omnicore(state.omnicore.input_capacity().unwrap())
                .unwrap()
        };
        assert!(prepare(&state).attachment_title.is_none());
        state
            .workbench
            .new_file("vista.rs", jc_text::TextEncoding::Utf8);
        state.workbench.insert_text("fn consultar() {}").unwrap();
        state.workbench.set_linear_selection_bytes(0, 2).unwrap();
        let context = prepare(&state);
        assert_eq!(
            context.attachment_title.as_deref(),
            Some("vista.rs · archivo")
        );
        assert!(context.input.contains("fn consultar() {}"));
        assert!(state
            .omnicore_presentation()
            .unwrap()
            .labels
            .iter()
            .any(|(_, label, _, _)| label == "☑ Incluir archivo activo"));
        click(&mut state, |target| {
            matches!(target, Target::ToggleActiveFile)
        });
        assert!(prepare(&state).attachment_title.is_none());
        assert!(state
            .omnicore_presentation()
            .unwrap()
            .labels
            .iter()
            .any(|(_, label, _, _)| label == "☐ Incluir archivo activo"));

        click(&mut state, |target| {
            matches!(target, Target::ToggleActiveFile)
        });
        state.attach_current_code_to_omnicore();
        assert_eq!(
            prepare(&state).attachment_title.as_deref(),
            Some("vista.rs · selección")
        );
        let chat = &state.omnicore.sessions.active.chat;
        assert_eq!(chat.attachment.as_ref().unwrap().text, "fn");
        click(&mut state, |target| {
            matches!(target, Target::RemoveAttachment)
        });

        state
            .workbench
            .new_file("otro.rs", jc_text::TextEncoding::Utf8);
        state.workbench.insert_text("fn nuevo() {}").unwrap();
        let context = prepare(&state);
        assert_eq!(
            context.attachment_title.as_deref(),
            Some("otro.rs · archivo")
        );
        assert!(context.input.contains("fn nuevo() {}"));
        assert!(!context.input.contains("fn consultar()"));
    }

    #[test]
    fn omnicore_large_file_context_starts_at_visible_line_and_cuts_on_utf8_boundary() {
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        context_model(&mut state, 2048);
        state
            .workbench
            .new_file("grande.txt", jc_text::TextEncoding::Utf8);
        let text = format!("fuera\nX{}\nfin", "ñ\"".repeat(5000));
        state.workbench.insert_text(&text).unwrap();
        let pane = state.workbench.panes().active();
        let scroll = state.workbench.active_editor().unwrap().scroll_line;
        state
            .workbench
            .scroll_view_vertical(pane, 1 - scroll as isize, 1);
        let prepare = |state: &State| {
            let capacity = state.omnicore.input_capacity().unwrap();
            let outgoing = state.prepare_omnicore(capacity).unwrap();
            assert!(outgoing.input.len() <= capacity);
            outgoing
        };
        let small = prepare(&state);
        assert_eq!(
            small.attachment_title.as_deref(),
            Some("grande.txt · extracto del archivo")
        );
        assert!(small.input.contains(r#"Xñ\"ñ\""#));
        assert!(!small.input.contains("fuera"));
        context_model(&mut state, 32768);
        let large = prepare(&state);
        assert_eq!(
            large.attachment_title.as_deref(),
            Some("grande.txt · archivo")
        );
        assert!(large.input.contains("fuera"));
        assert!(large.input.contains("fin"));
        assert!(large.input.len() > 8192);
        assert_eq!(state.workbench.active_bytes().unwrap(), text.as_bytes());
    }

    #[test]
    fn omnicore_attaches_unsaved_selection_without_changing_the_editor() {
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.omnicore.visible = true;
        context_model(&mut state, 4096);
        state
            .workbench
            .new_file("ejemplo.p", jc_text::TextEncoding::Utf8);
        let original = "inicio\nDISPLAY \"México\".\nfin";
        let selection = "DISPLAY \"México\".";
        state.workbench.insert_text(original).unwrap();
        state
            .workbench
            .set_linear_selection_bytes(7, 7 + selection.len())
            .unwrap();
        state.attach_current_code_to_omnicore();
        let attachment = state
            .omnicore
            .sessions
            .active
            .chat
            .attachment
            .as_ref()
            .unwrap();
        assert_eq!(attachment.title, "ejemplo.p · selección");
        assert_eq!(attachment.text, selection);
        assert_eq!(state.workbench.active_bytes().unwrap(), original.as_bytes());
        assert_eq!(
            state.workbench.active_editor().unwrap().selection(),
            Some(7..7 + selection.len()),
        );
    }

    #[test]
    fn omnicore_additions_open_as_source_and_edits_keep_an_existing_dirty_buffer() {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../work/test-temp");
        let directory = base.join(format!("omnicore-open-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let root = directory.canonicalize().unwrap();
        let file = root.join("cambio.txt");
        std::fs::write(&file, "nuevo").unwrap();

        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        assert!(state.project_root.is_none());
        state.omnicore.root = Some(root.clone());
        state.activate_omnicore_effect(Opened {
            path: file.clone(),
            kind: ChangeKind::Added,
            preview: None,
            unavailable: None,
        });
        assert_eq!(state.workbench.active_bytes().unwrap(), b"nuevo");
        assert!(!state.open_diff_visible);

        state.workbench.insert_text("local ").unwrap();
        std::fs::write(&file, "editado fuera").unwrap();
        state.activate_omnicore_effect(Opened {
            path: file.clone(),
            kind: ChangeKind::Modified,
            preview: Some(
                OmniCorePreview::compute("nuevo".into(), "editado fuera".into()).unwrap(),
            ),
            unavailable: None,
        });
        assert!(state.open_diff_visible);
        assert_eq!(state.workbench.active_bytes().unwrap(), b"local nuevo");
        state.clear_omnicore_diff();
        assert_eq!(std::fs::read(&file).unwrap(), b"editado fuera");
        drop(state);
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    #[ignore = "requiere un OmniCore de prueba, LOCALAPPDATA aislado y proveedor local"]
    fn omnicore_empty_editor_connects_and_answers_from_the_native_panel() {
        use std::{
            thread,
            time::{Duration, Instant},
        };
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.width = 1000;
        state.height = 650;
        assert!(state.project_root.is_none());
        state.workbench.close_active(false).unwrap();
        assert!(state.workbench.active_path().is_none());
        state.toggle_omnicore();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !state.omnicore.ready || state.omnicore.model_catalog.is_none() {
            state.poll_omnicore();
            assert!(Instant::now() < deadline, "{:?}", state.omnicore.status);
            thread::sleep(Duration::from_millis(20));
        }
        assert!(state.omnicore.ask_supported);
        assert!(state
            .omnicore
            .root
            .as_ref()
            .unwrap()
            .ends_with("Espacio de trabajo"));
        let models = state
            .omnicore
            .model_catalog
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .clone();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].provider, "cide-fixture");
        assert_eq!(models[1].provider, "cide-sin-servicio");
        let model_button = state
            .omnicore_presentation()
            .unwrap()
            .hits
            .buttons
            .iter()
            .find(|(_, target)| matches!(target, Target::ChooseModel))
            .unwrap()
            .0;
        state.omnicore.hits = Some(state.omnicore_presentation().unwrap().hits());
        assert!(matches!(
            state.omnicore_click(model_button.left + 1, model_button.top + 1),
            Some(Action::ChooseOmniCoreModel)
        ));
        state.set_omnicore_model(Some(&models[1]));
        state.omnicore.sessions.active.draft = "¿Consulta inicial desde CIDE?".into();
        state.ask_omnicore();
        let deadline = Instant::now() + Duration::from_secs(35);
        while state.omnicore.query.is_some() {
            state.poll_omnicore();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(20));
        }
        let failed = state.omnicore.sessions.active.chat.turns[0]
            .answer
            .as_ref()
            .unwrap()
            .as_ref()
            .err()
            .unwrap();
        assert!(
            failed.starts_with("No se pudo conectar con cide-sin-servicio"),
            "{failed}"
        );
        assert_eq!(
            state.omnicore.sessions.active.draft,
            "¿Consulta inicial desde CIDE?"
        );
        state.set_omnicore_model(Some(&models[0]));
        let frame = state.omnicore_presentation().unwrap();
        let draft = frame
            .hits
            .buttons
            .iter()
            .find(|(_, target)| matches!(target, Target::Draft))
            .unwrap()
            .0;
        state.omnicore.hits = Some(frame.hits());
        assert!(matches!(
            state.omnicore_click(draft.left + 1, draft.top + 1),
            Some(Action::None)
        ));
        assert!(matches!(
            state.omnicore_key(0x0d, Modifiers::NONE),
            Some(Action::None)
        ));
        let deadline = Instant::now() + Duration::from_secs(35);
        while state.omnicore.query.is_some() {
            state.poll_omnicore();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(20));
        }
        let answer = state
            .omnicore
            .sessions
            .active
            .chat
            .turns
            .back()
            .unwrap()
            .answer
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap();
        assert!(answer.complete, "{}", answer.text);
        assert_eq!(answer.text, "Respuesta sintética desde OmniCore");
        assert!(state.workbench.active_path().is_none());
        state
            .workbench
            .new_file("ejemplo.p", jc_text::TextEncoding::Utf8);
        state.workbench.insert_text("DISPLAY \"México\".").unwrap();
        let frame = state.omnicore_presentation().unwrap();
        let plan = frame
            .hits
            .buttons
            .iter()
            .find(|(_, target)| matches!(target, Target::Mode(Mode::Plan)))
            .unwrap()
            .0;
        state.omnicore.hits = Some(frame.hits());
        assert!(matches!(
            state.omnicore_click(plan.left + 1, plan.top + 1),
            Some(Action::None)
        ));
        assert!(state.omnicore.sessions.active.chat.mode == Mode::Plan);
        for ch in "Haz un plan con el contexto.".chars() {
            assert!(state.omnicore_character(ch));
        }
        assert!(matches!(
            state.omnicore_key(0x0d, Modifiers::NONE),
            Some(Action::None)
        ));
        let original_session = state.omnicore.sessions.active.id;
        for ch in "Borrador pendiente".chars() {
            assert!(state.omnicore_character(ch));
        }
        click(&mut state, |target| matches!(target, Target::Sessions));
        assert!(state.omnicore.show_sessions);
        assert!(state
            .omnicore_presentation()
            .unwrap()
            .hits
            .buttons
            .iter()
            .all(|(_, target)| !matches!(target, Target::Draft | Target::Ask)));
        click(&mut state, |target| matches!(target, Target::NewChat));
        assert!(state.omnicore.sessions.active.chat.turns.is_empty());
        assert!(state.omnicore.sessions.active.draft.is_empty());
        let deadline = Instant::now() + Duration::from_secs(35);
        while state.omnicore.query.is_some() {
            state.poll_omnicore();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(20));
        }
        assert!(state.omnicore.sessions.active.chat.turns.is_empty());
        click(&mut state, |target| matches!(target, Target::Sessions));
        click(&mut state, |target| matches!(target, Target::ToggleFolder));
        assert!(state
            .omnicore_presentation()
            .unwrap()
            .hits
            .buttons
            .iter()
            .all(|(_, target)| !matches!(target, Target::PickSession(_))));
        click(&mut state, |target| matches!(target, Target::ToggleFolder));
        click(
            &mut state,
            |target| matches!(target, Target::PickSession(id) if *id == original_session),
        );
        assert!(!state.omnicore.show_sessions);
        assert_eq!(state.omnicore.sessions.active.draft, "Borrador pendiente");
        assert_eq!(state.omnicore.sessions.active.chat.turns.len(), 3);
        assert!(matches!(
            state.omnicore.sessions.active.chat.turns[2].answer,
            Some(Ok(ref answer)) if answer.complete
        ));
        assert!(state.omnicore.sessions.active.chat.attachment.is_none());
        assert_eq!(
            state.omnicore.sessions.active.chat.turns[2]
                .attachment
                .as_deref(),
            Some("ejemplo.p · archivo")
        );
        assert_eq!(
            state.workbench.active_bytes().unwrap(),
            "DISPLAY \"México\".".as_bytes()
        );
        state
            .workbench
            .new_file("otro.p", jc_text::TextEncoding::Utf8);
        let long_file = format!(
            "{}DISPLAY \"Solo archivo activo\".",
            "/* contexto de prueba */\n".repeat(512)
        );
        assert!(long_file.len() > 8192);
        state.workbench.insert_text(&long_file).unwrap();
        for (include, question) in [
            (false, "Confirma la consulta sin contexto."),
            (true, "Confirma la consulta con contexto."),
        ] {
            click(&mut state, |target| {
                matches!(target, Target::ToggleActiveFile)
            });
            state.omnicore.sessions.active.draft = question.into();
            click(&mut state, |target| matches!(target, Target::Ask));
            let deadline = Instant::now() + Duration::from_secs(35);
            while state.omnicore.query.is_some() {
                state.poll_omnicore();
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(20));
            }
            let turn = state.omnicore.sessions.active.chat.turns.back().unwrap();
            assert_eq!(
                turn.attachment.as_deref(),
                include.then_some("otro.p · archivo")
            );
            assert!(matches!(turn.answer, Some(Ok(ref answer)) if answer.complete));
        }
    }
}
