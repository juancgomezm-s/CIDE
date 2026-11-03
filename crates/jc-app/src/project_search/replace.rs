use super::*;
use crate::TextReplacement;
use jc_files::FileSession;
use std::ops::Range;

struct FilePlan {
    path: PathBuf,
    origin: SearchOrigin,
    encoding: TextEncoding,
    ranges: Vec<Range<usize>>,
}
/// Immutable preview text + private versioned plans. Consumed once on acceptance.
pub struct ProjectReplacePreview {
    generation: u64,
    root: PathBuf,
    replacement: String,
    files: VecDeque<FilePlan>,
    lines: Vec<String>,
    matches: usize,
}
impl ProjectReplacePreview {
    pub fn lines(&self) -> &[String] {
        &self.lines
    }
    pub fn files_remaining(&self) -> usize {
        self.files.len()
    }
    pub fn matches(&self) -> usize {
        self.matches
    }
}
#[derive(Debug)]
pub struct ReplaceFileOutcome {
    pub path: PathBuf,
    pub matches: usize,
    pub saved_to_disk: bool,
    pub error: Option<String>,
}

impl ProjectSearch {
    pub fn can_replace(&self) -> bool {
        self.state == ProjectSearchState::Complete
            && !self.stale
            && !self.rows.is_empty()
            && self
                .rows
                .iter()
                .all(|r| matches!(r, ProjectSearchRow::Match(_)))
    }
    pub fn preview_replace(
        &self,
        app: &Workbench,
        replacement: String,
    ) -> Result<ProjectReplacePreview, String> {
        if !self.can_replace() {
            return Err(
                "Completa una búsqueda sin avisos ni fuentes obsoletos antes de reemplazar".into(),
            );
        }
        if replacement.len() > 4096 {
            return Err("El reemplazo excede4096 bytes".into());
        }
        let root = self.root.clone().ok_or("El proyecto cambió")?;
        let mut grouped: BTreeMap<PathBuf, Vec<&ProjectSearchMatch>> = BTreeMap::new();
        for row in &self.rows {
            if let ProjectSearchRow::Match(m) = row {
                grouped.entry(m.path.clone()).or_default().push(m);
            }
        }
        let mut files = VecDeque::new();
        let mut lines = Vec::new();
        let mut total = 0usize;
        let mut input = 0usize;
        let mut charge = 0;
        for (path, matches) in grouped {
            let first = matches[0];
            if let SearchOrigin::Open(stamp) = first.origin {
                if !app.task_stamp_is_current(stamp) {
                    return Err("El documento abierto cambió desde la búsqueda".into());
                }
            }
            let encoded =
                jc_text::encode(&replacement, first.encoding).map_err(|e| e.to_string())?;
            let payload = replacement
                .len()
                .checked_mul(matches.len())
                .ok_or("Desbordamiento de cuota del reemplazo")?;
            if payload > 2 * 1024 * 1024 {
                return Err("Las propuestas de reemplazo exceden2MiB por archivo".into());
            }
            let removed: usize = matches.iter().map(|m| m.range.len()).sum();
            let new_len = first
                .source_len
                .checked_sub(removed)
                .and_then(|n| {
                    encoded
                        .len()
                        .checked_mul(matches.len())
                        .and_then(|add| n.checked_add(add))
                })
                .ok_or("Tamaño de reemplazo inválido")?;
            total += new_len;
            input += first.source_len;
            if new_len > 2 * 1024 * 1024 || total > 8 * 1024 * 1024 || input > 8 * 1024 * 1024 {
                return Err(
                    "Se excedió la cuota de salida del reemplazo (2MiB/archivo,8MiB/consulta)"
                        .into(),
                );
            }
            lines.push(format!(
                "{}  ({} coincidencias)",
                path.strip_prefix(&root)
                    .unwrap_or(&path)
                    .as_os_str()
                    .as_encoded_bytes()
                    .escape_ascii(),
                matches.len()
            ));
            let preview_new: String = replacement
                .chars()
                .take(80)
                .flat_map(char::escape_debug)
                .collect();
            let preview_old: String = self
                .query
                .chars()
                .take(80)
                .flat_map(char::escape_debug)
                .collect();
            for m in &matches {
                lines.push(format!(
                    "  {}:{} bytes {}..{} | {}",
                    m.position.line + 1,
                    m.position.column + 1,
                    m.range.start,
                    m.range.end,
                    m.snippet
                ));
                lines.push(format!("- {preview_old}"));
                lines.push(format!("+ {preview_new}"));
            }
            charge += lines
                .iter()
                .rev()
                .take(matches.len() * 3 + 1)
                .map(String::capacity)
                .sum::<usize>();
            if charge > 2 * 1024 * 1024 {
                return Err("La vista previa excede2MiB; limita la búsqueda".into());
            }
            files.push_back(FilePlan {
                path,
                origin: first.origin,
                encoding: first.encoding,
                ranges: matches.iter().map(|m| m.range.clone()).collect(),
            });
        }
        Ok(ProjectReplacePreview {
            generation: self.generation,
            root,
            replacement,
            files,
            lines,
            matches: self.rows.len(),
        })
    }
    /// Apply at most one file on the application/UI owner. Callers process input
    /// between files; cancellation drops the remaining plans. No worker writes.
    pub fn apply_next_replace(
        &self,
        preview: &mut ProjectReplacePreview,
        app: &mut Workbench,
    ) -> Option<ReplaceFileOutcome> {
        let plan = preview.files.pop_front()?;
        let count = plan.ranges.len();
        let operation = (|| {
            if self.generation != preview.generation
                || self.root.as_ref() != Some(&preview.root)
                || self.state != ProjectSearchState::Complete
            {
                return Err("La consulta/proyecto cambió desde la vista previa".into());
            }
            // Revalidate every file at the actual acceptance/commit boundary.
            match plan.origin {
                SearchOrigin::Open(stamp) => {
                    if !app.task_stamp_is_current(stamp) {
                        return Err("El documento abierto cambió desde la vista previa".into());
                    }
                    app.apply_text_edits(
                        stamp.document,
                        stamp.revision,
                        plan.ranges
                            .iter()
                            .map(|range| TextReplacement {
                                range: range.clone(),
                                text: preview.replacement.clone(),
                            })
                            .collect(),
                    )
                    .map_err(|e| e.to_string())?;
                    if app
                        .document_by_id(stamp.document)
                        .is_some_and(|d| d.session.document().revision() == stamp.revision)
                    {
                        return Ok(false);
                    }
                }
                SearchOrigin::Disk(expected) => {
                    // Opening a file after preview changes ownership; never save
                    // behind that open document, even if it currently looks clean.
                    if find_open(app, &plan.path)?.is_some() {
                        return Err("Se abrió el archivo después de buscar; vuelve a buscar".into());
                    }
                    let mut session = FileSession::open_in_root_at_version(
                        &preview.root,
                        &plan.path,
                        Some(plan.encoding),
                        2 * 1024 * 1024,
                        expected,
                    )
                    .map_err(|e| e.to_string())?;
                    let replacement = jc_text::encode(&preview.replacement, plan.encoding)
                        .map_err(|e| e.to_string())?;
                    let revision = session.document().revision();
                    session
                        .document_mut()
                        .apply_batch(jc_core::EditBatch {
                            expected_revision: revision,
                            edits: plan
                                .ranges
                                .iter()
                                .map(|range| jc_core::Replacement {
                                    range: range.clone(),
                                    bytes: replacement.clone(),
                                })
                                .collect(),
                        })
                        .map_err(|e| e.to_string())?;
                    if session.document().revision() == revision {
                        return Ok(false);
                    }
                    session.save().map_err(|e| e.to_string())?;
                }
            }
            Ok(true)
        })();
        Some(ReplaceFileOutcome {
            path: plan.path,
            matches: if matches!(operation, Ok(true)) {
                count
            } else {
                0
            },
            saved_to_disk: matches!(plan.origin, SearchOrigin::Disk(_))
                && matches!(operation, Ok(true)),
            error: operation.err(),
        })
    }
}
fn find_open(app: &Workbench, path: &Path) -> Result<Option<crate::DocumentId>, String> {
    for doc in app.open_documents() {
        if scan::absolute_source_path(doc.session.path()).map_err(|e| e.to_string())? == path {
            return Ok(Some(doc.id));
        }
    }
    Ok(None)
}
