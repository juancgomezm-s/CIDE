//! Bounded declarative templates. Providers contribute data; the host validates
//! source identity, renders a reviewable proposal and commits one transaction.
use crate::{edit_field, source_services::display_label, FieldEdit, PaneId, TaskStamp, Workbench};
use jc_text::{decode_fragment, encode, TextEncoding};
use std::{ops::Range, path::PathBuf};
const MAX_SELECTION: usize = 16 * 1024;
const MAX_PROPOSAL: usize = 64 * 1024;
#[derive(Debug, Clone, Copy)]
pub enum ParameterRule {
    Identifier { extra: &'static str },
    SingleLine,
}
impl ParameterRule {
    fn valid(self, text: &str) -> bool {
        if text.is_empty() || text.len() > 256 || text.chars().any(char::is_control) {
            return false;
        }
        let first = match self {
            Self::SingleLine => true,
            Self::Identifier { .. } => {
                text.as_bytes()[0].is_ascii_alphabetic() || text.starts_with('_')
            }
        };
        let characters = match self {
            Self::SingleLine => true,
            Self::Identifier { extra } => text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || extra.as_bytes().contains(&b)),
        };
        first && characters
    }
}
#[derive(Debug, Clone, Copy)]
pub struct SnippetParameter {
    pub key: &'static str,
    pub label: &'static str,
    pub default: &'static str,
    pub rule: ParameterRule,
}
#[derive(Debug, Clone, Copy)]
pub struct SnippetTemplate {
    pub id: &'static str,
    pub name: &'static str,
    pub parameters: &'static [SnippetParameter],
    pub body: &'static str,
    pub surround: bool,
}
enum Part<'a> {
    Literal(&'a str),
    Field(&'a str),
}
fn walk(body: &str, mut emit: impl FnMut(Part<'_>) -> Result<(), String>) -> Result<(), String> {
    let mut at = 0;
    while at < body.len() {
        let Some(delta) = body[at..].find('$') else {
            emit(Part::Literal(&body[at..]))?;
            break;
        };
        let dollar = at + delta;
        if dollar > at {
            emit(Part::Literal(&body[at..dollar]))?;
        }
        if body.as_bytes().get(dollar + 1) == Some(&b'$') {
            emit(Part::Literal("$"))?;
            at = dollar + 2;
        } else if body.as_bytes().get(dollar + 1) == Some(&b'{') {
            let end = body[dollar + 2..]
                .find('}')
                .map(|p| p + dollar + 2)
                .ok_or("Unclosed snippet variable")?;
            emit(Part::Field(&body[dollar + 2..end]))?;
            at = end + 1;
        } else {
            emit(Part::Literal("$"))?;
            at = dollar + 1;
        }
    }
    Ok(())
}
impl SnippetTemplate {
    pub fn validate(&self, provider: &str) -> Result<(), String> {
        if self.id.len() > 96
            || !self.id.starts_with(provider)
            || self.id.as_bytes().get(provider.len()) != Some(&b'.')
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            || self.name.is_empty()
            || self.name.len() > 128
            || self.name.chars().any(char::is_control)
            || self.body.is_empty()
            || self.body.len() > 8192
            || self.parameters.len() > 4
            || self
                .body
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        {
            return Err("Invalid snippet metadata/budget".into());
        }
        for (i, p) in self.parameters.iter().enumerate() {
            if p.key.is_empty()
                || p.key.len() > 32
                || !p
                    .key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || matches!(p.key, "selection" | "cursor")
                || p.label.len() > 128
                || p.label.chars().any(char::is_control)
                || !p.rule.valid(p.default)
                || self.parameters[..i].iter().any(|other| other.key == p.key)
            {
                return Err("Invalid snippet parameter metadata/default".into());
            }
        }
        let mut selection = 0;
        let mut cursor = 0;
        let mut used = [false; 4];
        walk(self.body, |part| {
            if let Part::Field(key) = part {
                match key {
                    "selection" => selection += 1,
                    "cursor" => cursor += 1,
                    _ => {
                        let index = self
                            .parameters
                            .iter()
                            .position(|p| p.key == key)
                            .ok_or("Unknown snippet variable")?;
                        used[index] = true;
                    }
                }
            }
            Ok(())
        })?;
        if selection != usize::from(self.surround)
            || cursor > 1
            || used[..self.parameters.len()].iter().any(|v| !*v)
        {
            return Err("Snippet selection/cursor/parameter contract mismatch".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct SnippetTarget {
    stamp: TaskStamp,
    path: PathBuf,
    pane: PaneId,
    cursor: usize,
    anchor: Option<usize>,
    range: Range<usize>,
    selected: bool,
}
impl SnippetTarget {
    pub fn capture(app: &Workbench) -> Result<Self, String> {
        let editor = app.active_editor().ok_or("No snippet document")?;
        let doc = app.active_document().ok_or("No snippet document")?;
        let pane = app.panes().active();
        if app.rectangle_for_pane(pane).is_some() {
            return Err("Snippet requires a linear caret/selection".into());
        }
        let encoding = doc
            .session
            .encoding()
            .ok_or("Choose document encoding before snippets")?;
        if doc.session.path().as_os_str().len() > 4096 {
            return Err("Snippet source metadata limit".into());
        }
        let mut range = editor.selection().unwrap_or(editor.cursor..editor.cursor);
        if range.len() > MAX_SELECTION {
            return Err("Snippet selection exceeds16KiB source bytes".into());
        }
        if range.start == 0
            && encoding == TextEncoding::Utf8
            && doc.session.document().len() >= 3
            && doc
                .session
                .document()
                .read_range(0..3)
                .map_err(|e| e.to_string())?
                == b"\xef\xbb\xbf"
        {
            if range.is_empty() {
                range = 3..3;
            } else if range.end >= 3 {
                range.start = 3;
            } else {
                return Err("Snippet selection is inside BOM".into());
            }
        }
        if app.position_for_tab(doc.tab_id, range.start).is_none()
            || app.position_for_tab(doc.tab_id, range.end).is_none()
        {
            return Err("Snippet range is not a legal text boundary".into());
        }
        Ok(Self {
            stamp: TaskStamp {
                document: doc.id,
                revision: doc.session.document().revision(),
                encoding,
                view: Some(
                    app.panes()
                        .view(pane)
                        .and_then(|v| v.view_id())
                        .ok_or("No snippet ViewId")?,
                ),
            },
            path: doc.session.path().to_owned(),
            pane,
            cursor: editor.cursor,
            anchor: editor.anchor,
            selected: !range.is_empty(),
            range,
        })
    }
    pub fn has_selection(&self) -> bool {
        self.selected
    }
    pub fn current(&self, app: &Workbench) -> bool {
        app.task_stamp_is_current(self.stamp)
            && app.panes().active() == self.pane
            && app
                .panes()
                .view(self.pane)
                .is_some_and(|v| v.view_id() == self.stamp.view)
            && app
                .active_document()
                .is_some_and(|d| d.id == self.stamp.document && d.session.path() == self.path)
            && app
                .active_editor()
                .is_some_and(|e| e.cursor == self.cursor && e.anchor == self.anchor)
            && app.rectangle_for_pane(self.pane).is_none()
    }
}
#[derive(Debug)]
pub struct SnippetPlan {
    before: String,
    after: String,
    range: Range<usize>,
    cursor_bytes: usize,
    total_rows: usize,
}
struct Lines<'a> {
    text: &'a str,
    at: usize,
    done: bool,
}
impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            at: 0,
            done: false,
        }
    }
}
impl<'a> Iterator for Lines<'a> {
    type Item = (&'a str, &'static str);
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let bytes = self.text.as_bytes();
        let start = self.at;
        let Some(n) = bytes[start..]
            .iter()
            .position(|b| matches!(b, b'\r' | b'\n'))
        else {
            self.done = true;
            return Some((&self.text[start..], "EOF"));
        };
        let end = start + n;
        let (step, eol) = if bytes[end] == b'\r' && bytes.get(end + 1) == Some(&b'\n') {
            (2, "CRLF")
        } else if bytes[end] == b'\r' {
            (1, "CR")
        } else {
            (1, "LF")
        };
        self.at = end + step;
        Some((&self.text[start..end], eol))
    }
}
impl SnippetPlan {
    pub fn text(&self) -> &str {
        &self.after
    }
    pub fn total_rows(&self) -> usize {
        self.total_rows
    }
    pub fn rows(&self, first: usize, count: usize, columns: usize) -> Vec<String> {
        let columns = columns.min(256);
        let mut rows = Vec::new();
        let mut logical = 0;
        let end = first.saturating_add(count.min(96));
        for (sign, text, header) in [
            (
                "-",
                self.before.as_str(),
                format!("--- source bytes {}..{}", self.range.start, self.range.end),
            ),
            (
                "+",
                self.after.as_str(),
                format!("+++ proposal {} UTF8 bytes", self.after.len()),
            ),
        ] {
            if logical >= first && logical < end {
                rows.push(display_label(&header, columns));
            }
            logical += 1;
            for (line, eol) in Lines::new(text) {
                if logical >= end {
                    return rows;
                }
                if logical >= first {
                    let reserve = sign.len() + eol.len() + 4;
                    rows.push(display_label(
                        &format!(
                            "{sign} {} [{eol}]",
                            display_label(line, columns.saturating_sub(reserve))
                        ),
                        columns,
                    ));
                }
                logical += 1;
            }
        }
        rows
    }
}
#[derive(Debug)]
pub struct SnippetSession {
    target: SnippetTarget,
    template: &'static SnippetTemplate,
    values: Vec<String>,
    cursors: Vec<usize>,
    plan: Option<SnippetPlan>,
}
impl SnippetSession {
    pub fn new(target: SnippetTarget, template: &'static SnippetTemplate) -> Result<Self, String> {
        template.validate(
            template
                .id
                .rsplit_once('.')
                .map(|(provider, _)| provider)
                .ok_or("Snippet ID must be namespaced")?,
        )?;
        if target.has_selection() != template.surround {
            return Err(
                "Template caret/selection requirement differs from the original target".into(),
            );
        }
        Ok(Self {
            target,
            template,
            values: template
                .parameters
                .iter()
                .map(|p| p.default.to_owned())
                .collect(),
            cursors: template
                .parameters
                .iter()
                .map(|p| p.default.len())
                .collect(),
            plan: None,
        })
    }
    pub fn template(&self) -> &'static SnippetTemplate {
        self.template
    }
    pub fn current(&self, app: &Workbench) -> bool {
        self.target.current(app)
    }
    pub fn parameter(&self, index: usize) -> Option<(&SnippetParameter, &str, usize)> {
        Some((
            self.template.parameters.get(index)?,
            self.values.get(index)?.as_str(),
            *self.cursors.get(index)?,
        ))
    }
    pub fn edit_parameter(&mut self, index: usize, edit: FieldEdit<'_>) -> Result<(), String> {
        let value = self.values.get_mut(index).ok_or("No snippet parameter")?;
        edit_field(value, &mut self.cursors[index], edit, 256).map_err(str::to_owned)?;
        self.plan = None;
        Ok(())
    }
    pub fn valid_parameter(&self, index: usize) -> bool {
        self.template
            .parameters
            .get(index)
            .is_some_and(|p| p.rule.valid(&self.values[index]))
    }
    pub fn plan(&self) -> Option<&SnippetPlan> {
        self.plan.as_ref()
    }
    pub fn prepare(&mut self, app: &Workbench) -> Result<(), String> {
        self.plan = None;
        if !self.current(app) {
            return Err("Snippet target changed; reopen the wizard".into());
        }
        for i in 0..self.values.len() {
            if !self.valid_parameter(i) {
                return Err(format!(
                    "Invalid snippet parameter: {}",
                    self.template.parameters[i].label
                ));
            }
        }
        let doc = app.active_document().ok_or("No snippet document")?;
        let bytes = doc
            .session
            .document()
            .read_range(self.target.range.clone())
            .map_err(|e| e.to_string())?;
        let decoded = decode_fragment(&bytes, self.target.stamp.encoding, self.target.range.start);
        if !decoded.issues.is_empty() {
            return Err("Snippet selection contains uninterpretable bytes; no edits made".into());
        }
        let before = decoded.text;
        let mut selection = before.as_str();
        let mut trailing = "";
        if self.template.surround {
            for ending in ["\r\n", "\n", "\r"] {
                if let Some(content) = selection.strip_suffix(ending) {
                    selection = content;
                    trailing = ending;
                    break;
                }
            }
        }
        let ending =
            std::str::from_utf8(app.preferred_ending().map_err(|e| e.to_string())?).unwrap();
        let mut after = String::new();
        let mut cursor = None;
        let append = |after: &mut String, text: &str| -> Result<(), String> {
            if after.len().saturating_add(text.len()) > MAX_PROPOSAL {
                return Err("Snippet proposal exceeds64KiB UTF8".into());
            }
            after.push_str(text);
            Ok(())
        };
        walk(self.template.body, |part| {
            match part {
                Part::Field("selection") => append(&mut after, selection)?,
                Part::Field("cursor") => cursor = Some(after.len()),
                Part::Field(key) => {
                    let i = self
                        .template
                        .parameters
                        .iter()
                        .position(|p| p.key == key)
                        .ok_or("Unknown snippet variable")?;
                    append(&mut after, &self.values[i])?;
                }
                Part::Literal(text) => {
                    for fragment in text.split_inclusive('\n') {
                        if let Some(content) = fragment.strip_suffix('\n') {
                            append(&mut after, content)?;
                            append(&mut after, ending)?;
                        } else {
                            append(&mut after, fragment)?;
                        }
                    }
                }
            }
            Ok(())
        })?;
        let cursor = cursor.unwrap_or(after.len());
        append(&mut after, trailing)?;
        let encoded = encode(&after, self.target.stamp.encoding).map_err(|e| e.to_string())?;
        // The three explicit legacy codecs are single-byte; strict full encoding
        // above has already rejected every unrepresentable scalar.
        let cursor_bytes = match self.target.stamp.encoding {
            TextEncoding::Utf8 => cursor,
            TextEncoding::Windows1252 | TextEncoding::Iso8859_1 | TextEncoding::Ibm850 => {
                after[..cursor].chars().count()
            }
        };
        if self.target.range.start == 0
            && self.target.stamp.encoding == TextEncoding::Utf8
            && encoded.starts_with(b"\xef\xbb\xbf")
            && cursor_bytes < 3
        {
            return Err("Snippet cursor would enter the new BOM; no edits made".into());
        }
        let left = if cursor_bytes > 0 {
            encoded.get(cursor_bytes - 1).copied()
        } else if self.target.range.start > 0 {
            doc.session
                .document()
                .read_range(self.target.range.start - 1..self.target.range.start)
                .map_err(|e| e.to_string())?
                .first()
                .copied()
        } else {
            None
        };
        let right = if cursor_bytes < encoded.len() {
            encoded.get(cursor_bytes).copied()
        } else if self.target.range.end < doc.session.document().len() {
            doc.session
                .document()
                .read_range(self.target.range.end..self.target.range.end + 1)
                .map_err(|e| e.to_string())?
                .first()
                .copied()
        } else {
            None
        };
        if left == Some(b'\r') && right == Some(b'\n') {
            return Err("Snippet cursor would enter CRLF; no edits made".into());
        }
        let total_rows = 2 + Lines::new(&before).count() + Lines::new(&after).count();
        self.plan = Some(SnippetPlan {
            before,
            after,
            range: self.target.range.clone(),
            cursor_bytes,
            total_rows,
        });
        Ok(())
    }
    pub fn apply(&mut self, app: &mut Workbench) -> Result<(), String> {
        if !self.current(app) {
            return Err("Snippet target changed; reopen the wizard".into());
        }
        let plan = self
            .plan
            .take()
            .ok_or("Review the snippet proposal before applying")?;
        let cursor = plan.range.start + plan.cursor_bytes;
        app.apply_text_edits(
            self.target.stamp.document,
            self.target.stamp.revision,
            vec![crate::TextReplacement {
                range: plan.range,
                text: plan.after,
            }],
        )
        .map_err(|e| e.to_string())?;
        app.set_linear_selection_bytes(cursor, cursor)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
