use jc_app::{CommandId, CommandOutcome, Workbench, WorkbenchError};
use jc_files::FileError;
use jc_text::{TextEncoding, TextPosition};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "jc-ide-workbench-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn workbench_edits_saves_and_tracks_command_context() {
    let work = Workspace::new();
    let path = work.path("customer.p");
    fs::write(&path, b"DISPLAY x.\r\n").unwrap();

    let mut bench = Workbench::default();
    bench
        .open_file(&path, Some(TextEncoding::Iso8859_1))
        .unwrap();
    assert!(bench.context().has_document);
    assert_eq!(bench.context().active_language.as_deref(), Some("abl"));
    assert!(!bench.context().document_dirty);

    bench.move_end(false).unwrap();
    bench.insert_text(" /* niño */").unwrap();
    assert!(bench.context().document_dirty);
    assert!(bench.context().can_undo);

    assert_eq!(
        bench.execute(CommandId::FileSave).unwrap(),
        CommandOutcome::None
    );
    assert!(!bench.context().document_dirty);
    assert_eq!(fs::read(&path).unwrap(), b"DISPLAY x. /* ni\xf1o */\r\n");
}

#[test]
fn legacy_file_requires_explicit_encoding_before_text_editing() {
    let work = Workspace::new();
    let path = work.path("legacy.p");
    fs::write(&path, b"ni\xf1o").unwrap();

    let mut bench = Workbench::default();
    bench.open_file(&path, None).unwrap();
    assert_eq!(bench.active_encoding(), None);
    assert!(matches!(
        bench.insert_text("x"),
        Err(WorkbenchError::File(FileError::EncodingRequired))
    ));

    bench.set_encoding(TextEncoding::Iso8859_1).unwrap();
    bench.move_end(false).unwrap();
    bench.insert_text(" café").unwrap();
    assert_eq!(bench.active_bytes().unwrap(), b"ni\xf1o caf\xe9");
}

#[test]
fn utf8_cursor_movement_never_splits_multibyte_characters() {
    let work = Workspace::new();
    let path = work.path("utf8.txt");
    fs::write(&path, "aéñx").unwrap();

    let mut bench = Workbench::default();
    bench.open_file(&path, Some(TextEncoding::Utf8)).unwrap();

    bench.move_right(false).unwrap();
    assert_eq!(
        bench.active_position(),
        Some(TextPosition { line: 0, column: 1 })
    );
    bench.move_right(false).unwrap();
    assert_eq!(
        bench.active_position(),
        Some(TextPosition { line: 0, column: 2 })
    );
    bench.backspace().unwrap();
    assert_eq!(
        String::from_utf8(bench.active_bytes().unwrap()).unwrap(),
        "añx"
    );
}

#[test]
fn utf8_bom_starts_cursor_after_bom() {
    let work = Workspace::new();
    let path = work.path("bom.txt");
    fs::write(&path, b"\xef\xbb\xbfabc").unwrap();

    let mut bench = Workbench::default();
    bench.open_file(&path, None).unwrap();
    assert_eq!(
        bench.active_position(),
        Some(TextPosition { line: 0, column: 0 })
    );
    bench.insert_text("X").unwrap();
    assert_eq!(bench.active_bytes().unwrap(), b"\xef\xbb\xbfXabc");
}

#[test]
fn search_next_and_previous_share_byte_ranges_with_selection() {
    let work = Workspace::new();
    let path = work.path("search.txt");
    fs::write(&path, b"alpha beta alpha").unwrap();

    let mut bench = Workbench::default();
    bench.open_file(&path, Some(TextEncoding::Utf8)).unwrap();
    bench.set_search_query("alpha").unwrap();
    assert_eq!(bench.search().matches, vec![0..5, 11..16]);

    assert_eq!(bench.find_next().unwrap(), Some(0..5));
    assert_eq!(bench.active_document().unwrap().selection(), Some(0..5));
    assert_eq!(bench.find_next().unwrap(), Some(11..16));
    assert_eq!(bench.find_previous().unwrap(), Some(0..5));
}

#[test]
fn save_as_uses_safe_file_session_and_retitles_tab() {
    let work = Workspace::new();
    let original = work.path("old.txt");
    let target = work.path("new.txt");
    fs::write(&original, b"old").unwrap();

    let mut bench = Workbench::default();
    bench
        .open_file(&original, Some(TextEncoding::Utf8))
        .unwrap();
    bench.move_end(false).unwrap();
    bench.insert_text(" content").unwrap();
    bench.save_active_as(&target).unwrap();

    assert_eq!(fs::read(&original).unwrap(), b"old");
    assert_eq!(fs::read(&target).unwrap(), b"old content");
    assert_eq!(bench.tabs().active().unwrap().title, "new.txt");
    assert!(!bench.context().document_dirty);
}

#[test]
fn dirty_close_routes_to_one_explicit_decision() {
    let work = Workspace::new();
    let path = work.path("dirty.txt");
    fs::write(&path, b"x").unwrap();

    let mut bench = Workbench::default();
    let tab = bench.open_file(&path, Some(TextEncoding::Utf8)).unwrap();
    bench.move_end(false).unwrap();
    bench.insert_text("y").unwrap();

    assert_eq!(
        bench.execute(CommandId::FileClose).unwrap(),
        CommandOutcome::NeedsCloseDecision(tab)
    );
    assert_eq!(bench.tabs().active_id(), Some(tab));
}

#[test]
fn command_outcomes_keep_ui_prompts_out_of_widgets() {
    let mut bench = Workbench::default();
    assert_eq!(
        bench.execute(CommandId::FileOpen).unwrap(),
        CommandOutcome::PromptOpen
    );
    assert_eq!(
        bench.execute(CommandId::FileNew).unwrap(),
        CommandOutcome::PromptNew
    );
    assert_eq!(
        bench.execute(CommandId::PaletteOpen).unwrap(),
        CommandOutcome::ShowPalette
    );
}

#[test]
fn replace_current_and_all_use_the_document_encoding() {
    let work = Workspace::new();
    let path = work.path("replace.p");
    fs::write(&path, b"nino nino").unwrap();

    let mut bench = Workbench::default();
    bench
        .open_file(&path, Some(TextEncoding::Iso8859_1))
        .unwrap();
    bench.set_search_query("nino").unwrap();
    assert_eq!(bench.find_next().unwrap(), Some(0..4));
    assert!(bench.replace_current("niño").unwrap());
    assert_eq!(bench.active_bytes().unwrap(), b"ni\xf1o nino");

    bench.set_search_query("nino").unwrap();
    assert_eq!(bench.replace_all("café").unwrap(), 1);
    assert_eq!(bench.active_bytes().unwrap(), b"ni\xf1o caf\xe9");
}

#[test]
fn reopen_closed_tab_restores_the_same_document_session() {
    let work = Workspace::new();
    let path = work.path("reopen.txt");
    fs::write(&path, b"abc").unwrap();

    let mut bench = Workbench::default();
    let tab = bench.open_file(&path, Some(TextEncoding::Utf8)).unwrap();
    assert_eq!(bench.close_active(false).unwrap(), None);
    assert!(bench.context().can_reopen_closed_tab);
    assert!(bench.reopen_closed());
    assert_eq!(bench.tabs().active_id(), Some(tab));
    assert_eq!(bench.active_bytes().unwrap(), b"abc");
}


#[test]
fn line_commands_are_real_workbench_operations() {
    let work = Workspace::new();
    let path = work.path("lines.txt");
    fs::write(&path, b"one\ntwo\nthree").unwrap();

    let mut bench = Workbench::default();
    bench.open_file(&path, Some(TextEncoding::Utf8)).unwrap();
    bench.move_vertical(1, false).unwrap();

    bench.execute(CommandId::EditMoveLineUp).unwrap();
    assert_eq!(bench.active_bytes().unwrap(), b"two\none\nthree");
    assert_eq!(
        bench.active_position(),
        Some(TextPosition { line: 0, column: 0 })
    );

    bench.execute(CommandId::EditMoveLineDown).unwrap();
    assert_eq!(bench.active_bytes().unwrap(), b"one\ntwo\nthree");
    assert_eq!(
        bench.active_position(),
        Some(TextPosition { line: 1, column: 0 })
    );

    bench.execute(CommandId::EditDeleteLine).unwrap();
    assert_eq!(bench.active_bytes().unwrap(), b"one\nthree");
}

#[test]
fn duplicate_line_handles_final_line_without_a_terminal_newline() {
    let work = Workspace::new();
    let path = work.path("duplicate.txt");
    fs::write(&path, b"value").unwrap();

    let mut bench = Workbench::default();
    bench.open_file(&path, Some(TextEncoding::Utf8)).unwrap();
    bench.execute(CommandId::EditDuplicate).unwrap();

    assert_eq!(bench.active_bytes().unwrap(), b"value\r\nvalue");
    assert_eq!(
        bench.active_position(),
        Some(TextPosition { line: 1, column: 0 })
    );
}

#[test]
fn toggle_line_comment_uses_the_language_profile() {
    let work = Workspace::new();
    let path = work.path("comment.cs");
    fs::write(&path, b"    value();\n").unwrap();

    let mut bench = Workbench::default();
    bench.open_file(&path, Some(TextEncoding::Utf8)).unwrap();
    bench.execute(CommandId::EditToggleLineComment).unwrap();
    assert_eq!(bench.active_bytes().unwrap(), b"    // value();\n");

    bench.execute(CommandId::EditToggleLineComment).unwrap();
    assert_eq!(bench.active_bytes().unwrap(), b"    value();\n");
}
