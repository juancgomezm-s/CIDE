//! Selection metadata for the native Git workspace. Rendering and diff
//! viewport state live exclusively in git_workspace.
use jc_app::git_status_tasks::{GitStatusController, GitStatusTaskState};

#[derive(Default)]
pub(super) struct GitView {
    status_selected: usize,
    status_total: usize,
}
impl GitView {
    pub(super) fn selected_status(&self) -> usize {
        self.status_selected
    }
    pub(super) fn select_status(&mut self, index: usize) {
        self.status_selected = index.min(self.status_total.saturating_sub(1));
    }
    pub(super) fn first_status(&mut self) {
        self.status_selected = 0;
    }
    pub(super) fn last_status(&mut self) {
        self.status_selected = self.status_total.saturating_sub(1);
    }
    pub(super) fn move_status(&mut self, delta: isize) {
        self.status_selected = self
            .status_selected
            .saturating_add_signed(delta)
            .min(self.status_total.saturating_sub(1));
    }
    fn normalize_status(&mut self, total: usize) {
        self.status_total = total;
        self.status_selected = self.status_selected.min(total.saturating_sub(1));
    }
    /// Normalize only accepted metadata. No row labels, source projection,
    /// provider admission or parallel viewport remains in this compatibility
    /// entry point; git_workspace owns the visible frame and its bounds.
    pub(super) fn prepare_status(
        &mut self,
        controller: &GitStatusController,
        _max_rows: usize,
        _max_cols: usize,
    ) {
        let total = if controller.state() == GitStatusTaskState::Ready {
            controller
                .result()
                .map_or(0, |result| result.status().entries().len())
        } else {
            0
        };
        self.normalize_status(total);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_app::Wakeup;
    #[test]
    fn status_selection_clamps_to_accepted_count_and_saturates_navigation() {
        let mut view = GitView::default();
        view.normalize_status(5);
        view.last_status();
        assert_eq!(view.selected_status(), 4);
        view.move_status(-1);
        assert_eq!(view.selected_status(), 3);
        view.move_status(isize::MIN);
        assert_eq!(view.selected_status(), 0);
        view.move_status(isize::MAX);
        assert_eq!(view.selected_status(), 4);
        view.select_status(usize::MAX);
        assert_eq!(view.selected_status(), 4);
        view.normalize_status(2);
        assert_eq!(view.selected_status(), 1);
        view.first_status();
        assert_eq!(view.selected_status(), 0);
        view.normalize_status(0);
        view.last_status();
        assert_eq!(view.selected_status(), 0);
    }
    #[test]
    fn idle_status_normalization_never_admits_work_or_retains_stale_selection() {
        let controller = GitStatusController::new(Wakeup::default());
        let mut view = GitView::default();
        view.normalize_status(5);
        view.last_status();
        view.prepare_status(&controller, usize::MAX, usize::MAX);
        assert_eq!(view.selected_status(), 0);
        assert_eq!(controller.state(), GitStatusTaskState::Idle);
    }
}
