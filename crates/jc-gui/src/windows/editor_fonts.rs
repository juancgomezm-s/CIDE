//! A bounded native font name; no font files or installation are shipped.
use super::{appearance, ffi::*, owner};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Face(pub [u16; 32]);
impl Default for Face {
    fn default() -> Self {
        Self::from_name("Consolas").unwrap()
    }
}
impl Face {
    pub(super) fn from_name(name: &str) -> Option<Self> {
        if name.is_empty() || name.chars().any(char::is_control) {
            return None;
        }
        let chars: Vec<_> = name.encode_utf16().collect();
        if chars.len() > 31 {
            return None;
        }
        let mut data = [0; 32];
        data[..chars.len()].copy_from_slice(&chars);
        Some(Self(data))
    }
    pub(super) fn name(self) -> String {
        String::from_utf16_lossy(&self.0[..self.0.iter().position(|&v| v == 0).unwrap_or(32)])
    }
    pub(super) fn bytes(self) -> [u8; 64] {
        let mut bytes = [0; 64];
        for (i, c) in self.0.into_iter().enumerate() {
            bytes[2 * i..2 * i + 2].copy_from_slice(&c.to_le_bytes());
        }
        bytes
    }
    pub(super) fn read(bytes: [u8; 64]) -> Option<Self> {
        let chars: Vec<_> = bytes
            .chunks_exact(2)
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
            .collect();
        let end = chars.iter().position(|&v| v == 0)?;
        if chars[end..].iter().any(|&v| v != 0) {
            return None;
        }
        Self::from_name(&String::from_utf16(&chars[..end]).ok()?)
    }
}

#[repr(C)]
struct LogFont {
    height: i32,
    width: i32,
    escapement: i32,
    orientation: i32,
    weight: i32,
    italic: u8,
    underline: u8,
    strike: u8,
    charset: u8,
    output: u8,
    clip: u8,
    quality: u8,
    pitch: u8,
    face: [u16; 32],
}
#[repr(C)]
struct ChooseFont {
    size: u32,
    owner: Hwnd,
    dc: Hdc,
    font: *mut LogFont,
    point_size: i32,
    flags: u32,
    color: u32,
    custom: isize,
    hook: usize,
    template: *const u16,
    instance: Handle,
    style: *mut u16,
    font_type: u16,
    alignment: u16,
    min: i32,
    max: i32,
}
#[link(name = "comdlg32")]
extern "system" {
    fn ChooseFontW(font: *mut ChooseFont) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    fn GetTextFaceW(dc: Hdc, count: i32, name: *mut u16) -> i32;
}
#[link(name = "user32")]
extern "system" {
    fn GetDlgItem(hwnd: Hwnd, id: i32) -> Hwnd;
    fn EnableWindow(hwnd: Hwnd, enabled: i32) -> i32;
}
unsafe extern "system" fn family_only(hwnd: Hwnd, message: u32, _w: usize, _l: isize) -> usize {
    if matches!(message, 0x110 | 0x111) {
        // Windows SDK dlgs.h: cmb2/style, cmb3/size, cmb5/script. CIDE
        // controls size/zoom separately; retain a real, useful font preview.
        for id in [0x471, 0x472, 0x474, 0x441, 0x442, 0x446] {
            let control = GetDlgItem(hwnd, id);
            if !control.is_null() {
                EnableWindow(control, 0);
            }
        }
    }
    0
}

pub(super) unsafe fn selected_face(dc: Hdc) -> Option<Face> {
    let mut data = [0; 32];
    (GetTextFaceW(dc, 32, data.as_mut_ptr()) > 0)
        .then(|| Face::from_name(&Face(data).name()))
        .flatten()
}

pub(super) unsafe fn choose(hwnd: Hwnd) {
    let Some(model) = owner(hwnd) else { return };
    let (current, height) = {
        let state = model.state.borrow();
        (
            state.editor_face,
            appearance::editor_pixels(state.dpi, state.editor_text_size, state.editor_zoom),
        )
    };
    let mut font: LogFont = std::mem::zeroed();
    font.height = -height;
    font.weight = 400;
    font.charset = 1;
    font.pitch = 0x31;
    font.face = current.0;
    let mut dialog: ChooseFont = std::mem::zeroed();
    dialog.size = std::mem::size_of::<ChooseFont>() as u32;
    dialog.owner = hwnd;
    dialog.font = &mut font;
    // SCREENFONTS | ENABLEHOOK | INITTOLOGFONTSTRUCT | FIXEDPITCHONLY |
    // FORCEFONTEXIST | NOVERTFONTS (Windows SDK commdlg.h).
    dialog.flags = 0x1 | 0x8 | 0x40 | 0x4000 | 0x10000 | 0x1000000;
    dialog.hook = family_only as usize;
    if ChooseFontW(&mut dialog) == 0 {
        return;
    }
    let Some(face) = Face::from_name(&Face(font.face).name()) else {
        return;
    };
    let mut state = model.state.borrow_mut();
    if let Err(error) = appearance::preview_font(hwnd, model, &mut state, face) {
        state.error(error);
    }
    let theme = state.editor_theme;
    if let Some(selector) = state.selector.as_mut() {
        if let Some(row) = appearance::APPEARANCE
            .iter()
            .position(|(choice, _)| *choice == appearance::Choice::Theme(theme))
        {
            selector.select(row);
        }
    }
    InvalidateRect(hwnd, std::ptr::null(), 0);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn font_name_roundtrip_is_bounded_strict_and_terminated() {
        assert_eq!(std::mem::size_of::<LogFont>(), 92);
        assert_eq!(std::mem::size_of::<ChooseFont>(), 104);
        for name in ["Consolas", "Courier New", "Cascadia Mono"] {
            let face = Face::from_name(name).unwrap();
            assert_eq!(Face::read(face.bytes()), Some(face));
        }
        for invalid in ["", "Arial\n", &"a".repeat(32)] {
            assert!(Face::from_name(invalid).is_none());
        }
        assert!(Face::read([0xff; 64]).is_none());
        let mut broken = Face::default().bytes();
        broken[63] = 1;
        assert!(Face::read(broken).is_none());
        let mut surrogate = [0; 64];
        surrogate[0..2].copy_from_slice(&0xd800u16.to_le_bytes());
        assert!(Face::read(surrogate).is_none());
    }
}
