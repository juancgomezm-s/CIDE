use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExplorerEntry {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) is_dir: bool,
}

#[derive(Debug, Default)]
pub(crate) struct ExplorerState {
    root: Option<PathBuf>,
    directory: Option<PathBuf>,
    entries: Vec<ExplorerEntry>,
    selected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExplorerAction {
    None,
    OpenFile(PathBuf),
    DirectoryChanged,
}

impl ExplorerState {
    pub(crate) fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub(crate) fn directory(&self) -> Option<&Path> {
        self.directory.as_deref()
    }

    pub(crate) fn entries(&self) -> &[ExplorerEntry] {
        &self.entries
    }

    pub(crate) fn selected(&self) -> usize {
        self.selected.min(self.entries.len().saturating_sub(1))
    }

    pub(crate) fn set_root(&mut self, root: PathBuf) -> Result<(), String> {
        let canonical = root
            .canonicalize()
            .map_err(|error| format!("Cannot open folder {}: {error}", root.display()))?;
        if !canonical.is_dir() {
            return Err(format!("Not a folder: {}", canonical.display()));
        }
        self.root = Some(canonical.clone());
        self.directory = Some(canonical);
        self.selected = 0;
        self.refresh()
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            self.selected = 0;
            return;
        }
        if delta < 0 {
            self.selected = self.selected.saturating_sub(delta.unsigned_abs());
        } else {
            self.selected = (self.selected + delta as usize).min(self.entries.len() - 1);
        }
    }

    pub(crate) fn select_first(&mut self) {
        self.selected = 0;
    }

    pub(crate) fn select_last(&mut self) {
        self.selected = self.entries.len().saturating_sub(1);
    }

    pub(crate) fn activate(&mut self) -> Result<ExplorerAction, String> {
        let Some(entry) = self.entries.get(self.selected()).cloned() else {
            return Ok(ExplorerAction::None);
        };
        let path = self.checked_path(&entry.path)?;
        if entry.is_dir {
            self.directory = Some(path);
            self.selected = 0;
            self.refresh()?;
            Ok(ExplorerAction::DirectoryChanged)
        } else {
            Ok(ExplorerAction::OpenFile(path))
        }
    }

    pub(crate) fn enter_directory(&mut self) -> Result<ExplorerAction, String> {
        let Some(entry) = self.entries.get(self.selected()).cloned() else {
            return Ok(ExplorerAction::None);
        };
        if !entry.is_dir {
            return Ok(ExplorerAction::None);
        }
        self.activate()
    }

    pub(crate) fn parent(&mut self) -> Result<bool, String> {
        let (Some(root), Some(directory)) = (&self.root, &self.directory) else {
            return Ok(false);
        };
        if directory == root {
            return Ok(false);
        }
        let Some(parent) = directory.parent() else {
            return Ok(false);
        };
        let parent = parent.to_path_buf();
        let parent = self.checked_path(&parent)?;
        self.directory = Some(parent);
        self.selected = 0;
        self.refresh()?;
        Ok(true)
    }

    pub(crate) fn refresh(&mut self) -> Result<(), String> {
        let Some(directory) = self.directory.clone() else {
            self.entries.clear();
            self.selected = 0;
            return Ok(());
        };
        let mut entries = Vec::new();
        let read_dir = fs::read_dir(&directory)
            .map_err(|error| format!("Cannot read {}: {error}", directory.display()))?;
        for entry in read_dir {
            let entry = entry.map_err(|error| format!("Cannot read directory entry: {error}"))?;
            let file_type = entry
                .file_type()
                .map_err(|error| format!("Cannot inspect {}: {error}", entry.path().display()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            entries.push(ExplorerEntry {
                path: entry.path(),
                name,
                is_dir: file_type.is_dir(),
            });
        }
        entries.sort_by(|left, right| {
            right
                .is_dir
                .cmp(&left.is_dir)
                .then_with(|| left.name.to_ascii_lowercase().cmp(&right.name.to_ascii_lowercase()))
                .then_with(|| left.name.cmp(&right.name))
        });
        self.entries = entries;
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        Ok(())
    }

    fn checked_path(&self, path: &Path) -> Result<PathBuf, String> {
        let Some(root) = &self.root else {
            return Err("Explorer has no project root".to_owned());
        };
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("Cannot resolve {}: {error}", path.display()))?;
        if !canonical.starts_with(root) {
            return Err("Explorer navigation cannot leave the project root".to_owned());
        }
        Ok(canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn workspace() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "jc-ide-explorer-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("folder")).unwrap();
        fs::write(path.join("z.p"), b"").unwrap();
        fs::write(path.join("a.p"), b"").unwrap();
        fs::write(path.join("folder").join("nested.p"), b"").unwrap();
        path
    }

    #[test]
    fn directories_sort_before_files_and_navigation_stays_rooted() {
        let root = workspace();
        let mut explorer = ExplorerState::default();
        explorer.set_root(root.clone()).unwrap();
        assert!(explorer.entries()[0].is_dir);
        assert_eq!(explorer.entries()[1].name, "a.p");
        assert_eq!(explorer.entries()[2].name, "z.p");
        assert_eq!(explorer.enter_directory().unwrap(), ExplorerAction::DirectoryChanged);
        assert_eq!(explorer.entries()[0].name, "nested.p");
        assert!(explorer.parent().unwrap());
        assert!(!explorer.parent().unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}
