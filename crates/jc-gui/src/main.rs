#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// Shared entry keeps the historical binary alias behavior-compatible.
#[path = "entry.rs"]
mod entry;
fn main() {
    entry::run();
}
