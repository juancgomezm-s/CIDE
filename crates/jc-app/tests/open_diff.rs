use jc_app::{
    diff_model::DiffSide,
    open_diff::{OpenDiffController, OpenDiffState},
    open_diff_view::OpenDiffView,
    JobLimits, JobSystem, PaneId, Wakeup, Workbench,
};
use jc_text::TextEncoding;
use std::time::{Duration, Instant};
fn pair(left: &str, right: &str) -> Workbench {
    let mut a = Workbench::default();
    a.new_file("left-memory.txt", TextEncoding::Utf8);
    a.insert_text(left).unwrap();
    a.split_editor_right();
    a.focus_editor_pane(PaneId::Secondary);
    a.new_file("right-memory.txt", TextEncoding::Windows1252);
    a.insert_text(right).unwrap();
    a
}
fn complete(c: &mut OpenDiffController, a: &Workbench, jobs: &mut JobSystem, wake: &Wakeup) {
    let until = Instant::now() + Duration::from_secs(5);
    while matches!(c.state(), OpenDiffState::Queued | OpenDiffState::Running)
        || jobs.outstanding() > 0
    {
        let ticket = wake.ticket();
        for mut task in jobs.poll(a, 4) {
            c.accept(a, &mut task);
        }
        c.tick(a, jobs);
        assert!(Instant::now() < until);
        if jobs.outstanding() > 0 && !jobs.has_completed() {
            wake.wait(ticket, Some(Duration::from_millis(10)));
        }
    }
}
#[test]
fn open_diff_dirty_sources_two_encodings_navigation_and_readonly_views() {
    let mut a = pair("same\r\nniño\nlast", "same\r\nother\rlast");
    let left = a
        .panes()
        .view(PaneId::Primary)
        .unwrap()
        .location()
        .unwrap()
        .tab;
    let right = a.tabs().active_id().unwrap();
    let before = format!("{:?}", a.panes());
    let wake = Wakeup::default();
    let mut jobs = JobSystem::new(Default::default(), wake.clone()).unwrap();
    let mut c = OpenDiffController::new();
    c.request(&a, &mut jobs).unwrap();
    assert_eq!(
        jobs.snapshot_bytes(),
        a.document_by_tab(left).unwrap().session.document().len()
            + a.document_by_tab(right).unwrap().session.document().len()
    );
    complete(&mut c, &a, &mut jobs, &wake);
    assert_eq!(c.state(), OpenDiffState::Ready);
    assert_eq!(jobs.snapshot_bytes(), 0);
    assert_eq!(before, format!("{:?}", a.panes()));
    let r = c.result().unwrap();
    assert_eq!(
        r.model.source_bytes(DiffSide::Left),
        "same\r\nniño\nlast".as_bytes()
    );
    assert_eq!(
        r.model.source_bytes(DiffSide::Right),
        b"same\r\nother\rlast"
    );
    a.toggle_reference_pin();
    c.select_hunk(0, DiffSide::Left, &mut a).unwrap();
    assert_eq!(a.panes().active(), PaneId::Primary);
    assert_eq!(a.active_editor().unwrap().cursor, 6);
    c.select_hunk(0, DiffSide::Right, &mut a).unwrap();
    assert_eq!(a.panes().active(), PaneId::Secondary);
    assert_eq!(a.active_editor().unwrap().cursor, 6);
    assert_eq!(a.active_bytes().unwrap(), b"same\r\nother\rlast");
}
#[test]
fn open_diff_both_origins_reject_late_results_after_edit_encoding_group_and_undo() {
    for change in 0..4 {
        let mut a = pair("a\r\nleft", "a\r\nright");
        let wake = Wakeup::default();
        let mut jobs = JobSystem::new(Default::default(), wake.clone()).unwrap();
        let mut c = OpenDiffController::new();
        c.request(&a, &mut jobs).unwrap();
        a.focus_editor_pane(if change == 0 {
            PaneId::Primary
        } else {
            PaneId::Secondary
        });
        match change {
            0 => {
                a.insert_text("changed").unwrap();
            }
            1 => {
                a.set_encoding(TextEncoding::Iso8859_1).unwrap();
            }
            2 => {
                a.execute(jc_app::CommandId::EditorOneGroup).unwrap();
            }
            _ => {
                a.undo().unwrap();
            }
        }
        assert!(c.validate_context(&a, &mut jobs));
        complete(&mut c, &a, &mut jobs, &wake);
        assert_eq!(c.state(), OpenDiffState::Stale);
        assert!(c.result().is_none());
        assert_eq!(jobs.snapshot_bytes(), 0);
        assert!(c.select_hunk(0, DiffSide::Right, &mut a).is_err());
    }
}
#[test]
fn open_diff_refresh_storm_retains_one_flight_and_only_latest_metadata() {
    let a = pair(&"old\n".repeat(100), &"new\n".repeat(100));
    let wake = Wakeup::default();
    let mut jobs = JobSystem::new(Default::default(), wake.clone()).unwrap();
    let mut c = OpenDiffController::new();
    c.request(&a, &mut jobs).unwrap();
    let charge = jobs.snapshot_bytes();
    for _ in 0..20 {
        c.request(&a, &mut jobs).unwrap();
        assert!(jobs.outstanding() <= 1);
        assert_eq!(jobs.snapshot_bytes(), charge);
    }
    complete(&mut c, &a, &mut jobs, &wake);
    assert_eq!(c.state(), OpenDiffState::Ready);
    let workers = jobs.worker_count();
    c.cancel(&mut jobs);
    assert!(c.result().is_none());
    assert_eq!(c.state(), OpenDiffState::Cancelled);
    c.request(&a, &mut jobs).unwrap();
    complete(&mut c, &a, &mut jobs, &wake);
    assert_eq!(jobs.worker_count(), workers);
    assert_eq!(c.state(), OpenDiffState::Ready);
}
#[test]
fn open_diff_admission_checks_combined_snapshot_limit_before_creating_workers() {
    let a = pair("left", "rite");
    let wake = Wakeup::default();
    let mut jobs = JobSystem::new(
        JobLimits {
            snapshot_bytes: 4,
            total_snapshot_bytes: 7,
            ..JobLimits::default()
        },
        wake,
    )
    .unwrap();
    let mut c = OpenDiffController::new();
    assert!(c.request(&a, &mut jobs).is_err());
    assert_eq!(c.state(), OpenDiffState::Limited);
    assert_eq!(jobs.snapshot_bytes(), 0);
    assert_eq!(jobs.worker_count(), 0);
    let mut same = Workbench::default();
    same.new_file("one", TextEncoding::Utf8);
    same.split_editor_right();
    assert!(c.request(&same, &mut jobs).is_err());
    assert_eq!(jobs.worker_count(), 0);
}
#[test]
fn open_diff_view_escapes_sources_uses_side_encoding_and_bounds_rows_columns_and_scroll() {
    let a = pair("\u{feff}niño\r\nold\t\nlast", "niño\r\nnew\x1b\rlast");
    let wake = Wakeup::default();
    let mut jobs = JobSystem::new(Default::default(), wake.clone()).unwrap();
    let mut c = OpenDiffController::new();
    c.request(&a, &mut jobs).unwrap();
    complete(&mut c, &a, &mut jobs, &wake);
    let mut v = OpenDiffView::default();
    let dual = v.prepare(&c, 96, 140, true);
    assert!(dual
        .rows
        .iter()
        .any(|r| r.label.contains("[BOM]ni\\u{F1}o")));
    assert!(dual.rows.iter().any(|r| r.label.contains("ni\\u{F1}o")));
    assert!(dual.rows.iter().any(|r| r.label.contains("[CRLF]")));
    let unified = v.prepare(&c, 96, 60, false);
    assert!(unified.rows.iter().any(|r| r.label.starts_with("- ")));
    assert!(unified.rows.iter().any(|r| r.label.starts_with("+ ")));
    for width in [0, 1, 45, 140, usize::MAX] {
        let p = v.prepare(&c, usize::MAX, width, true);
        assert!(p.rows.len() <= 96);
        assert!(p.rows.iter().all(|r| r.label.len() <= width.min(256)
            && r.label.is_ascii()
            && !r.label.contains('\x1b')));
    }
    let many = pair("", &"new\n".repeat(600));
    let wake = Wakeup::default();
    let mut jobs = JobSystem::new(Default::default(), wake.clone()).unwrap();
    let mut c = OpenDiffController::new();
    c.request(&many, &mut jobs).unwrap();
    complete(&mut c, &many, &mut jobs, &wake);
    v.prepare(&c, 10, 45, false);
    v.last();
    let end = v.prepare(&c, 10, 45, false);
    assert!(end.rows.last().unwrap().label.contains("600 "));
    v.scroll(-1);
    let up = v.prepare(&c, 10, 45, false);
    assert!(up.rows.last().unwrap().label.contains("599 "));
}
