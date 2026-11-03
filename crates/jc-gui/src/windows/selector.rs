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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectorKind {
    Palette,
    Files,
    Encoding,
}

pub(super) struct Selector {
    kind: SelectorKind,
    input: String,
    input_selection: FieldSelection,
    input_revision: u64,
    selected: usize,
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
            title: "Choose Source Encoding",
            query: self.input.clone(),
            input_selection: self.input_selection,
            message: "Interpret existing bytes; no conversion or save".into(),
            footer: "Enter choose | Esc cancel; bytes stay intact",
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
            .ok_or("Selector input generation exhausted")?;
        self.input_selection.select_all(&self.input);
        self.input_revision = next;
        Ok(())
    }
    pub fn edit_input(&mut self, edit: FieldEdit<'_>, selecting: bool) -> Result<bool, String> {
        let next = self
            .input_revision
            .checked_add(1)
            .ok_or("Selector input generation exhausted")?;
        if let FieldEdit::Insert(value) = edit {
            if !valid_query(value) {
                return Err(
                    "Selector accepts at most256 UTF-8 bytes and no controls/bidi markers".into(),
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
            title: "Command Palette",
            query: self.input.clone(),
            input_selection: self.input_selection,
            message: if registry.iter().take(MAX_COMMANDS + 1).count() > MAX_COMMANDS {
                "Partial command list; registry limit reached"
            } else if total == 0 {
                "No matching commands"
            } else {
                "English label substring; unavailable commands disabled"
            }
            .into(),
            footer: "Shift+arrows select | Ctrl+A/C/X/V | Tab results | Ctrl+Home/End first/last | Enter | Esc",
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
            title: "Quick Open Project File",
            query: self.input.clone(),
            input_selection: self.input_selection,
            message: controller.message().into(),
            footer: "Enter open | Ctrl+Enter side | F5 | Esc",
            rows,
            total,
            selected: self.selected,
        }
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
        selector.append("sAvE");
        let frame = selector.prepare_palette(&registry, &keymap, &context, |_| false, 32);
        assert!(!frame.rows.is_empty());
        assert!(frame
            .rows
            .iter()
            .all(|row| !row.enabled && row.label.to_ascii_lowercase().contains("save")));
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
