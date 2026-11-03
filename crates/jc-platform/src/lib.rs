//! No editor, UI framework, external dependency, startup monitor or process.
//! Platform transports run only in response to explicit frontend actions.
#[cfg(windows)]
pub mod clipboard;
#[cfg(windows)]
mod clipboard_owner;
#[cfg(windows)]
pub use clipboard_owner::WindowsClipboard;
