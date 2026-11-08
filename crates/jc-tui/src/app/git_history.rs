//! Historical Git navigation over accepted immutable models. No editor writes,
//! source reads or completion acceptance occurs while preparing presentation.
use super::{
    git_diff::{hunk_label, model_rows},
    git_status::{GitStatusPresentation, GitStatusRow},
    GitHunkRow, GitPresentation, TuiApp,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{git_revision::Change, BottomPanel, PrimaryFocus};
use jc_text::{DecodedScalars, TextEncoding};

const ROWS: usize = 256;
const COLS: usize = 256;

#[derive(Default)]
pub(crate) struct HistoryView {
    pub(super) graph: Option<jc_app::git_graph::GraphView>,
    pub(super) active: bool,
    pub(super) files: bool,
    pub(super) selected: usize,
    pub(super) hunk: usize,
    pub(super) encoding: Option<TextEncoding>,
    pub(super) local: Option<(jc_app::git_revision::PairKind, Vec<u8>)>,
    pub(super) blame: Option<super::git_blame::BlameView>,
    pub(super) search: Option<super::git_commit_search::CommitSearchView>,
    /// Pinned start and opaque path only. The controller owns the accepted page.
    pub(super) path: Option<(String, Vec<u8>)>,
    pub(super) commit: Option<String>,
    commit_selected: usize,
}
impl HistoryView {
    pub(super) fn immutable(&self) -> bool {
        self.active || self.local.is_some()
    }
    fn viewport(&mut self, total: usize, height: usize) -> usize {
        self.selected = self.selected.min(total.saturating_sub(1));
        self.selected
            .saturating_sub(height.saturating_sub(1))
            .min(total.saturating_sub(height))
    }
    pub(super) fn cycle_encoding(&mut self) {
        self.encoding = Some(match self.encoding {
            None | Some(TextEncoding::Ibm850) => TextEncoding::Utf8,
            Some(TextEncoding::Utf8) => TextEncoding::Windows1252,
            Some(TextEncoding::Windows1252) => TextEncoding::Iso8859_1,
            Some(TextEncoding::Iso8859_1) => TextEncoding::Ibm850,
        });
    }
}

impl TuiApp {
    pub(crate) fn prepare_git_history(&mut self, width: u16, height: u16) -> GitStatusPresentation {
        if self
            .git_history
            .search
            .as_ref()
            .is_some_and(|s| !s.browsing)
        {
            return self.prepare_commit_search(width, height);
        }
        if self.git_history.blame.is_some() {
            return self.prepare_git_blame_list(width, height);
        }
        if self.git_history.path.is_some() {
            return self.prepare_git_path_history(width, height);
        }
        let height = usize::from(height).min(ROWS);
        let total = if self.git_history.files {
            self.git_revision.revision().map_or(0, |r| r.files.len())
        } else {
            self.git_status
                .history_info()
                .map_or(0, |i| i.commits.len())
        };
        let scroll = self.git_history.viewport(total, height);
        let rows = if self.git_history.files {
            self.git_revision
                .revision()
                .into_iter()
                .flat_map(|r| r.files.iter())
                .enumerate()
                .skip(scroll)
                .take(height)
                .map(|(ordinal, file)| GitStatusRow {
                    ordinal,
                    flags: change(file.change).into(),
                    path: opaque(&file.path, width.saturating_sub(3)),
                    selected: ordinal == self.git_history.selected,
                })
                .collect()
        } else {
            self.git_status
                .history_info()
                .into_iter()
                .flat_map(|i| i.commits.iter())
                .enumerate()
                .skip(scroll)
                .take(height)
                .map(|(ordinal, commit)| GitStatusRow {
                    ordinal,
                    flags: if commit.parents.len() > 1 { "M" } else { "C" }.into(),
                    path: label(
                        &format!(
                            "{} {}",
                            short_oid(&commit.id),
                            opaque(&commit.summary, width)
                        ),
                        width.saturating_sub(3),
                    ),
                    selected: ordinal == self.git_history.selected,
                })
                .collect()
        };
        let (parent, parents) = self.git_revision.parent_options();
        let parent_help = format!("Padre {}/{} · P cambiar", parent + 1, parents.len());
        GitStatusPresentation {
            title: label(
                if self.git_history.files {
                    "Archivos del commit"
                } else {
                    "Historial Git local"
                },
                width,
            ),
            message: label(
                if self.git_history.files {
                    if parents.len() > 1 {
                        &parent_help
                    } else {
                        "Intro comparar · B atrás"
                    }
                } else if total == 0 {
                    "Sin historial · F5 leer"
                } else {
                    "Intro archivos · H estado"
                },
                width,
            ),
            rows,
            total,
            selected: self.git_history.selected,
            scroll,
        }
    }

    /// Called only in the Git sidebar / historical diff focus, not globally.
    pub(crate) fn handle_git_history_key(&mut self, key: KeyEvent) -> bool {
        if self.handle_commit_search_key(key) {
            return true;
        }
        if key.modifiers != KeyModifiers::NONE {
            return false;
        }
        if self.handle_git_blame_key(key) {
            return true;
        }
        if matches!(key.code, KeyCode::Char('a' | 'A'))
            && !self.git_history.active
            && self.git_history.local.is_none()
            && self.focus.primary() == PrimaryFocus::Sidebar
        {
            let pinned = self
                .git_status
                .history_info()
                .and_then(|info| info.commits.first())
                .map(|commit| commit.id.clone());
            let path = self.git_status.selected_path().or_else(|| {
                let root = self.project_root.as_deref()?;
                let relative = self
                    .workbench
                    .active_document()?
                    .session
                    .path()
                    .strip_prefix(root)
                    .ok()?;
                if relative.as_os_str().len() > 4096 {
                    return None;
                }
                let parts = relative
                    .components()
                    .map(|component| match component {
                        std::path::Component::Normal(part) => part.to_str(),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()?;
                if parts.is_empty() {
                    return None;
                }
                let path = parts.join("/").into_bytes();
                (path.len() <= 4096).then_some(path)
            });
            if let (Some(root), Some(path), Some(start)) = (self.project_root.clone(), path, pinned)
            {
                match self
                    .git_revision
                    .request_path_history(root, start.clone(), path.clone())
                {
                    Ok(()) => {
                        self.git_diff.clear(&mut self.tasks);
                        self.git_history = HistoryView {
                            active: true,
                            path: Some((start, path)),
                            encoding: self.default_encoding,
                            ..HistoryView::default()
                        };
                        self.git_diff_scroll = 0;
                        self.panels.hide_bottom();
                        self.status = format!(
                            "Historial: {} · Intro compara · N más · R relee · H general",
                            opaque(self.git_history.path.as_ref().unwrap().1.as_slice(), 120)
                        );
                    }
                    Err(error) => self.status = error,
                }
            } else {
                self.status =
                    "Selecciona un archivo y actualiza el estado; se necesita un commit inicial"
                        .into();
            }
            return true;
        }
        if self.git_history.path.is_some() {
            return self.handle_git_path_history_key(key);
        }
        if self.git_history.files && matches!(key.code, KeyCode::Char('p' | 'P')) {
            let (current, parents) = self.git_revision.parent_options();
            if parents.len() > 1 {
                let parent = (current + 1) % parents.len();
                if let (Some(root), Some(commit)) =
                    (self.project_root.clone(), self.git_history.commit.clone())
                {
                    match self
                        .git_revision
                        .request_revision_parent(root, commit, parent)
                    {
                        Ok(()) => {
                            self.git_history.selected = 0;
                            self.git_history.hunk = 0;
                            self.git_diff_scroll = 0;
                            let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
                            self.status =
                                format!("Leyendo los archivos frente al padre {}", parent + 1);
                        }
                        Err(error) => self.status = error,
                    }
                }
                return true;
            }
        }
        if matches!(key.code, KeyCode::Char('p' | 'P' | 't' | 'T'))
            && !self.git_history.active
            && self.focus.primary() == PrimaryFocus::Sidebar
        {
            let kind = if matches!(key.code, KeyCode::Char('p' | 'P')) {
                jc_app::git_revision::PairKind::Prepared
            } else {
                jc_app::git_revision::PairKind::Worktree
            };
            if let (Some(root), Some(path)) =
                (self.project_root.clone(), self.git_status.selected_path())
            {
                self.git_diff.clear(&mut self.tasks);
                self.git_history.local = Some((kind, path.clone()));
                self.git_history.encoding = self.git_history.encoding.or(self.default_encoding);
                self.git_history.hunk = 0;
                self.git_diff_scroll = 0;
                match self.git_revision.request_local_file(root, path, kind) {
                    Ok(()) => {
                        self.panels.show_bottom(BottomPanel::Diff);
                        let _ = self.focus.focus(PrimaryFocus::BottomPanel, &self.panels);
                        self.status =
                            "Comparación de disco · bytes originales · E codificación · Esc estado"
                                .into();
                    }
                    Err(e) => self.status = e,
                }
            } else {
                self.status = "Selecciona un archivo del estado Git actualizado".into();
            }
            return true;
        }
        if self.git_history.local.is_some() {
            if self.handle_git_index_key(key) {
                return true;
            }
            match key.code {
                KeyCode::Esc | KeyCode::Char('b' | 'B') => {
                    let _ = self.git_index.discard_review();
                    self.git_revision.cancel();
                    self.git_history.local = None;
                    self.panels.hide_bottom();
                    let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
                    return true;
                }
                KeyCode::Char('e' | 'E') => {
                    self.git_history.cycle_encoding();
                    return true;
                }
                KeyCode::F(5) => {
                    let _ = self.git_index.discard_review();
                    if let (Some(root), Some((kind, path))) =
                        (self.project_root.clone(), self.git_history.local.clone())
                    {
                        if let Err(e) = self.git_revision.request_local_file(root, path, kind) {
                            self.status = e;
                        }
                    }
                    return true;
                }
                _ if self.focus.primary() == PrimaryFocus::BottomPanel => {
                    return self.historical_diff_key(key.code)
                }
                _ => {}
            }
        }
        if matches!(key.code, KeyCode::Char('h' | 'H')) {
            self.git_history.local = None;
            if self.git_history.active {
                self.git_revision.cancel();
                self.git_history.active = false;
                self.git_history.files = false;
                self.git_history.commit = None;
                self.panels.hide_bottom();
                let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
                self.status = "Estado Git local · H historial".into();
            } else {
                self.git_revision.cancel();
                self.git_diff.clear(&mut self.tasks);
                self.git_history.active = true;
                self.git_history.files = false;
                self.git_history.selected = 0;
                self.git_history.hunk = 0;
                self.git_history.encoding = self.git_history.encoding.or(self.default_encoding);
                self.git_diff_scroll = 0;
                self.status = "Historial Git · Intro archivos · H estado · E codificación".into();
            }
            return true;
        }
        if !self.git_history.active {
            return false;
        }
        if matches!(key.code, KeyCode::Char('e' | 'E')) {
            self.git_history.cycle_encoding();
            self.status = format!(
                "Histórico: {} · E cambia interpretación; los bytes no se modifican",
                self.git_history.encoding.unwrap().name()
            );
            return true;
        }
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('b' | 'B')) {
            if self.focus.primary() == PrimaryFocus::BottomPanel {
                let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
            } else if self.git_history.files {
                self.git_revision.cancel();
                self.git_history.files = false;
                self.git_history.selected = self.git_history.commit_selected;
                self.git_history.commit = None;
                self.git_history.hunk = 0;
                self.git_diff_scroll = 0;
                self.panels.hide_bottom();
            } else {
                self.git_history.active = false;
                self.git_revision.cancel();
            }
            self.status = "Histórico de solo lectura · H vuelve al estado local".into();
            return true;
        }
        if key.code == KeyCode::F(5) {
            let Some(root) = self.project_root.clone() else {
                return true;
            };
            let result = if self.git_history.files {
                match self.git_history.commit.clone() {
                    Some(commit) => self.git_revision.request_revision_parent(
                        root,
                        commit,
                        self.git_revision.parent_options().0,
                    ),
                    None => Ok(()),
                }
            } else {
                self.git_status.request(&root)
            };
            self.status = result
                .err()
                .unwrap_or_else(|| "Actualizando consulta Git".into());
            return true;
        }
        if self.focus.primary() == PrimaryFocus::BottomPanel {
            return self.historical_diff_key(key.code);
        }
        match key.code {
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End => {
                let total = if self.git_history.files {
                    self.git_revision.revision().map_or(0, |r| r.files.len())
                } else {
                    self.git_status
                        .history_info()
                        .map_or(0, |i| i.commits.len())
                };
                let delta = match key.code {
                    KeyCode::Up => -1,
                    KeyCode::Down => 1,
                    KeyCode::PageUp => -10,
                    KeyCode::PageDown => 10,
                    KeyCode::Home => isize::MIN,
                    _ => isize::MAX,
                };
                self.git_history.selected = self
                    .git_history
                    .selected
                    .saturating_add_signed(delta)
                    .min(total.saturating_sub(1));
                true
            }
            KeyCode::Enter => {
                let Some(root) = self.project_root.clone() else {
                    self.status = "Abre la carpeta raíz del repositorio".into();
                    return true;
                };
                if self.git_history.files {
                    let file = self.git_revision.revision().and_then(|r| {
                        r.files
                            .get(self.git_history.selected)
                            .map(|f| (r.commit.clone(), f.path.clone()))
                    });
                    if let Some((commit, path)) = file {
                        match self.git_revision.request_revision_file(root, commit, path) {
                            Ok(()) => {
                                self.git_history.hunk = 0;
                                self.git_diff_scroll = 0;
                                self.panels.show_bottom(BottomPanel::Diff);
                                let _ = self.focus.focus(PrimaryFocus::BottomPanel, &self.panels);
                                self.status = "Comparando con el padre elegido · E codificación · Esc archivos".into();
                            }
                            Err(error) => self.status = error,
                        }
                    } else {
                        self.status = self.git_revision.message().into();
                    }
                } else {
                    let commit = self
                        .git_status
                        .history_info()
                        .and_then(|i| i.commits.get(self.git_history.selected))
                        .map(|c| c.id.clone());
                    if let Some(commit) = commit {
                        match self.git_revision.request_revision(root, commit.clone()) {
                            Ok(()) => {
                                self.git_history.commit = Some(commit);
                                self.git_history.commit_selected = self.git_history.selected;
                                self.git_history.files = true;
                                self.git_history.selected = 0;
                                self.git_history.hunk = 0;
                                self.git_diff_scroll = 0;
                                self.panels.show_bottom(BottomPanel::Diff);
                                self.status = "Leyendo archivos del commit · Intro compara".into();
                            }
                            Err(error) => self.status = error,
                        }
                    }
                }
                true
            }

            _ => false,
        }
    }
    fn prepare_git_path_history(&mut self, width: u16, height: u16) -> GitStatusPresentation {
        let height = usize::from(height).min(ROWS);
        let total = self
            .git_revision
            .path_history()
            .map_or(0, |page| page.entries.len());
        let scroll = self.git_history.viewport(total, height);
        let page = self.git_revision.path_history();
        let rows = page
            .into_iter()
            .flat_map(|page| page.entries.iter())
            .enumerate()
            .skip(scroll)
            .take(height)
            .map(|(ordinal, entry)| GitStatusRow {
                ordinal,
                flags: if entry.previous_path.is_some() {
                    "R"
                } else {
                    change(entry.change)
                }
                .into(),
                path: label(
                    &format!(
                        "{} {}",
                        short_oid(&entry.commit),
                        opaque(&entry.summary, width)
                    ),
                    width.saturating_sub(3),
                ),
                selected: ordinal == self.git_history.selected,
            })
            .collect();
        let path = self
            .git_history
            .path
            .as_ref()
            .map_or(&[][..], |(_, path)| path.as_slice());
        let state = if let Some(page) = page {
            if page.shallow {
                "Historial superficial"
            } else if page.entries.is_empty() && page.next.is_some() {
                "Sin coincidencias en esta página"
            } else if page.next.is_none() {
                "Última página"
            } else {
                "Página de historial"
            }
        } else {
            self.git_revision.message()
        };
        let more = if page.is_some_and(|page| page.next.is_some()) {
            " · N más"
        } else {
            ""
        };
        let metadata = if page.is_some_and(|page| page.metadata_limited) {
            " · metadatos recortados"
        } else {
            ""
        };
        GitStatusPresentation {
            title: label(
                &format!("Historial del archivo: {}", opaque(path, width)),
                width,
            ),
            message: label(
                &format!("{state}{metadata} · Intro compara{more} · R relee · H general"),
                width,
            ),
            rows,
            total,
            selected: self.git_history.selected,
            scroll,
        }
    }

    fn handle_git_path_history_key(&mut self, key: KeyEvent) -> bool {
        if matches!(key.code, KeyCode::Char('h' | 'H')) {
            self.git_revision.cancel();
            self.git_history.path = None;
            self.git_history.files = false;
            self.git_history.selected = 0;
            self.git_history.commit = None;
            self.panels.hide_bottom();
            let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
            self.status = "Historial Git general · Intro archivos · H estado".into();
            return true;
        }
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('b' | 'B')) {
            if self.focus.primary() == PrimaryFocus::BottomPanel {
                self.panels.hide_bottom();
                let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
                self.status = "Historial del archivo · Intro compara · N más · H general".into();
            } else {
                self.git_revision.cancel();
                self.git_history.path = None;
                self.git_history.selected = 0;
                self.git_history.commit = None;
                self.status = "Historial Git general · Intro archivos · H estado".into();
            }
            return true;
        }
        if matches!(key.code, KeyCode::Char('e' | 'E')) {
            self.git_history.cycle_encoding();
            return true;
        }
        if matches!(key.code, KeyCode::F(5) | KeyCode::Char('r' | 'R')) {
            if let (Some(root), Some((start, path))) =
                (self.project_root.clone(), self.git_history.path.clone())
            {
                self.status = self
                    .git_revision
                    .request_path_history(root, start, path)
                    .err()
                    .unwrap_or_else(|| "Releyendo el historial fijado del archivo".into());
                self.git_history.selected = 0;
                self.git_history.hunk = 0;
                self.git_diff_scroll = 0;
                self.panels.hide_bottom();
                let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
            }
            return true;
        }
        if self.focus.primary() == PrimaryFocus::BottomPanel {
            return self.historical_diff_key(key.code);
        }
        if matches!(key.code, KeyCode::Char('n' | 'N')) {
            if self
                .git_revision
                .path_history()
                .is_some_and(|page| page.next.is_some())
            {
                if let Some(root) = self.project_root.clone() {
                    self.status = self
                        .git_revision
                        .request_path_history_next(root)
                        .err()
                        .unwrap_or_else(|| "Leyendo la siguiente página del archivo".into());
                    self.git_history.selected = 0;
                    self.git_history.hunk = 0;
                    self.git_diff_scroll = 0;
                }
            } else {
                self.status = "No hay otra página disponible del historial del archivo".into();
            }
            return true;
        }
        match key.code {
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End => {
                let total = self
                    .git_revision
                    .path_history()
                    .map_or(0, |page| page.entries.len());
                let delta = match key.code {
                    KeyCode::Up => -1,
                    KeyCode::Down => 1,
                    KeyCode::PageUp => -10,
                    KeyCode::PageDown => 10,
                    KeyCode::Home => isize::MIN,
                    _ => isize::MAX,
                };
                self.git_history.selected = self
                    .git_history
                    .selected
                    .saturating_add_signed(delta)
                    .min(total.saturating_sub(1));
                true
            }
            KeyCode::Enter => {
                if let Some(root) = self.project_root.clone() {
                    self.status = match self
                        .git_revision
                        .request_path_history_file(root, self.git_history.selected)
                    {
                        Ok(()) => {
                            self.git_history.hunk = 0;
                            self.git_diff_scroll = 0;
                            self.panels.show_bottom(BottomPanel::Diff);
                            let _ = self.focus.focus(PrimaryFocus::BottomPanel, &self.panels);
                            "Comparando el archivo con el primer padre · E codificación · Esc historial".into()
                        }
                        Err(error) => error,
                    };
                }
                true
            }
            _ => false,
        }
    }

    fn historical_diff_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Up => self.git_diff_scroll = self.git_diff_scroll.saturating_sub(1),
            KeyCode::Down => self.git_diff_scroll = self.git_diff_scroll.saturating_add(1),
            KeyCode::PageUp => self.git_diff_scroll = self.git_diff_scroll.saturating_sub(10),
            KeyCode::PageDown => self.git_diff_scroll = self.git_diff_scroll.saturating_add(10),
            KeyCode::Home => self.git_diff_scroll = 0,
            KeyCode::End => self.git_diff_scroll = usize::MAX,
            KeyCode::Left | KeyCode::Right => {
                let count = self
                    .git_revision
                    .comparison()
                    .map_or(0, |r| r.model.hunks().count());
                let delta = if code == KeyCode::Left { -1 } else { 1 };
                self.git_history.hunk = self
                    .git_history
                    .hunk
                    .saturating_add_signed(delta)
                    .min(count.saturating_sub(1));
                self.git_diff_scroll = 0;
            }
            KeyCode::Enter => {
                self.status =
                    "Fuentes inmutables · Esc vuelve a la lista; el buffer sigue intacto".into();
            }
            _ => return false,
        }
        true
    }
    pub(crate) fn prepare_historical_diff(
        &mut self,
        width: u16,
        rows: usize,
        hunk_rows: usize,
    ) -> GitPresentation {
        if self
            .git_history
            .blame
            .as_ref()
            .is_some_and(|b| !b.comparing)
        {
            return self.prepare_git_blame(width, rows);
        }
        let mut prepared = GitPresentation {
            status: label(self.git_revision.message(), width),
            ..GitPresentation::default()
        };
        if self.git_history.local.is_some() {
            if let Some(hint) = self.git_index_hint() {
                prepared.status = label(hint, width);
            } else if let Some(notice) = self.git_index_publication_notice() {
                prepared.status = label(&notice, width);
            }
        }
        prepared.details.push(label(
            if self.git_history.local.is_some() {
                "Comparación local · bytes originales"
            } else {
                "Histórico · comparación con el padre elegido · solo lectura"
            },
            width,
        ));
        prepared.details.push(label(
            "↑↓ desplazar · ←→ bloque · E codificación · Esc volver",
            width,
        ));
        let Some(result) = self.git_revision.comparison() else {
            if let Some(revision) = self.git_revision.revision() {
                prepared.details.push(label(
                    &format!(
                        "Commit {} · {} archivos cambiados",
                        short_oid(&revision.commit),
                        revision.files.len()
                    ),
                    width,
                ));
                prepared
                    .details
                    .push(label("Elige un archivo y pulsa Intro para comparar", width));
            }
            return prepared;
        };
        prepared.details.push(opaque(&result.path, width));
        if let jc_app::git_revision::ComparisonOrigin::Historical {
            previous_path: Some(previous),
            ..
        } = &result.origin
        {
            prepared.details.push(label(
                &format!(
                    "Renombrado · {} → {}",
                    opaque(previous, width),
                    opaque(&result.path, width)
                ),
                width,
            ));
        }
        if let Some(sources) = result.origin.sources_label() {
            prepared.details.push(label(&sources, width));
        }
        prepared.details.push(label(&result.origin.label(), width));
        let Some(encoding) = self.git_history.encoding else {
            prepared.details.push(label(
                "Pulsa E para elegir la codificación de esta vista histórica",
                width,
            ));
            return prepared;
        };
        prepared.details.push(label(
            &format!("{} · E cambia interpretación", encoding.name()),
            width,
        ));
        prepared.total_hunks = result.model.hunks().count();
        self.git_history.hunk = self
            .git_history
            .hunk
            .min(prepared.total_hunks.saturating_sub(1));
        let start = self
            .git_history
            .hunk
            .saturating_sub(hunk_rows.min(ROWS).saturating_sub(1));
        prepared.hunks = result
            .model
            .hunks()
            .enumerate()
            .skip(start)
            .take(hunk_rows.min(ROWS))
            .map(|(index, hunk)| GitHunkRow {
                index,
                label: label(&hunk_label(hunk), width),
                selected: index == self.git_history.hunk,
            })
            .collect();
        let (visible, scroll) = model_rows(
            &result.model,
            self.git_history.hunk,
            self.git_diff_scroll,
            width,
            rows.min(ROWS),
            encoding,
        );
        prepared.rows = visible;
        self.git_diff_scroll = scroll;
        prepared
    }
}

pub(super) fn short_oid(oid: &str) -> &str {
    let mut end = oid.len().min(8);
    while !oid.is_char_boundary(end) {
        end -= 1;
    }
    &oid[..end]
}
pub(super) fn label(text: &str, width: u16) -> String {
    jc_app::source_services::display_ui_label(text, usize::from(width).min(COLS))
}
pub(super) fn opaque(bytes: &[u8], width: u16) -> String {
    let mut output = String::new();
    for scalar in
        DecodedScalars::new(&bytes[..bytes.len().min(1024)], TextEncoding::Utf8, 0).take(COLS)
    {
        if scalar.valid {
            output.push(scalar.character);
        } else {
            for byte in &bytes[scalar.source_range] {
                output.push_str(&format!("\\x{byte:02X}"));
            }
        }
    }
    label(&output, width)
}
fn change(change: Change) -> &'static str {
    match change {
        Change::Unchanged => "=",
        Change::Modified => "M",
        Change::Added => "+",
        Change::Deleted => "-",
        Change::TypeChanged => "T",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LaunchOptions;
    use jc_app::git_revision::GitRevisionState;

    #[test]
    fn path_history_preparation_is_bounded_without_admitting_queries() {
        let mut app = TuiApp::new(LaunchOptions::default());
        app.workbench
            .new_file("path-history-fixture.txt", TextEncoding::Utf8);
        app.workbench.insert_text("sin guardar\r\n").unwrap();
        let original = app.workbench.active_bytes().unwrap();
        app.git_history.active = true;
        app.git_history.path = Some(("a".repeat(40), b"raw\xff\x1b.p".to_vec()));
        for width in [0, 18, 80, u16::MAX] {
            let prepared = app.prepare_git_history(width, u16::MAX);
            assert!(prepared.rows.is_empty());
            assert!(prepared.title.chars().count() <= usize::from(width).min(COLS));
            assert!(prepared.message.chars().count() <= usize::from(width).min(COLS));
            assert!(!prepared.title.contains('\x1b'));
        }
        assert_eq!(app.git_revision.state(), GitRevisionState::Idle);
        assert!(!app.git_revision.ready());
        assert_eq!(app.workbench.active_bytes().unwrap(), original);
    }

    #[test]
    fn historical_viewport_and_opaque_labels_stay_bounded() {
        let mut view = HistoryView {
            selected: usize::MAX,
            ..Default::default()
        };
        assert_eq!(view.viewport(24, 7), 17);
        assert_eq!(view.selected, 23);
        assert_eq!(view.viewport(0, 100), 0);
        assert_eq!(view.selected, 0);
        assert!(opaque(b"raw\xff\n", 80).contains("\\xFF"));
        assert!(!opaque(b"raw\x1b[31m", 80).contains('\x1b'));
        assert!(opaque(&vec![b'x'; 8000], 10).chars().count() <= 10);
    }
    #[test]
    fn history_preparation_and_codec_keys_never_edit_the_buffer_or_admit_reads() {
        let mut app = TuiApp::new(LaunchOptions::default());
        app.workbench
            .new_file("history-fixture.txt", TextEncoding::Utf8);
        app.workbench
            .insert_text("owned history fixture\r\n")
            .unwrap();
        let original = app.workbench.active_bytes().unwrap();
        assert!(app.handle_git_history_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE)));
        assert!(app.git_history.active);
        assert!(app.git_history.encoding.is_none());
        for width in [0, 18, 80, u16::MAX] {
            let frame = app.prepare_git_history(width, u16::MAX);
            assert!(frame.rows.len() <= ROWS);
            let detail = app.prepare_historical_diff(width, usize::MAX, usize::MAX);
            assert!(detail.rows.len() <= ROWS);
        }
        assert_eq!(app.git_revision.state(), GitRevisionState::Cancelled);
        assert!(!app.git_revision.ready());
        for expected in [
            TextEncoding::Utf8,
            TextEncoding::Windows1252,
            TextEncoding::Iso8859_1,
            TextEncoding::Ibm850,
        ] {
            assert!(
                app.handle_git_history_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE))
            );
            assert_eq!(app.git_history.encoding, Some(expected));
        }
        assert!(app.handle_git_history_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE)));
        assert!(!app.git_history.active);
        assert_eq!(app.workbench.active_bytes().unwrap(), original);
    }
}
