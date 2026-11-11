//! Bounded, cancelable reverse lookup of static file relationships on disk.
use super::*;
use std::collections::VecDeque;

pub(crate) struct IncomingScan {
    pub root: PathBuf,
    pub target: PathBuf,
    pub provider: SourceProviderDescriptor,
}

const FILES: usize = 512;
const DIRECTORIES: usize = 128;
const TOTAL_BYTES: usize = 16 * 1024 * 1024;

impl IncomingScan {
    pub(crate) fn run(
        &self,
        stamp: TaskStamp,
        cancel: &Cancellation,
        output: &mut TaskOutput,
    ) -> Result<(), TaskFailure> {
        let root = self.root.canonicalize().map_err(|_| TaskFailure::Failed)?;
        let target = self
            .target
            .canonicalize()
            .map_err(|_| TaskFailure::Failed)?;
        if !target.starts_with(&root) {
            return Err(TaskFailure::Failed);
        }
        let default_provider = self.provider.references.ok_or(TaskFailure::Failed)?;
        let ecmascript = matches!(self.provider.language, "javascript" | "typescript");
        let propath = if matches!(
            self.provider.file_resolution,
            FileResolution::OrderedProjectPath
        ) {
            load_propath(&root).map_err(|_| TaskFailure::Failed)?
        } else {
            None
        };
        let source_roots = if matches!(
            self.provider.file_resolution,
            FileResolution::ProjectImports
        ) {
            load_source_roots(&root).map_err(|_| TaskFailure::Failed)?
        } else {
            Vec::new()
        };
        let mut dirs = VecDeque::from([root.clone()]);
        let mut directories = 1;
        let mut files = 0;
        let mut remaining = TOTAL_BYTES;
        while let Some(directory) = dirs.pop_front() {
            let mut listing =
                jc_files::list_directory_in_root(&root, &directory, 2048, 512 * 1024, || {
                    cancel.is_cancelled()
                })
                .map_err(|_| {
                    if cancel.is_cancelled() {
                        TaskFailure::Cancelled
                    } else {
                        TaskFailure::Failed
                    }
                })?;
            if listing.limited {
                return Err(TaskFailure::LimitExceeded);
            }
            listing
                .entries
                .sort_unstable_by(|left, right| left.name.cmp(&right.name));
            for entry in listing.entries {
                if cancel.is_cancelled() {
                    return Err(TaskFailure::Cancelled);
                }
                let path = directory.join(entry.name);
                if entry.is_dir {
                    if matches!(
                        path.file_name().and_then(|name| name.to_str()),
                        Some("node_modules" | "target" | ".cide")
                    ) {
                        continue;
                    }
                    directories += 1;
                    if directories > DIRECTORIES {
                        return Err(TaskFailure::LimitExceeded);
                    }
                    dirs.push_back(path);
                    continue;
                }
                let language = jc_languages::detect(&path.to_string_lossy()).id;
                let provider = if language == self.provider.language {
                    default_provider
                } else if ecmascript && matches!(language, "javascript" | "typescript") {
                    crate::javascript_navigation::descriptor(language == "typescript")
                        .references
                        .ok_or(TaskFailure::Failed)?
                } else {
                    continue;
                };
                files += 1;
                if files > FILES || remaining == 0 {
                    return Err(TaskFailure::LimitExceeded);
                }
                let (path, bytes) = jc_files::read_in_root(&root, &path, SNAPSHOT.min(remaining))
                    .map_err(|_| TaskFailure::Failed)?;
                remaining -= bytes.len();
                let encoding = jc_text::probe_encoding(&bytes, None)
                    .encoding
                    .unwrap_or(stamp.encoding);
                let snapshot = TaskSnapshot::scan_source(TaskStamp { encoding, ..stamp }, bytes);
                let mut found = TaskOutput::for_source_scan(&snapshot);
                provider(&snapshot, cancel, &mut found)?;
                let mut version = None;
                for item in found.take_items() {
                    let TaskItem::Source(item) = item else {
                        continue;
                    };
                    let SourceAction::RelativeFile(relative) = item.action else {
                        continue;
                    };
                    let paths = match self.provider.file_resolution {
                        FileResolution::OrderedProjectPath => {
                            resolve_ordered(&root, &path, &relative, propath.as_deref(), false)
                        }
                        FileResolution::ProjectImports => {
                            resolve_import(&root, &relative, &source_roots)
                        }
                        resolution => {
                            resolve_from(&root, &path, &relative, resolution, SourceMode::Incoming)
                        }
                    };
                    let Ok(paths) = paths else {
                        continue;
                    };
                    if paths.len() != 1 || paths[0].canonicalize().ok().as_ref() != Some(&target) {
                        continue;
                    }
                    let offset = item.range.start;
                    let line = line_at(&snapshot.bytes()[..offset]);
                    let label = path.strip_prefix(&root).unwrap_or(&path).to_string_lossy();
                    let label = display_ui_label(&label, 96);
                    output.push(TaskItem::Source(SourceItem {
                        range: 0..0,
                        label: format!("{label}:{line}"),
                        action: SourceAction::Caller {
                            path: path.clone(),
                            offset,
                            relative,
                            encoding,
                            version: *version.get_or_insert_with(|| {
                                jc_files::ContentFingerprint::of(snapshot.bytes())
                            }),
                        },
                    }))?;
                }
            }
        }
        Ok(())
    }
}

fn line_at(bytes: &[u8]) -> usize {
    1 + bytes
        .iter()
        .enumerate()
        .filter(|&(index, byte)| {
            *byte == b'\n' || *byte == b'\r' && bytes.get(index + 1) != Some(&b'\n')
        })
        .count()
}
