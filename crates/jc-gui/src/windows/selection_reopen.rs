//! Native intentions over existing document search and closed-tab metadata.
use super::State;
use jc_app::CommandId;
impl State {
    pub(super) fn search_selection(&mut self, command: CommandId) {
        match self.workbench.execute(command) {
            Ok(_) => {
                self.status = "Literal selection search requested · F3 / Shift+F3 continues".into();
                self.schedule_search();
            }
            Err(error) => self.error(error),
        }
    }
    pub(super) fn reopen_tab(&mut self) {
        match self.workbench.try_reopen_closed() {
            Ok(true) => {
                self.close_selector();
                self.ensure_encoding_selector();
                self.status = "Tab reopened · discarded changes are not restored".into();
                self.schedule_search();
            }
            Ok(false) => self.status = "No closed tab available".into(),
            Err(error) => self.error(error),
        }
    }
}
