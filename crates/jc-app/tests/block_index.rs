//! Observable editor regressions for line metadata; logical retention is not RAM.
use jc_app::{PaneId, TabId, TextReplacement, Workbench};
use jc_text::{TextEncoding, TextPosition};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    ops::Range,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    directory: PathBuf,
    path: PathBuf,
}
impl Fixture {
    fn new(label: &str) -> Self {
        let project = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let base = project.join("work/test-temp");
        fs::create_dir_all(&base).unwrap();
        let directory = base.join(format!(
            "block-index-{}-{}-{label}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        Self {
            path: directory.join("source.txt"),
            directory,
        }
    }
    fn create(&self) -> File {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)
            .unwrap()
    }
    fn bytes(&self, bytes: &[u8]) {
        self.create().write_all(bytes).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only the explicitly owned fixture file; never recurse into a tree.
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir(&self.directory);
    }
}

/// Independent byte scanner: no production decoder/index or lossy conversion.
fn line_ranges(bytes: &[u8], bom: bool) -> Vec<(Range<usize>, Range<usize>)> {
    let mut start = if bom && bytes.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    let mut offset = start;
    let mut lines = Vec::new();
    while offset < bytes.len() {
        if bytes[offset] == b'\r' || bytes[offset] == b'\n' {
            let width = if bytes[offset] == b'\r' && bytes.get(offset + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
            lines.push((start..offset, offset..offset + width));
            offset += width;
            start = offset;
        } else {
            offset += 1;
        }
    }
    lines.push((start..bytes.len(), bytes.len()..bytes.len()));
    lines
}

fn assert_geometry(app: &mut Workbench, tab: TabId, expected: &[u8], bom: bool) {
    assert_eq!(app.active_bytes().unwrap(), expected);
    let lines = line_ranges(expected, bom);
    assert_eq!(app.line_count_for_tab(tab), Some(lines.len()));
    assert_eq!(app.line_range_for_tab(tab, lines.len()), None);
    for (line, (content, ending)) in lines.iter().enumerate() {
        assert_eq!(
            app.line_range_for_tab(tab, line),
            Some((content.clone(), ending.clone())),
            "line {line}"
        );
        assert_eq!(
            app.position_for_tab(tab, content.start),
            Some(TextPosition { line, column: 0 }),
            "start of line {line}"
        );
        if ending.len() == 2 {
            assert_eq!(&expected[ending.clone()], b"\r\n");
            let interior = ending.start + 1;
            assert_eq!(app.position_for_tab(tab, interior), None);
            let cursor = app.active_editor().unwrap().cursor;
            assert!(!app.jump_to_location(tab, interior));
            assert_eq!(app.active_editor().unwrap().cursor, cursor);
        }
    }
}

fn corpus(utf8: bool) -> Vec<u8> {
    let mut bytes = if utf8 {
        b"\xef\xbb\xbf".to_vec()
    } else {
        Vec::new()
    };
    for line in 0..8192 {
        if utf8 {
            bytes.extend_from_slice("é😀e\u{301} ".as_bytes());
            // Invalid UTF-8 and an incomplete sequence adjacent to plain bytes.
            bytes.extend_from_slice(b"\xff \xe2\x82 tail");
            if line == 4096 {
                bytes.extend_from_slice(b"\xef\xbb\xbf");
            }
        } else {
            bytes.extend_from_slice(b"ni\xf1o \x80 \x93caf\xe9\x94");
        }
        bytes.extend_from_slice(match line % 3 {
            0 => b"\r\n",
            1 => b"\n",
            _ => b"\r",
        });
    }
    bytes.extend_from_slice(if utf8 {
        b"last \xff without newline"
    } else {
        b"fin \xe9 sin newline"
    });
    bytes
}

struct ProposedEdit {
    range: Range<usize>,
    text: &'static str,
    encoded: &'static [u8],
}
fn assert_shared(app: &mut Workbench, tab: TabId, id: jc_app::DocumentId) {
    assert_eq!(app.tabs().tabs().len(), 1);
    let primary = app.panes().view(PaneId::Primary).unwrap();
    let secondary = app.panes().view(PaneId::Secondary).unwrap();
    assert_ne!(primary.view_id(), secondary.view_id());
    assert_eq!(primary.location().unwrap().tab, tab);
    assert_eq!(secondary.location().unwrap().tab, tab);
    for pane in [PaneId::Primary, PaneId::Secondary] {
        assert!(app.focus_editor_pane(pane));
        assert_eq!(app.active_document().unwrap().id, id);
        assert!(app.position_for_pane(pane).is_some());
    }
}

fn multiblock_transactions(utf8: bool) {
    let fixture = Fixture::new(if utf8 { "utf8" } else { "cp1252" });
    let original = corpus(utf8);
    fixture.bytes(&original);
    let encoding = if utf8 {
        TextEncoding::Utf8
    } else {
        TextEncoding::Windows1252
    };
    let mut app = Workbench::default();
    let tab = app.open_file(&fixture.path, Some(encoding)).unwrap();
    assert_eq!(app.active_encoding(), Some(encoding));
    assert_geometry(&mut app, tab, &original, utf8);
    let id = app.active_document().unwrap().id;
    let lines = line_ranges(&original, utf8);
    assert!(lines.len() > 4096);
    assert!(app.jump_to_location(tab, lines[128].0.start));
    assert!(app.split_editor_right());
    assert!(app.focus_editor_pane(PaneId::Secondary));
    assert!(app.jump_to_location(tab, lines[7500].0.start));
    assert_shared(&mut app, tab, id);

    let revision = app.active_document().unwrap().session.document().revision();
    let undos = app
        .active_document()
        .unwrap()
        .session
        .document()
        .undo_steps();
    // Rejection must leave both bytes and the shared undo untouched.
    let crlf = lines
        .iter()
        .find(|(_, ending)| ending.len() == 2)
        .unwrap()
        .1
        .start
        + 1;
    assert!(app
        .apply_text_edits(
            id,
            revision,
            vec![TextReplacement {
                range: crlf..crlf,
                text: "x".into()
            }]
        )
        .is_err());
    assert_eq!(app.active_bytes().unwrap(), original);
    assert_eq!(
        app.active_document()
            .unwrap()
            .session
            .document()
            .undo_steps(),
        undos
    );

    let (replacement_text, replacement_bytes, insertion_text, insertion_bytes, eof_text, eof_bytes) =
        if utf8 {
            (
                "\u{feff}réponse😀\r\nmid\nlast\r",
                "\u{feff}réponse😀\r\nmid\nlast\r".as_bytes(),
                "split\r\nα\rβ\n",
                "split\r\nα\rβ\n".as_bytes(),
                "\r\nEOF_é",
                "\r\nEOF_é".as_bytes(),
            )
        } else {
            (
                "niño €\r\nmid\nfin\r",
                &b"ni\xf1o \x80\r\nmid\nfin\r"[..],
                "café\r\nniño\r€\n",
                &b"caf\xe9\r\nni\xf1o\r\x80\n"[..],
                "\r\nEOF_é",
                &b"\r\nEOF_\xe9"[..],
            )
        };
    let edits = [
        ProposedEdit {
            range: lines[252].0.start..lines[1040].0.start,
            text: replacement_text,
            encoded: replacement_bytes,
        },
        ProposedEdit {
            range: lines[2045].0.start..lines[2600].0.start,
            text: "",
            encoded: b"",
        },
        ProposedEdit {
            range: lines[4095].0.start..lines[4095].0.start,
            text: insertion_text,
            encoded: insertion_bytes,
        },
        ProposedEdit {
            range: lines[6100].0.start..lines[6800].0.start,
            text: "tail\r\n",
            encoded: b"tail\r\n",
        },
        ProposedEdit {
            range: original.len()..original.len(),
            text: eof_text,
            encoded: eof_bytes,
        },
    ];
    let mut expected = original.clone();
    for edit in edits.iter().rev() {
        expected.splice(edit.range.clone(), edit.encoded.iter().copied());
    }
    app.apply_text_edits(
        id,
        revision,
        edits
            .iter()
            .map(|edit| TextReplacement {
                range: edit.range.clone(),
                text: edit.text.into(),
            })
            .collect(),
    )
    .unwrap();
    assert_eq!(
        app.active_document().unwrap().session.document().revision(),
        revision + 1
    );
    assert_eq!(
        app.active_document()
            .unwrap()
            .session
            .document()
            .undo_steps(),
        undos + 1
    );
    assert_shared(&mut app, tab, id);
    assert_geometry(&mut app, tab, &expected, utf8);
    assert_eq!(fs::read(&fixture.path).unwrap(), original);

    // Undo from one view, redo from the other: the content history is shared.
    assert!(app.focus_editor_pane(PaneId::Secondary));
    app.undo().unwrap();
    assert_geometry(&mut app, tab, &original, utf8);
    assert!(!app.context().document_dirty);
    assert!(app.focus_editor_pane(PaneId::Primary));
    assert_eq!(
        app.active_document()
            .unwrap()
            .session
            .document()
            .undo_steps(),
        undos
    );
    assert!(app.context().can_redo);
    app.redo().unwrap();
    assert_geometry(&mut app, tab, &expected, utf8);
    assert!(app.focus_editor_pane(PaneId::Secondary));
    app.undo().unwrap();
    assert_geometry(&mut app, tab, &original, utf8);
    assert_shared(&mut app, tab, id);
    assert_eq!(fs::read(&fixture.path).unwrap(), original);
}

#[test]
fn utf8_bom_invalid_bytes_and_mixed_endings_survive_multiblock_transactions_and_shared_history() {
    multiblock_transactions(true);
}

#[test]
fn explicit_windows1252_and_mixed_endings_survive_multiblock_transactions_and_shared_history() {
    multiblock_transactions(false);
}

const DENSE_BYTES: usize = 4 * 1024 * 1024;
const DENSE_UNIT: &[u8; 8] = b"abcdefg\n";
fn dense_block() -> Vec<u8> {
    let mut block = vec![0; 64 * 1024];
    for unit in block.chunks_exact_mut(DENSE_UNIT.len()) {
        unit.copy_from_slice(DENSE_UNIT);
    }
    block
}
fn assert_dense_disk(path: &Path) {
    let mut file = File::open(path).unwrap();
    let expected = dense_block();
    let mut block = vec![0; expected.len()];
    let mut remaining = DENSE_BYTES;
    while remaining > 0 {
        let count = remaining.min(block.len());
        file.read_exact(&mut block[..count]).unwrap();
        assert_eq!(&block[..count], &expected[..count]);
        remaining -= count;
    }
    assert_eq!(file.read(&mut block[..1]).unwrap(), 0);
}

#[test]
fn dense_four_mib_range_navigation_and_local_undo_keep_bounded_scan_and_projection_retention() {
    let fixture = Fixture::new("dense4m");
    {
        let block = dense_block();
        let mut file = fixture.create();
        let mut remaining = DENSE_BYTES;
        while remaining > 0 {
            let count = remaining.min(block.len());
            file.write_all(&block[..count]).unwrap();
            remaining -= count;
        }
    }
    let mut app = Workbench::default();
    let tab = app
        .open_file(&fixture.path, Some(TextEncoding::Utf8))
        .unwrap();
    assert_eq!(app.line_count_for_tab(tab), Some(DENSE_BYTES / 8 + 1));
    for line in [0, 255, 1024, DENSE_BYTES / 16, DENSE_BYTES / 8 - 1] {
        assert_eq!(
            app.line_range_for_tab(tab, line),
            Some((line * 8..line * 8 + 7, line * 8 + 7..line * 8 + 8))
        );
        assert_eq!(
            app.position_for_tab(tab, line * 8),
            Some(TextPosition { line, column: 0 })
        );
        assert!(app.visible_line(tab, line, 0, 80).is_some());
    }
    for line in 0..24 {
        app.visible_line(tab, line, 0, 80).unwrap();
    }
    assert!(app.jump_to_location(tab, 0));
    let before = app.text_statistics(tab).unwrap();
    for _ in 0..200 {
        app.move_right(false).unwrap();
        assert!(app.active_position().is_some());
    }
    for line in 0..24 {
        app.visible_line(tab, line, 0, 80).unwrap();
    }
    let navigated = app.text_statistics(tab).unwrap();
    assert_eq!(
        navigated.0, before.0,
        "warm movement must not rescan the source"
    );
    assert!(
        navigated.1 - before.1 < 64 * 1024,
        "range decoding must stay bounded"
    );
    // Abstract logical projection requirement, not a block-layout assertion,
    // private-process RAM target, working set or historical peak measurement.
    assert!(navigated.2 < DENSE_BYTES / 2);
    assert!(app.jump_to_location(tab, 0));
    let before_edit = app.text_statistics(tab).unwrap();
    app.insert_newline().unwrap();
    assert_eq!(app.line_count_for_tab(tab), Some(DENSE_BYTES / 8 + 2));
    assert_eq!(app.line_range_for_tab(tab, 0), Some((0..0, 0..1)));
    assert_eq!(app.line_range_for_tab(tab, 1), Some((1..8, 8..9)));
    for line in [255, 1024, DENSE_BYTES / 16, DENSE_BYTES / 8 - 1] {
        let start = line * 8 + 1;
        assert_eq!(
            app.line_range_for_tab(tab, line + 1),
            Some((start..start + 7, start + 7..start + 8))
        );
        assert_eq!(
            app.position_for_tab(tab, start),
            Some(TextPosition {
                line: line + 1,
                column: 0
            })
        );
    }
    let edited = app.text_statistics(tab).unwrap();
    assert!(edited.0 - before_edit.0 < 64 * 1024);
    assert!(edited.2 < DENSE_BYTES / 2);
    app.undo().unwrap();
    assert_eq!(app.line_count_for_tab(tab), Some(DENSE_BYTES / 8 + 1));
    for line in [0, 255, 1024, DENSE_BYTES / 16, DENSE_BYTES / 8 - 1] {
        assert_eq!(
            app.line_range_for_tab(tab, line),
            Some((line * 8..line * 8 + 7, line * 8 + 7..line * 8 + 8))
        );
        assert_eq!(
            app.position_for_tab(tab, line * 8),
            Some(TextPosition { line, column: 0 })
        );
    }
    let undone = app.text_statistics(tab).unwrap();
    assert!(undone.0 - before_edit.0 < 64 * 1024);
    assert!(undone.1 - before_edit.1 < 64 * 1024);
    assert!(undone.2 < DENSE_BYTES / 2);
    assert!(!app.context().document_dirty);
    let source = app.active_document().unwrap().session.document();
    assert_eq!(source.len(), DENSE_BYTES);
    for start in [0, 2048, DENSE_BYTES / 2 + 3, DENSE_BYTES - 64] {
        let end = (start + 64).min(DENSE_BYTES);
        let expected: Vec<_> = (start..end).map(|offset| DENSE_UNIT[offset % 8]).collect();
        assert_eq!(source.read_range(start..end).unwrap(), expected);
    }
    assert_dense_disk(&fixture.path);
}
