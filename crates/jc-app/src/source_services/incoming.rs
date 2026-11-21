//! Bounded, cancelable reverse lookup of static file relationships on disk.
use super::*;
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

pub(crate) struct IncomingScan {
    pub root: PathBuf,
    pub target: PathBuf,
    pub provider: SourceProviderDescriptor,
}

const FILES: usize = 4096;
const DIRECTORIES: usize = 2048;
const TOTAL_BYTES: usize = 128 * 1024 * 1024;

impl IncomingScan {
    pub(crate) fn run(
        &self,
        stamp: TaskStamp,
        cancel: &Cancellation,
        output: &mut TaskOutput,
    ) -> Result<(), TaskFailure> {
        let resolution = self.provider.file_resolution;
        let root = navigation_root(&self.root, &self.target, resolution)
            .map_err(|_| TaskFailure::Failed)?;
        let target = self
            .target
            .canonicalize()
            .map_err(|_| TaskFailure::Failed)?;
        if !target.starts_with(&root) {
            return Err(TaskFailure::Failed);
        }
        let default_provider = self
            .provider
            .provider(SourceMode::Incoming)
            .ok_or(TaskFailure::Failed)?;
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
        let mut enrolled = HashSet::from([root.clone()]);
        if let Some(parent) = target.parent().filter(|parent| *parent != root) {
            dirs.push_front(parent.to_owned());
            enrolled.insert(parent.to_owned());
        }
        let mut files = 0;
        let mut remaining = TOTAL_BYTES;
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut partial = false;
        'scan: while let Some(directory) = dirs.pop_front() {
            if Instant::now() >= deadline {
                partial = true;
                break;
            }
            let mut listing =
                match jc_files::list_directory_in_root(&root, &directory, 2048, 512 * 1024, || {
                    cancel.is_cancelled()
                }) {
                    Ok(listing) => listing,
                    Err(_) => {
                        if cancel.is_cancelled() {
                            return Err(TaskFailure::Cancelled);
                        }
                        partial = true;
                        continue;
                    }
                };
            partial |= listing.limited;
            listing
                .entries
                .sort_unstable_by(|left, right| left.name.cmp(&right.name));
            for entry in listing.entries {
                if cancel.is_cancelled() {
                    return Err(TaskFailure::Cancelled);
                }
                if Instant::now() >= deadline {
                    partial = true;
                    break 'scan;
                }
                let path = directory.join(entry.name);
                if entry.is_dir {
                    if matches!(
                        path.file_name().and_then(|name| name.to_str()),
                        Some("node_modules" | "target" | ".cide" | ".git")
                    ) {
                        continue;
                    }
                    if enrolled.len() >= DIRECTORIES {
                        partial = true;
                    } else if enrolled.insert(path.clone()) {
                        dirs.push_back(path);
                    }
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
                    partial = true;
                    break 'scan;
                }
                let Ok((path, bytes)) =
                    jc_files::read_in_root(&root, &path, SNAPSHOT.min(remaining))
                else {
                    partial = true;
                    continue;
                };
                remaining -= bytes.len();
                let encoding = jc_text::probe_encoding(&bytes, None)
                    .encoding
                    .unwrap_or(stamp.encoding);
                let snapshot = TaskSnapshot::scan_source(TaskStamp { encoding, ..stamp }, bytes);
                let mut found = TaskOutput::for_caller_scan(&snapshot);
                if let Err(failure) = provider(&snapshot, cancel, &mut found) {
                    if failure == TaskFailure::Cancelled {
                        return Err(failure);
                    }
                    partial = true;
                    continue;
                }
                let mut version = None;
                let mut callers = HashSet::new();
                for item in found.take_items() {
                    let TaskItem::Source(item) = item else {
                        continue;
                    };
                    let (relative, routine) = match item.action {
                        SourceAction::RelativeFile(relative) => (relative, None),
                        SourceAction::IncomingReference { relative, routine } => {
                            (relative, routine)
                        }
                        _ => continue,
                    };
                    let caller = routine.as_ref().map(|routine| routine.declaration);
                    if callers.contains(&caller) {
                        continue;
                    }
                    if matches!(resolution, FileResolution::OrderedProjectPath)
                        && !Path::new(&relative)
                            .file_name()
                            .zip(target.file_name())
                            .is_some_and(|(name, target)| {
                                name.to_string_lossy()
                                    .eq_ignore_ascii_case(&target.to_string_lossy())
                            })
                    {
                        continue;
                    }
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
                    if output.remaining_items() <= 1 {
                        partial = true;
                        break 'scan;
                    }
                    callers.insert(caller);
                    let offset = item.range.start;
                    let line = line_at(&snapshot.bytes()[..offset]);
                    let label = path.strip_prefix(&root).unwrap_or(&path).to_string_lossy();
                    let label = display_ui_label(&label, 72);
                    let label = if let Some(routine) = routine {
                        let routine = display_ui_label(&routine.name, 30);
                        format!("{routine} · {label}:{line}")
                    } else {
                        format!("{label}:{line}")
                    };
                    output.push(TaskItem::Source(SourceItem {
                        value_type: None,
                        range: 0..0,
                        label,
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
        output.push(TaskItem::Message(
            if partial {
                "Búsqueda parcial del proyecto"
            } else {
                "Referencias en el proyecto"
            }
            .into(),
        ))
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
