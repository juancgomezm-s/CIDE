use jc_core::{Document, RecoveryLimits};
use jc_files::{
    read_recovery_path, write_recovery_path, ExternalState, FileError, FileSession,
    SessionRecoveryError, SessionRecoveryInfo, SessionRecoveryLimits,
};
use jc_text::{EncodingSource, TextEncoding};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cide-session-recovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn encode(session: &FileSession) -> Vec<u8> {
    let mut bytes = Vec::new();
    session.recovery_info().write_to(&mut bytes).unwrap();
    session.document().write_recovery(&mut bytes).unwrap();
    bytes
}
fn restore(mut bytes: &[u8]) -> FileSession {
    let info =
        SessionRecoveryInfo::read_from(&mut bytes, SessionRecoveryLimits::default()).unwrap();
    let document = Document::read_recovery(&mut bytes, RecoveryLimits::default()).unwrap();
    assert!(bytes.is_empty());
    FileSession::from_recovery(info, document).unwrap()
}

#[test]
fn recovered_legacy_session_keeps_binary_bytes_history_and_original_disk_conflict_baseline() {
    let work = Workspace::new();
    let path = work.0.join("original.p");
    let original = b"\xef\xbb\xbfni\xf1o\r\n\x00\xff\rfin";
    fs::write(&path, original).unwrap();
    let mut session = FileSession::open(&path, Some(TextEncoding::Iso8859_1)).unwrap();
    session.insert_text(0, "á").unwrap();
    session.document_mut().insert(5, b"\xff\r\n").unwrap();
    assert!(session.document_mut().undo());
    let current = session.document().to_bytes();
    let encoded = encode(&session);
    fs::write(&path, b"version externa\r\n").unwrap();
    let mut recovered = restore(&encoded);
    assert_eq!(recovered.path(), path);
    assert_eq!(recovered.encoding(), Some(TextEncoding::Iso8859_1));
    assert_eq!(recovered.encoding_source(), EncodingSource::Explicit);
    assert_eq!(recovered.document().to_bytes(), current);
    assert_eq!(
        recovered.document().revision(),
        session.document().revision()
    );
    assert_eq!(
        recovered.document().undo_steps(),
        session.document().undo_steps()
    );
    assert_eq!(
        recovered.document().redo_steps(),
        session.document().redo_steps()
    );
    assert!(recovered.document().is_modified());
    assert_eq!(recovered.external_state().unwrap(), ExternalState::Changed);
    assert!(matches!(
        recovered.save(),
        Err(FileError::ExternalModification(ExternalState::Changed))
    ));
    assert_eq!(fs::read(&path).unwrap(), b"version externa\r\n");
    assert!(recovered.document_mut().redo());
    assert!(session.document_mut().redo());
    assert_eq!(
        recovered.document().to_bytes(),
        session.document().to_bytes()
    );
    assert!(recovered.document_mut().undo());
    assert!(recovered.document_mut().undo());
    assert_eq!(recovered.document().to_bytes(), original);
    assert!(!recovered.document().is_modified());
}

#[test]
fn restoring_missing_original_or_new_untitled_session_never_opens_or_creates_its_path() {
    let work = Workspace::new();
    let path = work.0.join("eliminado.p");
    fs::write(&path, b"ni\xf1o\r\n").unwrap();
    let session = FileSession::open(&path, None).unwrap();
    let encoded = encode(&session);
    fs::remove_file(&path).unwrap();
    let recovered = restore(&encoded);
    assert_eq!(recovered.encoding(), None);
    assert_eq!(
        recovered.encoding_source(),
        EncodingSource::UndeterminedLegacy
    );
    assert_eq!(recovered.document().to_bytes(), b"ni\xf1o\r\n");
    assert!(!path.exists());
    assert_eq!(recovered.external_state().unwrap(), ExternalState::Missing);
    let new_path = work.0.join("sin-titulo-3");
    let mut untitled = FileSession::new(&new_path, TextEncoding::Utf8);
    untitled.document_mut().insert(0, b"\x00\xff\r\n").unwrap();
    let recovered = restore(&encode(&untitled));
    assert!(recovered.is_new());
    assert_eq!(recovered.path(), new_path);
    assert!(!new_path.exists());
    assert_eq!(recovered.document().to_bytes(), b"\x00\xff\r\n");
    assert!(recovered.document().is_modified());
    let empty = FileSession::new(PathBuf::new(), TextEncoding::Ibm850);
    let recovered = restore(&encode(&empty));
    assert!(recovered.is_new());
    assert!(recovered.path().as_os_str().is_empty());
}

#[cfg(any(windows, unix))]
#[test]
fn session_recovery_preserves_native_non_scalar_paths_without_lossy_conversion() {
    #[cfg(windows)]
    let path = {
        use std::os::windows::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_wide(&[
            b'C' as u16,
            b':' as u16,
            b'\\' as u16,
            0xd800,
            b'x' as u16,
        ]))
    };
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/\xffname".to_vec()))
    };
    let session = FileSession::new(&path, TextEncoding::Windows1252);
    let recovered = restore(&encode(&session));
    assert_eq!(recovered.path(), path);
    assert_eq!(recovered.encoding(), Some(TextEncoding::Windows1252));
    let mut encoded = Vec::new();
    write_recovery_path(&path, &mut encoded).unwrap();
    encoded.extend_from_slice(b"next record");
    let mut reader = &encoded[..];
    assert_eq!(read_recovery_path(&mut reader, 32768).unwrap(), path);
    assert_eq!(reader, b"next record");
    assert!(matches!(
        read_recovery_path(&mut &encoded[..], 1),
        Err(SessionRecoveryError::LimitExceeded(_))
    ));
}

#[test]
fn clean_utf8_sessions_keep_their_original_probe_provenance_without_reopening_missing_files() {
    let work = Workspace::new();
    for (index, (bytes, source)) in [
        (&b"ascii\r\n"[..], EncodingSource::AsciiCompatible),
        ("niño\r\n".as_bytes(), EncodingSource::ValidUtf8),
        (&b"\xef\xbb\xbfascii\r\n"[..], EncodingSource::Utf8Bom),
    ]
    .into_iter()
    .enumerate()
    {
        let path = work.0.join(format!("original-{index}.rs"));
        fs::write(&path, bytes).unwrap();
        let session = FileSession::open(&path, None).unwrap();
        let encoded = encode(&session);
        fs::remove_file(&path).unwrap();
        let recovered = restore(&encoded);
        assert_eq!(recovered.encoding(), Some(TextEncoding::Utf8));
        assert_eq!(recovered.encoding_source(), source);
        assert_eq!(recovered.document().to_bytes(), bytes);
        assert!(!recovered.document().is_modified());
        assert_eq!(recovered.document().undo_steps(), 0);
        assert!(!path.exists());
    }
}

#[test]
fn session_metadata_rejects_truncation_hostile_lengths_and_incompatible_tags_before_large_allocation(
) {
    let info = FileSession::new("small.rs", TextEncoding::Utf8).recovery_info();
    let mut encoded = Vec::new();
    info.write_to(&mut encoded).unwrap();
    for end in 0..encoded.len() {
        let result = std::panic::catch_unwind(|| {
            SessionRecoveryInfo::read_from(&mut &encoded[..end], SessionRecoveryLimits::default())
        });
        assert!(result.is_ok());
        assert!(result.unwrap().is_err());
    }
    let unit_bytes = if cfg!(windows) { 2 } else { 1 };
    let tags = 32 + 8 * unit_bytes;
    for (at, value) in [
        (8, 99),
        (16, 999),
        (24, u64::MAX),
        (tags, 99),
        (tags + 8, 99),
        (tags + 16, 3),
    ] {
        let mut corrupt = encoded.clone();
        corrupt[at..at + 8].copy_from_slice(&value.to_le_bytes());
        assert!(SessionRecoveryInfo::read_from(
            &mut &corrupt[..],
            SessionRecoveryLimits::default()
        )
        .is_err());
    }
    let limits = SessionRecoveryLimits {
        max_path_units: 1,
        ..Default::default()
    };
    assert!(matches!(
        SessionRecoveryInfo::read_from(&mut &encoded[..], limits),
        Err(SessionRecoveryError::LimitExceeded(_))
    ));
    let incompatible = SessionRecoveryInfo {
        encoding: None,
        encoding_source: EncodingSource::Explicit,
        ..info
    };
    assert!(FileSession::from_recovery(incompatible, Document::default()).is_err());
}
