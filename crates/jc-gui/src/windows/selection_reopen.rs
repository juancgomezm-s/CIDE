//! Native intentions over existing document search and closed-tab metadata.
use super::State;
use jc_app::CommandId;
impl State {
    pub(super) fn search_selection(&mut self, command: CommandId) {
        match self.workbench.execute(command) {
            Ok(_) => {
                self.status =
                    "Búsqueda literal de selección solicitada · F3/Mayús+F3 continúa".into();
                self.schedule_search();
            }
            Err(error) => self.error(error),
        }
    }
    pub(super) fn reopen_tab(&mut self) {
        match self.workbench.try_reopen_closed() {
            Ok(true) => {
                self.close_selector();
                self.bottom.focused = false;
                self.ensure_encoding_selector();
                self.status = "Pestaña reabierta · no se restauran los cambios descartados".into();
                self.schedule_search();
            }
            Ok(false) => self.status = "No hay pestaña cerrada disponible".into(),
            Err(error) => self.error(error),
        }
    }
}
