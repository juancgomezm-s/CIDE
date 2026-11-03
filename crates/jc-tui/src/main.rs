// Shared entry keeps the historical binary alias behavior-compatible.
#[path = "entry.rs"]
mod entry;
fn main() {
    entry::run();
}
