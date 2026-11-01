mod commands;
mod events;
mod menus;
mod overlays;
mod paths;

use crate::render;
use crate::screen::Screen;
use crate::state::{LaunchOptions, UiOverlay};
use jc_app::{
    default_keymap, default_menu_model, default_registry, CommandRegistry, FocusManager, KeyMap,
    MenuModel, PanelManager, Workbench,
};
use jc_text::TextEncoding;
use std::path::PathBuf;

pub struct TuiApp {
    pub(crate) workbench: Workbench,
    pub(crate) registry: CommandRegistry,
    pub(crate) keymap: KeyMap,
    pub(crate) menus: MenuModel,
    pub(crate) focus: FocusManager,
    pub(crate) panels: PanelManager,
    pub(crate) overlay: UiOverlay,
    pub(crate) status: String,
    pub(crate) project_root: Option<PathBuf>,
    should_exit: bool,
    default_encoding: Option<TextEncoding>,
}

impl TuiApp {
    pub fn new(options: LaunchOptions) -> Self {
        let registry = default_registry();
        let keymap = default_keymap();
        let menus = default_menu_model();
        debug_assert!(keymap.validate(&registry).is_empty());
        debug_assert!(menus.validate(&registry).is_empty());

        let mut app = Self {
            workbench: Workbench::default(),
            registry,
            keymap,
            menus,
            focus: FocusManager::default(),
            panels: PanelManager::default(),
            overlay: UiOverlay::None,
            status: "Ready".to_owned(),
            project_root: None,
            should_exit: false,
            default_encoding: options.encoding,
        };
        for path in options.files {
            if let Err(error) = app.open_path(path) {
                app.status = error;
            }
        }
        app
    }

    pub fn render(&self, width: u16, height: u16) -> Screen {
        render::render(self, width, height)
    }

    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    pub(crate) fn command_context(&self) -> jc_app::CommandContext {
        let mut context = self.workbench.context();
        context.focus = self.focus.primary();
        // Project-aware search is not enabled until its provider exists.
        context.has_project = false;
        context
    }

    pub(crate) fn request_exit(&mut self) {
        self.should_exit = true;
    }
}
