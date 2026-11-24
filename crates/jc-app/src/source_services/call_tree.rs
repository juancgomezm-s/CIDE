//! Three outgoing levels, with pinned destinations and branch-local cycle checks.
use super::*;
use std::collections::{HashMap, VecDeque};

struct Source {
    path: PathBuf,
    snapshot: TaskSnapshot,
    version: Option<jc_files::ContentFingerprint>,
}
struct Pending {
    item: SourceItem,
    parent: Option<usize>,
    source: Arc<Source>,
    level: usize,
    ancestors: Vec<(PathBuf, usize)>,
}
fn job(
    provider: SourceProviderDescriptor,
    root: Option<PathBuf>,
    source: &Path,
    cursor: usize,
) -> Option<SourceJob> {
    let fallback = provider.calls.map(|provider_fn| SourceJob::Calls {
        provider: provider_fn,
        resolution: provider.file_resolution,
        root,
        source: source.to_owned(),
    });
    provider
        .routines
        .map(|routine| SourceJob::Routine {
            provider: routine,
            profile: jc_languages::detect(&source.to_string_lossy()).syntax_profile(),
            mode: SourceMode::Calls,
            cursor,
            fallback: fallback.map(Box::new),
        })
        .or_else(|| provider.calls.map(|p| SourceJob::Plain(p)))
}
fn items(
    work: &SourceJob,
    source: &Source,
    cancel: &Cancellation,
) -> Result<(Vec<SourceItem>, Option<String>), TaskFailure> {
    let mut output = TaskOutput::for_source_scan(&source.snapshot);
    work.run(&source.snapshot, cancel, &mut output)?;
    let notice = output.items().iter().find_map(|i| {
        if let TaskItem::Message(message) = i {
            Some(message.clone())
        } else {
            None
        }
    });
    Ok((
        output
            .take_items()
            .into_iter()
            .filter_map(|i| {
                if let TaskItem::Source(i) = i {
                    Some(i)
                } else {
                    None
                }
            })
            .collect(),
        notice,
    ))
}
pub(super) fn run(
    work: &SourceJob,
    provider: SourceProviderDescriptor,
    root: Option<&Path>,
    origin: (&Path, usize),
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    let (source, cursor) = origin;
    let canonical_root = root
        .map(|root| navigation_root(root, source, provider.file_resolution))
        .transpose()
        .map_err(|_| TaskFailure::Failed)?;
    let root = canonical_root.as_deref();
    let local_cancel = cancel.without_source_progress();
    let original = Arc::new(Source {
        path: source.to_owned(),
        snapshot: TaskSnapshot::shared_source(snapshot.stamp, snapshot.shared_bytes()),
        version: None,
    });
    let (first, context) = items(work, &original, &local_cancel)?;
    let mut pending: VecDeque<_> = first
        .into_iter()
        .map(|item| Pending {
            item,
            parent: None,
            source: original.clone(),
            level: 1,
            ancestors: vec![(source.to_owned(), cursor)],
        })
        .collect();
    let mut cache = HashMap::from([(source.to_owned(), original)]);
    let mut done = 0;
    let mut unresolved = 0;
    let mut failures = 0;
    let mut cache_bytes = snapshot.bytes().len();
    while let Some(mut next) = pending.pop_front() {
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        let source = next.source.clone();
        let target = match &next.item.action {
            SourceAction::Navigate | SourceAction::ScopedSymbol { .. } => {
                Some((source.clone(), next.item.range.start))
            }
            SourceAction::RelativeFile(relative) => {
                let paths = root
                    .and_then(|root| {
                        navigation_root(root, &source.path, provider.file_resolution).ok()
                    })
                    .and_then(|root| {
                        resolve_from(
                            &root,
                            &source.path,
                            relative,
                            provider.file_resolution,
                            SourceMode::Calls,
                        )
                        .ok()
                    });
                match paths.as_deref() {
                    Some([path]) => {
                        if !cache.contains_key(path) {
                            if let Some(root) = root {
                                match jc_files::read_in_root(root, path, 64 * 1024 * 1024) {
                                    Ok((path, bytes)) => {
                                        cache_bytes = cache_bytes.saturating_add(bytes.len());
                                        if cache_bytes > 128 * 1024 * 1024 {
                                            return Err(TaskFailure::LimitExceeded);
                                        }
                                        let encoding = jc_text::probe_encoding(&bytes, None)
                                            .encoding
                                            .unwrap_or(snapshot.stamp.encoding);
                                        let version = jc_files::ContentFingerprint::of(&bytes);
                                        cache.insert(
                                            path.clone(),
                                            Arc::new(Source {
                                                path,
                                                snapshot: TaskSnapshot::scan_source(
                                                    TaskStamp {
                                                        encoding,
                                                        ..snapshot.stamp
                                                    },
                                                    bytes,
                                                ),
                                                version: Some(version),
                                            }),
                                        );
                                    }
                                    Err(_) => {
                                        failures += 1;
                                    }
                                }
                            }
                        }
                        cache.get(path).cloned().map(|s| (s, 0))
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        let id = done;
        if let Some((target, offset)) = &target {
            if let Some(version) = target.version {
                next.item.range = 0..0;
                next.item.action = SourceAction::ProjectLocation {
                    path: target.path.clone(),
                    offset: *offset,
                    encoding: target.snapshot.stamp.encoding,
                    version,
                };
            }
        } else {
            unresolved += 1;
            if source.version.is_some() {
                // A nested unresolved call still opens its own call site.
                next.item.label =
                    display_ui_label(&format!("{} · sin resolver", next.item.label), 120);
                next.item.action = SourceAction::ProjectLocation {
                    path: source.path.clone(),
                    offset: next.item.range.start,
                    encoding: source.snapshot.stamp.encoding,
                    version: source.version.unwrap(),
                };
                next.item.range = 0..0;
            }
        }
        let cycle = target
            .as_ref()
            .is_some_and(|(s, offset)| next.ancestors.contains(&(s.path.clone(), *offset)));
        if cycle {
            next.item.label = display_ui_label(&format!("{} · recursiva", next.item.label), 120);
        }
        output.push_source_child(next.item, next.parent)?;
        done += 1;
        if next.level < 3 && !cycle {
            if let Some((source, cursor)) = target {
                if let Some(work) = job(provider, root.map(Path::to_owned), &source.path, cursor) {
                    let mut ancestors = next.ancestors;
                    ancestors.push((source.path.clone(), cursor));
                    match items(&work, &source, &local_cancel) {
                        Ok((children, _)) => {
                            pending.extend(children.into_iter().map(|item| Pending {
                                item,
                                parent: Some(id),
                                source: source.clone(),
                                level: next.level + 1,
                                ancestors: ancestors.clone(),
                            }))
                        }
                        Err(TaskFailure::Cancelled) => return Err(TaskFailure::Cancelled),
                        Err(_) => {
                            failures += 1;
                        }
                    }
                }
            }
        }
        cancel.report_source_files(done, done + pending.len());
    }
    if failures != 0 {
        output.mark_source_incomplete();
    }
    output.push(TaskItem::Message(if failures == 0 {
        format!("{} · 3 niveles analizados · {unresolved} destinos sin resolver", context.as_deref().unwrap_or("Llama a"))
    } else {
        format!("Análisis incompleto: {failures} archivos/rutinas no analizados · {unresolved} destinos sin resolver")
    }))
}
