//! Observable byte, selection and transaction contracts of universal line commands.
use jc_app::{PaneId, Workbench};
use jc_text::{encode, TextEncoding};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn bytes(bytes: &[u8]) -> Self {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let base = workspace.join("work/test-temp");
        fs::create_dir_all(&base).unwrap();
        let path = base.join(format!(
            "join-lines-{}-{}.txt",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn memory(text: &str, encoding: TextEncoding) -> Workbench {
    let mut app = Workbench::default();
    app.new_file("line-command-memory.txt", encoding);
    app.insert_text(text).unwrap();
    app
}
fn select(app: &mut Workbench, from: usize, to: usize) {
    let tab = app.tabs().active_id().unwrap();
    assert!(app.jump_to_location(tab, from));
    // Test fixtures choose valid scalar boundaries; walking preserves selection
    // direction rather than writing internal editor state.
    let steps = 2 * app.active_document().unwrap().session.document().len() + 4;
    for _ in 0..steps {
        let cursor = app.active_editor().unwrap().cursor;
        if cursor == to {
            return;
        }
        if cursor < to {
            app.move_right(true).unwrap();
        } else {
            app.move_left(true).unwrap();
        }
    }
    panic!("selection boundary unreachable");
}

#[test]
fn mixed_eol_ascii_indentation_and_blank_lines_join_as_one_shared_undo() {
    let original = "first\r\n \t\rsecond\t \n  third\r\n";
    let mut app = memory(original, TextEncoding::Utf8);
    app.select_all().unwrap();
    let before = app
        .active_document()
        .unwrap()
        .session
        .document()
        .undo_steps();
    assert!(app.split_editor_right());
    app.join_lines().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"first second\t third\r\n");
    assert_eq!(
        app.active_document()
            .unwrap()
            .session
            .document()
            .undo_steps(),
        before + 1
    );
    app.focus_editor_pane(PaneId::Secondary);
    app.undo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), original.as_bytes());
    app.focus_editor_pane(PaneId::Primary);
    app.redo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"first second\t third\r\n");
    for (source, expected) in [
        ("a \n b", "a b"),
        ("a\t\n b", "a\tb"),
        (" \n b", " b"),
        ("\n b", "b"),
        ("a\n \n b", "a b"),
        ("a\n \n", "a\n"),
    ] {
        let mut app = memory(source, TextEncoding::Utf8);
        app.select_all().unwrap();
        app.join_lines().unwrap();
        assert_eq!(app.active_bytes().unwrap(), expected.as_bytes());
    }
}

#[test]
fn bom_invalid_utf8_and_explicit_legacy_bytes_survive_join_and_disk_is_unchanged() {
    for (encoding, original, expected) in [
        (
            TextEncoding::Utf8,
            &b"\xef\xbb\xbfa\xff\r\n \tb\xfe\n"[..],
            &b"\xef\xbb\xbfa\xff b\xfe\n"[..],
        ),
        (
            TextEncoding::Windows1252,
            &b"ni\xf1o\r\n \x80 fin\r"[..],
            &b"ni\xf1o \x80 fin\r"[..],
        ),
        (
            TextEncoding::Ibm850,
            &b"ni\xa4o\n \x82 fin"[..],
            &b"ni\xa4o \x82 fin"[..],
        ),
    ] {
        let fixture = Fixture::bytes(original);
        let mut app = Workbench::default();
        app.open_file(&fixture.0, Some(encoding)).unwrap();
        app.select_all().unwrap();
        app.join_lines().unwrap();
        assert_eq!(app.active_bytes().unwrap(), expected);
        app.undo().unwrap();
        assert_eq!(app.active_bytes().unwrap(), original);
        assert_eq!(fs::read(&fixture.0).unwrap(), original);
    }
}

#[test]
fn selection_end_at_next_line_start_excludes_it_and_eof_join_keeps_terminal_newline() {
    let mut app = memory("a\r\n  b\nc\r\n", TextEncoding::Utf8);
    select(&mut app, 0, 7);
    app.join_lines().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"a b\nc\r\n");
    app.undo().unwrap();
    select(&mut app, 1, 0);
    let revision = app.active_document().unwrap().session.document().revision();
    app.join_lines().unwrap();
    assert_eq!(
        app.active_document().unwrap().session.document().revision(),
        revision
    );
    let tab = app.tabs().active_id().unwrap();
    assert!(app.jump_to_location(tab, 7));
    app.join_lines().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"a\r\n  b\nc\r\n");
    let mut no_final = memory("a\n b", TextEncoding::Utf8);
    let tab = no_final.tabs().active_id().unwrap();
    assert!(no_final.jump_to_location(tab, 0));
    no_final.join_lines().unwrap();
    assert_eq!(no_final.active_bytes().unwrap(), b"a b");
}

#[test]
fn split_delegates_preferred_eol_and_encoding_and_rejects_rectangles() {
    for (text, encoding, expected) in [
        ("éx\r\nnext", TextEncoding::Utf8, "é\r\nx\r\nnext"),
        ("éx\nnext", TextEncoding::Windows1252, "é\nx\nnext"),
    ] {
        let mut app = memory(text, encoding);
        let tab = app.tabs().active_id().unwrap();
        let offset = encode("é", encoding).unwrap().len();
        assert!(app.jump_to_location(tab, offset));
        let before = app
            .active_document()
            .unwrap()
            .session
            .document()
            .undo_steps();
        app.split_line().unwrap();
        assert_eq!(
            app.active_bytes().unwrap(),
            encode(expected, encoding).unwrap()
        );
        assert_eq!(
            app.active_document()
                .unwrap()
                .session
                .document()
                .undo_steps(),
            before + 1
        );
        app.undo().unwrap();
        assert_eq!(app.active_bytes().unwrap(), encode(text, encoding).unwrap());
    }
    let fixture = Fixture::bytes(b"\xef\xbb\xbfabc\n");
    let mut bom = Workbench::default();
    let tab = bom.open_file(&fixture.0, Some(TextEncoding::Utf8)).unwrap();
    assert!(bom.jump_to_location(tab, 4));
    bom.split_line().unwrap();
    assert_eq!(bom.active_bytes().unwrap(), b"\xef\xbb\xbfa\nbc\n");
    bom.undo().unwrap();
    assert_eq!(bom.active_bytes().unwrap(), b"\xef\xbb\xbfabc\n");
    assert_eq!(fs::read(&fixture.0).unwrap(), b"\xef\xbb\xbfabc\n");
    let mut app = memory("ab\ncd", TextEncoding::Utf8);
    let tab = app.tabs().active_id().unwrap();
    assert!(app.jump_to_location(tab, 0));
    app.start_rectangle().unwrap();
    let bytes = app.active_bytes().unwrap();
    let revision = app.active_document().unwrap().session.document().revision();
    assert!(app.join_lines().is_err());
    assert!(app.split_line().is_err());
    assert_eq!(app.active_bytes().unwrap(), bytes);
    assert_eq!(
        app.active_document().unwrap().session.document().revision(),
        revision
    );
    assert!(app.rectangle_for_pane(PaneId::Primary).is_some());
}

#[test]
fn boundary_and_source_budgets_reject_atomically_but_exact_boundary_count_is_allowed() {
    let mut app = memory(&"x\n".repeat(1026), TextEncoding::Utf8);
    app.select_all().unwrap();
    let bytes = app.active_bytes().unwrap();
    let revision = app.active_document().unwrap().session.document().revision();
    let undo = app
        .active_document()
        .unwrap()
        .session
        .document()
        .undo_steps();
    assert!(app.join_lines().is_err());
    assert_eq!(app.active_bytes().unwrap(), bytes);
    assert_eq!(
        app.active_document().unwrap().session.document().revision(),
        revision
    );
    assert_eq!(
        app.active_document()
            .unwrap()
            .session
            .document()
            .undo_steps(),
        undo
    );
    let mut exact = memory(&"x\n".repeat(1025), TextEncoding::Utf8);
    exact.select_all().unwrap();
    exact.join_lines().unwrap();
    assert_eq!(
        exact.active_bytes().unwrap(),
        format!("{}\n", vec!["x"; 1025].join(" ")).as_bytes()
    );
    let mut large = memory(
        &format!("a\n{}b", " ".repeat(2 * 1024 * 1024)),
        TextEncoding::Utf8,
    );
    let tab = large.tabs().active_id().unwrap();
    assert!(large.jump_to_location(tab, 0));
    let revision = large
        .active_document()
        .unwrap()
        .session
        .document()
        .revision();
    assert!(large.join_lines().is_err());
    assert_eq!(
        large
            .active_document()
            .unwrap()
            .session
            .document()
            .revision(),
        revision
    );
}
