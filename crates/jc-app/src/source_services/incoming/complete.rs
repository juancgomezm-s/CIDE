//! Complete one-level reverse scan. Two workers, ordered bounded handoff.
use super::*;
use std::sync::mpsc;

struct Scan<'a> {
    owner: &'a IncomingScan,
    root: PathBuf,
    target: PathBuf,
    propath: Option<Vec<PathBuf>>,
    source_roots: Vec<PathBuf>,
    stamp: TaskStamp,
}
impl Scan<'_> {
    fn file(&self, path: &Path, cancel: &Cancellation) -> Result<Vec<SourceItem>, TaskFailure> {
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        let language = jc_languages::detect(&path.to_string_lossy()).id;
        let provider = if language == self.owner.provider.language {
            self.owner.provider.provider(SourceMode::Incoming)
        } else if matches!(language, "javascript" | "typescript")
            && matches!(self.owner.provider.language, "javascript" | "typescript")
        {
            crate::javascript_navigation::descriptor(language == "typescript").references
        } else {
            None
        };
        let Some(provider) = provider else {
            return Ok(Vec::new());
        };
        let (path, bytes) = jc_files::read_in_root(&self.root, path, 64 * 1024 * 1024)
            .map_err(|_| TaskFailure::Failed)?;
        let encoding = jc_text::probe_encoding(&bytes, None)
            .encoding
            .unwrap_or(self.stamp.encoding);
        let version = jc_files::ContentFingerprint::of(&bytes);
        let snapshot = TaskSnapshot::scan_source(
            TaskStamp {
                encoding,
                ..self.stamp
            },
            bytes,
        );
        let mut found = TaskOutput::for_source_scan(&snapshot);
        provider(&snapshot, cancel, &mut found)?;
        let mut callers = HashSet::new();
        let mut result = Vec::new();
        for item in found.take_items() {
            if cancel.is_cancelled() {
                return Err(TaskFailure::Cancelled);
            }
            let TaskItem::Source(item) = item else {
                continue;
            };
            let (relative, routine) = match item.action {
                SourceAction::RelativeFile(relative) => (relative, None),
                SourceAction::IncomingReference { relative, routine } => (relative, routine),
                _ => continue,
            };
            let key = routine.as_ref().map(|r| r.declaration);
            if callers.contains(&key) {
                continue;
            }
            let paths = match self.owner.provider.file_resolution {
                FileResolution::OrderedProjectPath => {
                    resolve_ordered(&self.root, &path, &relative, self.propath.as_deref(), false)
                }
                FileResolution::ProjectImports => {
                    resolve_import(&self.root, &relative, &self.source_roots)
                }
                resolution => resolve_from(
                    &self.root,
                    &path,
                    &relative,
                    resolution,
                    SourceMode::Incoming,
                ),
            };
            if !paths.is_ok_and(|p| {
                p.len() == 1 && p[0].canonicalize().ok().as_ref() == Some(&self.target)
            }) {
                continue;
            }
            callers.insert(key);
            let offset = item.range.start;
            let line = line_at(&snapshot.bytes()[..offset]);
            let label = display_ui_label(
                &path
                    .strip_prefix(&self.root)
                    .unwrap_or(&path)
                    .to_string_lossy(),
                100,
            );
            let label = routine.map_or_else(
                || format!("{label}:{line}"),
                |r| format!("{} · {label}:{line}", display_ui_label(&r.name, 40)),
            );
            result.push(SourceItem {
                value_type: None,
                range: 0..0,
                label,
                action: SourceAction::Caller {
                    path: path.clone(),
                    offset,
                    relative,
                    encoding,
                    version,
                },
            });
        }
        Ok(result)
    }
}

pub(super) fn run(
    owner: &IncomingScan,
    stamp: TaskStamp,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    let root = navigation_root(&owner.root, &owner.target, owner.provider.file_resolution)
        .map_err(|_| TaskFailure::Failed)?;
    let target = owner
        .target
        .canonicalize()
        .map_err(|_| TaskFailure::Failed)?;
    if !target.starts_with(&root) {
        return Err(TaskFailure::Failed);
    }
    let scan = Scan {
        owner,
        propath: if matches!(
            owner.provider.file_resolution,
            FileResolution::OrderedProjectPath
        ) {
            load_propath(&root).map_err(|_| TaskFailure::Failed)?
        } else {
            None
        },
        source_roots: if matches!(
            owner.provider.file_resolution,
            FileResolution::ProjectImports
        ) {
            load_source_roots(&root).map_err(|_| TaskFailure::Failed)?
        } else {
            Vec::new()
        },
        root: root.clone(),
        target,
        stamp,
    };
    let mut dirs = VecDeque::from([root.clone()]);
    let mut files = Vec::new();
    let mut issues = 0;
    while let Some(dir) = dirs.pop_front() {
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        let listing = match jc_files::list_directory_complete_in_root(&root, &dir, || {
            cancel.is_cancelled()
        }) {
            Ok(listing) => listing,
            Err(_) => {
                issues += 1;
                continue;
            }
        };
        if listing.limited {
            return Err(TaskFailure::LimitExceeded);
        }
        for entry in listing.entries {
            let path = dir.join(entry.name);
            if entry.is_dir {
                if !matches!(
                    path.file_name().and_then(|p| p.to_str()),
                    Some(".git" | ".cide" | "target" | "node_modules")
                ) {
                    dirs.push_back(path);
                }
            } else {
                let language = jc_languages::detect(&path.to_string_lossy()).id;
                if language == owner.provider.language
                    || matches!(language, "javascript" | "typescript")
                        && matches!(owner.provider.language, "javascript" | "typescript")
                {
                    files.push(path);
                }
            }
        }
        cancel.report_source_files(0, files.len());
    }
    files.sort();
    let workers = std::thread::available_parallelism().map_or(1, |v| v.get().min(2));
    let local_cancel = cancel.without_source_progress();
    std::thread::scope(|scope| -> Result<(), TaskFailure> {
        let mut receivers = Vec::new();
        for worker in 0..workers {
            let (send, receive) = mpsc::sync_channel(1);
            receivers.push(receive);
            let scan = &scan;
            let files = &files;
            let cancel = &local_cancel;
            scope.spawn(move || {
                for index in (worker..files.len()).step_by(workers) {
                    let result = scan.file(&files[index], cancel);
                    if send.send(result).is_err() {
                        break;
                    }
                    if cancel.is_cancelled() {
                        break;
                    }
                }
            });
        }
        for index in 0..files.len() {
            if cancel.is_cancelled() {
                return Err(TaskFailure::Cancelled);
            }
            match receivers[index % workers]
                .recv()
                .map_err(|_| TaskFailure::Failed)?
            {
                Ok(items) => {
                    for item in items {
                        output.push(TaskItem::Source(item))?;
                    }
                }
                Err(TaskFailure::Cancelled) => return Err(TaskFailure::Cancelled),
                Err(_) => {
                    issues += 1;
                }
            }
            cancel.report_source_files(index + 1, files.len());
        }
        Ok(())
    })?;
    if issues != 0 {
        output.mark_source_incomplete();
    }
    output.push(TaskItem::Message(if issues == 0 {
        format!("Nivel 1 completo · {} archivos analizados", files.len())
    } else {
        format!("Análisis incompleto: {issues} archivos/directorios no analizados")
    }))
}
