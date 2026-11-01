//! A small, encoding-agnostic document core.
//!
//! All positions and ranges use **byte offsets**, never Unicode character,
//! UTF-16, grapheme, screen-column, or compiler coordinates. The caller is
//! responsible for interpreting bytes using the document's actual code page.
//! Editing does not decode, normalize line endings, insert a BOM, or add a final
//! newline. The original bytes remain immutable; inserts use an append-only
//! buffer. Undo/redo retain piece metadata rather than whole text copies.
//!
//! This first implementation uses a linear piece vector, not a balanced tree.
//! Its lookup/edit complexity is O(number of pieces); retained edit bytes and
//! history are currently unbounded. These are deliberate, documented limits of
//! the initial core, not claims of production editor performance.

mod document;
mod region;

pub use document::{Document, EditError};
pub use region::Region;
