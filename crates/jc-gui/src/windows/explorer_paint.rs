//! Proportional, pixel-based tree chrome over the bounded shared catalog frame.
use super::{
    explorer_panel::ExplorerFrame,
    ffi::{Hdc, Rect},
    paint, shell, wide,
};
pub(super) unsafe fn draw(dc: Hdc, frame: &ExplorerFrame, _ch: i32) -> Result<(), String> {
    let r = frame.rect;
    paint::fill(dc, r, shell::SIDEBAR)?;
    paint::text(
        dc,
        Rect {
            left: r.left + 18,
            top: r.top + 16,
            right: r.right - 10,
            bottom: r.top + 35,
        },
        &wide("CARPETAS"),
        shell::TEXT,
        shell::SIDEBAR,
    )?;
    paint::text(
        dc,
        Rect {
            left: r.left + 18,
            top: r.top + 38,
            right: r.right - 10,
            bottom: r.top + 57,
        },
        &wide("F6 · cambiar de panel"),
        shell::MUTED,
        shell::SIDEBAR,
    )?;
    for (i, row) in frame.presentation.rows.iter().take(128).enumerate() {
        let top = r.top + shell::TREE_HEAD + i as i32 * shell::TREE_ROW;
        if top >= r.bottom {
            break;
        }
        let bg = if row.selected {
            shell::SELECTED
        } else {
            shell::SIDEBAR
        };
        paint::fill(
            dc,
            Rect {
                left: r.left + 6,
                top,
                right: r.right - 6,
                bottom: (top + shell::TREE_ROW).min(r.bottom),
            },
            bg,
        )?;
        let x = r.left + 12 + row.depth.min(12) as i32 * 14;
        if row.is_dir {
            shell::chevron(dc, x, top + 9, row.expanded, shell::MUTED)?;
        }
        shell::file_icon(
            dc,
            x + 13,
            top + 6,
            row.is_dir,
            if row.is_dir {
                shell::ACCENT
            } else {
                shell::MUTED
            },
        )?;
        paint::text(
            dc,
            Rect {
                left: x + 34,
                top: top + 4,
                right: r.right - 10,
                bottom: (top + shell::TREE_ROW).min(r.bottom),
            },
            &wide(&row.label),
            shell::TEXT,
            bg,
        )?;
        if row.selected && frame.focused {
            paint::fill(
                dc,
                Rect {
                    left: r.left + 6,
                    top,
                    right: r.left + 8,
                    bottom: (top + shell::TREE_ROW).min(r.bottom),
                },
                shell::ACCENT,
            )?;
        }
    }
    if frame.presentation.rows.is_empty() {
        paint::text(
            dc,
            Rect {
                left: r.left + 18,
                top: r.top + shell::TREE_HEAD + 6,
                right: r.right - 10,
                bottom: (r.top + shell::TREE_HEAD + 54).min(r.bottom),
            },
            &wide(&frame.presentation.message),
            shell::MUTED,
            shell::SIDEBAR,
        )?;
    }
    paint::fill(
        dc,
        Rect {
            left: r.right - 1,
            ..r
        },
        shell::SURFACE,
    )?;
    Ok(())
}
