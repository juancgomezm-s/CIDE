//! Native appearance controls. Geometry is shared by painting and pointer input.
use super::{
    appearance::{Choice, Settings, Theme},
    dpi::Dpi,
    ffi::{Hdc, Rect},
    paint, shell, State,
};

#[derive(Clone, Copy)]
pub(super) enum Target {
    Choice(Choice),
    Apply,
    Cancel,
    Font,
}

pub(super) struct Frame {
    pub rect: Rect,
    dpi: Dpi,
    settings: Settings,
    focused: Option<Choice>,
    pub controls: Vec<(Rect, Target)>,
}

impl Frame {
    pub(super) fn prepare(state: &State) -> Option<Self> {
        if !state.appearance_open() {
            return None;
        }
        let px = |n| state.dpi.pixels(n);
        let top = state.chrome_layout().top();
        if state.width < px(660) || state.height - top < px(594) {
            return None;
        }
        let width = px(740).min(state.width - px(32));
        let height = px(576);
        let left = (state.width - width) / 2;
        let y = top + (state.height - top - px(24) - height).max(0) / 2;
        let mut frame = Self {
            rect: Rect {
                left,
                top: y,
                right: left + width,
                bottom: y + height,
            },
            dpi: state.dpi,
            settings: Settings::read(state),
            focused: state.selector.as_ref().and_then(|s| s.appearance_target()),
            controls: Vec::with_capacity(28),
        };
        let inner = width - px(48);
        let gap = px(10);
        let tile = (inner - 2 * gap) / 3;
        for (n, theme) in Theme::ALL.into_iter().enumerate() {
            let x = px(24) + n as i32 % 3 * (tile + gap);
            let y = px(96) + n as i32 / 3 * px(58);
            frame.add(x, y, tile, px(52), Target::Choice(Choice::Theme(theme)));
        }
        for (n, size) in [11, 12, 14, 16].into_iter().enumerate() {
            frame.add(
                px(136) + n as i32 * px(60),
                px(306),
                px(50),
                px(30),
                Target::Choice(Choice::Ui(size)),
            );
        }
        for (n, size) in [11, 12, 13, 15, 17, 20].into_iter().enumerate() {
            frame.add(
                px(136) + n as i32 * px(60),
                px(344),
                px(50),
                px(30),
                Target::Choice(Choice::Editor(size)),
            );
        }
        for (n, visible) in [true, false].into_iter().enumerate() {
            frame.add(
                px(136) + n as i32 * px(80),
                px(451),
                px(72),
                px(30),
                Target::Choice(Choice::Minimap(visible)),
            );
        }
        for (n, width) in [60, 96, 128, 160, 200].into_iter().enumerate() {
            frame.add(
                px(136) + n as i32 * px(60),
                px(489),
                px(50),
                px(30),
                Target::Choice(Choice::MinimapWidth(width)),
            );
        }
        frame.add(px(136), px(382), width - px(160), px(30), Target::Font);
        let right = frame.rect.right - px(24);
        frame.controls.push((
            Rect {
                left: right - px(94),
                right,
                top: y + px(535),
                bottom: y + px(565),
            },
            Target::Apply,
        ));
        frame.controls.push((
            Rect {
                left: right - px(196),
                right: right - px(102),
                top: y + px(535),
                bottom: y + px(565),
            },
            Target::Cancel,
        ));
        // Position controls share the right side with the compact minimap controls.
        for (n, left) in [false, true].into_iter().enumerate() {
            frame.add(
                width - px(200),
                px(451) + n as i32 * px(38),
                px(176),
                px(30),
                Target::Choice(Choice::OutlineLeft(left)),
            );
        }
        Some(frame)
    }
    fn add(&mut self, x: i32, y: i32, w: i32, h: i32, target: Target) {
        self.controls.push((
            Rect {
                left: self.rect.left + x,
                top: self.rect.top + y,
                right: self.rect.left + x + w,
                bottom: self.rect.top + y + h,
            },
            target,
        ));
    }
    pub(super) fn hit(&self, x: i32, y: i32) -> Option<Target> {
        self.controls
            .iter()
            .find(|(r, _)| contains(*r, x, y))
            .map(|(_, t)| *t)
    }
    pub(super) fn contains(&self, x: i32, y: i32) -> bool {
        contains(self.rect, x, y)
    }
}

fn contains(r: Rect, x: i32, y: i32) -> bool {
    (r.left..r.right).contains(&x) && (r.top..r.bottom).contains(&y)
}
fn inset(r: Rect, n: i32) -> Rect {
    Rect {
        left: r.left + n,
        top: r.top + n,
        right: r.right - n,
        bottom: r.bottom - n,
    }
}
fn name(theme: Theme) -> &'static str {
    match theme {
        Theme::Current => "CIDE",
        Theme::Neutral => "Oscuro neutro",
        Theme::Warm => "Oscuro cálido",
        Theme::Blue => "Azul profundo",
        Theme::Light => "Claro suave",
        Theme::Contrast => "Alto contraste",
        Theme::LightDay => "Claro nítido",
        Theme::LightPaper => "Papel cálido",
        Theme::ExtraDark => "Grafito profundo",
    }
}

pub(super) unsafe fn draw(dc: Hdc, frame: &Frame) -> Result<(), String> {
    let px = |n| frame.dpi.pixels(n);
    let r = frame.rect;
    shell::rounded(
        dc,
        Rect {
            left: r.left + px(3),
            top: r.top + px(6),
            right: r.right + px(3),
            bottom: r.bottom + px(6),
        },
        px(14),
        shell::SIDEBAR,
    )?;
    shell::rounded(dc, r, px(12), shell::BORDER)?;
    shell::rounded(dc, inset(r, 1), px(12), shell::SURFACE)?;
    let label = |x, y, w, text: &str, color| {
        shell::label(
            dc,
            Rect {
                left: r.left + px(x),
                top: r.top + px(y),
                right: (r.left + px(x + w)).min(r.right - px(16)),
                bottom: r.top + px(y + 24),
            },
            text,
            color,
        )
    };
    label(24, 18, 500, "Apariencia", shell::TEXT)?;
    label(
        24,
        42,
        650,
        "Personaliza CIDE. Los cambios se muestran al instante.",
        shell::MUTED,
    )?;
    label(24, 74, 500, "TEMA Y COLORES", shell::ACCENT)?;
    label(24, 278, 500, "TIPOGRAFÍA", shell::ACCENT)?;
    label(24, 311, 100, "Interfaz", shell::TEXT)?;
    label(24, 349, 100, "Código", shell::TEXT)?;
    label(24, 387, 100, "Fuente", shell::TEXT)?;
    label(24, 423, 500, "DISTRIBUCIÓN", shell::ACCENT)?;
    label(24, 456, 100, "Minimapa", shell::TEXT)?;
    label(24, 494, 100, "Ancho (px)", shell::TEXT)?;
    label(24, 541, 240, "Esc · descartar cambios", shell::MUTED)?;
    for (rect, target) in &frame.controls {
        let (text, selected, focus) = match *target {
            Target::Choice(c) => (
                match c {
                    Choice::Theme(t) => name(t).to_owned(),
                    Choice::Ui(n) | Choice::Editor(n) => format!("{n} px"),
                    Choice::Minimap(true) => "Mostrar".into(),
                    Choice::Minimap(false) => "Ocultar".into(),
                    Choice::MinimapWidth(n) => n.to_string(),
                    Choice::OutlineLeft(false) => "Esquema · derecha".into(),
                    Choice::OutlineLeft(true) => "Esquema · izquierda".into(),
                    Choice::Zoom(n) => format!("{n}%"),
                    Choice::FontPicker => "Elegir fuente…".into(),
                },
                frame.settings.checked().contains(&c),
                frame.focused == Some(c),
            ),
            Target::Apply => ("Aplicar".into(), true, false),
            Target::Cancel => ("Cancelar".into(), false, false),
            Target::Font => (format!("{}    ▾", frame.settings.face.name()), false, false),
        };
        let is_apply = matches!(target, Target::Apply);
        let border = if selected || focus {
            shell::ACCENT
        } else {
            shell::BORDER
        };
        let bg = if is_apply {
            shell::ACCENT
        } else if selected {
            shell::SELECTED
        } else {
            shell::RAISED
        };
        shell::rounded(dc, *rect, px(6), border)?;
        shell::rounded(dc, inset(*rect, 1), px(5), bg)?;
        if let Target::Choice(Choice::Theme(theme)) = *target {
            let preview = Rect {
                left: rect.left + px(8),
                right: rect.right - px(8),
                top: rect.top + px(6),
                bottom: rect.top + px(25),
            };
            paint::fill_marks(dc, std::iter::once(preview), theme.background())?;
            let line = |x, y, w| Rect {
                left: preview.left + px(x),
                top: preview.top + px(y),
                right: (preview.left + px(x + w)).min(preview.right),
                bottom: preview.top + px(y + 2),
            };
            paint::fill_marks(
                dc,
                [line(7, 5, 31), line(13, 11, 46), line(13, 17, 26)].into_iter(),
                theme.accent(),
            )?;
            shell::label(
                dc,
                Rect {
                    left: rect.left + px(9),
                    top: rect.top + px(30),
                    right: rect.right - px(8),
                    bottom: rect.bottom - px(3),
                },
                &text,
                shell::TEXT,
            )?;
            if selected {
                shell::label(
                    dc,
                    Rect {
                        left: rect.right - px(22),
                        top: rect.top + px(29),
                        right: rect.right - px(5),
                        bottom: rect.bottom,
                    },
                    "✓",
                    shell::ACCENT,
                )?;
            }
        } else {
            shell::label(
                dc,
                Rect {
                    left: rect.left + px(9),
                    top: rect.top + px(7),
                    right: rect.right - px(4),
                    bottom: rect.bottom,
                },
                &text,
                if is_apply {
                    shell::SIDEBAR
                } else {
                    shell::TEXT
                },
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preference_controls_are_visible_unique_and_nonoverlapping_at_each_dpi() {
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.open_selector(super::super::selector::SelectorKind::Appearance);
        for scale in [96, 144, 192] {
            state.dpi = Dpi::from_raw(scale);
            for width in [660, 740, 1000] {
                state.width = state.dpi.pixels(width);
                state.height = state.chrome_layout().top() + state.dpi.pixels(610);
                let f = Frame::prepare(&state).unwrap();
                assert_eq!(f.controls.len(), 31);
                for (i, (r, _)) in f.controls.iter().enumerate() {
                    assert!(
                        r.left >= f.rect.left
                            && r.right <= f.rect.right
                            && r.top >= f.rect.top
                            && r.bottom <= f.rect.bottom
                    );
                    assert!(f
                        .hit((r.left + r.right) / 2, (r.top + r.bottom) / 2)
                        .is_some());
                    for (other, _) in f.controls.iter().skip(i + 1) {
                        assert!(
                            r.right <= other.left
                                || other.right <= r.left
                                || r.bottom <= other.top
                                || other.bottom <= r.top
                        );
                    }
                }
            }
        }
    }
}
