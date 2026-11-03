//! Shared color data only. Each frontend owns its geometry, controls and paint.
use crate::syntax_theme::Rgb;
/// Exact solid tone sampled from the owner's reference image, 2026-11-03.
pub const BACKGROUND: Rgb = Rgb {
    r: 46,
    g: 52,
    b: 64,
};
pub const CHROME: Rgb = BACKGROUND;
pub const PANEL: Rgb = Rgb {
    r: 39,
    g: 44,
    b: 54,
};
pub const RAISED: Rgb = Rgb {
    r: 59,
    g: 66,
    b: 82,
};
pub const SELECTED: Rgb = Rgb {
    r: 67,
    g: 76,
    b: 94,
};
pub const BORDER: Rgb = Rgb {
    r: 76,
    g: 86,
    b: 106,
};
