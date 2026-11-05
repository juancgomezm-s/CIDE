//! Names derive from the live catalog, so a recovered historical counter cannot
//! keep advancing the first draft in an otherwise empty session.
use jc_app::Workbench;
use std::collections::BTreeSet;

pub(super) fn next_name(workbench: &Workbench) -> String {
    let occupied: BTreeSet<usize> = workbench
        .tabs()
        .tabs()
        .iter()
        .filter_map(|tab| {
            let document = workbench.document_by_tab(tab.id)?;
            let name = document
                .session
                .path()
                .file_name()?
                .to_str()?
                .to_lowercase();
            name.strip_prefix("sin título ")?
                .strip_suffix(".txt")?
                .parse()
                .ok()
        })
        .collect();
    let next = (1..=occupied.len() + 1)
        .find(|number| !occupied.contains(number))
        .unwrap();
    format!("Sin título {next}.txt")
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_text::TextEncoding;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn closing_all_drafts_starts_numbering_again_at_one() {
        let mut app = Workbench::default();
        for expected in ["Sin título 1.txt", "Sin título 2.txt"] {
            assert_eq!(next_name(&app), expected);
            app.new_file(expected, TextEncoding::Utf8);
        }
        app.close_active(true).unwrap();
        app.close_active(true).unwrap();
        assert_eq!(next_name(&app), "Sin título 1.txt");
    }

    #[test]
    fn absolute_live_draft_names_are_reserved_and_holes_are_reused_without_edits() {
        let mut app = Workbench::default();
        let base = std::env::current_dir().unwrap();
        app.new_file(base.join("Sin título 1.txt"), TextEncoding::Utf8);
        app.insert_text("borrador recuperado\r\n").unwrap();
        app.new_file(base.join("Sin título 3.txt"), TextEncoding::Utf8);
        app.insert_text("otro borrador\r\n").unwrap();
        assert_eq!(next_name(&app), "Sin título 2.txt");
        assert_eq!(app.active_bytes().unwrap(), b"otro borrador\r\n");
        app.undo().unwrap();
        assert_eq!(app.active_bytes().unwrap(), b"");
        app.select_tab(1);
        assert_eq!(app.active_bytes().unwrap(), b"borrador recuperado\r\n");
    }

    #[test]
    fn saved_catalog_paths_also_reserve_their_display_names() {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../work/test-temp")
            .canonicalize()
            .unwrap();
        let folder = root.join(format!(
            "untitled-numbers-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&folder).unwrap();
        let source = folder.join("SIN TÍTULO 1.TXT");
        std::fs::write(&source, b"guardado\r\n").unwrap();
        let mut app = Workbench::default();
        app.open_file(&source, Some(TextEncoding::Utf8)).unwrap();
        assert!(!app.active_document().unwrap().session.is_new());
        assert_eq!(next_name(&app), "Sin título 2.txt");
        assert_eq!(std::fs::read(&source).unwrap(), b"guardado\r\n");
        drop(app);
        std::fs::remove_file(source).unwrap();
        std::fs::remove_dir(folder).unwrap();
    }
}
