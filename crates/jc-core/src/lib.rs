//! A small, encoding-agnostic document core.
//!
//! All positions and ranges use **byte offsets**, never Unicode character,
//! UTF-16, grapheme, screen-column, or compiler coordinates. The caller is
//! responsible for interpreting bytes using the document's actual code page.
//! Editing does not decode, normalize line endings, insert a BOM, or add a final
//! newline. Source bytes are interpreted only by callers. Undo/redo retain
//! piece metadata rather than whole text copies; collection relocates reachable
//! source intervals without changing content, revisions or retained history.
//!
//! Pieces use a vector with a block prefix index for O(log P) range lookup.
//! Edits still rebuild piece metadata. History has step and logical byte limits;
//! those quotas are distinct from process RAM or the live document size.

mod document;
mod region;
pub mod source;

pub use document::{
    ByteEdit, ChangeSet, Document, DocumentRead, EditBatch, EditError, HistoryTransition,
    RangeChunks, RecoveryCursor, RecoveryError, RecoveryLimits, Replacement,
};
pub use region::Region;
