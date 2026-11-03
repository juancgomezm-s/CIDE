//! Streaming serialization of the canonical document, including retained undo.
//! Filesystem atomicity/checksums belong to the outer recovery store. No source
//! decoding, editor clones, path policy or UI dependencies belong here.
use super::*;
use std::io::Read;

const MAGIC: &[u8; 8] = b"CIDEDOC1";
const END: &[u8; 8] = b"ENDCDOC1";
const VERSION: u64 = 1;
const CHUNK: usize = 64 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct RecoveryLimits {
    /// Combined retained arenas and, independently, each logical state length.
    pub max_source_bytes: usize,
    pub max_metadata_bytes: usize,
    pub max_states: usize,
    pub max_pieces: usize,
    pub max_edits: usize,
    pub max_encoded_bytes: u64,
}
impl Default for RecoveryLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 256 * 1024 * 1024,
            max_metadata_bytes: 64 * 1024 * 1024,
            max_states: 2049,
            max_pieces: 1_000_000,
            max_edits: 1_000_000,
            max_encoded_bytes: 512 * 1024 * 1024,
        }
    }
}
#[derive(Debug)]
pub enum RecoveryError {
    Io(io::Error),
    InvalidFormat(&'static str),
    LimitExceeded(&'static str),
    UnsupportedVersion(u64),
    Allocation,
}
impl fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "No se pudo leer la recuperación: {error}"),
            Self::InvalidFormat(reason) => write!(f, "Recuperación inválida: {reason}"),
            Self::LimitExceeded(reason) => {
                write!(f, "La recuperación supera el presupuesto: {reason}")
            }
            Self::UnsupportedVersion(version) => {
                write!(f, "Versión de recuperación no admitida: {version}")
            }
            Self::Allocation => write!(f, "No se pudo reservar la memoria acotada de recuperación"),
        }
    }
}
impl std::error::Error for RecoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(error) = self {
            Some(error)
        } else {
            None
        }
    }
}
impl From<io::Error> for RecoveryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
fn invalid(reason: &'static str) -> RecoveryError {
    RecoveryError::InvalidFormat(reason)
}
fn integer(writer: &mut dyn Write, value: u64) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}
fn size(writer: &mut dyn Write, value: usize) -> io::Result<()> {
    integer(
        writer,
        u64::try_from(value).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Entero de recuperación no representable",
            )
        })?,
    )
}
fn write_change(writer: &mut dyn Write, change: Option<&ChangeSet>) -> io::Result<()> {
    integer(writer, u64::from(change.is_some()))?;
    if let Some(change) = change {
        integer(writer, change.before_revision)?;
        integer(writer, change.after_revision)?;
        size(writer, change.edits.len())?;
        for edit in &change.edits {
            size(writer, edit.replaced.start)?;
            size(writer, edit.replaced.end)?;
            size(writer, edit.inserted_len)?;
        }
    }
    Ok(())
}
fn write_state(writer: &mut dyn Write, state: &State) -> io::Result<()> {
    size(writer, state.len)?;
    integer(writer, state.revision)?;
    size(writer, state.pieces.len())?;
    for piece in &state.pieces {
        integer(
            writer,
            match piece.source {
                Source::Original => 0,
                Source::Added => 1,
            },
        )?;
        size(writer, piece.start)?;
        size(writer, piece.len)?;
    }
    write_change(writer, state.edit.as_ref())
}
#[derive(Default)]
struct RecoveryShape {
    source: usize,
    states: usize,
    pieces: usize,
    edits: usize,
    changes: usize,
    index_blocks: usize,
    metadata: usize,
    encoded: u64,
}
impl RecoveryShape {
    fn of(document: &Document) -> Result<Self, RecoveryError> {
        let history = document
            .undo
            .len()
            .checked_add(document.redo.len())
            .ok_or(invalid("cantidad de estados"))?;
        let mut shape = Self {
            source: document
                .original
                .len()
                .checked_add(document.added.len())
                .ok_or(invalid("longitud de fuentes"))?,
            states: history
                .checked_add(1)
                .ok_or(invalid("cantidad de estados"))?,
            ..Self::default()
        };
        for state in std::iter::once(&document.state)
            .chain(&document.undo)
            .chain(&document.redo)
        {
            shape.pieces = shape
                .pieces
                .checked_add(state.pieces.len())
                .ok_or(invalid("cantidad de piezas"))?;
            shape.index_blocks = shape
                .index_blocks
                .checked_add(state.pieces.len().div_ceil(PIECE_BLOCK))
                .ok_or(invalid("cantidad de bloques"))?;
            shape.change(state.edit.as_ref())?;
        }
        shape.change(document.last_edit.as_ref())?;
        // Decoder builds exact logical vectors, not the live Vec capacities.
        shape.metadata = std::mem::size_of::<Document>();
        shape.charge(history, std::mem::size_of::<State>())?;
        shape.charge(shape.pieces, std::mem::size_of::<Piece>())?;
        shape.charge(shape.pieces, std::mem::size_of::<usize>())?;
        shape.charge(shape.index_blocks, std::mem::size_of::<usize>())?;
        shape.charge(shape.edits, std::mem::size_of::<ByteEdit>())?;
        // Header80 + last-map tag8 + terminator8. Each State has a32-byte
        // header/tag; each present map adds24 bytes beyond its ordinary tag.
        shape.encoded = 96;
        for (count, bytes) in [
            (shape.source, 1u64),
            (shape.states, 32),
            (shape.pieces, 24),
            (shape.changes, 24),
            (shape.edits, 24),
        ] {
            let count = u64::try_from(count)
                .map_err(|_| invalid("cantidad codificada no representable"))?;
            shape.encoded = shape
                .encoded
                .checked_add(
                    count
                        .checked_mul(bytes)
                        .ok_or(invalid("longitud codificada"))?,
                )
                .ok_or(invalid("longitud codificada"))?;
        }
        Ok(shape)
    }
    fn change(&mut self, change: Option<&ChangeSet>) -> Result<(), RecoveryError> {
        if let Some(change) = change {
            self.changes = self
                .changes
                .checked_add(1)
                .ok_or(invalid("cantidad de mapas"))?;
            self.edits = self
                .edits
                .checked_add(change.edits.len())
                .ok_or(invalid("cantidad de ediciones"))?;
        }
        Ok(())
    }
    fn charge(&mut self, count: usize, width: usize) -> Result<(), RecoveryError> {
        self.metadata = self
            .metadata
            .checked_add(
                count
                    .checked_mul(width)
                    .ok_or(invalid("tamaño de metadatos"))?,
            )
            .ok_or(invalid("tamaño de metadatos"))?;
        Ok(())
    }
}
impl Document {
    /// Exact serialized length using only arena/vector lengths. This neither
    /// reads source bytes nor allocates, clones or serializes the document.
    pub fn recovery_encoded_len(&self) -> Result<u64, RecoveryError> {
        Ok(RecoveryShape::of(self)?.encoded)
    }

    /// Admit the complete payload against the same limits/geometry as restore.
    /// Work is proportional to retained metadata; source arenas are not scanned.
    /// This cannot predict an OS allocation/I/O failure during the later load.
    pub fn validate_recovery_limits(&self, limits: RecoveryLimits) -> Result<(), RecoveryError> {
        let shape = RecoveryShape::of(self)?;
        for (actual, maximum, label) in [
            (shape.source, limits.max_source_bytes, "arenas de bytes"),
            (shape.states, limits.max_states, "estados de historial"),
            (shape.pieces, limits.max_pieces, "piezas"),
            (shape.edits, limits.max_edits, "mapas de edición"),
            (shape.metadata, limits.max_metadata_bytes, "metadatos"),
        ] {
            if actual > maximum {
                return Err(RecoveryError::LimitExceeded(label));
            }
        }
        if shape.encoded > limits.max_encoded_bytes {
            return Err(RecoveryError::LimitExceeded("bytes codificados"));
        }
        if self.next_revision == 0 || self.saved_revision >= self.next_revision {
            return Err(invalid("revisión guardada o siguiente"));
        }
        if self.undo.len() > self.history_limit || self.redo.len() > self.history_limit {
            return Err(invalid("límite de pasos del historial"));
        }
        let mut restored_history = 0usize;
        let mut current_payload = 0usize;
        for (ordinal, state) in std::iter::once(&self.state)
            .chain(&self.undo)
            .chain(&self.redo)
            .enumerate()
        {
            if state.revision >= self.next_revision {
                return Err(invalid("revisión de estado"));
            }
            if state.len > limits.max_source_bytes {
                return Err(RecoveryError::LimitExceeded("longitud lógica de estado"));
            }
            let mut total = 0usize;
            for piece in &state.pieces {
                validate_piece(piece, self.original.len(), self.added.len())?;
                total = total
                    .checked_add(piece.len)
                    .ok_or(invalid("suma de piezas"))?;
            }
            if total != state.len {
                return Err(invalid("longitud de estado y piezas"));
            }
            let payload = state_change_payload(
                state.edit.as_ref(),
                state.len,
                state.revision,
                self.next_revision,
            )?;
            if ordinal == 0 {
                current_payload = payload;
            } else {
                restored_history = restored_history
                    .checked_add(state_metadata_charge(state)?)
                    .ok_or(invalid("cargo del historial"))?;
                // undo[0] is the uncharged transition into the oldest retained
                // state. Every redo and every later undo payload is charged.
                if ordinal != 1 || self.undo.is_empty() {
                    restored_history = restored_history
                        .checked_add(payload)
                        .ok_or(invalid("cargo del historial"))?;
                }
            }
        }
        if !self.undo.is_empty() {
            restored_history = restored_history
                .checked_add(current_payload)
                .ok_or(invalid("cargo del historial"))?;
        }
        if restored_history > self.history_bytes_limit {
            return Err(invalid("cargo del historial excedido"));
        }
        validate_last_change(
            &self.state,
            &self.redo,
            self.last_edit.as_ref(),
            self.next_revision,
        )?;
        validate_transitions(&self.undo, &self.state, &self.redo, self.next_revision)
    }
    /// Write one self-delimited payload. Arenas are borrowed and streamed in
    /// <=64KiB requests; the outer store supplies atomic generation publication.
    pub fn write_recovery(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(MAGIC)?;
        integer(writer, VERSION)?;
        size(writer, self.original.len())?;
        size(writer, self.added.len())?;
        integer(writer, self.saved_revision)?;
        integer(writer, self.next_revision)?;
        size(writer, self.history_limit)?;
        size(writer, self.history_bytes_limit)?;
        size(writer, self.undo.len())?;
        size(writer, self.redo.len())?;
        for arena in [&self.original, &self.added] {
            for bytes in arena.chunks(CHUNK) {
                writer.write_all(bytes)?;
            }
        }
        write_state(writer, &self.state)?;
        for state in &self.undo {
            write_state(writer, state)?;
        }
        for state in &self.redo {
            write_state(writer, state)?;
        }
        write_change(writer, self.last_edit.as_ref())?;
        writer.write_all(END)
    }

    /// Restore a complete document or return an error without publishing one.
    /// No checksum is implied here; validate the outer generation's checksum.
    /// Source/metadata limits charge retained capacities, not only logical data;
    /// allocator transient reallocations and fixed64KiB scratch are separate.
    pub fn read_recovery(
        reader: &mut dyn Read,
        limits: RecoveryLimits,
    ) -> Result<Self, RecoveryError> {
        let mut decoder = Decoder {
            reader,
            limits,
            encoded: 0,
            metadata: 0,
            pieces: 0,
            edits: 0,
        };
        let mut magic = [0u8; 8];
        decoder.bytes(&mut magic)?;
        if &magic != MAGIC {
            return Err(invalid("encabezado"));
        }
        let version = decoder.integer()?;
        if version != VERSION {
            return Err(RecoveryError::UnsupportedVersion(version));
        }
        let original_len = decoder.size()?;
        let added_len = decoder.size()?;
        let source_len = original_len
            .checked_add(added_len)
            .ok_or(invalid("longitud de fuentes"))?;
        if source_len > limits.max_source_bytes {
            return Err(RecoveryError::LimitExceeded("arenas de bytes"));
        }
        let saved_revision = decoder.integer()?;
        let next_revision = decoder.integer()?;
        if next_revision == 0 || saved_revision >= next_revision {
            return Err(invalid("revisión guardada o siguiente"));
        }
        let history_limit = decoder.size()?;
        let history_bytes_limit = decoder.size()?;
        let undo_count = decoder.size()?;
        let redo_count = decoder.size()?;
        let state_count = undo_count
            .checked_add(redo_count)
            .and_then(|n| n.checked_add(1))
            .ok_or(invalid("cantidad de estados"))?;
        if state_count > limits.max_states {
            return Err(RecoveryError::LimitExceeded("estados de historial"));
        }
        if undo_count > history_limit || redo_count > history_limit {
            return Err(invalid("límite de pasos del historial"));
        }
        decoder.charge_metadata(std::mem::size_of::<Document>())?;
        let original = decoder.arena(original_len)?;
        let added = decoder.arena(added_len)?;
        let state = decoder.state(original_len, added_len, next_revision)?;
        let mut undo = decoder.vector::<State>(undo_count)?;
        for _ in 0..undo_count {
            undo.push(decoder.state(original_len, added_len, next_revision)?);
        }
        let mut redo = decoder.vector::<State>(redo_count)?;
        for _ in 0..redo_count {
            redo.push(decoder.state(original_len, added_len, next_revision)?);
        }
        let last_edit = decoder.change()?;
        validate_last_change(&state, &redo, last_edit.as_ref(), next_revision)?;
        decoder.bytes(&mut magic)?;
        if &magic != END {
            return Err(invalid("cierre de documento"));
        }
        // Every retained transition must link the exact neighboring states.
        validate_transitions(&undo, &state, &redo, next_revision)?;
        let document = Self {
            original,
            added,
            state,
            undo,
            redo,
            saved_revision,
            next_revision,
            history_limit,
            history_bytes_limit,
            last_edit,
        };
        if document.history_bytes() > history_bytes_limit {
            return Err(invalid("cargo del historial excedido"));
        }
        Ok(document)
    }
}

struct Decoder<'a> {
    reader: &'a mut dyn Read,
    limits: RecoveryLimits,
    encoded: u64,
    metadata: usize,
    pieces: usize,
    edits: usize,
}
impl Decoder<'_> {
    fn bytes(&mut self, bytes: &mut [u8]) -> Result<(), RecoveryError> {
        self.encoded = self
            .encoded
            .checked_add(bytes.len() as u64)
            .ok_or(invalid("longitud codificada"))?;
        if self.encoded > self.limits.max_encoded_bytes {
            return Err(RecoveryError::LimitExceeded("bytes codificados"));
        }
        self.reader.read_exact(bytes)?;
        Ok(())
    }
    fn integer(&mut self) -> Result<u64, RecoveryError> {
        let mut value = [0u8; 8];
        self.bytes(&mut value)?;
        Ok(u64::from_le_bytes(value))
    }
    fn size(&mut self) -> Result<usize, RecoveryError> {
        usize::try_from(self.integer()?)
            .map_err(|_| invalid("entero no representable en esta plataforma"))
    }
    fn charge_metadata(&mut self, bytes: usize) -> Result<(), RecoveryError> {
        self.metadata = self
            .metadata
            .checked_add(bytes)
            .ok_or(invalid("tamaño de metadatos"))?;
        if self.metadata > self.limits.max_metadata_bytes {
            return Err(RecoveryError::LimitExceeded("metadatos"));
        }
        Ok(())
    }
    fn vector<T>(&mut self, count: usize) -> Result<Vec<T>, RecoveryError> {
        let bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or(invalid("reserva de metadatos"))?;
        self.charge_metadata(bytes)?;
        let mut result = Vec::new();
        result
            .try_reserve_exact(count)
            .map_err(|_| RecoveryError::Allocation)?;
        if result.capacity() > count {
            self.charge_metadata(
                (result.capacity() - count)
                    .checked_mul(std::mem::size_of::<T>())
                    .ok_or(invalid("capacidad de metadatos"))?,
            )?;
        }
        Ok(result)
    }
    fn arena(&mut self, len: usize) -> Result<Vec<u8>, RecoveryError> {
        if self
            .encoded
            .checked_add(len as u64)
            .is_none_or(|end| end > self.limits.max_encoded_bytes)
        {
            return Err(RecoveryError::LimitExceeded("arena codificada"));
        }
        // Read a real bounded fragment before growing. A truncated header that
        // claims a huge arena does not immediately allocate that claimed arena.
        let mut result = Vec::new();
        let mut scratch = [0u8; CHUNK];
        while result.len() < len {
            let count = (len - result.len()).min(CHUNK);
            self.bytes(&mut scratch[..count])?;
            let needed = result.len() + count;
            if needed > result.capacity() {
                let target = result.capacity().saturating_mul(2).max(needed).min(len);
                result
                    .try_reserve_exact(target - result.len())
                    .map_err(|_| RecoveryError::Allocation)?;
                if result.capacity() > len {
                    return Err(RecoveryError::LimitExceeded("capacidad de arena"));
                }
            }
            result.extend_from_slice(&scratch[..count]);
        }
        Ok(result)
    }
    fn change(&mut self) -> Result<Option<ChangeSet>, RecoveryError> {
        match self.integer()? {
            0 => Ok(None),
            1 => {
                let before_revision = self.integer()?;
                let after_revision = self.integer()?;
                let count = self.size()?;
                if count == 0 {
                    return Err(invalid("mapa de cambios vacío"));
                }
                self.edits = self
                    .edits
                    .checked_add(count)
                    .ok_or(invalid("cantidad de ediciones"))?;
                if self.edits > self.limits.max_edits {
                    return Err(RecoveryError::LimitExceeded("mapas de edición"));
                }
                let mut edits = self.vector::<ByteEdit>(count)?;
                for _ in 0..count {
                    let start = self.size()?;
                    let end = self.size()?;
                    let inserted_len = self.size()?;
                    if start > end || start == end && inserted_len == 0 {
                        return Err(invalid("rango de edición"));
                    }
                    edits.push(ByteEdit {
                        replaced: start..end,
                        inserted_len,
                    });
                }
                Ok(Some(ChangeSet {
                    before_revision,
                    after_revision,
                    edits,
                }))
            }
            _ => Err(invalid("etiqueta de mapa de cambios")),
        }
    }
    fn state(
        &mut self,
        original_len: usize,
        added_len: usize,
        next_revision: u64,
    ) -> Result<State, RecoveryError> {
        let len = self.size()?;
        let revision = self.integer()?;
        let count = self.size()?;
        if revision >= next_revision {
            return Err(invalid("revisión de estado"));
        }
        if len > self.limits.max_source_bytes {
            return Err(RecoveryError::LimitExceeded("longitud lógica de estado"));
        }
        self.pieces = self
            .pieces
            .checked_add(count)
            .ok_or(invalid("cantidad de piezas"))?;
        if self.pieces > self.limits.max_pieces {
            return Err(RecoveryError::LimitExceeded("piezas"));
        }
        let mut pieces = self.vector::<Piece>(count)?;
        let mut total = 0usize;
        for _ in 0..count {
            let source = match self.integer()? {
                0 => Source::Original,
                1 => Source::Added,
                _ => return Err(invalid("fuente de pieza")),
            };
            let start = self.size()?;
            let size = self.size()?;
            let piece = Piece {
                source,
                start,
                len: size,
            };
            validate_piece(&piece, original_len, added_len)?;
            total = total.checked_add(size).ok_or(invalid("suma de piezas"))?;
            pieces.push(piece);
        }
        if total != len {
            return Err(invalid("longitud de estado y piezas"));
        }
        let edit = self.change()?;
        let history_payload = state_change_payload(edit.as_ref(), len, revision, next_revision)?;
        // Rebuild index with exact logical counts, not serialized capacities.
        let mut blocks = self.vector::<usize>(pieces.len().div_ceil(PIECE_BLOCK))?;
        let mut local = self.vector::<usize>(pieces.len())?;
        let mut end = 0usize;
        for block in pieces.chunks(PIECE_BLOCK) {
            let mut subtotal = 0usize;
            for piece in block {
                subtotal = subtotal
                    .checked_add(piece.len)
                    .ok_or(invalid("prefijo de pieza"))?;
                local.push(subtotal);
            }
            end = end
                .checked_add(subtotal)
                .ok_or(invalid("prefijo de bloque"))?;
            blocks.push(end);
        }
        let ends = PieceIndex { blocks, local };
        let history_charge = std::mem::size_of::<State>()
            .checked_add(pieces.capacity() * std::mem::size_of::<Piece>())
            .and_then(|n| n.checked_add(ends.memory_bytes()))
            .and_then(|n| {
                n.checked_add(edit.as_ref().map_or(0, |change| {
                    change.edits.capacity() * std::mem::size_of::<ByteEdit>()
                }))
            })
            .ok_or(invalid("cargo de metadatos del historial"))?;
        Ok(State {
            pieces,
            ends,
            history_charge,
            history_payload,
            len,
            revision,
            edit,
        })
    }
}

fn validate_piece(
    piece: &Piece,
    original_len: usize,
    added_len: usize,
) -> Result<(), RecoveryError> {
    let limit = match piece.source {
        Source::Original => original_len,
        Source::Added => added_len,
    };
    if piece.len == 0
        || piece
            .start
            .checked_add(piece.len)
            .is_none_or(|end| end > limit)
    {
        return Err(invalid("límites de pieza"));
    }
    Ok(())
}
fn state_change_payload(
    change: Option<&ChangeSet>,
    len: usize,
    revision: u64,
    next_revision: u64,
) -> Result<usize, RecoveryError> {
    let mut payload = 0usize;
    if let Some(change) = change {
        if change.after_revision != revision {
            return Err(invalid("revisión final del mapa de estado"));
        }
        validate_change(change, len, next_revision, true)?;
        for edit in &change.edits {
            payload = payload
                .checked_add(edit.replaced.len())
                .and_then(|n| n.checked_add(edit.inserted_len))
                .ok_or(invalid("cargo de bytes del historial"))?;
        }
    }
    Ok(payload)
}
fn state_metadata_charge(state: &State) -> Result<usize, RecoveryError> {
    let mut bytes = std::mem::size_of::<State>();
    for (count, width) in [
        (state.pieces.len(), std::mem::size_of::<Piece>()),
        (state.pieces.len(), std::mem::size_of::<usize>()),
        (
            state.pieces.len().div_ceil(PIECE_BLOCK),
            std::mem::size_of::<usize>(),
        ),
        (
            state.edit.as_ref().map_or(0, |change| change.edits.len()),
            std::mem::size_of::<ByteEdit>(),
        ),
    ] {
        bytes = bytes
            .checked_add(
                count
                    .checked_mul(width)
                    .ok_or(invalid("cargo de metadatos del historial"))?,
            )
            .ok_or(invalid("cargo de metadatos del historial"))?;
    }
    Ok(bytes)
}
fn validate_last_change(
    state: &State,
    redo: &[State],
    last_edit: Option<&ChangeSet>,
    next_revision: u64,
) -> Result<(), RecoveryError> {
    if let Some(change) = last_edit {
        if change.after_revision != state.revision {
            return Err(invalid("último mapa no termina en la revisión actual"));
        }
        validate_change(change, state.len, next_revision, false)?;
        if change.before_revision < change.after_revision {
            if state.edit.as_ref() != Some(change) {
                return Err(invalid("último mapa directo no corresponde al estado"));
            }
        } else if let Some(undone) = redo.last() {
            let forward = undone
                .edit
                .as_ref()
                .ok_or(invalid("estado rehecho sin mapa"))?;
            if change.before_revision != forward.after_revision
                || change.after_revision != forward.before_revision
                || change.edits.len() != forward.edits.len()
                || change
                    .edits
                    .iter()
                    .zip(forward.edits.iter().rev())
                    .any(|(inverse, direct)| {
                        inverse.replaced.start != direct.replaced.start
                            || direct.replaced.start.checked_add(direct.inserted_len)
                                != Some(inverse.replaced.end)
                            || inverse.inserted_len != direct.replaced.len()
                    })
            {
                return Err(invalid("último mapa inverso no corresponde al historial"));
            }
        }
    }
    Ok(())
}
fn validate_transitions(
    undo: &[State],
    state: &State,
    redo: &[State],
    next_revision: u64,
) -> Result<(), RecoveryError> {
    let mut previous: Option<&State> = None;
    for next in undo
        .iter()
        .chain(std::iter::once(state))
        .chain(redo.iter().rev())
    {
        if let Some(previous) = previous {
            let change = next
                .edit
                .as_ref()
                .ok_or(invalid("transición de historial ausente"))?;
            if change.before_revision != previous.revision
                || previous.revision >= next.revision
                || validate_change(change, next.len, next_revision, true)? != previous.len
            {
                return Err(invalid("transición de historial incoherente"));
            }
        }
        previous = Some(next);
    }
    Ok(())
}
/// Validate the inverse sequence first to infer the old length, then replay the
/// original geometry. Last-change maps may be inverse/ascending after undo;
/// committed State.edit maps must retain their original rightmost-first order.
fn validate_change(
    change: &ChangeSet,
    after_len: usize,
    next_revision: u64,
    forward: bool,
) -> Result<usize, RecoveryError> {
    if change.before_revision >= next_revision
        || change.after_revision >= next_revision
        || change.before_revision == change.after_revision
        || change.edits.is_empty()
        || forward && change.before_revision >= change.after_revision
    {
        return Err(invalid("revisiones de mapa"));
    }
    if forward
        && change.edits.windows(2).any(|pair| {
            pair[0].replaced.start <= pair[1].replaced.start
                || pair[1].replaced.end > pair[0].replaced.start
        })
    {
        return Err(invalid("orden o solapamiento de ediciones"));
    }
    let mut before_len = after_len;
    for edit in change.edits.iter().rev() {
        if edit.replaced.start > edit.replaced.end
            || edit.replaced.start == edit.replaced.end && edit.inserted_len == 0
            || edit
                .replaced
                .start
                .checked_add(edit.inserted_len)
                .is_none_or(|end| end > before_len)
        {
            return Err(invalid("rango inverso del mapa"));
        }
        before_len = before_len
            .checked_sub(edit.inserted_len)
            .and_then(|n| n.checked_add(edit.replaced.len()))
            .ok_or(invalid("longitud inversa del mapa"))?;
    }
    let mut len = before_len;
    for edit in &change.edits {
        if edit.replaced.end > len {
            return Err(invalid("rango secuencial del mapa"));
        }
        len = len
            .checked_sub(edit.replaced.len())
            .and_then(|n| n.checked_add(edit.inserted_len))
            .ok_or(invalid("longitud secuencial del mapa"))?;
    }
    if len != after_len {
        return Err(invalid("longitud final del mapa"));
    }
    Ok(before_len)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn encode(document: &Document) -> Vec<u8> {
        let mut bytes = Vec::new();
        document.write_recovery(&mut bytes).unwrap();
        bytes
    }
    fn restore(mut bytes: &[u8]) -> Result<Document, RecoveryError> {
        Document::read_recovery(&mut bytes, RecoveryLimits::default())
    }
    fn same(left: &Document, right: &Document) {
        assert_eq!(left.to_bytes(), right.to_bytes());
        assert_eq!(left.revision(), right.revision());
        assert_eq!(left.is_modified(), right.is_modified());
        assert_eq!(left.undo_steps(), right.undo_steps());
        assert_eq!(left.redo_steps(), right.redo_steps());
        assert_eq!(left.history_limit(), right.history_limit());
        assert_eq!(left.history_bytes_limit(), right.history_bytes_limit());
        assert_eq!(left.last_change(), right.last_change());
    }
    #[test]
    fn recovery_roundtrip_keeps_binary_arenas_saved_revision_undo_redo_and_position_maps() {
        let mut document = Document::from_bytes(b"\xef\xbb\xbfa\xff\r\nb\r\x00\nfin".to_vec());
        document.insert(3, b"prefix").unwrap();
        document.mark_saved();
        document
            .apply_batch(EditBatch {
                expected_revision: document.revision(),
                edits: vec![
                    Replacement {
                        range: 0..1,
                        bytes: b"XY".to_vec(),
                    },
                    Replacement {
                        range: 10..12,
                        bytes: b"\xff\x00\r\n".to_vec(),
                    },
                ],
            })
            .unwrap();
        document.insert(document.len(), b"tail").unwrap();
        assert!(document.undo());
        let encoded = encode(&document);
        let mut recovered = restore(&encoded).unwrap();
        same(&document, &recovered);
        assert_eq!(encode(&recovered), encoded);
        assert_eq!(document.original, recovered.original);
        assert_eq!(document.added, recovered.added);
        while document.can_undo() {
            let old_len = document.len();
            assert!(document.undo());
            assert!(recovered.undo());
            same(&document, &recovered);
            for offset in 0..=old_len {
                assert_eq!(
                    document.last_change().unwrap().map_offset(offset),
                    recovered.last_change().unwrap().map_offset(offset)
                );
            }
        }
        while document.can_redo() {
            let old_len = document.len();
            assert!(document.redo());
            assert!(recovered.redo());
            same(&document, &recovered);
            for offset in 0..=old_len {
                assert_eq!(
                    document.last_change().unwrap().map_offset(offset),
                    recovered.last_change().unwrap().map_offset(offset)
                );
            }
        }
        assert!(!recovered.can_redo());
        document.insert(0, b"branch").unwrap();
        recovered.insert(0, b"branch").unwrap();
        same(&document, &recovered);
        assert_eq!(encode(&document), encode(&recovered));
    }
    #[test]
    fn recovery_restores_compacted_and_budget_trimmed_histories_without_inventing_a_dirty_edit() {
        let mut document = Document::from_bytes(b"base\r\n".to_vec());
        document.set_history_limit(2);
        for i in 0..8 {
            document.insert(0, &[i, 0xff]).unwrap();
        }
        document.mark_saved();
        assert!(document.undo());
        let recovered = restore(&encode(&document)).unwrap();
        same(&document, &recovered);
        assert_eq!(document.original, recovered.original);
        assert_eq!(document.added, recovered.added);
        document.compact_storage();
        document.mark_saved();
        let mut recovered = restore(&encode(&document)).unwrap();
        same(&document, &recovered);
        assert!(!recovered.is_modified());
        assert!(!recovered.undo());
        assert!(!recovered.redo());
        let mut no_history = Document::from_bytes(vec![0xff; 4096]);
        no_history.set_history_bytes_limit(0);
        no_history.delete(0..4096).unwrap();
        assert_eq!(no_history.original.len(), 4096); // Below collector slack; maps remain meaningful.
        let recovered = restore(&encode(&no_history)).unwrap();
        same(&no_history, &recovered);
        assert_eq!(recovered.len(), 0);
        assert!(!recovered.can_undo());
        let mut collected = Document::from_bytes(vec![0xff; 2 * CHUNK]);
        collected.set_history_bytes_limit(0);
        collected.delete(0..2 * CHUNK).unwrap();
        assert!(collected.original.is_empty());
        let recovered = restore(&encode(&collected)).unwrap();
        same(&collected, &recovered);
    }
    fn replace_integer(bytes: &mut [u8], at: usize, value: u64) {
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    #[test]
    fn recovery_rejects_truncation_extreme_counts_and_invalid_piece_or_map_geometry_without_panics()
    {
        let mut document = Document::from_bytes(b"abcdef\r\n\xff".to_vec());
        document.insert(1, b"XY").unwrap();
        let encoded = encode(&document);
        for end in 0..encoded.len() {
            let result = std::panic::catch_unwind(|| restore(&encoded[..end]));
            assert!(result.is_ok(), "panic at truncation{end}");
            assert!(result.unwrap().is_err(), "accepted truncation{end}");
        }
        let state = 80 + document.original.len() + document.added.len();
        let change = state + 24 + document.state.pieces.len() * 24;
        for (at, value) in [
            (8, 99),
            (16, u64::MAX),
            (24, u64::MAX),
            (32, document.next_revision),
            (40, 0),
            (64, u64::MAX),
            (72, u64::MAX),
            (state, u64::MAX),
            (state + 8, document.next_revision),
            (state + 16, u64::MAX),
            (state + 24, 2),
            (state + 32, u64::MAX),
            (state + 40, 0),
            (change, 3),
            (change + 16, 0),
            (change + 24, u64::MAX),
            (change + 32, u64::MAX),
        ] {
            let mut corrupt = encoded.clone();
            replace_integer(&mut corrupt, at, value);
            let result = std::panic::catch_unwind(|| restore(&corrupt));
            assert!(result.is_ok(), "panic at field{at}");
            assert!(result.unwrap().is_err(), "accepted field{at}={value}");
        }
        let mut wrong = encoded.clone();
        wrong[0] ^= 1;
        assert!(restore(&wrong).is_err());
        let mut wrong = encoded;
        let last = wrong.len() - 1;
        wrong[last] ^= 1;
        assert!(restore(&wrong).is_err());
        let limits = RecoveryLimits {
            max_source_bytes: 1,
            ..Default::default()
        };
        assert!(matches!(
            Document::read_recovery(&mut &wrong[..], limits),
            Err(RecoveryError::LimitExceeded(_))
        ));
        let limits = RecoveryLimits {
            max_metadata_bytes: 0,
            ..Default::default()
        };
        assert!(matches!(
            Document::read_recovery(&mut &wrong[..], limits),
            Err(RecoveryError::LimitExceeded(_))
        ));
    }
    struct Sink {
        written: usize,
        largest: usize,
        fail_after: usize,
    }
    impl Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.largest = self.largest.max(bytes.len());
            if self.written >= self.fail_after {
                return Err(io::Error::other("Fallo de escritura inyectado"));
            }
            let count = bytes.len().min(self.fail_after - self.written);
            self.written += count;
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn recovery_writes_sources_in_bounded_fragments_and_writer_failure_preserves_live_document() {
        let mut document = Document::from_bytes(vec![0xff; 3 * CHUNK + 17]);
        document.insert(3, b"\x00\r\n").unwrap();
        let revision = document.revision();
        let undo = document.undo_steps();
        let mut sink = Sink {
            written: 0,
            largest: 0,
            fail_after: usize::MAX,
        };
        document.write_recovery(&mut sink).unwrap();
        assert!(sink.largest <= CHUNK);
        assert!(sink.written > document.len());
        let mut failing = Sink {
            written: 0,
            largest: 0,
            fail_after: CHUNK + 80,
        };
        assert!(document.write_recovery(&mut failing).is_err());
        assert!(failing.largest <= CHUNK);
        assert_eq!(document.revision(), revision);
        assert_eq!(document.undo_steps(), undo);
        assert_eq!(
            document.read_range(0..8).unwrap(),
            b"\xff\xff\xff\x00\r\n\xff\xff"
        );
    }
    fn preflight_document() -> Document {
        let mut document = Document::from_bytes(b"base".to_vec());
        document.insert(1, b"XY").unwrap();
        document.insert(document.len(), b"Z").unwrap();
        assert!(document.undo());
        document
    }
    #[test]
    fn recovery_preflight_exact_counts_metadata_and_encoded_boundaries_match_the_reader() {
        let document = preflight_document();
        let bytes = encode(&document);
        assert_eq!(bytes.len(), 535);
        assert_eq!(document.recovery_encoded_len().unwrap(), bytes.len() as u64);
        let mut limits = RecoveryLimits {
            max_source_bytes: 7,
            max_states: 3,
            max_pieces: 8,
            max_edits: 3,
            max_encoded_bytes: 535,
            ..Default::default()
        };
        // Measure the reader's actual admission boundary rather than duplicate
        // the production accounting formula in an expected-value expression.
        let (mut low, mut high) = (0, limits.max_metadata_bytes);
        while low < high {
            let middle = low + (high - low) / 2;
            let candidate = RecoveryLimits {
                max_metadata_bytes: middle,
                ..limits
            };
            if Document::read_recovery(&mut &bytes[..], candidate).is_ok() {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        limits.max_metadata_bytes = low;
        document.validate_recovery_limits(limits).unwrap();
        assert!(Document::read_recovery(&mut &bytes[..], limits).is_ok());
        for kind in 0..6 {
            let mut smaller = limits;
            match kind {
                0 => smaller.max_source_bytes -= 1,
                1 => smaller.max_states -= 1,
                2 => smaller.max_pieces -= 1,
                3 => smaller.max_edits -= 1, // Includes last_edit as a real second serialized map.
                4 => smaller.max_metadata_bytes -= 1,
                _ => smaller.max_encoded_bytes -= 1,
            }
            assert!(
                document.validate_recovery_limits(smaller).is_err(),
                "preflight boundary{kind}"
            );
            assert!(
                Document::read_recovery(&mut &bytes[..], smaller).is_err(),
                "reader boundary{kind}"
            );
        }
        let mut sink = Sink {
            written: 0,
            largest: 0,
            fail_after: usize::MAX,
        };
        document.write_recovery(&mut sink).unwrap();
        assert_eq!(
            sink.written as u64,
            document.recovery_encoded_len().unwrap()
        );
    }
    #[test]
    fn recovery_preflight_zero_limits_and_reused_source_intervals_follow_logical_not_arena_sum() {
        let empty = Document::default();
        let limits = RecoveryLimits {
            max_source_bytes: 0,
            max_metadata_bytes: std::mem::size_of::<Document>(),
            max_states: 1,
            max_pieces: 0,
            max_edits: 0,
            max_encoded_bytes: 128,
        };
        empty.validate_recovery_limits(limits).unwrap();
        assert_eq!(empty.recovery_encoded_len().unwrap(), 128);
        assert!(Document::read_recovery(&mut &encode(&empty)[..], limits).is_ok());
        let zeros = RecoveryLimits {
            max_source_bytes: 0,
            max_metadata_bytes: 0,
            max_states: 0,
            max_pieces: 0,
            max_edits: 0,
            max_encoded_bytes: 0,
        };
        assert!(empty.validate_recovery_limits(zeros).is_err());
        assert!(Document::read_recovery(&mut &encode(&empty)[..], zeros).is_err());
        // Sharing one retained interval twice is valid piece-table geometry.
        // It must not require two physical copies of that source in recovery.
        let mut repeated = Document::from_bytes(b"abc".to_vec());
        repeated.state.pieces.push(Piece {
            source: Source::Original,
            start: 0,
            len: 3,
        });
        repeated.state.len = 6;
        repeated.state.ends = PieceIndex::build(&repeated.state.pieces).unwrap();
        let limits = RecoveryLimits {
            max_source_bytes: 6,
            ..Default::default()
        };
        repeated.validate_recovery_limits(limits).unwrap();
        let encoded = encode(&repeated);
        let restored = Document::read_recovery(&mut &encoded[..], limits).unwrap();
        assert_eq!(restored.to_bytes(), b"abcabc");
        assert_eq!(restored.original, b"abc");
        assert_eq!(
            encoded.len() as u64,
            repeated.recovery_encoded_len().unwrap()
        );
        let smaller = RecoveryLimits {
            max_source_bytes: 5,
            ..limits
        };
        assert!(repeated.validate_recovery_limits(smaller).is_err());
        assert!(Document::read_recovery(&mut &encoded[..], smaller).is_err());
    }
    #[test]
    fn recovery_preflight_rejects_invalid_live_geometry_and_restored_history_charge_before_writing()
    {
        let mut document = preflight_document();
        document.state.pieces[0].start = usize::MAX;
        assert!(document
            .validate_recovery_limits(Default::default())
            .is_err());
        assert!(restore(&encode(&document)).is_err());
        let mut document = preflight_document();
        document.state.edit.as_mut().unwrap().edits[0].inserted_len = 0;
        assert!(document
            .validate_recovery_limits(Default::default())
            .is_err());
        assert!(restore(&encode(&document)).is_err());
        let mut document = preflight_document();
        document.history_bytes_limit = 0;
        assert!(document
            .validate_recovery_limits(Default::default())
            .is_err());
        assert!(restore(&encode(&document)).is_err());
        let mut document = Document::from_bytes(vec![0xff; CHUNK * 2]);
        document.set_history_bytes_limit(0);
        document.delete(0..CHUNK * 2).unwrap();
        assert!(document.original.is_empty());
        let limits = RecoveryLimits {
            max_source_bytes: 0,
            ..Default::default()
        };
        // Last-change maps may refer to the old, collected length while the
        // current document and retained arenas are empty and fully recoverable.
        document.validate_recovery_limits(limits).unwrap();
        assert!(Document::read_recovery(&mut &encode(&document)[..], limits).is_ok());
    }
}
