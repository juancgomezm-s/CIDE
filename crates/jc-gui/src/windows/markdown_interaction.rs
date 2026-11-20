//! Versioned Markdown links and image layout; events/poll alone admit work.
use super::*;
use jc_app::markdown_preview::{resolve_destination, MarkdownAnchorProgress, MarkdownDestination};
#[derive(Clone)]
pub(in crate::windows) struct MarkdownHits {
    token: Rc<()>,
    stamp: TaskStamp,
    width: i32,
    height: i32,
    viewport_left: i32,
    dpi: super::super::dpi::Dpi,
    pub(super) actions: Vec<(Rect, Arc<MarkdownTarget>)>,
}
impl MarkdownHits {
    pub(super) fn new(
        token: Rc<()>,
        stamp: TaskStamp,
        width: i32,
        height: i32,
        dpi: super::super::dpi::Dpi,
        viewport_left: i32,
    ) -> Self {
        Self {
            token,
            stamp,
            width,
            height,
            viewport_left,
            dpi,
            actions: Vec::new(),
        }
    }
    fn current(&self, state: &State) -> bool {
        state.selector.is_none()
            && state.snippet_gui.is_none()
            && !state.source_visible()
            && !state.open_diff_visible
            && state.git_panel.is_none()
            && super::super::outline_panel::editor_horizontal(state)
                == (self.viewport_left, self.width)
            && state.height == self.height
            && state.dpi == self.dpi
            && state.workbench.task_stamp_is_current(self.stamp)
            && state.markdown_gui.as_ref().is_some_and(|ui| {
                Rc::ptr_eq(&ui.token, &self.token)
                    && ui.project == state.project_root
                    && state
                        .workbench
                        .active_document()
                        .is_some_and(|d| d.id == self.stamp.document)
            })
    }
    pub(super) fn targets(&self) -> Vec<Arc<MarkdownTarget>> {
        let mut result: Vec<Arc<MarkdownTarget>> = Vec::new();
        for (_, target) in &self.actions {
            if !result.iter().any(|t| Arc::ptr_eq(t, target)) {
                result.push(target.clone());
            }
        }
        result
    }
}
pub(super) struct RowImage {
    pub row: usize,
    pub target: Arc<MarkdownTarget>,
    pub source: Option<ImageSource>,
    pub decoded: Option<Arc<DecodedImage>>,
    pub message: String,
    pub height: i32,
    pub allowed: bool,
}
pub(super) struct ImageLayout<'a> {
    pub cache: &'a super::super::markdown_images::ImageController,
    pub context: &'a ImageContext,
    pub document: &'a std::path::Path,
    pub remote_allowed: &'a [String],
    pub extents: &'a [(ImageSource, u32, u32)],
    pub width: i32,
    pub dpi: super::super::dpi::Dpi,
    pub max_group_height: i32,
}
pub(super) fn prepare_images(
    presentation: &MarkdownPresentation,
    layout: ImageLayout<'_>,
) -> Vec<RowImage> {
    let ImageLayout {
        cache,
        context,
        document,
        remote_allowed,
        extents,
        width,
        dpi,
        max_group_height,
    } = layout;
    let mut images: Vec<RowImage> = Vec::new();
    for (row_index, row) in presentation.rows.iter().enumerate() {
        for target in row
            .spans
            .iter()
            .filter_map(|s| s.target.as_ref())
            .filter(|t| t.image)
        {
            if images.len() == 16 {
                break;
            }
            if images
                .iter()
                .any(|image| Arc::ptr_eq(&image.target, target))
            {
                continue;
            }
            let resolved = resolve_destination(document, &target.destination);
            let (source, allowed, mut message) = match resolved {
                Ok(MarkdownDestination::External(url)) => {
                    let allowed = remote_allowed.contains(&url);
                    (
                        Some(ImageSource::Remote(url)),
                        allowed,
                        if allowed {
                            "Cargando imagen remota…"
                        } else {
                            "Imagen remota · pulsa para cargar"
                        }
                        .into(),
                    )
                }
                Ok(MarkdownDestination::Local { path, .. }) if path.starts_with(&context.root) => (
                    Some(ImageSource::Local(path)),
                    true,
                    "Cargando imagen…".into(),
                ),
                Ok(MarkdownDestination::Local { .. }) => (
                    None,
                    false,
                    "Imagen fuera de la carpeta del proyecto".into(),
                ),
                Ok(MarkdownDestination::Anchor(_)) => (
                    None,
                    false,
                    "La imagen necesita un archivo o una dirección web".into(),
                ),
                Err(error) => (None, false, error),
            };
            let decoded = source
                .as_ref()
                .and_then(|s| cache.get(s))
                .and_then(|entry| match &entry.result {
                    Ok(image) => Some(image.clone()),
                    Err(error) => {
                        message = error.clone();
                        None
                    }
                });
            let dimensions = decoded
                .as_ref()
                .map(|image| (image.width, image.height))
                .or_else(|| {
                    source.as_ref().and_then(|source| {
                        extents
                            .iter()
                            .find(|(key, _, _)| key == source)
                            .map(|(_, w, h)| (*w, *h))
                    })
                });
            let height = dimensions.map_or(dpi.pixels(72), |(image_width, image_height)| {
                let available = (width.min(dpi.pixels(1000)) - dpi.pixels(96)).max(1);
                let w = (image_width as i32).min(available);
                let h = (u64::from(image_height) * w as u64 / u64::from(image_width)).max(1) as i32;
                h.min(dpi.pixels(280)) + dpi.pixels(12)
            });
            images.push(RowImage {
                row: row_index,
                target: target.clone(),
                source,
                decoded,
                message,
                height,
                allowed,
            });
        }
    }
    for index in 0..images.len() {
        let row = images[index].row;
        let count = images.iter().filter(|image| image.row == row).count() as i32;
        images[index].height = images[index].height.min((max_group_height / count).max(1));
    }
    images
}
impl MarkdownFrame {
    pub(super) fn content_bounds(&self) -> (i32, i32) {
        let available = (self.width - self.viewport_left).max(0);
        let width = available.min(self.dpi.pixels(1000));
        let margin = (available - width) / 2 + self.dpi.pixels(40);
        let left = (self.viewport_left + margin).min(self.width);
        (left, (self.width - margin).max(left))
    }
    pub(super) fn prepare_hits(&mut self) {
        // Font metrics are prepared with the scene; painting has no mutable model.
        let dc = unsafe { CreateCompatibleDC(null_mut()) };
        if dc.is_null() {
            return;
        }
        let (left, right) = self.content_bounds();
        let p = |v| self.dpi.pixels(v);
        let mut y = self.top + p(48);
        for (row_index, row) in self.presentation.rows.iter().enumerate() {
            let h = row_height(row, self.dpi)
                + self
                    .images
                    .iter()
                    .filter(|i| i.row == row_index)
                    .map(|i| i.height)
                    .sum::<i32>();
            let mut x = left
                + match row.block {
                    MarkdownBlock::Code { .. } => p(12),
                    MarkdownBlock::Quote => p(16),
                    _ => 0,
                };
            if !matches!(
                row.block,
                MarkdownBlock::Table { .. } | MarkdownBlock::Rule | MarkdownBlock::Blank
            ) {
                for (index, span) in row.spans.iter().enumerate() {
                    let text = span_text(row, index, span);
                    let value = wide(text);
                    let mut size = TextSize::default();
                    unsafe {
                        SelectObject(dc, span_font(&self.fonts, row, span));
                        GetTextExtentPoint32W(
                            dc,
                            value.as_ptr(),
                            (value.len() - 1) as i32,
                            &mut size,
                        );
                    }
                    if let Some(target) = &span.target {
                        if self.hits.actions.len() < 512 && x < right {
                            self.hits.actions.push((
                                area(
                                    x,
                                    y + p(3),
                                    (x + size.width).min(right),
                                    (y + row_height(row, self.dpi)).min(self.height - self.ch),
                                ),
                                target.clone(),
                            ));
                        }
                    }
                    x = (x + size.width).min(right);
                }
            }
            let mut image_top = y + row_height(row, self.dpi);
            for image in self.images.iter().filter(|image| image.row == row_index) {
                if self.hits.actions.len() < 512 {
                    self.hits.actions.push((
                        area(left, image_top, right, image_top + image.height),
                        image.target.clone(),
                    ));
                }
                image_top += image.height;
            }
            y += h;
        }
        unsafe {
            DeleteDC(dc);
        }
    }
}
#[link(name = "gdi32")]
extern "system" {
    fn StretchDIBits(
        dc: Hdc,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        bits: *const std::ffi::c_void,
        info: *const BitmapInfo,
        usage: u32,
        operation: u32,
    ) -> i32;
}
pub(super) unsafe fn draw_image(
    dc: Hdc,
    frame: &MarkdownFrame,
    image: &RowImage,
    left: i32,
    right: i32,
    top: i32,
) -> Result<(), String> {
    let p = |v| frame.dpi.pixels(v);
    let bottom = top + image.height - p(6);
    if let Some(decoded) = &image.decoded {
        let w = (decoded.width as i32).min((right - left - p(24)).max(1));
        let h = (u64::from(decoded.height) * w as u64 / u64::from(decoded.width)).max(1) as i32;
        let h = h.min((image.height - p(12)).max(1));
        let w = (u64::from(decoded.width) * h as u64 / u64::from(decoded.height)).max(1) as i32;
        let info = BitmapInfo {
            header: BitmapHeader {
                size: 40,
                width: decoded.width as i32,
                height: -(decoded.height as i32),
                planes: 1,
                bit_count: 32,
                compression: 0,
                image_size: decoded.bgra.len() as u32,
                x_meter: 0,
                y_meter: 0,
                used: 0,
                important: 0,
            },
            color: 0,
        };
        if StretchDIBits(
            dc,
            left + p(12),
            top + p(3),
            w,
            h,
            0,
            0,
            decoded.width as i32,
            decoded.height as i32,
            decoded.bgra.as_ptr().cast(),
            &info,
            0,
            0x00cc0020,
        ) == 0
        {
            return Err("No se pudo dibujar la imagen Markdown".into());
        }
    } else {
        paint::fill(
            dc,
            area(left, top, right, bottom),
            super::super::shell::SIDEBAR,
        )?;
        SelectObject(dc, frame.fonts.body);
        paint::text(
            dc,
            area(left + p(12), top + p(16), right - p(12), bottom),
            &wide(&image.message),
            super::super::shell::MUTED,
            super::super::shell::SIDEBAR,
        )?;
    }
    Ok(())
}
impl State {
    pub(in crate::windows) fn publish_markdown_hits(&mut self, hits: Option<MarkdownHits>) {
        if let Some(ui) = &mut self.markdown_gui {
            ui.hits = hits;
        }
    }
    pub(in crate::windows) fn markdown_click(&mut self, x: i32, y: i32, to_side: bool) -> Action {
        let target = self
            .markdown_gui
            .as_ref()
            .and_then(|ui| ui.hits.as_ref())
            .filter(|h| h.current(self))
            .and_then(|h| {
                h.actions
                    .iter()
                    .find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
                    .map(|(_, t)| t.clone())
            });
        target.map_or(Action::None, |target| {
            self.activate_markdown_target(target, to_side)
        })
    }
    pub(super) fn markdown_select_link(&mut self, backwards: bool) {
        let Some(hits) = self
            .markdown_gui
            .as_ref()
            .and_then(|ui| ui.hits.as_ref())
            .filter(|h| h.current(self))
        else {
            return;
        };
        let total = hits.targets().len();
        if total == 0 {
            return;
        }
        let ui = self.markdown_gui.as_mut().unwrap();
        ui.selected = if !ui.keyboard_focus {
            if backwards {
                total - 1
            } else {
                0
            }
        } else if backwards {
            (ui.selected + total - 1) % total
        } else {
            (ui.selected + 1) % total
        };
        ui.keyboard_focus = true;
        self.status = format!(
            "Enlace {}/{} · Intro abre · Ctrl+Intro abre al lado",
            ui.selected + 1,
            total
        );
    }
    pub(super) fn markdown_activate_selected(&mut self, to_side: bool) -> Action {
        let target = self
            .markdown_gui
            .as_ref()
            .filter(|ui| ui.keyboard_focus)
            .and_then(|ui| {
                ui.hits
                    .as_ref()
                    .filter(|h| h.current(self))
                    .and_then(|h| h.targets().get(ui.selected).cloned())
            });
        target.map_or(Action::None, |target| {
            self.activate_markdown_target(target, to_side)
        })
    }
    fn activate_markdown_target(&mut self, target: Arc<MarkdownTarget>, to_side: bool) -> Action {
        let Some(ui) = self.markdown_gui.as_ref() else {
            return Action::None;
        };
        if !self.workbench.task_stamp_is_current(ui.stamp) {
            self.error("El documento cambió; actualiza la vista Markdown");
            return Action::None;
        }
        let root = ui.context.root.clone();
        match resolve_destination(&ui.document_path, &target.destination) {
            Ok(MarkdownDestination::External(url)) => {
                if let Err(error) = jc_platform::web_link::validate(&url) {
                    self.error(error);
                    return Action::None;
                }
                if target.image {
                    let ui = self.markdown_gui.as_mut().unwrap();
                    if !ui.remote_allowed.contains(&url) {
                        if ui.remote_allowed.len() == 16 {
                            self.error("Límite de16 imágenes remotas por vista");
                            return Action::None;
                        }
                        ui.remote_allowed.push(url);
                    }
                    self.markdown_images.cancel();
                    ui.sent.clear();
                    ui.token = Rc::new(());
                    ui.hits = None;
                    self.status = "Cargando imagen remota en segundo plano…".into();
                    Action::None
                } else {
                    Action::OpenWeb(url)
                }
            }
            Ok(MarkdownDestination::Anchor(fragment)) if !target.image => {
                let ui = self.markdown_gui.as_mut().unwrap();
                ui.anchor = Some(MarkdownAnchorScan::new(ui.stamp, fragment));
                self.wakeup.signal();
                self.status = "Buscando encabezado…".into();
                Action::None
            }
            Ok(MarkdownDestination::Local { path, fragment }) if !target.image => {
                match self.workbench.open_project_file(
                    &root,
                    path,
                    self.encoding,
                    to_side,
                    16 * 1024 * 1024,
                ) {
                    Ok(_) => {
                        self.markdown_gui = None;
                        self.markdown_images.cancel();
                        self.status = "Enlace local abierto; fuente sin cambios".into();
                        if let Some(fragment) = fragment.filter(|f| !f.is_empty()) {
                            self.toggle_markdown();
                            if let Some(ui) = &mut self.markdown_gui {
                                ui.anchor = Some(MarkdownAnchorScan::new(ui.stamp, fragment));
                                self.wakeup.signal();
                            }
                        }
                    }
                    Err(error) => self.error(error),
                }
                Action::None
            }
            Ok(_) => {
                self.status = "Imagen local · vista de lectura".into();
                Action::None
            }
            Err(error) => {
                self.error(error);
                Action::None
            }
        }
    }
    pub(in crate::windows) fn poll_markdown(&mut self) -> bool {
        let Some(ui) = self.markdown_gui.as_mut() else {
            self.markdown_images.cancel();
            return false;
        };
        if !self.workbench.task_stamp_is_current(ui.stamp) || ui.project != self.project_root {
            self.markdown_images.cancel();
            ui.anchor = None;
            ui.hits = None;
            return false;
        }
        let mut changed = self.markdown_images.poll(&ui.context);
        if changed {
            for source in &ui.sent {
                if let Some(image) = self
                    .markdown_images
                    .get(source)
                    .and_then(|entry| entry.result.as_ref().ok())
                {
                    if let Some((_, w, h)) = ui.extents.iter_mut().find(|(key, _, _)| key == source)
                    {
                        *w = image.width;
                        *h = image.height;
                    } else {
                        if ui.extents.len() == 64 {
                            ui.extents.remove(0);
                        }
                        ui.extents.push((source.clone(), image.width, image.height));
                    }
                }
            }
            ui.token = Rc::new(());
            ui.hits = None;
        }
        if ui.wanted != ui.sent {
            if ui.wanted.is_empty() {
                self.markdown_images.cancel();
                ui.sent.clear();
            } else {
                match self
                    .markdown_images
                    .request(ui.context.clone(), ui.wanted.clone())
                {
                    Ok(_) => ui.sent = ui.wanted.clone(),
                    Err(error) => {
                        ui.sent = ui.wanted.clone();
                        self.status = error;
                    }
                }
            }
            changed = true;
        }
        if let Some(scan) = &mut ui.anchor {
            match scan.step(&self.workbench) {
                MarkdownAnchorProgress::Pending => {
                    self.wakeup.signal();
                }
                MarkdownAnchorProgress::Found(line) => {
                    ui.view = MarkdownView::at_line(line);
                    ui.anchor = None;
                    ui.token = Rc::new(());
                    ui.hits = None;
                    changed = true;
                    self.status = "Encabezado localizado · fuente sin cambios".into();
                }
                progress => {
                    ui.anchor = None;
                    self.status = match progress {
                        MarkdownAnchorProgress::NotFound => "No se encontró el encabezado",
                        MarkdownAnchorProgress::Stale => {
                            "El documento cambió; vuelve a abrir el enlace"
                        }
                        _ => "Búsqueda de encabezado limitada por presupuesto",
                    }
                    .into();
                    changed = true;
                }
            }
        }
        changed
    }
}
