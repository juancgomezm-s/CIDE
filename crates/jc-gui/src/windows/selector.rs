//! Input and bounded owned presentation only. No filesystem calls, worker,
//! command execution, document ownership or Win32 dispatch lives here.
use jc_app::{
    edit_selected_field, quick_open::QuickOpenController, CommandContext, CommandId,
    CommandRegistry, FieldEdit, FieldSelection, KeyMap,
};
use jc_text::TextEncoding;
const ENCODINGS: [TextEncoding; 4] = [
    TextEncoding::Utf8,
    TextEncoding::Windows1252,
    TextEncoding::Iso8859_1,
    TextEncoding::Ibm850,
];

const QUERY_BYTES: usize = 256;
const MAX_ROWS: usize = 32;
const MAX_COMMANDS: usize = 200;

struct TabFilter {
    query: Vec<char>,
    prefix: Vec<usize>,
}
impl TabFilter {
    fn new(query: &str) -> Self {
        let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
        let mut prefix = vec![0; query.len()];
        let mut matched = 0;
        for at in 1..query.len() {
            while matched > 0 && query[at] != query[matched] {
                matched = prefix[matched - 1];
            }
            if query[at] == query[matched] {
                matched += 1;
            }
            prefix[at] = matched;
        }
        Self { query, prefix }
    }
    fn matches(&self, label: &str) -> bool {
        if self.query.is_empty() {
            return true;
        }
        let mut matched = 0;
        for ch in label.chars().flat_map(char::to_lowercase) {
            while matched > 0 && ch != self.query[matched] {
                matched = self.prefix[matched - 1];
            }
            if ch == self.query[matched] {
                matched += 1;
            }
            if matched == self.query.len() {
                return true;
            }
        }
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectorKind {
    Palette,
    Files,
    Encoding,
    OpenTabs,
    Appearance,
    Zoom,
}

pub(super) struct Selector {
    kind: SelectorKind,
    input: String,
    input_selection: FieldSelection,
    input_revision: u64,
    selected: usize,
    tab_targets: Vec<(usize, jc_app::TabId)>,
}
pub(super) struct SelectorPresentation {
    pub title: &'static str,
    pub query: String,
    pub input_selection: FieldSelection,
    pub message: String,
    pub footer: &'static str,
    pub rows: Vec<SelectorRow>,
    pub total: usize,
    pub selected: usize,
}
pub(super) struct SelectorRow {
    pub label: String,
    pub selected: bool,
    pub enabled: bool,
    /// Index in the filtered palette or in the core's original candidate list.
    pub ordinal: usize,
}

fn valid_query(text: &str) -> bool {
    text.len() <= QUERY_BYTES && !text.chars().any(|ch| ch.is_control()
        || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
}
fn matches(label: &str, query: &str) -> bool {
    // Palette labels are the registry's English strings. This deliberately
    // differs from Files, whose core matching uses full Unicode case folding.
    query.is_empty()
        || label
            .as_bytes()
            .windows(query.len())
            .any(|window| window.eq_ignore_ascii_case(query.as_bytes()))
}
fn visible(selected: usize, count: usize, max_rows: usize) -> std::ops::Range<usize> {
    let rows = max_rows.min(MAX_ROWS).min(count);
    if rows == 0 {
        return 0..0;
    }
    let first = selected.saturating_sub(rows - 1).min(count - rows);
    first..first + rows
}
impl Selector {
    fn appearance_options(&self) -> &'static [(super::appearance::Choice, &'static str)] {
        if self.kind == SelectorKind::Zoom {
            super::appearance::ZOOMS
        } else {
            super::appearance::APPEARANCE
        }
    }
    pub(super) fn appearance_target(&self) -> Option<super::appearance::Choice> {
        let query = TabFilter::new(&self.input);
        self.appearance_options()
            .iter()
            .filter(|(_, label)| query.matches(label))
            .nth(self.selected)
            .map(|(choice, _)| *choice)
    }
    pub(super) fn prepare_appearance(
        &mut self,
        current: &[super::appearance::Choice],
        rows: usize,
    ) -> SelectorPresentation {
        let query = TabFilter::new(&self.input);
        let entries = self.appearance_options();
        let total = entries
            .iter()
            .filter(|(_, label)| query.matches(label))
            .count();
        self.normalize(total);
        let range = visible(self.selected, total, rows);
        let rows = entries
            .iter()
            .filter(|(_, label)| query.matches(label))
            .enumerate()
            .skip(range.start)
            .take(range.len())
            .map(|(ordinal, (choice, label))| SelectorRow {
                label: format!(
                    "{} {label}",
                    if current.contains(choice) { "✓" } else { " " }
                ),
                enabled: true,
                selected: ordinal == self.selected,
                ordinal,
            })
            .collect();
        let zoom = self.kind == SelectorKind::Zoom;
        SelectorPresentation {
            title: if zoom {
                "Zoom del editor"
            } else {
                "Preferencias de apariencia"
            },
            query: self.input.clone(),
            input_selection: self.input_selection,
            message: if zoom {
                "Escala del texto respecto al tamaño base".into()
            } else {
                "Tamaños base de interfaz, editor y minimapa".into()
            },
            footer: "↑↓ elegir · Intro aplicar · Esc volver",
            rows,
            total,
            selected: self.selected,
        }
    }
    pub(super) fn prepare_tabs(
        &mut self,
        tabs: &jc_app::TabManager,
        rows: usize,
    ) -> SelectorPresentation {
        let query = TabFilter::new(&self.input);
        let matches = |tab: &&jc_app::Tab| query.matches(&tab.title);
        let total = tabs.tabs().iter().filter(matches).count();
        self.normalize(total);
        let range = visible(self.selected, total, rows);
        self.tab_targets.clear();
        if rows == 0 {
            if let Some(tab) = tabs.tabs().iter().filter(matches).nth(self.selected) {
                self.tab_targets.push((self.selected, tab.id));
            }
        }
        let rows = tabs
            .tabs()
            .iter()
            .filter(matches)
            .enumerate()
            .skip(range.start)
            .take(range.len())
            .map(|(ordinal, tab)| {
                self.tab_targets.push((ordinal, tab.id));
                SelectorRow {
                    label: format!(
                        "{}{}",
                        jc_app::source_services::display_ui_label(&tab.title, 80),
                        if tab.dirty { "  ●" } else { "" }
                    ),
                    selected: ordinal == self.selected,
                    enabled: true,
                    ordinal,
                }
            })
            .collect();
        SelectorPresentation {
            title: "Documentos abiertos",
            query: self.input.clone(),
            input_selection: self.input_selection,
            message: format!("{total} documentos · escribe para filtrar"),
            footer: "↑↓ elegir · Intro abrir en este grupo · Esc volver",
            rows,
            total,
            selected: self.selected,
        }
    }
    pub(super) fn tab_target(&self) -> Option<jc_app::TabId> {
        self.tab_targets
            .iter()
            .find(|(ordinal, _)| *ordinal == self.selected)
            .map(|(_, id)| *id)
    }
    pub fn encoding_target(&self) -> Option<TextEncoding> {
        ENCODINGS
            .iter()
            .copied()
            .filter(|e| matches(e.name(), &self.input))
            .nth(self.selected)
    }
    pub fn prepare_encoding(&mut self, rows: usize) -> SelectorPresentation {
        let total = ENCODINGS
            .iter()
            .filter(|e| matches(e.name(), &self.input))
            .count();
        self.normalize(total);
        let window = visible(self.selected, total, rows);
        let rows = ENCODINGS
            .iter()
            .filter(|e| matches(e.name(), &self.input))
            .enumerate()
            .skip(window.start)
            .take(window.len())
            .map(|(ordinal, e)| SelectorRow {
                label: e.name().to_owned(),
                enabled: true,
                selected: ordinal == self.selected,
                ordinal,
            })
            .collect();
        SelectorPresentation {
            title: "Elegir codificación del fuente",
            query: self.input.clone(),
            input_selection: self.input_selection,
            message: "Interpretar bytes existentes; sin convertir ni guardar".into(),
            footer: "Intro elige | Esc cancela; bytes intactos",
            rows,
            total,
            selected: self.selected,
        }
    }
    pub fn new(kind: SelectorKind) -> Self {
        Self {
            kind,
            input: String::new(),
            input_selection: FieldSelection::default(),
            input_revision: 0,
            selected: 0,
            tab_targets: Vec::new(),
        }
    }
    pub fn kind(&self) -> SelectorKind {
        self.kind
    }
    pub fn input(&self) -> &str {
        &self.input
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn input_revision(&self) -> u64 {
        self.input_revision
    }
    pub fn field_selection(&self) -> FieldSelection {
        self.input_selection
    }
    pub fn selected_text(&self) -> Option<&str> {
        self.input_selection.range().map(|range| &self.input[range])
    }
    pub fn select_all_input(&mut self) -> Result<(), String> {
        let next = self
            .input_revision
            .checked_add(1)
            .ok_or("Se agotó la generación del campo del selector")?;
        self.input_selection.select_all(&self.input);
        self.input_revision = next;
        Ok(())
    }
    pub fn edit_input(&mut self, edit: FieldEdit<'_>, selecting: bool) -> Result<bool, String> {
        let next = self
            .input_revision
            .checked_add(1)
            .ok_or("Se agotó la generación del campo del selector")?;
        if let FieldEdit::Insert(value) = edit {
            if !valid_query(value) {
                return Err(
                    "El selector admite hasta256 bytes UTF8 y ningún control/marcador bidi".into(),
                );
            }
        }
        let before = self.input_selection;
        let changed = edit_selected_field(
            &mut self.input,
            &mut self.input_selection,
            edit,
            selecting,
            QUERY_BYTES,
        )
        .map_err(str::to_owned)?;
        if changed {
            self.selected = 0;
        }
        if changed || before != self.input_selection {
            self.input_revision = next;
        }
        Ok(changed)
    }
    /// Reject a whole paste atomically; never truncate inside UTF-8 or retain
    /// caller-owned large allocations. A changed query resets selection.
    #[cfg(test)]
    pub fn append(&mut self, text: &str) -> bool {
        !text.is_empty()
            && self
                .edit_input(FieldEdit::Insert(text), false)
                .unwrap_or(false)
    }
    #[cfg(test)]
    pub fn backspace(&mut self) -> bool {
        self.edit_input(FieldEdit::Backspace, false)
            .unwrap_or(false)
    }
    /// Normalize against the latest count before accepting or presenting.
    pub fn move_selection(&mut self, delta: isize) {
        self.selected = if delta >= 0 {
            self.selected.saturating_add(delta as usize)
        } else {
            self.selected.saturating_sub(delta.unsigned_abs())
        };
    }
    pub fn first(&mut self) {
        self.selected = 0;
    }
    pub fn last(&mut self) {
        self.selected = usize::MAX;
    }
    pub fn normalize(&mut self, count: usize) {
        self.selected = self.selected.min(count.saturating_sub(1));
    }

    pub fn prepare_palette(
        &mut self,
        registry: &CommandRegistry,
        keymap: &KeyMap,
        context: &CommandContext,
        supported: impl Fn(CommandId) -> bool,
        max_rows: usize,
    ) -> SelectorPresentation {
        let total = registry
            .iter()
            .take(MAX_COMMANDS)
            .filter(|spec| self.input.is_empty() || matches(spec.label, &self.input))
            .count();
        self.normalize(total);
        let window = visible(self.selected, total, max_rows);
        let rows = registry
            .iter()
            .take(MAX_COMMANDS)
            .filter(|spec| self.input.is_empty() || matches(spec.label, &self.input))
            .enumerate()
            .skip(window.start)
            .take(window.len())
            .map(|(ordinal, spec)| {
                let label = match keymap.primary_shortcut(spec.id) {
                    Some(shortcut) => format!("{}  {shortcut}", spec.label),
                    None => spec.label.to_owned(),
                };
                SelectorRow {
                    label,
                    selected: ordinal == self.selected,
                    enabled: supported(spec.id) && registry.is_enabled(spec.id, context),
                    ordinal,
                }
            })
            .collect();
        SelectorPresentation {
            title: "Paleta de comandos",
            query: self.input.clone(),
            input_selection: self.input_selection,
            message: if registry.iter().take(MAX_COMMANDS + 1).count() > MAX_COMMANDS {
                "Lista parcial; se alcanzó el límite del registro"
            } else if total == 0 {
                "No hay comandos coincidentes"
            } else {
                "Subcadena de etiqueta; comandos no disponibles deshabilitados"
            }
            .into(),
            footer: "Mayús+flechas seleccionan | Ctrl+A/C/X/V | Tab resultados | Ctrl+Inicio/Fin primero/último | Intro | Esc",
            rows,
            total,
            selected: self.selected,
        }
    }
    /// This identifies the selected command, including a disabled command.
    /// The owning frontend MUST revalidate context and support before executing.
    /// keymap/context/support are accepted symmetrically with prepare_palette;
    /// they determine row metadata, never hide commands or change ordinals.
    pub fn palette_target(
        &self,
        registry: &CommandRegistry,
        _keymap: &KeyMap,
        _context: &CommandContext,
        _supported: impl Fn(CommandId) -> bool,
    ) -> Option<CommandId> {
        registry
            .iter()
            .take(MAX_COMMANDS)
            .filter(|spec| self.input.is_empty() || matches(spec.label, &self.input))
            .nth(self.selected)
            .map(|spec| spec.id)
    }
    pub fn prepare_files(
        &mut self,
        controller: &QuickOpenController,
        max_rows: usize,
    ) -> SelectorPresentation {
        let total = controller.results().len();
        self.normalize(total);
        let window = visible(self.selected, total, max_rows);
        let rows = controller.results()[window]
            .iter()
            .map(|candidate| SelectorRow {
                label: candidate.label().to_owned(),
                selected: candidate.ordinal() == self.selected,
                enabled: true,
                ordinal: candidate.ordinal(),
            })
            .collect();
        SelectorPresentation {
            title: "Apertura rápida de archivo del proyecto",
            query: self.input.clone(),
            input_selection: self.input_selection,
            message: controller.message().into(),
            footer: "Intro abre | Ctrl+Intro al lado | F5 | Esc",
            rows,
            total,
            selected: self.selected,
        }
    }
}

#[cfg(test)]
mod open_tabs_tests {
    use super::*;
    #[test]
    fn thousands_of_open_tabs_keep_only_visible_rows_and_accept_by_identity() {
        let mut tabs = jc_app::TabManager::default();
        let mut last = None;
        for n in 0..5000 {
            last = Some(tabs.open(format!("módulo-{n:05}.rs")));
        }
        let mut selector = Selector::new(SelectorKind::OpenTabs);
        let p = selector.prepare_tabs(&tabs, usize::MAX);
        assert_eq!(p.total, 5000);
        assert_eq!(p.rows.len(), MAX_ROWS);
        selector.last();
        selector.prepare_tabs(&tabs, 12);
        assert_eq!(selector.tab_target(), last);
        selector
            .edit_input(FieldEdit::Insert("MÓDULO-04999"), false)
            .unwrap();
        let p = selector.prepare_tabs(&tabs, 0);
        assert_eq!(p.total, 1);
        assert!(p.rows.is_empty());
        assert_eq!(selector.tab_target(), last);
        tabs.discard_and_close(last.unwrap());
        let p = selector.prepare_tabs(&tabs, 12);
        assert_eq!(p.total, 0);
        assert!(selector.tab_target().is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_app::{default_keymap, default_registry, Wakeup};

    #[test]
    fn input_paste_is_atomic_utf8_bounded_and_backspace_removes_one_scalar() {
        let mut selector = Selector::new(SelectorKind::Files);
        assert_eq!(selector.kind(), SelectorKind::Files);
        assert!(selector.append("Straße"));
        selector.last();
        assert!(selector.backspace());
        assert_eq!(selector.input(), "Straß");
        assert_eq!(selector.selected(), 0);
        let before = selector.input().to_owned();
        for invalid in ["\n", "\0", "\u{202e}", "\u{061c}", &"x".repeat(257)] {
            assert!(!selector.append(invalid));
            assert_eq!(selector.input(), before);
        }
        let mut exact = Selector::new(SelectorKind::Palette);
        assert!(exact.append(&"é".repeat(128)));
        assert_eq!(exact.input().len(), 256);
        assert!(!exact.append("x"));
        assert_eq!(exact.input().len(), 256);
    }
    #[test]
    fn english_filter_shortcut_and_disabled_target_agree_with_registry() {
        let registry = default_registry();
        let keymap = default_keymap();
        let context = CommandContext::default();
        let mut selector = Selector::new(SelectorKind::Palette);
        selector.append("gUaRdAr");
        let frame = selector.prepare_palette(&registry, &keymap, &context, |_| false, 32);
        assert!(!frame.rows.is_empty());
        assert!(frame
            .rows
            .iter()
            .all(|row| !row.enabled && row.label.to_ascii_lowercase().contains("guardar")));
        let target = selector
            .palette_target(&registry, &keymap, &context, |_| false)
            .unwrap();
        let row = &frame.rows[0];
        let spec = registry.get(target).unwrap();
        assert!(row.label.starts_with(spec.label));
        if let Some(shortcut) = keymap.primary_shortcut(target) {
            assert!(row.label.ends_with(&shortcut.to_string()));
        }
        assert!(!registry.is_enabled(target, &context));
    }
    #[test]
    fn selection_stays_visible_and_empty_filtered_results_have_no_target() {
        let registry = default_registry();
        let keymap = default_keymap();
        let context = CommandContext::default();
        let mut selector = Selector::new(SelectorKind::Palette);
        selector.last();
        let frame = selector.prepare_palette(&registry, &keymap, &context, |_| true, 3);
        assert!(frame.total > 3);
        assert_eq!(frame.rows.len(), 3);
        assert_eq!(frame.rows.last().unwrap().ordinal, frame.total - 1);
        assert!(frame.rows.last().unwrap().selected);
        selector.move_selection(-1);
        let next = selector.prepare_palette(&registry, &keymap, &context, |_| true, 3);
        assert_eq!(next.selected, frame.selected - 1);
        assert!(next
            .rows
            .iter()
            .any(|row| row.ordinal == next.selected && row.selected));
        selector.first();
        assert_eq!(selector.selected(), 0);
        selector.move_selection(isize::MIN);
        assert_eq!(selector.selected(), 0);
        selector.append("this command does not exist");
        let empty = selector.prepare_palette(&registry, &keymap, &context, |_| true, 1000);
        assert_eq!(empty.total, 0);
        assert!(empty.rows.is_empty());
        assert!(selector
            .palette_target(&registry, &keymap, &context, |_| true)
            .is_none());
    }
    #[test]
    fn empty_file_presentation_is_read_only_and_tiny_viewport_allocates_no_rows() {
        let controller = QuickOpenController::new(Wakeup::default());
        let mut selector = Selector::new(SelectorKind::Files);
        selector.last();
        let frame = selector.prepare_files(&controller, 32);
        assert_eq!(frame.total, 0);
        assert_eq!(frame.selected, 0);
        assert!(frame.rows.is_empty());
        assert_eq!(frame.message, controller.message());
        assert_eq!(controller.state(), jc_app::quick_open::QuickOpenState::Idle);
        assert!(!controller.ready());
        let registry = default_registry();
        let keymap = default_keymap();
        let context = CommandContext::default();
        let frame = selector.prepare_palette(&registry, &keymap, &context, |_| true, 0);
        assert!(frame.total > 0);
        assert!(frame.rows.is_empty());
    }
}
