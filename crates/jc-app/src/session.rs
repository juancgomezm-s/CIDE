//! Frontend-owned recovery lease; no background service or original-file writes.
use jc_files::recovery::{RecoveryLimits, RecoveryStore};
pub use jc_files::{read_recovery_path, write_recovery_path};
use std::path::PathBuf;

pub struct EditorRecovery {
    pub store: RecoveryStore,
    pub base: PathBuf,
}
impl EditorRecovery {
    pub fn open_in(root: &std::path::Path) -> Result<Self, String> {
        let base = std::env::current_dir().map_err(|e| e.to_string())?;
        let limits = RecoveryLimits {
            max_total_bytes: 8 * 1024 * 1024 * 1024,
            max_document_bytes: 512 * 1024 * 1024,
            ..RecoveryLimits::default()
        };
        let store = RecoveryStore::open(root, limits).map_err(|e| e.to_string())?;
        Ok(Self { store, base })
    }
    /// CIDE_NO_SESSION=1 confines automation to fixture data. CIDE_STATE_DIR
    /// redirects the entire depot, including the frontend-specific subdirectory.
    pub fn open(frontend: &str) -> Result<Option<Self>, String> {
        if !matches!(frontend, "gui" | "tui") {
            return Err("Tipo de sesión inválido".into());
        }
        if !cfg!(windows) {
            return Ok(None);
        }
        if std::env::var_os("CIDE_NO_SESSION").is_some_and(|v| v == "1") {
            return Ok(None);
        }
        let root = if let Some(path) = std::env::var_os("CIDE_STATE_DIR") {
            PathBuf::from(path)
        } else {
            PathBuf::from(
                std::env::var_os("LOCALAPPDATA")
                    .ok_or("No se encontró la carpeta local para recuperar la sesión")?,
            )
            .join("CIDE")
            .join("recuperacion")
        };
        let root = std::path::absolute(root)
            .map_err(|e| e.to_string())?
            .join(frontend);
        Self::open_in(&root).map(Some)
    }
}
