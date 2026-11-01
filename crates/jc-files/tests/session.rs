use jc_files::{ExternalState, FileError, FileSession};
use jc_text::{EncodingSource, LineEndingKind, TextEncoding};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "jc-ide-file-session-{}-{}",
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
fn opening_and_saving_without_edits_preserves_every_byte() {
    let work = Workspace::new();
    let path = work.path("legacy.p");
    let original = b"/* ni\xf1o */\r\nDISPLAY x.\r\n".to_vec();
    fs::write(&path, &original).unwrap();

    let mut session = FileSession::open(&path, Some(TextEncoding::Iso8859_1)).unwrap();
    assert_eq!(session.document().to_bytes(), original);
    assert_eq!(session.external_state().unwrap(), ExternalState::Unchanged);
    session.save().unwrap();

    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!session.document().is_modified());
    let inspection = session.inspection();
    assert_eq!(inspection.line_endings.kind, LineEndingKind::CrLf);
    assert!(inspection.line_endings.final_newline);
}

#[test]
fn unknown_legacy_encoding_requires_explicit_selection_before_text_insertion() {
    let work = Workspace::new();
    let path = work.path("unknown.p");
    fs::write(&path, b"ni\xf1o").unwrap();

    let mut session = FileSession::open(&path, None).unwrap();
    assert_eq!(session.encoding(), None);
    assert_eq!(
        session.encoding_source(),
        EncodingSource::UndeterminedLegacy
    );
    assert!(matches!(
        session.insert_text(0, "x"),
        Err(FileError::EncodingRequired)
    ));

    session.set_encoding(TextEncoding::Iso8859_1);
    session.insert_text(0, "á").unwrap();
    assert_eq!(session.document().to_bytes()[0], 0xe1);
}

#[test]
fn unsupported_characters_do_not_modify_the_document() {
    let work = Workspace::new();
    let path = work.path("cp1252.p");
    fs::write(&path, b"DISPLAY x.").unwrap();

    let mut session = FileSession::open(&path, Some(TextEncoding::Windows1252)).unwrap();
    let before = session.document().to_bytes();
    assert!(matches!(
        session.insert_text(0, "😀"),
        Err(FileError::Encode(_))
    ));
    assert_eq!(session.document().to_bytes(), before);
    assert!(!session.document().is_modified());
}

#[test]
fn save_replaces_existing_file_and_marks_revision_saved() {
    let work = Workspace::new();
    let path = work.path("edit.p");
    fs::write(&path, b"abc\r\n").unwrap();

    let mut session = FileSession::open(&path, Some(TextEncoding::Iso8859_1)).unwrap();
    session.replace_text(0..3, "niño").unwrap();
    assert!(session.document().is_modified());
    session.save().unwrap();

    assert_eq!(fs::read(&path).unwrap(), b"ni\xf1o\r\n");
    assert!(!session.document().is_modified());
    assert_eq!(session.external_state().unwrap(), ExternalState::Unchanged);
}

#[test]
fn external_content_change_blocks_normal_save_and_preserves_external_version() {
    let work = Workspace::new();
    let path = work.path("conflict.p");
    fs::write(&path, b"original").unwrap();

    let mut session = FileSession::open(&path, Some(TextEncoding::Utf8)).unwrap();
    session.insert_text(8, " local").unwrap();
    fs::write(&path, b"external").unwrap();

    assert_eq!(session.external_state().unwrap(), ExternalState::Changed);
    assert!(matches!(
        session.save(),
        Err(FileError::ExternalModification(ExternalState::Changed))
    ));
    assert_eq!(fs::read(&path).unwrap(), b"external");
    assert!(session.document().is_modified());

    session.force_save().unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"original local");
    assert!(!session.document().is_modified());
}

#[test]
fn deletion_is_reported_as_an_external_conflict() {
    let work = Workspace::new();
    let path = work.path("deleted.p");
    fs::write(&path, b"abc").unwrap();
    let mut session = FileSession::open(&path, None).unwrap();
    fs::remove_file(&path).unwrap();

    assert_eq!(session.external_state().unwrap(), ExternalState::Missing);
    assert!(matches!(
        session.save(),
        Err(FileError::ExternalModification(ExternalState::Missing))
    ));
    assert!(!path.exists());
}

#[test]
fn new_session_creates_file_only_when_name_is_still_free() {
    let work = Workspace::new();
    let path = work.path("new.p");
    let mut session = FileSession::new(&path, TextEncoding::Iso8859_1);
    session.insert_text(0, "niño\r\n").unwrap();
    assert_eq!(session.external_state().unwrap(), ExternalState::Unchanged);
    session.save().unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"ni\xf1o\r\n");
    assert!(!session.document().is_modified());
}

#[test]
fn new_session_refuses_to_overwrite_a_file_that_appeared_after_creation() {
    let work = Workspace::new();
    let path = work.path("race.p");
    let mut session = FileSession::new(&path, TextEncoding::Utf8);
    session.insert_text(0, "mine").unwrap();
    fs::write(&path, b"someone else").unwrap();

    assert_eq!(session.external_state().unwrap(), ExternalState::Changed);
    assert!(matches!(
        session.save(),
        Err(FileError::ExternalModification(ExternalState::Changed))
    ));
    assert_eq!(fs::read(&path).unwrap(), b"someone else");
}
