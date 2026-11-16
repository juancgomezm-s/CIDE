//! Modal filesystem navigation over the existing bounded metadata catalog.
//! Only explicit owner events read metadata/open files; draw consumes cached rows.
use super::TuiApp;
use crate::{
    input::is_plain_text,
    state::{PromptKind, UiOverlay},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{
    explorer::{
        ExplorerLimits, ExplorerLoadState, ExplorerPresentation, ExplorerView, NodeId,
        ProjectExplorer,
    },
    FieldEdit, FieldSelection, Wakeup,
};
use std::path::{Component, Path, PathBuf};

pub(crate) struct PathAtom {
    pub range: std::ops::Range<usize>,
    pub column: usize,
    pub width: usize,
}
pub(crate) struct PathWindow {
    pub atoms: Vec<PathAtom>,
    pub cursor: usize,
}
pub(crate) fn path_window(input: &str, cursor: usize, width: usize) -> PathWindow {
    let mut source = 0;
    let mut cells = 0;
    let mut all = Vec::new();
    let requested = cursor.min(input.len());
    let cursor = if requested == input.len() {
        requested
    } else {
        jc_text::grapheme::graphemes(input)
            .scan(0, |offset, glyph| {
                let start = *offset;
                *offset += glyph.len();
                Some(start)
            })
            .take_while(|&start| start <= requested)
            .last()
            .unwrap_or(0)
    };
    for glyph in jc_text::grapheme::graphemes(input) {
        let next = source + glyph.len();
        let size = jc_text::grapheme::width(glyph).max(1);
        all.push(PathAtom {
            range: source..next,
            column: cells,
            width: size,
        });
        source = next;
        cells += size;
    }
    let cursor_cells = all
        .iter()
        .find(|atom| atom.range.start == cursor)
        .map_or(cells, |atom| atom.column);
    let desired = cursor_cells.saturating_sub(width.saturating_sub(1));
    let first = all
        .iter()
        .position(|atom| atom.column >= desired)
        .unwrap_or(all.len());
    let left = all.get(first).map_or(cursor_cells, |atom| atom.column);
    let atoms = all
        .into_iter()
        .skip(first)
        .take_while(|atom| atom.column + atom.width <= left + width)
        .map(|mut atom| {
            atom.column -= left;
            atom
        })
        .collect();
    PathWindow {
        atoms,
        cursor: cursor_cells
            .saturating_sub(left)
            .min(width.saturating_sub(1)),
    }
}

pub(crate) struct FileBrowser {
    pub(crate) kind: Option<PromptKind>,
    pub(crate) catalog: ProjectExplorer,
    pub(crate) view: ExplorerView,
    pub(crate) frame: Option<ExplorerPresentation>,
    pub(crate) field: FieldSelection,
    pub(crate) path_focused: bool,
}
impl FileBrowser {
    pub(crate) fn new(wakeup: Wakeup) -> Self {
        let limits = ExplorerLimits {
            max_nodes: 2049,
            max_pending: 1,
            max_expanded: 1,
            ..Default::default()
        };
        Self {
            kind: None,
            catalog: ProjectExplorer::new(wakeup, limits).expect("valid browser quotas"),
            view: Default::default(),
            frame: None,
            field: Default::default(),
            path_focused: false,
        }
    }
    pub(crate) fn active(&self) -> bool {
        self.kind.is_some()
    }
    pub(crate) fn clear(&mut self) {
        if self.active() {
            self.catalog.clear();
        }
        self.kind = None;
        self.frame = None;
        self.view = Default::default();
        self.field = Default::default();
        self.path_focused = false;
    }
    pub(crate) fn poll(&mut self) -> (bool, bool) {
        let changed = self
            .catalog
            .root()
            .map(Path::to_owned)
            .is_some_and(|root| self.catalog.poll(&root, 1));
        (changed, self.catalog.take_recovery())
    }
    pub(crate) fn prepare(&mut self, height: u16, width: u16) {
        if !self.active() {
            return;
        }
        let mut frame = self.view.prepare(
            &self.catalog,
            usize::from(height.saturating_sub(2).min(26).saturating_sub(8)),
            usize::from(width.saturating_sub(2).min(112).saturating_sub(4)),
        );
        // A normal Unicode filename stays readable; opaque names retain the
        // catalog's diagnostic escape and their original native acceptance path.
        let root_id = self.catalog.root_id();
        let has_parent = self.catalog.root().and_then(Path::parent).is_some();
        for row in &mut frame.rows {
            if Some(row.id) == root_id && has_parent {
                row.label = "..".into();
                continue;
            }
            if let Some(path) = self.catalog.path(row.id) {
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    let prefix = if Some(row.id) == root_id {
                        "[.] "
                    } else if row.is_dir {
                        "[+] "
                    } else {
                        "    "
                    };
                    row.label = format!(
                        "{prefix}{}",
                        jc_app::source_services::display_ui_label(name, 248)
                    );
                }
            }
        }
        self.frame = Some(frame);
    }
}

impl TuiApp {
    pub(crate) fn start_file_browser(&mut self, kind: PromptKind) -> Option<String> {
        if !matches!(kind, PromptKind::Open | PromptKind::OpenFolder) {
            return None;
        }
        let root = self
            .project_root
            .clone()
            .or_else(|| {
                self.workbench
                    .active_document()
                    .and_then(|d| d.session.path().parent().map(Path::to_owned))
            })
            .unwrap_or_else(|| self.git_cwd.clone());
        let root = if root.is_absolute() {
            root
        } else {
            self.git_cwd.join(root)
        };
        self.file_browser.kind = Some(kind);
        match self.navigate_browser(root) {
            Ok(input) => Some(input),
            Err(error) => {
                self.status = error;
                Some(String::new())
            }
        }
    }
    fn navigate_browser(&mut self, path: PathBuf) -> Result<String, String> {
        let path = normalized_absolute(&path)?;
        ordinary_directory(&path)?;
        // The scoped Windows directory reader compares the final handle name
        // with the canonical root (including its native verbatim prefix).
        // Reject reparses before resolving spelling; then revalidate the result.
        let path = path
            .canonicalize()
            .map_err(|_| "La carpeta no pudo resolverse")?;
        ordinary_directory(&path)?;
        self.file_browser.catalog.set_root(path.clone())?;
        self.file_browser.view = Default::default();
        self.file_browser.frame = None;
        self.file_browser.path_focused = false;
        let input = path
            .to_str()
            .filter(|value| !value.chars().any(char::is_control))
            .unwrap_or("")
            .to_owned();
        self.file_browser.field.select_all(&input);
        Ok(input)
    }
    fn browser_path(&self, input: &str) -> Result<PathBuf, String> {
        if input.is_empty() {
            return Err("La ruta no puede estar vacía".into());
        }
        let value = PathBuf::from(input);
        let path = if value.is_absolute() {
            value
        } else {
            self.file_browser
                .catalog
                .root()
                .unwrap_or(&self.git_cwd)
                .join(value)
        };
        normalized_absolute(&path)
    }
    pub(crate) fn browser_field_budget(&self, input: &str) -> usize {
        4096usize
            .saturating_sub(
                input.len().saturating_sub(
                    self.file_browser
                        .field
                        .range()
                        .map_or(0, |range| range.len()),
                ),
            )
            .max(1)
    }
    pub(crate) fn edit_browser_field(&mut self, edit: FieldEdit<'_>, selecting: bool) {
        let UiOverlay::Prompt { kind, mut input } = self.overlay.clone() else {
            return;
        };
        if !self.file_browser.active() {
            return;
        }
        if let FieldEdit::Insert(text) = edit {
            if text.chars().any(char::is_control) {
                self.status = "La ruta no admite saltos de línea ni controles".into();
                return;
            }
        }
        self.file_browser.path_focused = true;
        match jc_app::edit_selected_field(
            &mut input,
            &mut self.file_browser.field,
            edit,
            selecting,
            4096,
        ) {
            Ok(_) => self.overlay = UiOverlay::Prompt { kind, input },
            Err(error) => self.status = error.into(),
        }
    }
    pub(crate) fn handle_browser_key(&mut self, key: KeyEvent, kind: PromptKind, input: String) {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let selecting = key.modifiers.contains(KeyModifiers::SHIFT);
        let page = self
            .last_frame_height
            .saturating_sub(2)
            .min(26)
            .saturating_sub(8)
            .max(1) as isize;
        if key.code == KeyCode::Esc {
            self.close_overlay();
            return;
        }
        if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
            self.file_browser.path_focused = !self.file_browser.path_focused;
            return;
        }
        if matches!(key.code, KeyCode::Char('l' | 'a')) && control {
            self.file_browser.path_focused = true;
            self.file_browser.field.select_all(&input);
            return;
        }
        if key.code == KeyCode::F(5) {
            if let Some(id) = self.file_browser.catalog.root_id() {
                if let Err(error) = self.file_browser.catalog.refresh(id) {
                    self.status = error;
                }
            }
            return;
        }
        if (key.code == KeyCode::Up && alt)
            || (key.code == KeyCode::Backspace && !self.file_browser.path_focused)
        {
            if let Some(parent) = self
                .file_browser
                .catalog
                .root()
                .and_then(Path::parent)
                .map(Path::to_owned)
            {
                self.browser_navigate_prompt(kind, parent);
            }
            return;
        }
        if key.code == KeyCode::Enter {
            if kind == PromptKind::OpenFolder && control {
                if self.file_browser.path_focused {
                    match self.browser_path(&input) {
                        Ok(path) => self.choose_browser_folder(path),
                        Err(error) => self.status = error,
                    }
                } else {
                    self.accept_browser_folder();
                }
            } else if self.file_browser.path_focused {
                match self.browser_path(&input) {
                    Ok(path) => match std::fs::symlink_metadata(&path) {
                        Ok(metadata) if metadata.is_dir() && !reparse(&metadata) => {
                            self.browser_navigate_prompt(kind, path)
                        }
                        Ok(metadata)
                            if metadata.is_file()
                                && !reparse(&metadata)
                                && kind == PromptKind::Open =>
                        {
                            self.accept_typed_browser_file(path, control)
                        }
                        _ => {
                            self.status =
                                "La ruta no es un archivo o carpeta ordinarios disponibles".into()
                        }
                    },
                    Err(error) => self.status = error,
                }
            } else {
                self.file_browser
                    .view
                    .prepare(&self.file_browser.catalog, 0, 0);
                if let Some(id) = self.file_browser.view.selected() {
                    self.activate_browser_node(kind, id, control);
                }
            }
            return;
        }
        if !self.file_browser.path_focused {
            match key.code {
                KeyCode::Up => self
                    .file_browser
                    .view
                    .move_selection(&self.file_browser.catalog, -1),
                KeyCode::Down => self
                    .file_browser
                    .view
                    .move_selection(&self.file_browser.catalog, 1),
                KeyCode::PageUp => self
                    .file_browser
                    .view
                    .move_selection(&self.file_browser.catalog, -page),
                KeyCode::PageDown => self
                    .file_browser
                    .view
                    .move_selection(&self.file_browser.catalog, page),
                KeyCode::Home => self.file_browser.view.first(&self.file_browser.catalog),
                KeyCode::End => self.file_browser.view.last(&self.file_browser.catalog),
                KeyCode::Right => {
                    self.file_browser
                        .view
                        .prepare(&self.file_browser.catalog, 0, 0);
                    if let Some(id) = self.file_browser.view.selected() {
                        if self
                            .file_browser
                            .catalog
                            .node(id)
                            .is_some_and(|node| node.is_dir())
                        {
                            self.activate_browser_node(kind, id, false);
                        }
                    }
                }
                KeyCode::Left => {
                    if let Some(parent) = self
                        .file_browser
                        .catalog
                        .root()
                        .and_then(Path::parent)
                        .map(Path::to_owned)
                    {
                        self.browser_navigate_prompt(kind, parent);
                    }
                }
                KeyCode::Char(ch) if is_plain_text(key) => {
                    self.edit_browser_field(FieldEdit::Insert(&ch.to_string()), false)
                }
                _ => {}
            }
            return;
        }
        let edit = match key.code {
            KeyCode::Left => FieldEdit::Left,
            KeyCode::Right => FieldEdit::Right,
            KeyCode::Home => FieldEdit::Home,
            KeyCode::End => FieldEdit::End,
            KeyCode::Backspace => FieldEdit::Backspace,
            KeyCode::Delete => FieldEdit::Delete,
            KeyCode::Char('u') if control => FieldEdit::Clear,
            KeyCode::Down | KeyCode::Up => {
                self.file_browser.path_focused = false;
                return;
            }
            KeyCode::Char(ch) if is_plain_text(key) => {
                self.edit_browser_field(FieldEdit::Insert(&ch.to_string()), false);
                return;
            }
            _ => return,
        };
        self.edit_browser_field(edit, selecting);
    }
    fn browser_navigate_prompt(&mut self, kind: PromptKind, path: PathBuf) {
        match self.navigate_browser(path) {
            Ok(input) => self.overlay = UiOverlay::Prompt { kind, input },
            Err(error) => self.status = error,
        }
    }
    fn activate_browser_node(&mut self, kind: PromptKind, id: NodeId, side: bool) {
        let Some(node) = self.file_browser.catalog.node(id) else {
            self.status = "La selección del explorador quedó obsoleta".into();
            return;
        };
        if node.is_dir() {
            if let Some(path) = self.file_browser.catalog.path(id) {
                let path = if Some(id) == self.file_browser.catalog.root_id() {
                    let Some(parent) = path.parent() else {
                        return;
                    };
                    parent.to_owned()
                } else {
                    path
                };
                self.browser_navigate_prompt(kind, path);
            }
        } else if kind == PromptKind::Open {
            match self.file_browser.catalog.request_file_open(
                id,
                &mut self.workbench,
                self.default_encoding,
                side,
                16 * 1024 * 1024,
            ) {
                Ok(opened) => {
                    self.finish_file_open(opened.is_some(), "Archivo abierto desde el selector")
                }
                Err(error) => self.status = error,
            }
        } else {
            self.status =
                "Selecciona una carpeta; Intro entra y Ctrl+Intro elige la carpeta actual".into();
        }
    }
    fn accept_typed_browser_file(&mut self, path: PathBuf, side: bool) {
        // Explicit typed paths are admitted by the same confined catalog opener.
        // Admitting the parent first avoids a second production opener or guessing
        // a native filename from its display/escape representation.
        let Some(parent) = path.parent().map(Path::to_owned) else {
            return;
        };
        match ordinary_directory(&parent).and_then(|()| {
            let parent = parent
                .canonicalize()
                .map_err(|_| "La carpeta no pudo resolverse")?;
            ordinary_directory(&parent)?;
            let path = parent.join(path.file_name().ok_or("La ruta no identifica un archivo")?);
            self.workbench
                .request_file_open_in_root(
                    &parent,
                    path,
                    self.default_encoding,
                    side,
                    16 * 1024 * 1024,
                )
                .map_err(|e| e.to_string())
        }) {
            Ok(opened) => self.finish_file_open(opened.is_some(), "Archivo abierto"),
            Err(error) => self.status = error,
        }
    }
    fn accept_browser_folder(&mut self) {
        let Some(id) = self.file_browser.catalog.root_id() else {
            return;
        };
        if !self.file_browser.catalog.node(id).is_some_and(|node| {
            matches!(
                node.state(),
                ExplorerLoadState::Ready | ExplorerLoadState::Limited
            )
        }) {
            self.status = "Espera a que termine la lectura de la carpeta o pulsa F5".into();
            return;
        }
        let Some(path) = self.file_browser.catalog.path(id) else {
            return;
        };
        self.choose_browser_folder(path);
    }
    fn choose_browser_folder(&mut self, path: PathBuf) {
        if let Err(error) = ordinary_directory(&path).and_then(|()| self.set_explorer_root(path)) {
            self.status = error;
            return;
        }
        self.close_overlay();
        self.status = "Carpeta del proyecto seleccionada".into();
    }
}
fn normalized_absolute(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("La ruta debe ser absoluta".into());
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    return Err("La ruta intenta salir de su raíz".into());
                }
            }
            component => result.push(component.as_os_str()),
        }
    }
    Ok(result)
}
pub(super) fn reparse(metadata: &std::fs::Metadata) -> bool {
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
fn ordinary_directory(path: &Path) -> Result<(), String> {
    // Explicit navigation only, bounded by the same path admission. Never silently
    // canonicalize a link typed by the user or accepted from old metadata.
    if path.as_os_str().len() > 4096 || path.components().count() > 128 {
        return Err("La ruta de carpeta supera el límite del selector".into());
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let metadata =
            std::fs::symlink_metadata(&current).map_err(|_| "La carpeta no está disponible")?;
        if !metadata.is_dir() || reparse(&metadata) {
            return Err(
                "La carpeta contiene un enlace, punto de reanálisis o componente no admitido"
                    .into(),
            );
        }
    }
    Ok(())
}
