//! Bounded sparse visual checkpoints shared by document views. Source glyphs are
//! decoded only while preparing ranges; composition receives owned visible atoms.
use crate::source_graphemes::SourceGraphemes;
use jc_core::Document;
use jc_text::{
    grapheme::{self, GraphemeCursor},
    DecodedScalars, TextEncoding,
};
use std::fmt::Write as _;
use std::{collections::VecDeque, ops::Range, sync::Arc};
pub const CACHE_BYTES: usize = 256 * 1024;
pub const MAX_VISUAL_ROW_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_PRESENTATION_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_LINES: usize = 64;
const MAX_WIDTH: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Glyph {
    Scalar(char),
    Cluster(Arc<str>),
}
impl Glyph {
    pub fn byte_len(&self) -> usize {
        match self {
            Self::Scalar(ch) => ch.len_utf8(),
            Self::Cluster(text) => text.len(),
        }
    }
    pub fn with_str<R>(&self, visit: impl FnOnce(&str) -> R) -> R {
        match self {
            Self::Scalar(ch) => visit(ch.encode_utf8(&mut [0; 4])),
            Self::Cluster(text) => visit(text),
        }
    }
    pub fn first(&self) -> char {
        match self {
            Self::Scalar(ch) => *ch,
            Self::Cluster(text) => text.chars().next().unwrap_or(' '),
        }
    }
    fn text(text: &str) -> Self {
        let mut chars = text.chars();
        if let Some(ch) = chars.next() {
            if chars.next().is_none() {
                return Self::Scalar(ch);
            }
        }
        Self::Cluster(Arc::from(text))
    }
}
#[derive(Clone, Copy)]
pub(crate) enum RowMode {
    Plain,
    Rectangle { markers: bool },
}
#[derive(Debug, Clone)]
pub struct VisualAtom {
    pub source: Range<usize>,
    /// Relative visible cell, not a scalar offset.
    pub column: usize,
    pub width: usize,
    pub glyph: Glyph,
    pub marker: bool,
}
#[derive(Debug, Clone)]
struct Point {
    byte: usize,
    visual: usize,
    cursor: Option<GraphemeCursor>,
}
#[derive(Debug)]
struct Line {
    range: Range<usize>,
    width: usize,
    points: Vec<Point>,
    tail: Point,
    complete: bool,
    dirty: bool,
}
impl Line {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.points.capacity() * std::mem::size_of::<Point>()
    }
}
#[derive(Debug, Default)]
pub(crate) struct UnicodeLayout {
    revision: Option<u64>,
    encoding: Option<TextEncoding>,
    lines: VecDeque<Line>,
    pub scanned_bytes: u64,
}
fn glyph(bytes: &[u8], encoding: TextEncoding, start: usize) -> (Glyph, usize, bool) {
    fn from_text(text: &str) -> (Glyph, usize, bool) {
        let width = grapheme::width(text);
        if width > 0 && !text.chars().any(char::is_control) {
            return (Glyph::text(text), width, false);
        }
        let mut escaped = String::new();
        for ch in text.chars() {
            write!(&mut escaped, "\\u{{{:X}}}", ch as u32).expect("String formatting");
        }
        let width = escaped.len();
        (Glyph::Cluster(Arc::from(escaped)), width, true)
    }
    if encoding == TextEncoding::Utf8 {
        if let Ok(text) = std::str::from_utf8(bytes) {
            return from_text(text);
        }
    } else if let Some(scalar) = DecodedScalars::new(bytes, encoding, start)
        .next()
        .filter(|s| s.valid)
    {
        return from_text(scalar.character.encode_utf8(&mut [0; 4]));
    }
    let mut escaped = String::new();
    for b in bytes {
        write!(&mut escaped, "\\x{b:02X}").expect("String formatting");
    }
    let width = escaped.len();
    (Glyph::Cluster(Arc::from(escaped)), width, true)
}
fn ascii_glyph(bytes: &[u8]) -> Option<(Glyph, usize, bool)> {
    match bytes {
        [b' '..=b'~'] => Some((Glyph::Scalar(bytes[0] as char), 1, false)),
        [b'\t'] => Some((Glyph::Scalar('\t'), 0, false)),
        _ => None,
    }
}
struct DecodedAtom {
    source: Range<usize>,
    glyph: Glyph,
    width: usize,
    marker: bool,
}
struct Walker<'a> {
    doc: &'a Document,
    encoding: TextEncoding,
    range: Range<usize>,
    byte: usize,
    visual: usize,
    utf8: Option<SourceGraphemes<'a>>,
    legacy: Vec<u8>,
    legacy_start: usize,
    scanned: u64,
}
impl<'a> Walker<'a> {
    fn new(doc: &'a Document, encoding: TextEncoding, range: Range<usize>, point: &Point) -> Self {
        let byte = range.start + point.byte;
        Self {
            doc,
            encoding,
            byte,
            visual: point.visual,
            utf8: (encoding == TextEncoding::Utf8).then(|| {
                point.cursor.clone().map_or_else(
                    || SourceGraphemes::new(doc, range.clone(), byte).unwrap(),
                    |c| SourceGraphemes::from_checkpoint(doc, range.clone(), c),
                )
            }),
            range,
            legacy: Vec::new(),
            legacy_start: usize::MAX,
            scanned: 0,
        }
    }
    fn point(&self) -> Point {
        Point {
            byte: self.byte - self.range.start,
            visual: self.visual,
            cursor: self.utf8.as_ref().map(SourceGraphemes::checkpoint),
        }
    }
    fn scanned(&self) -> u64 {
        self.scanned + self.utf8.as_ref().map_or(0, |c| c.scanned as u64)
    }
    fn next(&mut self) -> Result<Option<DecodedAtom>, String> {
        if self.byte >= self.range.end {
            return Ok(None);
        }
        let from = self.byte;
        let (glyph, width, marker) = if let Some(cursor) = &mut self.utf8 {
            self.byte = cursor
                .adjacent(true)?
                .ok_or("Frontera de grafema no disponible")?;
            let source = from..self.byte;
            if source.len() > grapheme::MAX_CLUSTER_BYTES {
                let label = format!("<grafema {}B>", source.len());
                let len = label.len();
                (Glyph::Cluster(Arc::from(label)), len, true)
            } else {
                cursor.visit_text(source, |bytes, _| {
                    ascii_glyph(bytes).unwrap_or_else(|| glyph(bytes, self.encoding, from))
                })?
            }
        } else {
            if from < self.legacy_start
                || from >= self.legacy_start.saturating_add(self.legacy.len())
            {
                self.legacy = self
                    .doc
                    .read_range(from..from.saturating_add(4096).min(self.range.end))
                    .map_err(|e| e.to_string())?;
                self.scanned += self.legacy.len() as u64;
                self.legacy_start = from;
            }
            self.byte += 1;
            let bytes = &self.legacy[from - self.legacy_start..from - self.legacy_start + 1];
            ascii_glyph(bytes).unwrap_or_else(|| glyph(bytes, self.encoding, from))
        };
        let width = if glyph == Glyph::Scalar('\t') {
            4 - self.visual % 4
        } else {
            width
        };
        self.visual = self
            .visual
            .checked_add(width)
            .ok_or("Desbordamiento de columna visual")?;
        Ok(Some(DecodedAtom {
            source: from..self.byte,
            glyph,
            width,
            marker,
        }))
    }
}
#[derive(Clone, Copy)]
enum Need {
    Byte(usize),
    Visual(usize),
}
impl Need {
    fn reached(self, line: &Line) -> bool {
        line.complete
            || match self {
                Self::Byte(byte) => line.range.start + line.tail.byte >= byte,
                Self::Visual(column) => line.width >= column,
            }
    }
}
impl UnicodeLayout {
    fn next_atom(&mut self, walker: &mut Walker<'_>) -> Result<Option<DecodedAtom>, String> {
        let before = walker.scanned();
        let result = walker.next();
        self.scanned_bytes = self
            .scanned_bytes
            .saturating_add(walker.scanned().saturating_sub(before));
        result
    }
    pub fn memory_bytes(&self) -> usize {
        self.lines.iter().map(Line::bytes).sum::<usize>()
            + (self.lines.capacity() - self.lines.len()) * std::mem::size_of::<Line>()
    }
    fn trim(&mut self) {
        while self.lines.len() > MAX_LINES || self.memory_bytes() > CACHE_BYTES {
            self.lines.pop_front();
            if self.lines.is_empty() {
                self.lines.shrink_to_fit();
                break;
            }
        }
    }
    fn ensure(&mut self, doc: &Document, encoding: TextEncoding) {
        if self.revision == Some(doc.revision()) && self.encoding == Some(encoding) {
            return;
        }
        if self.encoding == Some(encoding) {
            if let Some(change) = doc
                .last_change()
                .filter(|c| Some(c.before_revision) == self.revision)
            {
                for line in &mut self.lines {
                    let mut low = None;
                    for edit in &change.edits {
                        if edit.replaced.start <= line.range.end
                            && edit.replaced.end >= line.range.start
                        {
                            low = Some(low.map_or(edit.replaced.start, |n: usize| {
                                n.min(edit.replaced.start)
                            }));
                        }
                    }
                    if let Some(low) = low {
                        let relative = low.saturating_sub(line.range.start).saturating_sub(4);
                        let keep = line.points.partition_point(|p| p.byte < relative).max(1);
                        line.points.truncate(keep);
                        line.dirty = true;
                        line.complete = false;
                        line.tail = line.points.last().unwrap().clone();
                        line.width = line.tail.visual;
                    }
                    line.range =
                        change.map_offset(line.range.start)..change.map_offset(line.range.end);
                }
            } else {
                self.lines.clear();
            }
        } else {
            self.lines.clear();
        }
        self.revision = Some(doc.revision());
        self.encoding = Some(encoding);
    }
    fn line(
        &mut self,
        doc: &Document,
        encoding: TextEncoding,
        range: Range<usize>,
        need: Need,
    ) -> Result<usize, String> {
        self.ensure(doc, encoding);
        let mut line = if let Some(index) = self.lines.iter().position(|l| l.range == range) {
            let line = self.lines.remove(index).unwrap();
            if !line.dirty && need.reached(&line) {
                self.lines.push_back(line);
                return Ok(self.lines.len() - 1);
            }
            line
        } else {
            Line {
                range: range.clone(),
                width: 0,
                points: vec![Point {
                    byte: 0,
                    visual: 0,
                    cursor: None,
                }],
                tail: Point {
                    byte: 0,
                    visual: 0,
                    cursor: None,
                },
                complete: range.is_empty(),
                dirty: true,
            }
        };
        let last = line.tail.clone();
        let mut walker = Walker::new(doc, encoding, range.clone(), &last);
        let stride = 1024usize.max(
            range
                .len()
                .saturating_mul(std::mem::size_of::<Point>())
                .div_ceil(CACHE_BYTES / 4),
        );
        let mut checkpoint = line.points.last().unwrap().byte;
        loop {
            // Store cursor state BEFORE the next atom, so edit continuation
            // never carries categories from bytes at/after the edited suffix.
            if (walker.byte - range.start).saturating_sub(checkpoint) >= stride {
                let point = walker.point();
                checkpoint = point.byte;
                line.points.push(point);
            }
            let reached = match need {
                Need::Byte(byte) => walker.byte >= byte,
                Need::Visual(column) => walker.visual >= column,
            };
            if reached || self.next_atom(&mut walker)?.is_none() {
                break;
            }
        }
        line.width = walker.visual;
        line.tail = walker.point();
        line.complete = walker.byte >= range.end;
        line.dirty = false;
        self.lines.push_back(line);
        self.trim();
        self.lines
            .iter()
            .position(|l| l.range == range)
            .ok_or_else(|| "Los metadatos de línea Unicode exceden la cuota del caché".into())
    }
    pub fn column(
        &mut self,
        doc: &Document,
        encoding: TextEncoding,
        range: Range<usize>,
        byte: usize,
    ) -> Result<usize, String> {
        let index = self.line(doc, encoding, range.clone(), Need::Byte(byte))?;
        let line = &self.lines[index];
        if byte == range.end {
            return Ok(line.width);
        }
        if byte < range.start || byte > range.end {
            return Err("Byte fuera de la línea visual".into());
        }
        let point = &line.points[line
            .points
            .partition_point(|p| p.byte <= byte - range.start)
            .saturating_sub(1)];
        let mut walker = Walker::new(doc, encoding, range, point);
        while let Some(DecodedAtom { source, width, .. }) = self.next_atom(&mut walker)? {
            if byte < source.end {
                return Ok(walker.visual - width);
            }
        }
        Err("Frontera visual no disponible".into())
    }
    pub fn adjacent(
        &mut self,
        doc: &Document,
        encoding: TextEncoding,
        range: Range<usize>,
        byte: usize,
        forward: bool,
    ) -> Result<Option<usize>, String> {
        if forward && byte >= range.end || !forward && byte <= range.start {
            return Ok(None);
        }
        let index = self.line(
            doc,
            encoding,
            range.clone(),
            Need::Byte(byte.saturating_add(usize::from(forward)).min(range.end)),
        )?;
        let line = &self.lines[index];
        let relative = byte - range.start;
        let point = &line.points[line
            .points
            .partition_point(|p| {
                if forward {
                    p.byte <= relative
                } else {
                    p.byte < relative
                }
            })
            .saturating_sub(1)];
        let mut walker = Walker::new(doc, encoding, range, point);
        while let Some(DecodedAtom { source, .. }) = self.next_atom(&mut walker)? {
            let result = if forward {
                (source.end > byte).then_some(source.end)
            } else {
                (source.end >= byte).then_some(source.start)
            };
            if result.is_some() {
                return Ok(result);
            }
        }
        Ok(None)
    }
    pub fn byte_at(
        &mut self,
        doc: &Document,
        encoding: TextEncoding,
        range: Range<usize>,
        column: usize,
    ) -> Result<usize, String> {
        let index = self.line(
            doc,
            encoding,
            range.clone(),
            Need::Visual(column.saturating_add(1)),
        )?;
        let line = &self.lines[index];
        if column >= line.width {
            return Ok(range.end);
        }
        let point = &line.points[line
            .points
            .partition_point(|p| p.visual <= column)
            .saturating_sub(1)];
        let mut walker = Walker::new(doc, encoding, range, point);
        while let Some(DecodedAtom { source, .. }) = self.next_atom(&mut walker)? {
            if column < walker.visual {
                return Ok(source.start);
            }
        }
        Err("Columna visual no disponible".into())
    }
    pub fn row(
        &mut self,
        doc: &Document,
        encoding: TextEncoding,
        range: Range<usize>,
        scroll: usize,
        width: usize,
        mode: RowMode,
    ) -> Result<Vec<VisualAtom>, String> {
        if width > MAX_WIDTH {
            return Err("La fila Unicode visible excede 4096 celdas".into());
        }
        let (markers, pad) = match mode {
            RowMode::Plain => (false, false),
            RowMode::Rectangle { markers } => (markers, true),
        };
        if width == 0 || range.is_empty() && !pad {
            return Ok(Vec::new());
        }
        let index = self.line(
            doc,
            encoding,
            range.clone(),
            Need::Visual(scroll.saturating_add(width)),
        )?;
        let line = &self.lines[index];
        let point = &line.points[line
            .points
            .partition_point(|p| p.visual <= scroll)
            .saturating_sub(1)];
        let mut walker = Walker::new(doc, encoding, range.clone(), point);
        let mut atoms = Vec::with_capacity(width.min(range.len()));
        let mut text_bytes = 0usize;
        let end = scroll.saturating_add(width);
        while walker.visual < end {
            let left = walker.visual;
            let Some(DecodedAtom {
                source,
                glyph,
                width: cells,
                marker,
            }) = self.next_atom(&mut walker)?
            else {
                break;
            };
            let right = walker.visual;
            if right <= scroll {
                continue;
            }
            let charge = if glyph == Glyph::Scalar('\t') || left < scroll || right > end {
                right.min(end) - left.max(scroll)
            } else {
                glyph.byte_len()
            };
            text_bytes = text_bytes
                .checked_add(charge)
                .filter(|&n| n <= MAX_VISUAL_ROW_TEXT_BYTES)
                .ok_or("La fila Unicode excede la cuota de64 KiB de glifos")?;
            if glyph == Glyph::Scalar('\t') || left < scroll || right > end {
                // Clip a wide glyph atomically: show padding at an edge, never
                // emit half a cluster or attach a combining suffix to a border.
                for column in left.max(scroll)..right.min(end) {
                    let ch = if markers && glyph == Glyph::Scalar('\t') {
                        if column == left {
                            '>'
                        } else {
                            '.'
                        }
                    } else {
                        ' '
                    };
                    atoms.push(VisualAtom {
                        source: source.clone(),
                        column: column - scroll,
                        width: 1,
                        glyph: Glyph::Scalar(ch),
                        marker: markers && glyph == Glyph::Scalar('\t'),
                    });
                }
            } else {
                atoms.push(VisualAtom {
                    source,
                    column: left - scroll,
                    width: cells,
                    glyph: if markers && glyph == Glyph::Scalar(' ') {
                        Glyph::Scalar('.')
                    } else {
                        glyph
                    },
                    marker,
                });
            }
        }
        if pad {
            if end.saturating_sub(walker.visual.max(scroll))
                > MAX_VISUAL_ROW_TEXT_BYTES.saturating_sub(text_bytes)
            {
                return Err("El relleno de fila Unicode excede la cuota de glifos".into());
            }
            for column in walker.visual.max(scroll)..end {
                atoms.push(VisualAtom {
                    source: range.end..range.end,
                    column: column - scroll,
                    width: 1,
                    glyph: Glyph::Scalar(if markers { '.' } else { ' ' }),
                    marker: markers,
                });
            }
        }
        Ok(atoms)
    }
}
