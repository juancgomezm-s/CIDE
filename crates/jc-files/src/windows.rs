//! ReplaceFileW preserves metadata. Never fall back to delete + rename.
//! https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew
use super::{fingerprint_file, ExternalState, FileError, Fingerprint};
use std::{ffi::c_void, fs, io, os::windows::ffi::OsStrExt, path::Path, ptr, time::Duration};

#[link(name = "kernel32")]
extern "system" {
    fn MoveFileExW(existing: *const u16, destination: *const u16, flags: u32) -> i32;
    fn ReplaceFileW(
        target: *const u16,
        replacement: *const u16,
        backup: *const u16,
        flags: u32,
        exclude: *mut c_void,
        reserved: *mut c_void,
    ) -> i32;
}

/// Publish an already flushed sibling without requiring hard-link support.
/// No REPLACE_EXISTING or COPY_ALLOWED: a competing destination must win, and
/// publication must not fall back to a partially visible cross-volume copy.
pub(super) fn commit_new_path(temporary: &Path, target: &Path) -> io::Result<()> {
    fn native(path: &Path) -> io::Result<Vec<u16>> {
        if path.as_os_str().encode_wide().any(|unit| unit == 0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "La ruta contiene NUL",
            ));
        }
        let name = path.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "El destino no tiene nombre de archivo",
            )
        })?;
        // canonicalize the existing parent, not the absent destination. Windows
        // returns an absolute verbatim path, retaining support beyond MAX_PATH.
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        Ok(parent
            .canonicalize()?
            .join(name)
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect())
    }
    let source = native(temporary)?;
    let destination = native(target)?;
    let ok = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            8, /* WRITE_THROUGH */
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn native_replace(temporary: &Path, target: &Path) -> io::Result<()> {
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let temporary: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            temporary.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(test)]
pub(super) fn replace_path(temporary: &Path, target: &Path) -> Result<(), FileError> {
    replace_expected(temporary, target, None)
}
pub(super) fn replace_expected(
    temporary: &Path,
    target: &Path,
    expected: Option<Fingerprint>,
) -> Result<(), FileError> {
    if !target.try_exists()? {
        if expected.is_some() {
            return Err(FileError::ExternalModification(ExternalState::Missing));
        }
        return fs::rename(temporary, target).map_err(Into::into);
    }
    guarded_expected(
        temporary,
        target,
        expected,
        native_replace,
        std::thread::sleep,
    )
}

#[cfg(test)]
fn guarded_replace(
    temporary: &Path,
    target: &Path,
    replace: impl FnMut(&Path, &Path) -> io::Result<()>,
    wait: impl FnMut(Duration),
) -> Result<(), FileError> {
    guarded_expected(temporary, target, None, replace, wait)
}
fn guarded_expected(
    temporary: &Path,
    target: &Path,
    expected: Option<Fingerprint>,
    mut replace: impl FnMut(&Path, &Path) -> io::Result<()>,
    mut wait: impl FnMut(Duration),
) -> Result<(), FileError> {
    // A retry must not overwrite a new version created while waiting for a lock.
    let original = fingerprint_file(target)?;
    if expected.is_some_and(|expected| original != expected) {
        return Err(FileError::ExternalModification(ExternalState::Changed));
    }
    let delays = [5, 10, 20, 40];
    for attempt in 0..=delays.len() {
        if attempt > 0 {
            match fingerprint_file(target) {
                Ok(actual) if actual == original => {}
                Ok(_) => return Err(FileError::ExternalModification(ExternalState::Changed)),
                Err(FileError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(FileError::ExternalModification(ExternalState::Missing))
                }
                Err(error) => return Err(error),
            }
        }
        match replace(temporary, target) {
            Ok(()) => return Ok(()),
            Err(error) if matches!(error.raw_os_error(), Some(1176 | 1177)) => {
                // These codes can leave names partially moved. Cleanup of the
                // replacement could destroy the only remaining copy of the edit.
                return Err(FileError::SaveRecovery {
                    target: target.to_owned(),
                    temporary: temporary.to_owned(),
                    error,
                });
            }
            Err(error)
                if matches!(error.raw_os_error(), Some(32 | 33 | 1175))
                    && attempt < delays.len() =>
            {
                wait(Duration::from_millis(delays[attempt]))
            }
            Err(error) => return Err(error.into()),
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::test_dir;
    #[test]
    fn create_new_save_publishes_complete_bytes_and_keeps_undo() {
        let dir = test_dir();
        let path = dir.join("nuevo-é.p");
        let bytes = b"\xef\xbb\xbfDEFINE VARIABLE x AS INTEGER.\r\n\xff";
        let mut session =
            crate::FileSession::new(dir.join("borrador.p"), jc_text::TextEncoding::Utf8);
        session.document_mut().insert(0, bytes).unwrap();
        session.save_as(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(!session.document().is_modified());
        assert_eq!(session.path(), path);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        assert!(session.document_mut().undo());
        assert!(session.document().is_empty());
        assert!(session.document().is_modified());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn create_new_publication_race_never_replaces_competing_file() {
        let dir = test_dir();
        let target = dir.join("competidor.p");
        let result = crate::atomic_replace_with_commit(
            &target,
            true,
            |file| {
                use std::io::Write;
                file.write_all(b"mine\r\n")?;
                fs::write(&target, b"external\r\n")
            },
            |_, _| panic!("create-only must not call replacement"),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&target).unwrap(), b"external\r\n");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_file(target).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn create_new_save_keeps_long_unicode_path_support() {
        let dir = test_dir();
        let nested = dir
            .join("á".repeat(90))
            .join("b".repeat(90))
            .join("c".repeat(90));
        fs::create_dir_all(&nested).unwrap();
        let target = nested.join("nuevo.p");
        assert!(target.as_os_str().encode_wide().count() > 260);
        let mut session = crate::FileSession::new(&target, jc_text::TextEncoding::Utf8);
        session.insert_text(0, "contenido\r\n").unwrap();
        session.save().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"contenido\r\n");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn transient_1175_retries_same_commit_and_persistent_lock_is_bounded() {
        let dir = test_dir();
        let target = dir.join("source.p");
        let temporary = dir.join("replacement");
        fs::write(&target, b"original").unwrap();
        fs::write(&temporary, b"edited").unwrap();
        let mut calls = 0;
        let mut waits = Vec::new();
        guarded_replace(
            &temporary,
            &target,
            |temp, dest| {
                calls += 1;
                if calls < 3 {
                    Err(io::Error::from_raw_os_error(1175))
                } else {
                    native_replace(temp, dest)
                }
            },
            |delay| waits.push(delay),
        )
        .unwrap();
        assert_eq!(calls, 3);
        assert_eq!(waits.len(), 2);
        assert_eq!(fs::read(&target).unwrap(), b"edited");
        fs::write(&temporary, b"next").unwrap();
        calls = 0;
        waits.clear();
        let result = guarded_replace(
            &temporary,
            &target,
            |_, _| {
                calls += 1;
                Err(io::Error::from_raw_os_error(32))
            },
            |delay| waits.push(delay),
        );
        assert!(matches!(result,Err(FileError::Io(error)) if error.raw_os_error()==Some(32)));
        assert_eq!(calls, 5);
        assert_eq!(waits.iter().sum::<Duration>(), Duration::from_millis(75));
        assert_eq!(fs::read(&target).unwrap(), b"edited");
        assert_eq!(fs::read(&temporary).unwrap(), b"next");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn external_change_or_deletion_during_retry_is_a_conflict() {
        let dir = test_dir();
        let target = dir.join("source.p");
        let temporary = dir.join("replacement");
        for removed in [false, true] {
            fs::write(&target, b"original").unwrap();
            fs::write(&temporary, b"edited").unwrap();
            let mut calls = 0;
            let result = guarded_replace(
                &temporary,
                &target,
                |_, dest| {
                    calls += 1;
                    if removed {
                        fs::remove_file(dest)?;
                    } else {
                        fs::write(dest, b"external")?;
                    }
                    Err(io::Error::from_raw_os_error(1175))
                },
                |_| {},
            );
            assert!(matches!(result, Err(FileError::ExternalModification(_))));
            assert_eq!(calls, 1);
            if !removed {
                assert_eq!(fs::read(&target).unwrap(), b"external");
            }
            assert_eq!(fs::read(&temporary).unwrap(), b"edited");
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn partial_replacement_keeps_recovery_bytes_and_reports_their_path() {
        let dir = test_dir();
        let target = dir.join("source.p");
        for code in [1176, 1177] {
            fs::write(&target, b"original").unwrap();
            let result = crate::atomic_replace_with_commit(
                &target,
                false,
                |file| {
                    use std::io::Write;
                    file.write_all(b"edited")
                },
                |temp, dest| {
                    guarded_replace(
                        temp,
                        dest,
                        |_, _| Err(io::Error::from_raw_os_error(code)),
                        |_| {},
                    )
                },
            );
            let Err(FileError::SaveRecovery {
                temporary, error, ..
            }) = result
            else {
                panic!("expected recoverable failure");
            };
            assert_eq!(error.raw_os_error(), Some(code));
            assert_eq!(fs::read(&temporary).unwrap(), b"edited");
            assert_eq!(fs::read(&target).unwrap(), b"original");
            fs::remove_file(temporary).unwrap();
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
