//! Bounded persistent view metadata. Documents/undo remain in their one owner.
use super::{
    EditorView, NavLocation, PaneId, PaneManager, PaneView, RectangularSelection, ViewId,
    ViewState, HISTORY_LIMIT,
};
use crate::{
    rectangle::{VisualColumn, MAX_COLUMNS},
    TabId,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read, Write},
};

const MAGIC: &[u8; 8] = b"CIDEPAN1";
const VERSION: u32 = 1;
const MAX_TABS: usize = 4096;
const MAX_VIEWS: usize = 8192;
#[derive(Clone, Copy)]
struct TabMapping {
    target: TabId,
    len: usize,
    revision: u64,
}

impl PaneManager {
    pub(crate) fn write_recovery(&self, writer: &mut dyn Write) -> io::Result<()> {
        validate_manager(self)?;
        writer.write_all(MAGIC)?;
        u32_out(writer, VERSION)?;
        let flags = u32::from(self.secondary_open)
            | (u32::from(self.active == PaneId::Secondary) << 1)
            | (u32::from(self.scroll_linked) << 2);
        u32_out(writer, flags)?;
        u64_out(writer, self.next_view)?;
        for group in [&self.primary, &self.secondary] {
            u32_out(writer, u32::from(group.pinned))?;
            u32_out(writer, group.views.len() as u32)?;
            u64_out(writer, group.active.map_or(0, |id| id.0))?;
            u32_out(writer, group.back.len() as u32)?;
            u32_out(writer, group.forward.len() as u32)?;
            for view in &group.views {
                u64_out(writer, view.id.0)?;
                u64_out(writer, view.tab.0)?;
                usize_out(writer, view.state.cursor)?;
                let flags = u32::from(view.state.anchor.is_some())
                    | (u32::from(view.state.rectangle.is_some()) << 1)
                    | (u32::from(view.state.manual_scroll) << 2);
                u32_out(writer, flags)?;
                usize_out(writer, view.state.anchor.unwrap_or(0))?;
                usize_out(writer, view.state.scroll_line)?;
                usize_out(writer, view.state.scroll_column)?;
                if let Some(rectangle) = view.state.rectangle {
                    usize_out(writer, rectangle.anchor_column.0)?;
                    usize_out(writer, rectangle.head_column.0)?;
                    usize_out(writer, rectangle.scroll_column.0)?;
                    u64_out(writer, rectangle.revision)?;
                }
            }
            for location in group.back.iter().chain(&group.forward) {
                u64_out(writer, location.tab.0)?;
                usize_out(writer, location.byte_offset)?;
            }
        }
        Ok(())
    }
    /// old tab, new tab, exact restored document length, exact restored revision.
    /// View IDs are newly assigned above the persisted allocator watermark.
    /// Source scalar/grapheme/CRLF boundary normalization belongs to Workbench.
    pub(crate) fn read_recovery(
        reader: &mut dyn Read,
        tabs: &[(TabId, TabId, usize, u64)],
    ) -> io::Result<Self> {
        if tabs.len() > MAX_TABS {
            return Err(invalid("demasiados documentos en los grupos"));
        }
        let mut mappings = BTreeMap::new();
        let mut targets = BTreeSet::new();
        for &(old, target, len, revision) in tabs {
            if mappings
                .insert(
                    old,
                    TabMapping {
                        target,
                        len,
                        revision,
                    },
                )
                .is_some()
                || !targets.insert(target)
            {
                return Err(invalid("identidades de documento duplicadas en los grupos"));
            }
        }
        let mut magic = [0; 8];
        reader.read_exact(&mut magic)?;
        if &magic != MAGIC || u32_in(reader)? != VERSION {
            return Err(invalid("versión de grupos de recuperación no admitida"));
        }
        let flags = u32_in(reader)?;
        if flags & !7 != 0 || flags & 1 == 0 && flags & 6 != 0 {
            return Err(invalid("estado de grupos de recuperación inválido"));
        }
        let mut next_view = u64_in(reader)?;
        if next_view == 0 || next_view.checked_add(MAX_VIEWS as u64 + 1).is_none() {
            return Err(invalid("identidad inicial de vista inválida o agotada"));
        }
        let old_next = next_view;
        let mut views = BTreeSet::new();
        let mut count = 0;
        let primary = read_group(
            reader,
            &mappings,
            &mut views,
            &mut count,
            &mut next_view,
            old_next,
        )?;
        let secondary = read_group(
            reader,
            &mappings,
            &mut views,
            &mut count,
            &mut next_view,
            old_next,
        )?;
        let manager = Self {
            secondary_open: flags & 1 != 0,
            active: if flags & 2 != 0 {
                PaneId::Secondary
            } else {
                PaneId::Primary
            },
            primary,
            secondary,
            scroll_linked: flags & 4 != 0,
            next_view,
        };
        validate_manager(&manager)?;
        Ok(manager)
    }
}

fn read_group(
    reader: &mut dyn Read,
    mappings: &BTreeMap<TabId, TabMapping>,
    used: &mut BTreeSet<u64>,
    total: &mut usize,
    next: &mut u64,
    old_next: u64,
) -> io::Result<PaneView> {
    let flags = u32_in(reader)?;
    let count = u32_in(reader)? as usize;
    let active = u64_in(reader)?;
    let back = u32_in(reader)? as usize;
    let forward = u32_in(reader)? as usize;
    if flags & !1 != 0
        || count > mappings.len()
        || count > MAX_VIEWS.saturating_sub(*total)
        || back > HISTORY_LIMIT
        || forward > HISTORY_LIMIT
        || back.saturating_add(forward) > HISTORY_LIMIT
    {
        return Err(invalid("cantidad o banderas del grupo exceden los límites"));
    }
    next.checked_add(count as u64)
        .ok_or_else(|| invalid("se agotó la identidad de vistas recuperadas"))?;
    *total += count;
    let mut group = PaneView {
        views: Vec::with_capacity(count),
        active: None,
        pinned: flags & 1 != 0,
        back: Vec::with_capacity(back),
        forward: Vec::with_capacity(forward),
    };
    let mut tabs = BTreeSet::new();
    let mut selected = active == 0;
    for _ in 0..count {
        let old = ViewId(u64_in(reader)?);
        let tab = TabId(u64_in(reader)?);
        let mapping = mappings
            .get(&tab)
            .ok_or_else(|| invalid("la vista refiere un documento desconocido"))?;
        if old.0 == 0 || old.0 >= old_next || !used.insert(old.0) || !tabs.insert(tab) {
            return Err(invalid(
                "identidad de vista o documento duplicada en un grupo",
            ));
        }
        let cursor = usize_in(reader)?;
        let flags = u32_in(reader)?;
        let anchor = usize_in(reader)?;
        let scroll_line = usize_in(reader)?;
        let scroll_column = usize_in(reader)?;
        if flags & !7 != 0
            || cursor > mapping.len
            || flags & 1 != 0 && anchor > mapping.len
            || flags & 1 == 0 && anchor != 0
        {
            return Err(invalid("posición o selección de vista fuera del documento"));
        }
        let rectangle = if flags & 2 != 0 {
            let anchor_column = usize_in(reader)?;
            let head_column = usize_in(reader)?;
            let scroll = usize_in(reader)?;
            let revision = u64_in(reader)?;
            if anchor_column > MAX_COLUMNS
                || head_column > MAX_COLUMNS
                || scroll > MAX_COLUMNS
                || revision != mapping.revision
            {
                return Err(invalid("rectángulo de vista obsoleto o fuera de límites"));
            }
            Some(RectangularSelection {
                anchor_column: VisualColumn(anchor_column),
                head_column: VisualColumn(head_column),
                scroll_column: VisualColumn(scroll),
                revision,
            })
        } else {
            None
        };
        let id = ViewId(*next);
        *next += 1;
        if old.0 == active {
            group.active = Some(id);
            selected = true;
        }
        group.views.push(EditorView {
            id,
            tab: mapping.target,
            state: ViewState {
                cursor,
                anchor: (flags & 1 != 0).then_some(anchor),
                rectangle,
                scroll_line,
                scroll_column,
                manual_scroll: flags & 4 != 0,
            },
        });
    }
    if !selected {
        return Err(invalid("la vista activa no pertenece al grupo"));
    }
    for (count, stack) in [(back, &mut group.back), (forward, &mut group.forward)] {
        for _ in 0..count {
            let old = TabId(u64_in(reader)?);
            let offset = usize_in(reader)?;
            let mapping = mappings
                .get(&old)
                .ok_or_else(|| invalid("el historial refiere un documento desconocido"))?;
            if offset > mapping.len {
                return Err(invalid("posición de historial fuera del documento"));
            }
            stack.push(NavLocation::new(mapping.target, offset));
        }
    }
    Ok(group)
}
fn validate_manager(manager: &PaneManager) -> io::Result<()> {
    if !manager.secondary_open
        && (manager.active == PaneId::Secondary
            || manager.scroll_linked
            || !manager.secondary.views.is_empty()
            || manager.secondary.active.is_some()
            || manager.secondary.pinned
            || !manager.secondary.back.is_empty()
            || !manager.secondary.forward.is_empty())
        || manager.primary.pinned && manager.secondary.pinned
    {
        return Err(invalid("grupos cerrados o anclajes incompatibles"));
    }
    let mut views = BTreeSet::new();
    let mut tabs = BTreeSet::new();
    let total = manager
        .primary
        .views
        .len()
        .checked_add(manager.secondary.views.len())
        .ok_or_else(|| invalid("desbordamiento de vistas"))?;
    if total > MAX_VIEWS
        || manager.next_view == 0
        || manager
            .next_view
            .checked_add(MAX_VIEWS as u64 + 1)
            .is_none()
    {
        return Err(invalid(
            "demasiadas vistas persistentes o identidades agotadas",
        ));
    }
    for group in [&manager.primary, &manager.secondary] {
        if group.views.len() > MAX_TABS
            || group.back.len().saturating_add(group.forward.len()) > HISTORY_LIMIT
        {
            return Err(invalid("grupo o historial excede su cuota"));
        }
        let mut group_tabs = BTreeSet::new();
        for view in &group.views {
            if view.id.0 == 0
                || view.id.0 >= manager.next_view
                || !views.insert(view.id.0)
                || !group_tabs.insert(view.tab)
            {
                return Err(invalid("identidad de vista duplicada o inválida"));
            }
            tabs.insert(view.tab);
            if let Some(rectangle) = view.state.rectangle {
                if rectangle.anchor_column.0 > MAX_COLUMNS
                    || rectangle.head_column.0 > MAX_COLUMNS
                    || rectangle.scroll_column.0 > MAX_COLUMNS
                {
                    return Err(invalid("columnas rectangulares fuera de límites"));
                }
            }
        }
        if group
            .active
            .is_some_and(|id| !group.views.iter().any(|view| view.id == id))
        {
            return Err(invalid("referencia de vista activa inválida"));
        }
        tabs.extend(
            group
                .back
                .iter()
                .chain(&group.forward)
                .map(|location| location.tab),
        );
        if tabs.len() > MAX_TABS {
            return Err(invalid("demasiados documentos referidos por los grupos"));
        }
    }
    Ok(())
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn u32_out(writer: &mut dyn Write, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}
fn u64_out(writer: &mut dyn Write, value: u64) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}
fn usize_out(writer: &mut dyn Write, value: usize) -> io::Result<()> {
    u64_out(
        writer,
        u64::try_from(value).map_err(|_| invalid("coordenada no representable"))?,
    )
}
fn u32_in(reader: &mut dyn Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn u64_in(reader: &mut dyn Read) -> io::Result<u64> {
    let mut bytes = [0; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}
fn usize_in(reader: &mut dyn Read) -> io::Result<usize> {
    usize::try_from(u64_in(reader)?)
        .map_err(|_| invalid("coordenada de vista no representable en esta plataforma"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn mapping() -> [(TabId, TabId, usize, u64); 3] {
        [
            (TabId(1), TabId(101), 200, 7),
            (TabId(2), TabId(102), 200, 8),
            (TabId(3), TabId(103), 200, 9),
        ]
    }
    fn manager() -> PaneManager {
        let mut panes = PaneManager::default();
        panes.navigate(NavLocation::new(TabId(1), 3), false);
        {
            let state = panes.active_state_mut().unwrap();
            state.anchor = Some(1);
            state.scroll_line = 11;
            state.scroll_column = 17;
            state.manual_scroll = true;
        }
        panes.navigate(NavLocation::new(TabId(2), 8), false);
        panes.split_right();
        panes.focus(PaneId::Secondary);
        panes.navigate(NavLocation::new(TabId(3), 9), false);
        {
            let state = panes.active_state_mut().unwrap();
            state.anchor = Some(3);
            state.rectangle = Some(RectangularSelection {
                anchor_column: VisualColumn(2),
                head_column: VisualColumn(8),
                scroll_column: VisualColumn(1),
                revision: 9,
            });
            state.scroll_line = 4;
            state.scroll_column = 12;
            state.manual_scroll = true;
        }
        panes.toggle_pin();
        panes.toggle_linked_scroll();
        panes.primary.forward.push(NavLocation::new(TabId(3), 17));
        panes.secondary.forward.push(NavLocation::new(TabId(1), 18));
        panes
    }
    #[test]
    fn panes_recovery_preserves_all_inactive_views_split_link_pin_rectangles_and_group_history() {
        let original = manager();
        let mut bytes = vec![];
        original.write_recovery(&mut bytes).unwrap();
        let mut input = Cursor::new(&bytes);
        let restored = PaneManager::read_recovery(&mut input, &mapping()).unwrap();
        assert_eq!(input.position(), bytes.len() as u64);
        assert!(restored.secondary_open && restored.scroll_linked && restored.secondary.pinned);
        assert_eq!(restored.active, PaneId::Secondary);
        for (old_group, new_group) in [
            (&original.primary, &restored.primary),
            (&original.secondary, &restored.secondary),
        ] {
            assert_eq!(old_group.views.len(), new_group.views.len());
            for (old, new) in old_group.views.iter().zip(&new_group.views) {
                assert_ne!(old.id, new.id);
                assert_eq!(new.tab.0, old.tab.0 + 100);
                assert_eq!(format!("{:?}", old.state), format!("{:?}", new.state));
            }
            for (old, new) in old_group
                .back
                .iter()
                .chain(&old_group.forward)
                .zip(new_group.back.iter().chain(&new_group.forward))
            {
                assert_eq!(new.tab.0, old.tab.0 + 100);
                assert_eq!(new.byte_offset, old.byte_offset);
            }
            let old_active = old_group.editor().unwrap();
            let new_active = new_group.editor().unwrap();
            assert_eq!(new_active.tab.0, old_active.tab.0 + 100);
        }
        assert_eq!(restored.primary.views[0].state.selection(), Some(1..3));
    }
    #[test]
    fn panes_recovery_rejects_truncation_counts_duplicate_views_unknown_refs_and_stale_rectangles()
    {
        let original = manager();
        let mut bytes = vec![];
        original.write_recovery(&mut bytes).unwrap();
        for end in 0..bytes.len() {
            assert!(
                PaneManager::read_recovery(&mut Cursor::new(&bytes[..end]), &mapping()).is_err(),
                "prefix{end}"
            );
        }
        let mut count = bytes.clone();
        count[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(PaneManager::read_recovery(&mut Cursor::new(count), &mapping()).is_err());
        let mut active = bytes.clone();
        active[32..40].copy_from_slice(&99999u64.to_le_bytes());
        assert!(PaneManager::read_recovery(&mut Cursor::new(active), &mapping()).is_err());
        let mut overflow = bytes.clone();
        overflow[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(PaneManager::read_recovery(&mut Cursor::new(overflow), &mapping()).is_err());
        let mut history = bytes.clone();
        history[40..44].copy_from_slice(&129u32.to_le_bytes());
        assert!(PaneManager::read_recovery(&mut Cursor::new(history), &mapping()).is_err());
        let mut duplicate = bytes.clone();
        duplicate[100..108].copy_from_slice(&1u64.to_le_bytes());
        assert!(PaneManager::read_recovery(&mut Cursor::new(duplicate), &mapping()).is_err());
        assert!(PaneManager::read_recovery(&mut Cursor::new(&bytes), &mapping()[..2]).is_err());
        let mut stale = mapping();
        stale[2].3 = 10;
        assert!(PaneManager::read_recovery(&mut Cursor::new(&bytes), &stale).is_err());
        let mut short = mapping();
        short[0].2 = 2;
        assert!(PaneManager::read_recovery(&mut Cursor::new(&bytes), &short).is_err());
        let mut duplicate_mapping = mapping();
        duplicate_mapping[1].1 = duplicate_mapping[0].1;
        assert!(PaneManager::read_recovery(&mut Cursor::new(&bytes), &duplicate_mapping).is_err());
        let mut bad = original.clone();
        bad.secondary.views[0].id = bad.primary.views[0].id;
        assert!(bad.write_recovery(&mut vec![]).is_err());
        let mut bad = original;
        bad.primary.views.push(bad.primary.views[0].clone());
        assert!(bad.write_recovery(&mut vec![]).is_err());
    }
    #[test]
    fn panes_recovery_empty_and_history_only_groups_remain_bounded_without_consuming_following_metadata(
    ) {
        let mut empty = PaneManager::default();
        empty.primary.back.push(NavLocation::new(TabId(1), 0));
        let mut bytes = vec![];
        empty.write_recovery(&mut bytes).unwrap();
        let end = bytes.len();
        bytes.extend_from_slice(b"NEXT");
        let mut input = Cursor::new(bytes);
        let restored = PaneManager::read_recovery(&mut input, &mapping()).unwrap();
        assert_eq!(input.position(), end as u64);
        assert!(restored.primary.active.is_none());
        assert_eq!(restored.primary.back, vec![NavLocation::new(TabId(101), 0)]);
        let mut empty = PaneManager {
            secondary_open: true,
            ..PaneManager::default()
        };
        empty.secondary.pinned = true;
        let mut bytes = vec![];
        empty.write_recovery(&mut bytes).unwrap();
        let restored = PaneManager::read_recovery(&mut Cursor::new(bytes), &[]).unwrap();
        assert!(restored.secondary_open && restored.secondary.pinned);
    }
}
