//! Documented Win32 owner-draw menus. Item data is a bounded numeric identity,
//! never an application pointer; drawing reads only owned UI labels and colors.
use super::{ffi::*, shell, wide};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    ptr::null_mut,
    rc::Rc,
};

const OWNER_DRAW: u32 = 0x100;
const TOKEN_BASE: usize = 0x4349_0000;
const MAX_ITEMS: usize = 512;
#[repr(C)]
struct MenuInfo {
    size: u32,
    mask: u32,
    style: u32,
    max_height: u32,
    background: Handle,
    help: u32,
    data: usize,
}
#[repr(C)]
struct MenuItemInfo {
    size: u32,
    mask: u32,
    kind: u32,
    state: u32,
    id: u32,
    submenu: Handle,
    checked: Handle,
    unchecked: Handle,
    data: usize,
    text: *mut u16,
    length: u32,
    bitmap: Handle,
}
#[repr(C)]
struct MeasureItem {
    control_type: u32,
    control_id: u32,
    item_id: u32,
    width: u32,
    height: u32,
    data: usize,
}
#[repr(C)]
struct DrawItem {
    control_type: u32,
    control_id: u32,
    item_id: u32,
    action: u32,
    state: u32,
    menu: Handle,
    dc: Hdc,
    rect: Rect,
    data: usize,
}
#[link(name = "user32")]
extern "system" {
    fn SetMenuInfo(menu: Handle, info: *const MenuInfo) -> i32;
    fn SetMenuItemInfoW(
        menu: Handle,
        item: u32,
        by_position: i32,
        info: *const MenuItemInfo,
    ) -> i32;
    fn DrawTextW(dc: Hdc, text: *const u16, len: i32, rect: *mut Rect, flags: u32) -> i32;
    fn GetMenuState(menu: Handle, item: u32, flags: u32) -> u32;
}
#[link(name = "gdi32")]
extern "system" {
    fn SaveDC(dc: Hdc) -> i32;
    fn RestoreDC(dc: Hdc, saved: i32) -> i32;
    fn SetBkMode(dc: Hdc, mode: i32) -> i32;
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Palette {
    pub surface: u32,
    pub popup: u32,
    pub selected: u32,
    pub text: u32,
    pub muted: u32,
}
impl Palette {
    pub(super) fn current() -> Self {
        Self {
            surface: shell::SURFACE,
            popup: shell::SIDEBAR,
            selected: shell::SELECTED,
            text: shell::TEXT,
            muted: shell::MUTED,
        }
    }
}
struct Brushes {
    bar: Handle,
    popup: Handle,
    selected: Handle,
}
impl Brushes {
    unsafe fn new(palette: Palette) -> Result<Self, String> {
        let result = Self {
            bar: CreateSolidBrush(palette.surface),
            popup: CreateSolidBrush(palette.popup),
            selected: CreateSolidBrush(palette.selected),
        };
        if result.bar.is_null() || result.popup.is_null() || result.selected.is_null() {
            return Err("No se pudieron preparar los colores del menú".into());
        }
        Ok(result)
    }
}
impl Drop for Brushes {
    fn drop(&mut self) {
        unsafe {
            for brush in [self.bar, self.popup, self.selected] {
                if !brush.is_null() {
                    DeleteObject(brush);
                }
            }
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Bar,
    Item,
    Separator,
}
struct Entry {
    menu: Handle,
    id: usize,
    position: u32,
    kind: Kind,
    full: String,
    text: Vec<u16>,
    shortcut: Vec<u16>,
    mnemonic: Option<char>,
    text_width: i32,
    shortcut_width: i32,
    applied_state: Cell<Option<u32>>,
}
#[derive(Clone, Copy, Default)]
pub(super) struct Statistics {
    pub measures: usize,
    pub draws: usize,
    pub labels: usize,
    pub style_updates: usize,
    pub width_calculations: usize,
}
pub(super) struct Theme {
    font: Handle,
    scaled_font: Cell<Handle>,
    dpi: Cell<super::dpi::Dpi>,
    palette: Cell<Palette>,
    brushes: RefCell<Brushes>,
    menus: RefCell<Vec<(Handle, bool)>>,
    entries: RefCell<BTreeMap<usize, Rc<Entry>>>,
    measures: Cell<usize>,
    draws: Cell<usize>,
    style_updates: Cell<usize>,
    width_calculations: Cell<usize>,
}

fn split_label(label: &str) -> (String, String, Option<char>) {
    let (left, right) = label.split_once('\t').unwrap_or((label, ""));
    let mut chars = left.chars().peekable();
    let mut mnemonic = None;
    while let Some(ch) = chars.next() {
        if ch == '&' {
            match chars.next() {
                Some('&') => {}
                Some(next) if mnemonic.is_none() => {
                    mnemonic = Some(next.to_lowercase().next().unwrap_or(next))
                }
                _ => {}
            }
        }
    }
    (left.to_owned(), right.to_owned(), mnemonic)
}
fn plain_label(label: &str) -> String {
    let mut text = String::new();
    let mut chars = label.chars();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            if let Some(next) = chars.next() {
                text.push(next);
            }
        } else {
            text.push(ch);
        }
    }
    text
}
impl Theme {
    pub(super) unsafe fn new() -> Result<Self, String> {
        let font = super::dpi::ui_font(super::dpi::Dpi::BASE);
        if font.is_null() {
            return Err("No se pudo preparar la tipografía del menú".into());
        }
        let palette = Palette::current();
        let brushes = match Brushes::new(palette) {
            Ok(brushes) => brushes,
            Err(error) => {
                DeleteObject(font);
                return Err(error);
            }
        };
        Ok(Self {
            font,
            scaled_font: Cell::new(null_mut()),
            dpi: Cell::new(super::dpi::Dpi::BASE),
            palette: Cell::new(palette),
            brushes: RefCell::new(brushes),
            menus: RefCell::new(Vec::new()),
            entries: RefCell::new(BTreeMap::new()),
            measures: Cell::new(0),
            draws: Cell::new(0),
            style_updates: Cell::new(0),
            width_calculations: Cell::new(0),
        })
    }
    pub(super) unsafe fn set_dpi(&self, dpi: super::dpi::Dpi) -> Result<(), String> {
        if self.dpi.get() == dpi {
            return Ok(());
        }
        let font = if dpi == super::dpi::Dpi::BASE {
            null_mut()
        } else {
            let font = super::dpi::ui_font(dpi);
            if font.is_null() {
                return Err(
                    "No se pudo ajustar la tipografía del menú a la escala de la ventana".into(),
                );
            }
            font
        };
        let previous = self.scaled_font.replace(font);
        self.dpi.set(dpi);
        if !previous.is_null() {
            DeleteObject(previous);
        }
        Ok(())
    }
    pub(super) fn statistics(&self) -> Statistics {
        Statistics {
            measures: self.measures.get(),
            draws: self.draws.get(),
            labels: self.entries.borrow().len(),
            style_updates: self.style_updates.get(),
            width_calculations: self.width_calculations.get(),
        }
    }
    pub(super) fn palette(&self) -> Palette {
        self.palette.get()
    }
    unsafe fn background(menu: Handle, brush: Handle) -> Result<(), String> {
        let info = MenuInfo {
            size: std::mem::size_of::<MenuInfo>() as u32,
            mask: 2,
            style: 0,
            max_height: 0,
            background: brush,
            help: 0,
            data: 0,
        };
        if SetMenuInfo(menu, &info) == 0 {
            Err("No se pudo aplicar el fondo del menú nativo".into())
        } else {
            Ok(())
        }
    }
    pub(super) unsafe fn register_menu(&self, menu: Handle, bar: bool) -> Result<(), String> {
        let brush = {
            let brushes = self.brushes.borrow();
            if bar {
                brushes.bar
            } else {
                brushes.popup
            }
        };
        Self::background(menu, brush)?;
        self.menus.borrow_mut().push((menu, bar));
        Ok(())
    }
    /// Update only this window's brushes. Every registered HMENU is updated
    /// before old brushes are deleted; a failed update restores previous handles.
    pub(super) unsafe fn apply_palette(&self, palette: Palette) -> Result<(), String> {
        if self.palette.get() == palette {
            return Ok(());
        }
        let next = Brushes::new(palette)?;
        let menus = self.menus.borrow().clone();
        for (changed, (menu, bar)) in menus.iter().enumerate() {
            if let Err(error) = Self::background(*menu, if *bar { next.bar } else { next.popup }) {
                let old = self.brushes.borrow();
                for (menu, bar) in menus.iter().take(changed) {
                    let _ = Self::background(*menu, if *bar { old.bar } else { old.popup });
                }
                return Err(error);
            }
        }
        self.palette.set(palette);
        self.brushes.replace(next);
        Ok(())
    }
    unsafe fn width(&self, text: &str) -> Result<i32, String> {
        if text.is_empty() {
            return Ok(0);
        }
        self.width_calculations
            .set(self.width_calculations.get().saturating_add(1));
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            return Err("No se pudo medir la etiqueta del menú".into());
        }
        let previous = SelectObject(dc, self.font);
        let value = wide(&plain_label(text));
        let mut rect = Rect::default();
        let result = if value.len() <= 1 {
            0
        } else {
            DrawTextW(
                dc,
                value.as_ptr(),
                (value.len() - 1) as i32,
                &mut rect,
                0x400 | 0x20 | 0x800,
            )
        };
        SelectObject(dc, previous);
        DeleteDC(dc);
        if result == 0 && value.len() > 1 {
            Err("No se pudo medir el texto del menú".into())
        } else {
            Ok(rect.right.max(0))
        }
    }
    unsafe fn entry(
        &self,
        menu: Handle,
        id: usize,
        position: u32,
        kind: Kind,
        label: &[u16],
    ) -> Result<Entry, String> {
        let end = label
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(label.len());
        if end > 512 {
            return Err("La etiqueta del menú supera512 unidades UTF-16".into());
        }
        let full = String::from_utf16(&label[..end])
            .map_err(|_| "La etiqueta del menú no es UTF-16 válido")?;
        let (text, shortcut, mnemonic) = split_label(&full);
        let text_width = self.width(&text)?;
        let shortcut_width = self.width(&shortcut)?;
        Ok(Entry {
            menu,
            id,
            position,
            kind,
            full,
            text: wide(&text),
            shortcut: wide(&shortcut),
            mnemonic,
            text_width,
            shortcut_width,
            applied_state: Cell::new(None),
        })
    }
    pub(super) unsafe fn register(
        &self,
        menu: Handle,
        id: usize,
        position: u32,
        kind: Kind,
        label: &[u16],
    ) -> Result<usize, String> {
        let entry = self.entry(menu, id, position, kind, label)?;
        let mut entries = self.entries.borrow_mut();
        if entries.len() >= MAX_ITEMS {
            return Err("El menú supera el catálogo máximo de512 entradas".into());
        }
        let token = TOKEN_BASE + entries.len() + 1;
        entries.insert(token, Rc::new(entry));
        Ok(token)
    }
    pub(super) unsafe fn update_label(
        &self,
        menu: Handle,
        id: usize,
        label: &[u16],
    ) -> Result<usize, String> {
        let (token, previous) = {
            let entries = self.entries.borrow();
            entries
                .iter()
                .find(|(_, entry)| entry.menu == menu && entry.id == id && entry.kind == Kind::Item)
                .map(|(token, entry)| (*token, entry.clone()))
                .ok_or("El comando no pertenece al catálogo de menú propio")?
        };
        // Most focus updates change only enabled/check flags. Do not create DCs
        // or remeasure every label each time a native popup opens.
        let end = label
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(label.len());
        if end > 512 {
            return Err("La etiqueta del menú supera512 unidades UTF-16".into());
        }
        if previous
            .full
            .encode_utf16()
            .eq(label[..end].iter().copied())
        {
            return Ok(token);
        }
        let entry = self.entry(menu, id, previous.position, previous.kind, label)?;
        self.entries.borrow_mut().insert(token, Rc::new(entry));
        Ok(token)
    }
    pub(super) unsafe fn style_item(&self, token: usize, state: u32) -> Result<(), String> {
        let entry = self
            .entries
            .borrow()
            .get(&token)
            .cloned()
            .ok_or("La identidad del menú no es propia")?;
        if entry.applied_state.get() == Some(state) {
            return Ok(());
        }
        let mut text = wide(&entry.full);
        let info = MenuItemInfo {
            size: std::mem::size_of::<MenuItemInfo>() as u32,
            mask: 0x100 | 0x20 | 0x40 | 1, // FTYPE, DATA, STRING, STATE: preserve ID/submenu.
            kind: OWNER_DRAW
                | if entry.kind == Kind::Separator {
                    0x800
                } else {
                    0
                },
            state,
            id: 0,
            submenu: null_mut(),
            checked: null_mut(),
            unchecked: null_mut(),
            data: token,
            text: text.as_mut_ptr(),
            length: (text.len() - 1) as u32,
            bitmap: null_mut(),
        };
        if SetMenuItemInfoW(entry.menu, entry.position, 1, &info) == 0 {
            Err("No se pudo actualizar el estilo del menú nativo".into())
        } else {
            entry.applied_state.set(Some(state));
            self.style_updates
                .set(self.style_updates.get().saturating_add(1));
            Ok(())
        }
    }
    pub(super) fn label_for(&self, menu: Handle, id: usize) -> Option<String> {
        self.entries
            .borrow()
            .values()
            .find(|entry| entry.menu == menu && entry.id == id && entry.kind == Kind::Item)
            .map(|entry| entry.full.clone())
    }
    /// lparam must be the native WM_MEASUREITEM structure supplied to this HWND.
    /// Its item data is matched numerically and never dereferenced as a pointer.
    pub(super) unsafe fn measure(&self, lparam: isize) -> Result<bool, String> {
        if lparam == 0 {
            return Ok(false);
        }
        let item = &mut *(lparam as *mut MeasureItem);
        if item.control_type != 1 {
            return Ok(false);
        }
        let entry = self.entries.borrow().get(&item.data).cloned();
        let Some(entry) = entry else {
            return Ok(false);
        };
        if entry.kind == Kind::Item && item.item_id != entry.id as u32 {
            return Ok(false);
        }
        (item.width, item.height) = match entry.kind {
            Kind::Bar => ((entry.text_width + 24).clamp(24, 600) as u32, 25),
            Kind::Separator => (20, 9),
            Kind::Item => (
                (entry.text_width
                    + entry.shortcut_width
                    + if entry.shortcut_width > 0 { 76 } else { 48 })
                .clamp(100, 1200) as u32,
                27,
            ),
        };
        let dpi = self.dpi.get();
        item.width = dpi.pixels(item.width as i32) as u32;
        item.height = dpi.pixels(item.height as i32) as u32;
        self.measures.set(self.measures.get().saturating_add(1));
        Ok(true)
    }
    /// lparam must be the native WM_DRAWITEM structure supplied to this HWND.
    pub(super) unsafe fn draw(&self, lparam: isize) -> Result<bool, String> {
        if lparam == 0 {
            return Ok(false);
        }
        let item = &*(lparam as *const DrawItem);
        if item.control_type != 1 || item.dc.is_null() {
            return Ok(false);
        }
        let entry = self.entries.borrow().get(&item.data).cloned();
        let Some(entry) = entry else {
            return Ok(false);
        };
        if item.menu != entry.menu || (entry.kind == Kind::Item && item.item_id != entry.id as u32)
        {
            return Ok(false);
        }
        let palette = self.palette.get();
        let px = |value| self.dpi.get().pixels(value);
        let selected = item.state & (1 | 0x40) != 0;
        let disabled = item.state & (2 | 4) != 0;
        let brush = {
            let brushes = self.brushes.borrow();
            if selected {
                brushes.selected
            } else if entry.kind == Kind::Bar {
                brushes.bar
            } else {
                brushes.popup
            }
        };
        let saved = SaveDC(item.dc);
        if saved == 0 {
            return Err("No se pudo preservar el contexto del menú".into());
        }
        let result = (|| -> Result<(), String> {
            if FillRect(item.dc, &item.rect, brush) == 0 {
                return Err("No se pudo dibujar el fondo del menú".into());
            }
            if entry.kind == Kind::Separator {
                let rect = Rect {
                    left: item.rect.left + px(28),
                    right: item.rect.right - px(8),
                    top: (item.rect.top + item.rect.bottom) / 2,
                    bottom: (item.rect.top + item.rect.bottom) / 2 + 1,
                };
                let border = self.brushes.borrow().bar;
                if FillRect(item.dc, &rect, border) == 0 {
                    return Err("No se pudo dibujar el separador del menú".into());
                }
                return Ok(());
            }
            let scaled = self.scaled_font.get();
            SelectObject(item.dc, if scaled.is_null() { self.font } else { scaled });
            SetBkMode(item.dc, 1);
            SetTextColor(
                item.dc,
                if disabled {
                    palette.muted
                } else {
                    palette.text
                },
            );
            let mut text = Rect {
                left: item.rect.left + px(if entry.kind == Kind::Bar { 12 } else { 30 }),
                right: item.rect.right
                    - px(if entry.shortcut_width > 0 {
                        entry.shortcut_width + 24
                    } else {
                        12
                    }),
                ..item.rect
            };
            if DrawTextW(
                item.dc,
                entry.text.as_ptr(),
                (entry.text.len() - 1) as i32,
                &mut text,
                0x20 | 4 | 0x8000,
            ) == 0
            {
                return Err("No se pudo dibujar la etiqueta del menú".into());
            }
            if entry.shortcut.len() > 1 {
                let mut right = Rect {
                    left: item.rect.right - px(entry.shortcut_width + 16),
                    right: item.rect.right - px(12),
                    ..item.rect
                };
                SetTextColor(
                    item.dc,
                    if disabled {
                        palette.muted
                    } else {
                        palette.text
                    },
                );
                if DrawTextW(
                    item.dc,
                    entry.shortcut.as_ptr(),
                    (entry.shortcut.len() - 1) as i32,
                    &mut right,
                    0x20 | 4 | 2 | 0x800,
                ) == 0
                {
                    return Err("No se pudo dibujar el atajo del menú".into());
                }
            }
            if item.state & 8 != 0 && entry.kind == Kind::Item {
                let mark = wide("✓");
                let mut gutter = Rect {
                    left: item.rect.left + px(7),
                    right: item.rect.left + px(26),
                    ..item.rect
                };
                if DrawTextW(item.dc, mark.as_ptr(), 1, &mut gutter, 0x20 | 4 | 0x800) == 0 {
                    return Err("No se pudo dibujar la marca del menú".into());
                }
            }
            Ok(())
        })();
        let restored = RestoreDC(item.dc, saved);
        result?;
        if restored == 0 {
            return Err("No se pudo restaurar el contexto del menú".into());
        }
        self.draws.set(self.draws.get().saturating_add(1));
        Ok(true)
    }
    pub(super) unsafe fn menu_char(&self, wparam: usize, lparam: isize) -> Option<isize> {
        let menu = lparam as Handle;
        if !self.menus.borrow().iter().any(|(known, _)| *known == menu) {
            return None;
        }
        let character = char::from_u32((wparam & 0xffff) as u32)?
            .to_lowercase()
            .next()?;
        let entries: Vec<_> = self
            .entries
            .borrow()
            .values()
            .filter(|entry| entry.menu == menu && entry.mnemonic == Some(character))
            .cloned()
            .collect();
        let mut matches = Vec::new();
        for entry in entries {
            let flags = GetMenuState(menu, entry.position, 0x400);
            if flags != u32::MAX && flags & 3 == 0 {
                matches.push((entry.position, flags & 0x80 != 0));
            }
        }
        if matches.is_empty() {
            return Some(0);
        }
        matches.sort_unstable_by_key(|item| item.0);
        let index = matches
            .iter()
            .position(|item| item.1)
            .map_or(0, |n| (n + 1) % matches.len());
        Some(((if matches.len() == 1 { 2 } else { 3 }) << 16) | matches[index].0 as isize)
    }
}
impl Drop for Theme {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.font);
            if !self.scaled_font.get().is_null() {
                DeleteObject(self.scaled_font.get());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_menu_mnemonics_keep_escaped_ampersands_and_shortcuts_distinct() {
        let (text, shortcut, mnemonic) = split_label("Guardar && &Abrir\tCtrl+Mayús+S");
        assert_eq!(plain_label(&text), "Guardar & Abrir");
        assert_eq!(shortcut, "Ctrl+Mayús+S");
        assert_eq!(mnemonic, Some('a'));
        assert_eq!(split_label("Ú&ltima pestaña").2, Some('l'));
    }
    #[test]
    fn own_menu_metadata_preserves_native_checks_labels_and_rejects_alien_item_data() {
        struct OwnedMenu(Handle);
        impl Drop for OwnedMenu {
            fn drop(&mut self) {
                unsafe {
                    DestroyMenu(self.0);
                }
            }
        }
        unsafe {
            let theme = Theme::new().unwrap();
            let menu = OwnedMenu(CreatePopupMenu());
            assert!(!menu.0.is_null());
            theme.register_menu(menu.0, false).unwrap();
            let label = wide("&Guardar\tCtrl+S");
            assert_ne!(AppendMenuW(menu.0, 0, 77, label.as_ptr()), 0);
            let token = theme.register(menu.0, 77, 0, Kind::Item, &label).unwrap();
            theme.style_item(token, 8).unwrap();
            let flags = GetMenuState(menu.0, 77, 0);
            assert_ne!(flags, u32::MAX);
            assert_ne!(flags & 8, 0);
            assert_eq!(flags & 3, 0);
            assert_eq!(
                theme.label_for(menu.0, 77).as_deref(),
                Some("&Guardar\tCtrl+S")
            );
            assert_eq!(
                theme.menu_char('g' as usize, menu.0 as isize),
                Some(2 << 16)
            );
            let mut measured = MeasureItem {
                control_type: 1,
                control_id: 0,
                item_id: 77,
                width: 0,
                height: 0,
                data: token,
            };
            assert!(theme
                .measure((&mut measured as *mut MeasureItem) as isize)
                .unwrap());
            assert!(measured.width >= 100);
            assert_eq!(measured.height, 27);
            measured.data = usize::MAX;
            assert!(!theme
                .measure((&mut measured as *mut MeasureItem) as isize)
                .unwrap());
            theme.style_item(token, 1 | 8).unwrap();
            assert_eq!(theme.menu_char('g' as usize, menu.0 as isize), Some(0));
            assert_eq!(theme.statistics().labels, 1);
            assert_eq!(theme.statistics().measures, 1);
        }
    }
    #[test]
    fn repeated_menu_refresh_reuses_widths_and_native_style_until_label_or_flags_change() {
        struct OwnedMenu(Handle);
        impl Drop for OwnedMenu {
            fn drop(&mut self) {
                unsafe {
                    DestroyMenu(self.0);
                }
            }
        }
        unsafe {
            let theme = Theme::new().unwrap();
            let menu = OwnedMenu(CreatePopupMenu());
            assert!(!menu.0.is_null());
            theme.register_menu(menu.0, false).unwrap();
            let first = wide("&Guardar\tCtrl+S");
            assert_ne!(AppendMenuW(menu.0, 0, 77, first.as_ptr()), 0);
            let token = theme.register(menu.0, 77, 0, Kind::Item, &first).unwrap();
            theme.style_item(token, 0).unwrap();
            let initial = theme.statistics();
            for _ in 0..100 {
                assert_eq!(
                    theme
                        .update_label(menu.0, 77, &first[..first.len() - 1])
                        .unwrap(),
                    token
                );
                theme.style_item(token, 0).unwrap();
            }
            let repeated = theme.statistics();
            assert_eq!(repeated.style_updates, initial.style_updates);
            assert_eq!(repeated.width_calculations, initial.width_calculations);
            theme.style_item(token, 8).unwrap();
            assert_ne!(GetMenuState(menu.0, 77, 0) & 8, 0);
            assert_eq!(theme.statistics().style_updates, initial.style_updates + 1);
            let changed = wide("Guardar &como\tCtrl+Mayús+S");
            theme.update_label(menu.0, 77, &changed).unwrap();
            theme.style_item(token, 8).unwrap();
            let updated = theme.statistics();
            assert!(updated.width_calculations > initial.width_calculations);
            assert_eq!(updated.style_updates, initial.style_updates + 2);
            theme.update_label(menu.0, 77, &changed).unwrap();
            theme.style_item(token, 8).unwrap();
            let palette = theme.palette();
            theme
                .apply_palette(Palette {
                    surface: palette.surface ^ 0x010101,
                    ..palette
                })
                .unwrap();
            theme.apply_palette(palette).unwrap();
            assert_eq!(
                theme.statistics().width_calculations,
                updated.width_calculations
            );
            assert_eq!(theme.statistics().style_updates, updated.style_updates);
        }
    }
}
