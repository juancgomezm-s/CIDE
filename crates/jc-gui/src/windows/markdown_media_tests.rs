//! Directed native-model tests with owned images; no HWND, desktop or browser.
use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
struct Fixture {
    root: PathBuf,
    base: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .canonicalize()
            .unwrap();
        let base = workspace.join("work/test-temp");
        fs::create_dir_all(&base).unwrap();
        let base = base.canonicalize().unwrap();
        let root = base.join(format!(
            "markdown-media-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        assert_eq!(root.parent(), Some(base.as_path()));
        Self { root, base }
    }
    fn bmp(&self, name: &str, width: u32, height: u32) -> PathBuf {
        let size = (width * height * 4) as usize;
        let mut bytes = vec![0u8; 54 + size];
        bytes[..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&((54 + size) as u32).to_le_bytes());
        bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
        bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&(width as i32).to_le_bytes());
        bytes[22..26].copy_from_slice(&(-(height as i32)).to_le_bytes());
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&32u16.to_le_bytes());
        bytes[34..38].copy_from_slice(&(size as u32).to_le_bytes());
        for pixel in bytes[54..].chunks_exact_mut(4) {
            pixel.copy_from_slice(&[60, 120, 220, 255]);
        }
        let path = self.root.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
    fn state(&self, source: &str, width: i32, height: i32) -> State {
        let path = self.root.join("source.md");
        fs::write(&path, source.as_bytes()).unwrap();
        let mut state = State::new(&crate::GuiOptions {
            project_root: Some(self.root.clone()),
            encoding: Some(jc_text::TextEncoding::Utf8),
            ..Default::default()
        })
        .unwrap();
        state.width = width;
        state.height = height;
        state
            .workbench
            .open_project_file(
                &self.root,
                path,
                Some(jc_text::TextEncoding::Utf8),
                false,
                65536,
            )
            .unwrap();
        state
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(self.base.as_path()));
        assert!(self.root.starts_with(&self.base));
        assert!(!fs::symlink_metadata(&self.root)
            .unwrap()
            .file_type()
            .is_symlink());
        for entry in fs::read_dir(&self.root).unwrap() {
            let entry = entry.unwrap();
            let meta = fs::symlink_metadata(entry.path()).unwrap();
            assert!(meta.is_file() && !meta.file_type().is_symlink());
            fs::remove_file(entry.path()).unwrap();
        }
        fs::remove_dir(&self.root).unwrap();
    }
}
fn settle(state: &mut State) -> MarkdownFrame {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let _ = state.markdown_presentation().unwrap();
        state.poll_markdown();
        let frame = state.markdown_presentation().unwrap();
        let ui = state.markdown_gui.as_ref().unwrap();
        if ui.wanted == ui.sent && !state.markdown_images.loading() {
            return frame;
        }
        assert!(
            Instant::now() < deadline,
            "image load/reflow did not settle"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn click(state: &mut State, destination: &str, side: bool) -> Action {
    let frame = state.markdown_presentation().unwrap();
    let rect = frame
        .hits
        .actions
        .iter()
        .find(|(_, target)| target.destination == destination)
        .unwrap()
        .0;
    state.publish_markdown_hits(Some(frame.hits));
    state.markdown_click(
        (rect.left + rect.right) / 2,
        (rect.top + rect.bottom) / 2,
        side,
    )
}
#[test]
fn markdown_media_source_frame_releases_pixels_without_waiting_for_task_poll() {
    let fixture = Fixture::new();
    fixture.bmp("release.bmp", 320, 240);
    for change_tab in [false, true] {
        let mut state = fixture.state("![Carga](release.bmp)\n", 1000, 700);
        state.toggle_markdown();
        let frame = settle(&mut state);
        let pixels = Arc::downgrade(frame.images[0].decoded.as_ref().unwrap());
        drop(frame);
        assert!(pixels.upgrade().is_some());
        if change_tab {
            state
                .workbench
                .new_file("otro.txt", jc_text::TextEncoding::Utf8);
        } else {
            state.execute(jc_app::CommandId::ViewToggleBottomPanel);
            assert!(state.markdown_gui.is_none());
        }
        // No poll_markdown/poll_tasks between the command and replacement frame.
        assert!(state.markdown_presentation().is_none());
        assert!(pixels.upgrade().is_none(), "decoded pixels stayed retained");
        assert!(!state.markdown_images.loading());
        assert!(state.markdown_presentation().is_none());
    }
}
#[test]
fn markdown_media_reflow_retains_geometry_after_payload_eviction_without_reload_loop() {
    let fixture = Fixture::new();
    fixture.bmp("a.bmp", 80, 400);
    let b = fixture.bmp("b.bmp", 80, 400);
    let mut state = fixture.state("![A](a.bmp)\n![B](b.bmp)\n", 1000, 450);
    state.toggle_markdown();
    let initial = state.markdown_presentation().unwrap();
    assert_eq!(initial.images.len(), 2);
    let before = state.workbench.active_bytes().unwrap();
    let frame = settle(&mut state);
    assert!(!frame.images.is_empty());
    assert!(state
        .markdown_gui
        .as_ref()
        .unwrap()
        .extents
        .iter()
        .any(|(source, _, _)| source == &ImageSource::Local(b.clone())));
    let wanted = state.markdown_gui.as_ref().unwrap().wanted.clone();
    let heights: Vec<_> = frame.images.iter().map(|image| image.height).collect();
    for _ in 0..10 {
        let frame = state.markdown_presentation().unwrap();
        state.poll_markdown();
        assert_eq!(state.markdown_gui.as_ref().unwrap().wanted, wanted);
        assert_eq!(
            frame
                .images
                .iter()
                .map(|image| image.height)
                .collect::<Vec<_>>(),
            heights
        );
        assert!(!state.markdown_images.loading());
    }
    assert_eq!(state.workbench.active_bytes().unwrap(), before);
}
#[test]
fn markdown_media_inline_images_and_compact_first_row_remain_visible_and_actionable() {
    let fixture = Fixture::new();
    fixture.bmp("a.bmp", 100, 300);
    fixture.bmp("b.bmp", 100, 300);
    let mut state = fixture.state("![A](a.bmp) ![B](b.bmp)\n", 1000, 240);
    state.toggle_markdown();
    let initial = state.markdown_presentation().unwrap();
    // A final blank source line may also fit; the image-bearing first row must
    // remain visible rather than disappearing in the compact viewport.
    assert_eq!(
        initial.presentation.rows.first().map(|row| row.source_line),
        Some(0)
    );
    assert_eq!(initial.images.len(), 2);
    assert_eq!(state.markdown_gui.as_ref().unwrap().wanted.len(), 2);
    assert_eq!(initial.hits.targets().len(), 2);
    let frame = settle(&mut state);
    assert_eq!(frame.images.len(), 2);
    assert!(frame.images.iter().all(|image| image.decoded.is_some()));
    assert!(frame
        .hits
        .actions
        .iter()
        .all(|(rect, _)| rect.top < rect.bottom && rect.bottom <= state.height));
    assert!(frame.images.iter().map(|image| image.height).sum::<i32>() <= state.height);
}
#[test]
fn markdown_media_web_hits_return_intentions_and_reject_stale_geometry_without_launch() {
    let fixture = Fixture::new();
    let mut state = fixture.state("[Web](https://example.com/guia)\n", 1000, 650);
    state.toggle_markdown();
    let before = state.workbench.active_bytes().unwrap();
    assert!(
        matches!(click(&mut state,"https://example.com/guia",false),Action::OpenWeb(url) if url=="https://example.com/guia")
    );
    let frame = state.markdown_presentation().unwrap();
    let rect = frame.hits.actions[0].0;
    state.publish_markdown_hits(Some(frame.hits));
    state.width += 1;
    assert!(matches!(
        state.markdown_click(rect.left + 1, rect.top + 1, false),
        Action::None
    ));
    assert_eq!(state.workbench.active_bytes().unwrap(), before);
}
#[test]
fn markdown_media_cli_path_loads_image_and_opens_relative_link() {
    let fixture = Fixture::new();
    fixture.bmp("sample.bmp", 24, 24);
    fs::write(fixture.root.join("target.md"), b"# Destino\n").unwrap();
    let source = "[Otro](target.md)\n\n![Imagen](sample.bmp)\n";
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let path = workspace
        .join("work/test-temp")
        .join(fixture.root.file_name().unwrap())
        .join("source.md");
    fs::write(&path, source).unwrap();
    let mut state = State::new(&crate::GuiOptions {
        paths: vec![path],
        project_root: Some(fixture.root.clone()),
        encoding: Some(jc_text::TextEncoding::Utf8),
        ..Default::default()
    })
    .unwrap();
    state.toggle_markdown();
    let frame = settle(&mut state);
    assert_eq!(
        frame
            .images
            .iter()
            .filter(|image| image.decoded.is_some())
            .count(),
        1
    );
    assert!(matches!(click(&mut state, "target.md", true), Action::None));
    assert_eq!(state.workbench.panes().active(), jc_app::PaneId::Secondary);
    assert_eq!(state.workbench.active_bytes().unwrap(), b"# Destino\n");
    assert_eq!(
        fs::read(fixture.root.join("source.md")).unwrap(),
        source.as_bytes()
    );
}

#[test]
fn markdown_media_local_side_link_and_anchor_preserve_dirty_source_and_original_files() {
    use jc_app::PaneId;
    let fixture = Fixture::new();
    let target = fixture.root.join("target.md");
    let target_bytes = b"# Inicio\nTexto\n# Final\nFin\n";
    fs::write(&target, target_bytes).unwrap();
    let source = "[Otro](target.md#final)\n";
    let mut state = fixture.state(source, 1000, 650);
    state.workbench.insert_text("sin guardar\n").unwrap();
    state
        .workbench
        .go_to_position(jc_text::TextPosition { line: 0, column: 0 })
        .unwrap();
    let original = state.workbench.active_document().unwrap().id;
    let original_tab = state.workbench.active_document().unwrap().tab_id;
    let before = state.workbench.active_bytes().unwrap();
    let revision = state
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision();
    state.toggle_markdown();
    assert!(matches!(
        click(&mut state, "target.md#final", true),
        Action::None
    ));
    state.poll_markdown();
    let frame = state.markdown_presentation().unwrap();
    assert_eq!(frame.presentation.rows.first().unwrap().source_line, 2);
    assert_eq!(state.workbench.panes().active(), PaneId::Secondary);
    assert_eq!(
        state
            .workbench
            .panes()
            .view(PaneId::Primary)
            .unwrap()
            .location()
            .unwrap()
            .tab,
        original_tab
    );
    let original = state.workbench.document_by_id(original).unwrap();
    assert_eq!(
        original
            .session
            .document()
            .read_range(0..original.session.document().len())
            .unwrap(),
        before
    );
    assert_eq!(original.session.document().revision(), revision);
    assert_eq!(
        fs::read(fixture.root.join("source.md")).unwrap(),
        source.as_bytes()
    );
    assert_eq!(fs::read(target).unwrap(), target_bytes);
}

#[test]
fn markdown_media_native_image_raster_preserves_document_and_indentation() {
    let fixture = Fixture::new();
    let image = fixture.bmp("muestra.bmp", 240, 100);
    let mut bytes = fs::read(&image).unwrap();
    for (n, pixel) in bytes[54..].chunks_exact_mut(4).enumerate() {
        let x = n % 240;
        let y = n / 240;
        pixel.copy_from_slice(&[(x * 255 / 239) as u8, (y * 255 / 99) as u8, 190, 255]);
    }
    fs::write(&image, bytes).unwrap();
    let source="# Markdown con imágenes y enlaces\n\n- Nivel principal\n    - Segundo nivel\n        - Tercer nivel con texto que conserva su sangría\n\n[Enlace web](https://example.com) y [archivo al lado](destino.md).\n\n![Imagen local](muestra.bmp)\n";
    let mut state = fixture.state(source, 1100, 850);
    state.toggle_markdown();
    let frame = settle(&mut state);
    assert_eq!(
        frame
            .images
            .iter()
            .filter(|image| image.decoded.is_some())
            .count(),
        1
    );
    let before = state.workbench.active_bytes().unwrap();
    let full = super::super::paint::prepare(&mut state);
    let folder =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/markdown-media-review-20261106");
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join(format!("native-media-{}.bmp", std::process::id()));
    unsafe {
        super::super::paint::bitmap(null_mut(), &full, &path).unwrap();
    }
    assert_eq!(state.workbench.active_bytes().unwrap(), before);
    println!("Native Markdown media raster: {}", path.display());
}
