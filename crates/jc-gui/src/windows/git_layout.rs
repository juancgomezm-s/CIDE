//! Pure pixel geometry and persistent preferences for the native Git workspace.
//! No repository query, source ownership, event dispatch or rendering lives here.
use super::*;

const SCALE: i32 = 1000;
const MIN_RATIO: u16 = 50;
const MAX_SECOND: u16 = 950;
const MIN_GAP: u16 = 50;
const ENCODED_BYTES: usize = 7;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Arrangement {
    One,
    TwoFilesLeft,
    TwoFilesRight,
    #[default]
    Three,
}
impl Arrangement {
    pub(super) const ALL: [Self; 4] = [
        Self::One,
        Self::TwoFilesLeft,
        Self::TwoFilesRight,
        Self::Three,
    ];
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::One => "Una columna",
            Self::TwoFilesLeft => "Dos columnas, archivos a la izquierda",
            Self::TwoFilesRight => "Dos columnas, archivos a la derecha",
            Self::Three => "Tres columnas",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Preferences {
    pub arrangement: Arrangement,
    /// Preferred divider center as permille of the workspace. In two-column
    /// mode this is the share allocated to the file/history side (including
    /// half of its splitter), measured from the appropriate outside edge.
    pub first: u16,
    /// Second global divider center for the three-column arrangement.
    pub second: u16,
    pub parallel: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            arrangement: Arrangement::Three,
            first: 200,
            second: 420,
            parallel: false,
        }
    }
}
impl Preferences {
    fn canonical(mut self) -> Self {
        self.first = self.first.clamp(MIN_RATIO, MAX_SECOND - MIN_GAP);
        self.second = self.second.clamp(self.first + MIN_GAP, MAX_SECOND);
        self
    }
    fn valid(self) -> bool {
        self.first >= MIN_RATIO
            && self.first <= MAX_SECOND - MIN_GAP
            && self.second >= self.first + MIN_GAP
            && self.second <= MAX_SECOND
    }
    /// Seven bytes: version, arrangement, first LE u16, second LE u16, bool.
    /// Canonicalize public-field values rather than panicking during checkpoint.
    pub(super) fn encode(self) -> [u8; ENCODED_BYTES] {
        let p = self.canonical();
        let first = p.first.to_le_bytes();
        let second = p.second.to_le_bytes();
        [
            1,
            match p.arrangement {
                Arrangement::One => 0,
                Arrangement::TwoFilesLeft => 1,
                Arrangement::TwoFilesRight => 2,
                Arrangement::Three => 3,
            },
            first[0],
            first[1],
            second[0],
            second[1],
            u8::from(p.parallel),
        ]
    }
    pub(super) fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() != ENCODED_BYTES || bytes[0] != 1 || bytes[6] > 1 {
            return Err("Preferencias de distribución Git inválidas");
        }
        let arrangement = match bytes[1] {
            0 => Arrangement::One,
            1 => Arrangement::TwoFilesLeft,
            2 => Arrangement::TwoFilesRight,
            3 => Arrangement::Three,
            _ => return Err("Distribución Git desconocida"),
        };
        let result = Self {
            arrangement,
            first: u16::from_le_bytes([bytes[2], bytes[3]]),
            second: u16::from_le_bytes([bytes[4], bytes[5]]),
            parallel: bytes[6] != 0,
        };
        if !result.valid() {
            return Err("Los divisores Git están fuera de sus límites");
        }
        Ok(result)
    }
    /// x is the requested divider center in workspace pixels. Physical column
    /// minimums are applied by layout and scale with DPI, not by persistence.
    pub(super) fn drag(&mut self, divider: Divider, x: i32, width: i32) -> bool {
        if width <= 0 || self.arrangement == Arrangement::One {
            return false;
        }
        if divider == Divider::Second && self.arrangement != Arrangement::Three {
            return false;
        }
        let previous = *self;
        *self = self.canonical();
        let x = x.clamp(0, width);
        let ratio = ((i64::from(x) * i64::from(SCALE)) / i64::from(width)) as u16;
        match divider {
            Divider::First => {
                self.first = if self.arrangement == Arrangement::TwoFilesRight {
                    SCALE as u16 - ratio
                } else {
                    ratio
                }
                .clamp(MIN_RATIO, MAX_SECOND - MIN_GAP);
                self.second = self.second.max(self.first + MIN_GAP).min(MAX_SECOND);
            }
            Divider::Second => self.second = ratio.clamp(self.first + MIN_GAP, MAX_SECOND),
        }
        *self = self.canonical();
        *self != previous
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Column {
    History,
    #[default]
    Files,
    Comparison,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Divider {
    First,
    Second,
}
pub(super) struct Geometry {
    pub rect: Rect,
    pub toolbar: Rect,
    pub history: Option<Rect>,
    pub files: Option<Rect>,
    pub preview: Option<Rect>,
    pub dividers: Vec<(Rect, Divider)>,
}

/// Pixel minimums: history 140dp, files 160dp and comparison 300dp. A requested
/// three-column arrangement falls back to two, then one; the requested preference
/// itself is preserved so enlarging the window recovers the chosen arrangement.
pub(super) fn layout(
    width: i32,
    height: i32,
    top: i32,
    bottom: i32,
    dpi_pixels: impl Fn(i32) -> i32,
    prefs: Preferences,
    active: Column,
) -> Geometry {
    let width = width.max(0);
    let height = height.max(0);
    let top = top.clamp(0, height);
    let bottom = bottom.clamp(top, height);
    let p = |n| dpi_pixels(n).clamp(1, 1_000_000);
    let body_bottom = bottom.saturating_sub(p(24)).max(top);
    let body_top = top.saturating_add(p(76)).min(body_bottom);
    let gap = p(6);
    let half = gap / 2;
    let remaining_half = gap - half;
    let history_min = p(140);
    let files_min = p(160);
    let comparison_min = p(300);
    let prefs = prefs.canonical();
    let requested = prefs.arrangement;
    let mut effective = requested;
    if effective == Arrangement::Three && width < history_min + files_min + comparison_min + 2 * gap
    {
        effective = Arrangement::TwoFilesLeft;
    }
    let side_min = if active == Column::History {
        history_min
    } else {
        files_min
    };
    if matches!(
        effective,
        Arrangement::TwoFilesLeft | Arrangement::TwoFilesRight
    ) && width < side_min + comparison_min + gap
    {
        effective = Arrangement::One;
    }
    let mut geometry = Geometry {
        rect: rect(0, top, width, bottom),
        toolbar: rect(0, top, width, body_top),
        history: None,
        files: None,
        preview: None,
        dividers: Vec::with_capacity(2),
    };
    let area = |left, right| rect(left, body_top, right, body_bottom);
    let preferred = |ratio: u16| ((i64::from(width) * i64::from(ratio)) / i64::from(SCALE)) as i32;
    match effective {
        Arrangement::One => {
            let whole = area(0, width);
            match active {
                Column::History => geometry.history = Some(whole),
                Column::Files => geometry.files = Some(whole),
                Column::Comparison => geometry.preview = Some(whole),
            }
        }
        Arrangement::TwoFilesLeft | Arrangement::TwoFilesRight => {
            let wanted = preferred(prefs.first);
            let (side, preview, divider) = if effective == Arrangement::TwoFilesLeft {
                let center = wanted.clamp(side_min + half, width - comparison_min - remaining_half);
                (
                    area(0, center - half),
                    area(center + remaining_half, width),
                    area(center - half, center + remaining_half),
                )
            } else {
                let center = (width - wanted)
                    .clamp(comparison_min + half, width - side_min - remaining_half);
                (
                    area(center + remaining_half, width),
                    area(0, center - half),
                    area(center - half, center + remaining_half),
                )
            };
            if active == Column::History {
                geometry.history = Some(side);
            } else {
                geometry.files = Some(side);
            }
            geometry.preview = Some(preview);
            if body_bottom > body_top {
                geometry.dividers.push((divider, Divider::First));
            }
        }
        Arrangement::Three => {
            let first = preferred(prefs.first).clamp(
                history_min + half,
                width - files_min - comparison_min - gap - remaining_half,
            );
            let second = preferred(prefs.second).clamp(
                first + files_min + gap,
                width - comparison_min - remaining_half,
            );
            geometry.history = Some(area(0, first - half));
            geometry.files = Some(area(first + remaining_half, second - half));
            geometry.preview = Some(area(second + remaining_half, width));
            if body_bottom > body_top {
                geometry
                    .dividers
                    .push((area(first - half, first + remaining_half), Divider::First));
                geometry.dividers.push((
                    area(second - half, second + remaining_half),
                    Divider::Second,
                ));
            }
        }
    }
    geometry
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pixels(dpi: i32, value: i32) -> i32 {
        ((i64::from(value) * i64::from(dpi) + 48) / 96) as i32
    }
    fn width(r: Rect) -> i32 {
        r.right - r.left
    }
    fn rect_tuple(r: Rect) -> (i32, i32, i32, i32) {
        (r.left, r.top, r.right, r.bottom)
    }
    fn check_tiling(g: &Geometry, width: i32) {
        let mut columns = [g.history, g.files, g.preview]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        columns.extend(g.dividers.iter().map(|(r, _)| *r));
        columns.sort_by_key(|r| r.left);
        assert_eq!(columns.first().unwrap().left, 0);
        assert_eq!(columns.last().unwrap().right, width);
        for pair in columns.windows(2) {
            assert_eq!(pair[0].right, pair[1].left);
            assert_eq!(pair[0].top, pair[1].top);
            assert_eq!(pair[0].bottom, pair[1].bottom);
        }
        for r in columns {
            assert!(r.left >= 0 && r.right >= r.left && r.right <= width);
            assert!(r.top >= g.toolbar.bottom && r.bottom >= r.top && r.bottom <= g.rect.bottom);
        }
    }
    #[test]
    fn arrangements_tile_at_three_dpi_scales_and_compact_modes_keep_active_column() {
        for dpi in [96, 144, 192] {
            for arrangement in Arrangement::ALL {
                for active in [Column::History, Column::Files, Column::Comparison] {
                    for design_width in [0, 1, 320, 450, 500, 612, 800, 1200, 8192] {
                        let w = pixels(dpi, design_width);
                        let h = pixels(dpi, 700);
                        let g = layout(
                            w,
                            h,
                            pixels(dpi, 60),
                            h - pixels(dpi, 20),
                            |v| pixels(dpi, v),
                            Preferences {
                                arrangement,
                                ..Default::default()
                            },
                            active,
                        );
                        check_tiling(&g, w);
                        assert_eq!(g.toolbar.bottom - g.toolbar.top, pixels(dpi, 76));
                        if design_width <= 320 || arrangement == Arrangement::One {
                            assert_eq!(
                                usize::from(g.history.is_some())
                                    + usize::from(g.files.is_some())
                                    + usize::from(g.preview.is_some()),
                                1
                            );
                            assert!(match active {
                                Column::History => g.history.is_some(),
                                Column::Files => g.files.is_some(),
                                Column::Comparison => g.preview.is_some(),
                            });
                        }
                        if g.dividers.len() == 2 {
                            assert!(width(g.history.unwrap()) >= pixels(dpi, 140));
                            assert!(width(g.files.unwrap()) >= pixels(dpi, 160));
                            assert!(width(g.preview.unwrap()) >= pixels(dpi, 300));
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn two_column_orientation_and_history_tab_replace_the_file_side() {
        let prefs = Preferences {
            arrangement: Arrangement::TwoFilesRight,
            ..Default::default()
        };
        let files = layout(1200, 800, 0, 800, |x| x, prefs, Column::Files);
        let history = layout(1200, 800, 0, 800, |x| x, prefs, Column::History);
        assert!(files.preview.unwrap().right < files.files.unwrap().left);
        assert_eq!(
            rect_tuple(files.files.unwrap()),
            rect_tuple(history.history.unwrap())
        );
        assert!(history.files.is_none());
        assert_eq!(
            rect_tuple(files.preview.unwrap()),
            rect_tuple(history.preview.unwrap())
        );
    }
    #[test]
    fn persistent_preferences_roundtrip_and_reject_corrupt_or_overlapping_values() {
        for arrangement in Arrangement::ALL {
            for parallel in [false, true] {
                let value = Preferences {
                    arrangement,
                    parallel,
                    first: 250,
                    second: 575,
                };
                assert_eq!(Preferences::decode(&value.encode()).unwrap(), value);
            }
        }
        let valid = Preferences::default().encode();
        assert!(Preferences::decode(&valid[..6]).is_err());
        for (index, value) in [(0, 2), (1, 4), (6, 2)] {
            let mut invalid = valid;
            invalid[index] = value;
            assert!(Preferences::decode(&invalid).is_err());
        }
        let mut overlap = valid;
        overlap[4..6].copy_from_slice(&200u16.to_le_bytes());
        assert!(Preferences::decode(&overlap).is_err());
        let mut outside = valid;
        outside[2..4].copy_from_slice(&0u16.to_le_bytes());
        assert!(Preferences::decode(&outside).is_err());
        assert!(Preferences::decode(&[]).is_err());
    }
    #[test]
    fn divider_drag_preserves_valid_ratios_and_layout_minimums() {
        for arrangement in [
            Arrangement::Three,
            Arrangement::TwoFilesLeft,
            Arrangement::TwoFilesRight,
        ] {
            let mut prefs = Preferences {
                arrangement,
                ..Default::default()
            };
            for divider in [Divider::First, Divider::Second] {
                for x in [i32::MIN, -1, 0, 200, 800, 1000, i32::MAX] {
                    prefs.drag(divider, x, 1000);
                    assert!(prefs.valid());
                    assert_eq!(Preferences::decode(&prefs.encode()).unwrap(), prefs);
                    let g = layout(1000, 700, 0, 700, |x| x, prefs, Column::Files);
                    check_tiling(&g, 1000);
                    assert!(width(g.preview.unwrap()) >= 300);
                }
            }
        }
        let mut prefs = Preferences::default();
        let before = prefs;
        assert!(!prefs.drag(Divider::First, 1, 0));
        assert_eq!(prefs, before);
        prefs.arrangement = Arrangement::One;
        assert!(!prefs.drag(Divider::First, 800, 1000));
        prefs.arrangement = Arrangement::TwoFilesRight;
        prefs.drag(Divider::First, 700, 1000);
        assert_eq!(prefs.first, 300);
        let mut invalid = Preferences {
            first: u16::MAX,
            second: 0,
            ..Default::default()
        };
        invalid.drag(Divider::Second, 100, 1000);
        assert!(invalid.valid());
    }
    #[test]
    fn tiny_vertical_geometry_never_places_toolbar_or_columns_outside_the_client() {
        for h in [0, 1, 20, 60, 99, 100] {
            let g = layout(
                320,
                h,
                60,
                h - 1,
                |x| x,
                Preferences::default(),
                Column::Comparison,
            );
            assert!(g.rect.top <= g.rect.bottom && g.rect.bottom <= h);
            assert!(g.toolbar.bottom <= g.rect.bottom);
            let r = g.preview.unwrap();
            assert!(r.top <= r.bottom && r.bottom <= g.rect.bottom);
        }
    }
}
