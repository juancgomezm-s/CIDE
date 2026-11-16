//! Native full checkpoints for edits that run trusted database triggers.
use super::{
    avm_path, bounded_read, fs, io, io_error, load_profile, new_backup_id, publish_verified,
    read_in_root, run_mode_with_triggers, wait, AvmMode, Cancellation, Command, LocalDatabase,
    Path, PathBuf, RecordEvent, Selection, Stdio,
};

const PROFILE: &str = ".cide/disparadores-registros.local.ini";
const LOG_LIMIT: u64 = 256 * 1024;

pub(super) fn propath(root: &Path) -> Result<Option<PathBuf>, String> {
    let profile = root.join(PROFILE);
    match fs::symlink_metadata(&profile) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    }
    let (_, bytes) = read_in_root(root, &profile, 4096).map_err(io_error)?;
    let value = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| text.trim().strip_prefix("propath="))
        .filter(|value| {
            !value.is_empty() && !value.contains([',', ';']) && !value.chars().any(char::is_control)
        })
        .ok_or("El perfil de disparadores requiere propath=ruta con un solo directorio")?;
    let path = root.join(value).canonicalize().map_err(io_error)?;
    if !path.starts_with(root) || !path.is_dir() {
        return Err("El PROPATH de disparadores debe estar dentro del proyecto".into());
    }
    Ok(Some(path))
}

pub(super) fn check_propath(root: &Path, expected: Option<&Path>) -> Result<(), String> {
    if propath(root)?.as_deref() != expected {
        return Err("Cambió el perfil de disparadores desde la vista previa".into());
    }
    Ok(())
}

pub(super) fn require_row_restore(root: &Path) -> Result<(), String> {
    if propath(root)?.is_some() {
        return Err(
            "Para revertir efectos de disparadores usa la base anterior de la operación".into(),
        );
    }
    Ok(())
}

pub(super) fn apply(
    root: &Path,
    selection: &Selection,
    database: &Path,
    propath: &Path,
    mode: AvmMode<'_>,
    cancel: &Cancellation,
) -> Result<RecordEvent, String> {
    let config = load_profile(root)?;
    let bin = config
        .avm
        .executable
        .parent()
        .ok_or("Ruta del AVM inválida")?;
    let utility = bin.join(if cfg!(windows) {
        "_dbutil.exe"
    } else {
        "_dbutil"
    });
    if !utility.is_file() {
        return Err(
            "La instalación configurada de OpenEdge requiere la utilidad de respaldo".into(),
        );
    }
    if database
        .with_extension("lk")
        .try_exists()
        .map_err(io_error)?
    {
        return Err("Cierra la base antes de preparar el respaldo completo".into());
    }
    let parent = checkpoint_root(root)?;
    let id = new_backup_id()?;
    let short_id = id
        .split('-')
        .map(|component| component.trim_start_matches('0'))
        .collect::<Vec<_>>()
        .join("-");
    let directory = parent.join(short_id.trim_end_matches('-'));
    fs::create_dir(&directory).map_err(io_error)?;
    let work = avm_path(&directory);
    let backup = work.join("anterior.pbk");
    let original = avm_path(database);
    let anterior = work.join("anterior.db");
    let resultado = work.join("resultado.db");
    let run = |operation, database: &Path, verify| {
        run_utility(
            &utility, &work, operation, database, &backup, verify, cancel,
        )
    };
    run("probkup", &original, false)?;
    let image = fs::OpenOptions::new()
        .write(true)
        .open(&backup)
        .map_err(io_error)?;
    image.sync_all().map_err(io_error)?;
    drop(image);
    run("prorest", &anterior, true)?;
    run("prorest", &anterior, false)?;
    run("prorest", &resultado, false)?;
    if !anterior.is_file() || !resultado.is_file() {
        return Err("OpenEdge no creó las dos bases del punto de restauración".into());
    }
    let target = LocalDatabase {
        path: directory.join("resultado.db"),
        active: false,
    };
    run_mode_with_triggers(root, selection, &target, mode, Some(propath), cancel).map_err(
        |error| {
            format!(
                "{error}. Respaldo completo disponible en {}",
                directory.display()
            )
        },
    )?;
    for entry in fs::read_dir(&directory).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        if entry.file_type().map_err(io_error)?.is_file() {
            fs::OpenOptions::new()
                .write(true)
                .open(entry.path())
                .and_then(|file| file.sync_all())
                .map_err(io_error)?;
        }
    }
    publish_verified(
        root,
        &directory.join("completado.txt"),
        b"Anterior: anterior.db\nResultado: resultado.db\nRespaldo: anterior.pbk\n",
    )?;
    Ok(RecordEvent::Replica(directory))
}

fn checkpoint_root(root: &Path) -> Result<PathBuf, String> {
    let mut parent = root.to_owned();
    for component in [".cide", "respaldos-registros", "bases"] {
        parent.push(component);
        match fs::create_dir(&parent) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_error(error)),
        }
        let metadata = fs::symlink_metadata(&parent).map_err(io_error)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("El respaldo de base requiere directorios locales ordinarios".into());
            }
        }
        if !metadata.is_dir() || parent.canonicalize().map_err(io_error)? != parent {
            return Err("La ruta del respaldo de base cambió o salió del proyecto".into());
        }
    }
    Ok(parent)
}

fn run_utility(
    utility: &Path,
    work: &Path,
    operation: &str,
    database: &Path,
    backup: &Path,
    verify: bool,
    cancel: &Cancellation,
) -> Result<(), String> {
    if cancel.is_cancelled() {
        return Err("Operación ABL cancelada".into());
    }
    let log_path = work.join("respaldo.log");
    let log = fs::File::create(&log_path).map_err(io_error)?;
    let mut command = Command::new(utility);
    command.arg(operation).arg(database).arg(backup);
    if verify {
        command.arg("-vp");
    }
    let dlc = utility
        .parent()
        .and_then(Path::parent)
        .ok_or("Ruta de OpenEdge inválida")?;
    command
        .current_dir(work)
        .env("DLC", dlc)
        .env_remove("PROSTARTUP")
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(io_error)?)
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(io_error)?;
    let status = match wait(&mut child, &[(&log_path, LOG_LIMIT)], cancel) {
        Ok(status) => status,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let bytes = bounded_read(&log_path, LOG_LIMIT)?;
    let log = String::from_utf8_lossy(&bytes);
    if !status.success()
        || (verify && !log.contains("(6758)"))
        || (operation == "probkup" && !log.contains("(3740)"))
    {
        let message = log.lines().take(8).collect::<Vec<_>>().join(" · ");
        return Err(format!(
            "Falló el respaldo o la restauración de OpenEdge ({status}): {message}"
        ));
    }
    Ok(())
}
