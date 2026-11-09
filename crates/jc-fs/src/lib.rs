//! Bounded, byte-preserving filesystem captures without editor dependencies.
mod scoped_read;
pub use scoped_read::{read_in_root, verify_file_in_root};
mod capture;
pub use capture::{
    capture_in_root, create_missing_parents, inspect_tree, pin_tree_parents, remove_captured_file,
    remove_empty_directory, stage_existing_file, stage_missing_file, Capture, DirectoryToken,
    FileToken, MissingToken, PendingExistingFile, PendingMissingFile, ReplaceError, TreeNode,
    TreeParents,
};
