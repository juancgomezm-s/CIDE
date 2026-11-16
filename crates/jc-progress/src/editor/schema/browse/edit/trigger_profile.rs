use super::{fs, io, io_error, read_in_root, Path, PathBuf};

const PROFILE: &str = ".cide/disparadores-registros.local.ini";

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
