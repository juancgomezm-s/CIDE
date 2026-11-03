//! Fixture-only inspector for native process restart acceptance. No original I/O.
use jc_app::Workbench;
use jc_files::recovery::RecoveryStore;
use std::path::{Path, PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .canonicalize()?;
    let root = PathBuf::from(std::env::args_os().nth(1).ok_or("Falta depósito fixture")?)
        .canonicalize()?;
    if !root.starts_with(workspace.join("work")) {
        return Err("El inspector sólo admite fixtures propios bajo work".into());
    }
    let mut store = RecoveryStore::open(&root, Default::default())?;
    let (bench, _, backup) =
        Workbench::restore_recovery(&mut store)?.ok_or("No hay sesión recuperable")?;
    let editor = bench
        .active_editor()
        .ok_or("No hay documento activo recuperado")?;
    let document = editor.session.document();
    if document.len() > 65536 {
        return Err("El inspector admite sólo fuentes fixture de64KiB".into());
    }
    let mut hex = String::new();
    for chunk in document.range_chunks(0..document.len())? {
        for byte in chunk {
            use std::fmt::Write;
            write!(hex, "{byte:02x}")?;
        }
    }
    println!("{{\"bytes_hex\":\"{}\",\"dirty\":{},\"undo\":{},\"redo\":{},\"cursor\":{},\"tabs\":{},\"backup\":{}}}",hex,document.is_modified(),document.undo_steps(),document.redo_steps(),editor.cursor,bench.tabs().tabs().len(),backup);
    Ok(())
}
