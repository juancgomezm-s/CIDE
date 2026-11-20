mod appearance;
mod back_buffer;
mod bottom_paint;
mod bottom_panel;
mod bottom_smoke;
mod chrome;
mod clipboard;
mod clipboard_flow;
mod close_flow;
mod data_connections;
mod dpi;
mod editor_scroll;
mod explorer_paint;
mod explorer_panel;
mod explorer_scroll;
mod explorer_smoke;
mod ffi;
mod folder;
mod git_code;
mod git_index_panel;
mod git_panel;
mod git_patch;
mod git_repositories;
mod git_smoke;
mod git_view;
mod git_workspace;
mod ime;
mod interaction_smoke;
mod line_edit;
mod line_edit_smoke;
mod markdown_image_http;
mod markdown_images;
mod markdown_panel;
mod menu_input;
mod menu_invoker_smoke;
mod menu_theme;
mod message_dialog;
mod native_menu;
mod navigation_paint;
mod navigation_panel;
mod navigation_smoke;
mod notify;
mod omnicore_panel;
mod omnicore_runtime;
mod omnicore_transport;
mod open_diff_panel;
mod outline_panel;
mod paint;
mod parity_smoke;
mod performance_probe;
mod pointer_input;
mod project_paint;
mod project_panel;
mod project_results;
mod project_smoke;
mod records;
mod recovery;
mod rectangle_input;
mod rectangle_smoke;
mod render_probe;
mod replace_panel;
mod replace_smoke;
mod replace_view;
mod save_flow;
mod save_plan;
mod save_smoke;
mod search_paint;
mod search_panel;
mod selection_reopen;
mod selection_reopen_smoke;
mod selector;
mod selector_flow;
mod selector_input;
mod selector_input_smoke;
mod selector_paint;
mod shell;
mod shell_smoke;
mod smoke;
mod snippet_panel;
mod source_panel;
mod status_bar;
mod syntax_smoke;
mod tab_bar;
mod uia;
mod untitled;
mod workflow_smoke;
mod zoom;
use crate::GuiOptions;
use ffi::*;
use jc_app::quick_open::QuickOpenController;
use jc_app::{
    default_keymap, default_registry, CommandId, CommandOutcome, CommandRegistry, Key, KeyMap,
    KeyScope, KeyStroke, Modifiers, PrimaryFocus, Resolution, Wakeup, Workbench,
};
use jc_text::TextEncoding;
use selector::{Selector, SelectorKind};
use std::os::windows::ffi::OsStringExt;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    ptr::null_mut,
    sync::Arc,
};

pub(super) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn os_error(operation: &str) -> String {
    format!("{operation}: {}", std::io::Error::last_os_error())
}

const OPEN_SIDE: usize = 190;
const COMMANDS: &[(usize, CommandId, &str)] = &[
    (
        220,
        CommandId::DiffCompareOpen,
        "Comparar grupos abiertos (solo lectura)",
    ),
    (
        221,
        CommandId::DiffRefreshOpen,
        "Actualizar comparación de abiertos",
    ),
    (
        222,
        CommandId::DiffCancelOpen,
        "Cancelar comparación de abiertos",
    ),
    (
        223,
        CommandId::DiffOpenNavigateLeft,
        "Abrir bloque en grupo izquierdo",
    ),
    (
        224,
        CommandId::DiffOpenNavigateRight,
        "Abrir bloque en grupo derecho",
    ),
    (230, CommandId::ViewSymbols, "Símbolos"),
    (
        231,
        CommandId::SourceReferences,
        "Referencias del documento",
    ),
    (232, CommandId::SourceComplete, "Completar palabra local"),
    (237, CommandId::SourceCalls, "Llama a"),
    (239, CommandId::SourceIncoming, "Llamado por"),
    (246, CommandId::SourceRename, "Renombrar símbolo ABL"),
    (
        249,
        CommandId::SourceLspComplete,
        "Autocompletado semántico (LSP)",
    ),
    (265, CommandId::SourceLspDiagnostics, "Comprobar con LSP"),
    (247, CommandId::SourceDefinition, "Ir a definición"),
    (248, CommandId::SourceLspReferences, "Buscar referencias"),
    (260, CommandId::SourceXref, "Buscar referencias con XREF"),
    (
        261,
        CommandId::SourceOrganizeImports,
        jc_app::source_services::ReviewedEdit::OrganizeImports.label(),
    ),
    (
        262,
        CommandId::SourceUpdatePrototype,
        jc_app::source_services::ReviewedEdit::UpdatePrototype.label(),
    ),
    (
        263,
        CommandId::SourceGenerateConstructors,
        jc_app::source_services::ReviewedEdit::GenerateConstructors.label(),
    ),
    (
        264,
        CommandId::SourceGenerateOverrides,
        jc_app::source_services::ReviewedEdit::GenerateOverrides.label(),
    ),
    (
        238,
        CommandId::SourceDependencies,
        "Dependencias e inclusiones",
    ),
    (
        233,
        CommandId::SourceRefresh,
        "Actualizar servicio del documento",
    ),
    (
        234,
        CommandId::SourceCancel,
        "Cancelar servicio del documento",
    ),
    (
        235,
        CommandId::SourceAccept,
        "Aceptar elemento del documento",
    ),
    (236, CommandId::SourceOpenToSide, "Abrir elemento al lado"),
    (240, CommandId::EditInsertSnippet, "Insertar plantilla"),
    (241, CommandId::EditSurroundWith, "Envolver selección"),
    (242, CommandId::SnippetNext, "Plantilla: siguiente paso"),
    (243, CommandId::SnippetPrevious, "Plantilla: paso anterior"),
    (
        244,
        CommandId::SnippetAccept,
        "Plantilla: aplicar vista previa",
    ),
    (245, CommandId::SnippetCancel, "Plantilla: cancelar"),
    (272, CommandId::EditCompleteWithAi, "Completar con OmniCore"),
    (
        273,
        CommandId::EditCompleteBlockWithAi,
        "Completar bloque con OmniCore",
    ),
    (
        274,
        CommandId::EditToggleAutomaticAi,
        "Autocompletado automático de OmniCore",
    ),
    (
        275,
        CommandId::EditSuggestNextWithAi,
        "Sugerir siguiente edición con OmniCore",
    ),
    (
        270,
        CommandId::EditSelectionWithAi,
        "Editar selección con IA…",
    ),
    (
        250,
        CommandId::ViewMarkdownPreview,
        "Vista previa de Markdown",
    ),
    (
        251,
        CommandId::ViewAppearance,
        "&Preferencias de apariencia…",
    ),
    (252, CommandId::ViewEditorZoom, "&Zoom del editor…"),
    (253, CommandId::ProgressCheck, "ABL: comprobar sintaxis"),
    (254, CommandId::ProgressCompile, "ABL: compilar"),
    (255, CommandId::ProgressRun, "ABL: ejecutar"),
    (256, CommandId::ProgressCancel, "Cancelar tarea ABL"),
    (
        257,
        CommandId::ProgressRefreshSchema,
        "Actualizar esquema de base de datos",
    ),
    (258, CommandId::ViewDatabase, "Ver esquema ABL"),
    (269, CommandId::ProgressBrowseRecords, "Ver registros…"),
    (
        268,
        CommandId::ProjectDataConnections,
        "&Conexiones de datos…",
    ),
    (
        259,
        CommandId::SourceSchemaComplete,
        "Completar desde esquema ABL",
    ),
    (
        210,
        CommandId::ViewToggleBottomPanel,
        "Mostrar/ocultar panel &inferior",
    ),
    (211, CommandId::ViewProblems, "&Problemas"),
    (212, CommandId::ViewOutput, "&Salida"),
    (
        200,
        CommandId::EditRectangleStart,
        "Rectángulo: &iniciar desde cursor/selección",
    ),
    (201, CommandId::EditRectangleCancel, "Rectángulo: &cancelar"),
    (
        202,
        CommandId::EditRectangleLeft,
        "Rectángulo: extender &izquierda",
    ),
    (
        203,
        CommandId::EditRectangleRight,
        "Rectángulo: extender &derecha",
    ),
    (
        204,
        CommandId::EditRectangleUp,
        "Rectángulo: extender &arriba",
    ),
    (
        205,
        CommandId::EditRectangleDown,
        "Rectángulo: extender a&bajo",
    ),
    (160, CommandId::SearchGoToPosition, "Ir a &línea/columna..."),
    (161, CommandId::NavigateBack, "Navegar &atrás"),
    (162, CommandId::NavigateForward, "Navegar a&delante"),
    (163, CommandId::SearchHistoryPrevious, "Consulta &anterior"),
    (164, CommandId::SearchHistoryNext, "Consulta &siguiente"),
    (138, CommandId::SearchToggleCase, "Distinguir &mayúsculas"),
    (
        139,
        CommandId::SearchToggleWholeWord,
        "Solo &palabras completas",
    ),
    (165, CommandId::SearchToggleRegex, "E&xpresión regular"),
    (101, CommandId::FileNew, "&Nuevo"),
    (102, CommandId::FileOpen, "&Abrir..."),
    (103, CommandId::FileSave, "&Guardar"),
    (104, CommandId::FileSaveAs, "Guardar &como..."),
    (105, CommandId::FileClose, "&Cerrar pestaña"),
    (106, CommandId::FileExit, "&Salir"),
    (107, CommandId::FileOpenFolder, "Abrir &carpeta..."),
    (108, CommandId::FileQuickOpen, "Apertura &rápida..."),
    (
        109,
        CommandId::FileChooseEncoding,
        "Elegir &codificación del fuente...",
    ),
    (110, CommandId::EditUndo, "&Deshacer"),
    (111, CommandId::EditRedo, "&Rehacer"),
    (112, CommandId::EditCut, "Cor&tar"),
    (113, CommandId::EditCopy, "&Copiar"),
    (114, CommandId::EditPaste, "&Pegar"),
    (115, CommandId::EditSelectAll, "Seleccionar &todo"),
    (116, CommandId::EditJoinLines, "&Unir líneas"),
    (117, CommandId::EditSplitLine, "&Dividir línea"),
    (170, CommandId::EditDuplicate, "&Duplicar selección/línea"),
    (171, CommandId::EditDeleteLine, "E&liminar línea"),
    (172, CommandId::EditMoveLineUp, "Subir &línea"),
    (173, CommandId::EditMoveLineDown, "Bajar lí&nea"),
    (
        174,
        CommandId::EditToggleLineComment,
        "Alternar &comentario",
    ),
    (
        175,
        CommandId::EditToggleBlockComment,
        "Alternar comentario de &bloque",
    ),
    (176, CommandId::EditIndent, "&Aumentar sangría"),
    (177, CommandId::EditOutdent, "&Reducir sangría"),
    (
        178,
        CommandId::EditorToggleScrollLink,
        "Vincular/desvincular &desplazamiento",
    ),
    (118, CommandId::FileSaveAll, "Guardar &todo..."),
    (119, CommandId::FileReopenClosed, "Reabrir &pestaña cerrada"),
    (
        194,
        CommandId::SearchSelectionNext,
        "Buscar selección &siguiente",
    ),
    (
        195,
        CommandId::SearchSelectionPrevious,
        "Buscar selección &anterior",
    ),
    (
        196,
        CommandId::SearchCancel,
        "Cancelar &búsqueda/rectángulo",
    ),
    (180, CommandId::Tab1, "Pestaña &1"),
    (181, CommandId::Tab2, "Pestaña &2"),
    (182, CommandId::Tab3, "Pestaña &3"),
    (183, CommandId::Tab4, "Pestaña &4"),
    (184, CommandId::Tab5, "Pestaña &5"),
    (185, CommandId::Tab6, "Pestaña &6"),
    (186, CommandId::Tab7, "Pestaña &7"),
    (187, CommandId::Tab8, "Pestaña &8"),
    (188, CommandId::Tab9, "Ú&ltima pestaña (Alt+9)"),
    (120, CommandId::EditorSplitRight, "Dividir a la &derecha"),
    (121, CommandId::EditorOneGroup, "&Un grupo"),
    (122, CommandId::EditorFocusPrimary, "Enfocar grupo &1"),
    (123, CommandId::EditorFocusSecondary, "Enfocar grupo &2"),
    (124, CommandId::TabNext, "Pestaña &siguiente"),
    (125, CommandId::TabPrevious, "Pestaña &anterior"),
    (126, CommandId::PaletteOpen, "&Paleta de comandos..."),
    (
        127,
        CommandId::ViewToggleInvisibles,
        "Mostrar caracteres &invisibles",
    ),
    (130, CommandId::SearchFind, "&Buscar multilínea..."),
    (128, CommandId::ViewExplorer, "E&xplorador del proyecto"),
    (
        129,
        CommandId::EditorTogglePin,
        "&Anclar/desanclar referencia",
    ),
    (150, CommandId::ExplorerRefresh, "&Actualizar directorio"),
    (
        151,
        CommandId::ExplorerCancel,
        "Cancelar carga/volver al &editor",
    ),
    (152, CommandId::EditorOpenToSide, "Abrir al &lado"),
    (153, CommandId::FocusNext, "Área &siguiente"),
    (154, CommandId::FocusPrevious, "Área &anterior"),
    (131, CommandId::SearchReplace, "&Reemplazar multilínea..."),
    (132, CommandId::SearchFindNext, "Buscar &siguiente"),
    (133, CommandId::SearchFindPrevious, "Buscar &anterior"),
    (
        134,
        CommandId::SearchProject,
        "Search Plus: buscar en &proyecto...",
    ),
    (
        135,
        CommandId::SearchCancelProject,
        "&Cancelar búsqueda del proyecto",
    ),
    (
        136,
        CommandId::ViewSearchResults,
        "&Resultados de búsqueda del proyecto",
    ),
    (
        137,
        CommandId::SearchReplaceProject,
        "Reemplazar en pro&yecto...",
    ),
    (
        140,
        CommandId::GitRepositoryStatus,
        "&Estado del repositorio (solo lectura)",
    ),
    (
        141,
        CommandId::ViewGit,
        "&Comparar este archivo con un commit…",
    ),
    (142, CommandId::GitDiffHead, "Comparar en Código con &HEAD"),
    (
        143,
        CommandId::GitDiffIndex,
        "Comparar en Código con el &índice",
    ),
    (
        144,
        CommandId::GitDiffRefresh,
        "&Actualizar comparación activa",
    ),
    (145, CommandId::GitDiffCancel, "&Cerrar comparación"),
    (
        148,
        CommandId::GitCloneWorktree,
        "C&lonar con directorio de trabajo…",
    ),
    (
        146,
        CommandId::GitStatusRefresh,
        "Actualizar es&tado del repositorio",
    ),
    (
        147,
        CommandId::GitStatusCancel,
        "Cancelar estado del re&positorio",
    ),
];
pub(super) fn command_id(command: CommandId) -> usize {
    COMMANDS
        .iter()
        .find(|(_, c, _)| *c == command)
        .map_or(0, |item| item.0)
}

enum Action {
    None,
    CapturePointer,
    NativeMenu(native_menu::NativeMenuGroup),
    Appearance(appearance::Choice),
    Clipboard(CommandId),
    PasteSearch(u64),
    PasteGitSearch(std::sync::Arc<()>),
    PasteGitCommit(std::sync::Arc<()>),
    PasteNavigation(u64),
    PasteSnippet(snippet_panel::SnippetInputTarget),
    SelectorClipboard(CommandId, Box<selector_input::SelectorInputTarget>),
    PasteProject(u64),
    PasteReplacement(u64),
    OpenWeb(String),
    ExportGitPatch(Arc<jc_app::git_revision::RevisionComparison>),
    ChooseFolder,
    ChooseOmniCore,
    ChooseOmniCoreModel,
    ConfirmOmniCoreEdit,
    EditOmniCoreSelection,
    PasteOmniCore(std::rc::Rc<()>),
    DataConnections,
    BrowseRecords,
    Open(bool),
    SaveAs,
    SaveAll,
    CloseTab,
    Exit,
    FinishExit,
    Minimize,
    MaxRestore,
}
struct State {
    recovery: Option<jc_app::session::EditorRecovery>,
    periodic_recovery: jc_app::session::PeriodicRecovery,
    exit_waiting_for_save: bool,
    exit_ready: bool,
    restore_window: Option<(i32, i32, bool)>,
    open_diff: jc_app::open_diff::OpenDiffController,
    open_diff_view: jc_app::open_diff_view::OpenDiffView,
    open_diff_visible: bool,
    omnicore: omnicore_panel::Panel,
    omnicore_diff: Option<open_diff_panel::OmniCoreComparison>,
    source_gui: source_panel::SourceGui,
    snippet_gui: Option<snippet_panel::SnippetGui>,
    markdown_gui: Option<markdown_panel::MarkdownGui>,
    markdown_images: markdown_images::ImageController,
    pub workbench: Workbench,
    startup_files: std::collections::VecDeque<PathBuf>,
    registry: CommandRegistry,
    keymap: KeyMap,
    encoding: Option<TextEncoding>,
    project_root: Option<PathBuf>,
    explorer: jc_app::explorer::ProjectExplorer,
    explorer_view: jc_app::explorer::ExplorerView,
    explorer_visible: bool,
    explorer_focused: bool,
    explorer_hits: Option<explorer_panel::ExplorerHits>,
    explorer_scroll: explorer_scroll::Interaction,
    outline: outline_panel::Outline,
    bottom: bottom_panel::BottomState,
    bottom_hits: Option<bottom_panel::BottomHits>,
    rectangle_drag: Option<rectangle_input::RectangleDrag>,
    linear_drag: Option<pointer_input::LinearDrag>,
    editor_scroll_drag: Option<editor_scroll::Drag>,
    editor_maps: [editor_scroll::MapCache; 2],
    wheel: Option<pointer_input::WheelState>,
    menu_stamp: Option<Box<menu_input::MenuStamp>>,
    interaction_epoch: u64,
    tab_scroll: [tab_bar::Scroll; 2],
    tab_hits: Option<tab_bar::Bars>,
    smoke_wheel_lines: Option<u32>,
    smoke_menu_choices: std::collections::VecDeque<Option<usize>>,
    selector: Option<Selector>,
    selector_generation: u64,
    selector_explorer_focus: bool,
    selector_search_panel: Option<search_panel::SearchPanel>,
    selector_editor_input: bool,
    selector_edit_target: Option<line_edit::EditTarget>,
    search_panel: Option<search_panel::SearchPanel>,
    navigation_panel: Option<navigation_panel::NavigationPanel>,
    navigation_generation: u64,
    project_panel: Option<project_panel::ProjectPanel>,
    project_search: jc_app::ProjectSearch,
    project_options: Option<jc_app::ProjectSearchOptions>,
    project_workers: usize,
    replace_panel: Option<replace_panel::ReplacePanel>,
    replace_generation: u64,
    git_status_due: bool,
    git_context: Option<PathBuf>,
    git_repositories: git_repositories::Repositories,
    git_source: jc_app::git_read::GitBlobSource,
    git_panel: Option<git_panel::GitIntent>,
    git_workspace: git_workspace::Workspace,
    git_code: git_code::View,
    git_patch: git_patch::Controller,
    git_index: jc_app::git_index::GitIndexController,
    git_commit: jc_app::git_commit::CommitController,
    git_view: git_view::GitView,
    git_diff: jc_app::git_diff::GitDiffController,
    git_status: jc_app::git_status_tasks::GitStatusController,
    git_revision: jc_app::git_revision::GitRevisionController,
    git_status_options: Option<jc_app::git_status_tasks::GitStatusOptions>,
    jobs: jc_app::JobSystem,
    progress_schema: jc_progress::editor::SchemaController,
    encoding_target: Option<(jc_app::DocumentId, u64, Option<TextEncoding>)>,
    smoke_clipboard: Option<String>,
    quick_open: QuickOpenController,
    wakeup: Wakeup,
    bridge: Arc<notify::Bridge>,
    deliveries: usize,
    pub status: String,
    pub width: i32,
    pub height: i32,
    pub cell_width: i32,
    pub cell_height: i32,
    dpi: dpi::Dpi,
    ui_text_size: u8,
    editor_text_size: u8,
    editor_zoom: u8,
    zoom: zoom::Zoom,
    minimap_visible: bool,
    minimap_width: u16,
    editor_theme: appearance::Theme,
    pub font: Handle,
    pub ui_font: Handle,
    ascii_runs: [bool; 128],
    owned_ui_font: bool,
    owned_font: bool,
    pub focused: bool,
    chrome_hover: Option<chrome::Action>,
    maximized: bool,
    icon: Handle,
    pending_surrogate: Option<u16>,
    ime_target: Option<(line_edit::EditTarget, u64)>,
    next_untitled: usize,
    pub paint_count: usize,
    pub paint_errors: usize,
    syntax_pending: bool,
    syntax_targets: [Option<(jc_app::PaneId, jc_app::TabId)>; 2],
    pub save_count: usize,
    pub smoke: bool,
    smoke_root: Option<PathBuf>,
    smoke_save_paths: std::collections::VecDeque<Option<PathBuf>>,
    smoke_close_decisions: std::collections::VecDeque<bool>,
    save_all_active: bool,
    pending_save_all: Option<save_plan::SavePlan>,
    pub key_override: Option<Modifiers>,
}
impl Drop for State {
    fn drop(&mut self) {
        if self.owned_font {
            unsafe {
                DeleteObject(self.font);
            }
        }
        if self.owned_ui_font {
            unsafe {
                DeleteObject(self.ui_font);
            }
        }
    }
}
impl State {
    fn new(options: &GuiOptions) -> Result<Self, String> {
        let bridge = Arc::new(notify::Bridge::default());
        let callback = bridge.clone();
        let wakeup = Wakeup::with_notifier(move || callback.signal());
        let created = unsafe {
            dpi::editor_font(
                dpi::Dpi::BASE,
                appearance::DEFAULT_EDITOR,
                appearance::DEFAULT_ZOOM,
            )
        };
        let font = if created.is_null() {
            unsafe { GetStockObject(16) }
        } else {
            created
        };
        if font.is_null() {
            return Err(os_error("Crear fuente monoespaciada nativa"));
        }
        let ui_created = unsafe { dpi::ui_font(dpi::Dpi::BASE) };
        let ui_font = if ui_created.is_null() {
            unsafe { GetStockObject(17) }
        } else {
            ui_created
        };
        let mut state = Self {
            open_diff: Default::default(),
            open_diff_view: Default::default(),
            open_diff_visible: false,
            omnicore: omnicore_panel::Panel::default(),
            omnicore_diff: None,
            source_gui: source_panel::SourceGui::new(),
            snippet_gui: None,
            markdown_gui: None,
            markdown_images: markdown_images::ImageController::new(wakeup.clone()),
            workbench: Workbench::default(),
            startup_files: Default::default(),
            recovery: None,
            periodic_recovery: Default::default(),
            exit_waiting_for_save: false,
            exit_ready: false,
            restore_window: None,
            registry: default_registry(),
            keymap: default_keymap(),
            encoding: options.encoding,
            project_root: None,
            explorer: jc_app::explorer::ProjectExplorer::new(wakeup.clone(), Default::default())
                .map_err(|e| e.to_string())?,
            explorer_view: Default::default(),
            explorer_visible: false,
            explorer_focused: false,
            explorer_hits: None,
            explorer_scroll: Default::default(),
            outline: outline_panel::Outline::default(),
            bottom: Default::default(),
            bottom_hits: None,
            rectangle_drag: None,
            linear_drag: None,
            editor_scroll_drag: None,
            editor_maps: Default::default(),
            wheel: None,
            menu_stamp: None,
            interaction_epoch: 0,
            smoke_wheel_lines: None,
            smoke_menu_choices: Default::default(),
            selector: None,
            selector_generation: 0,
            selector_explorer_focus: false,
            selector_search_panel: None,
            selector_editor_input: false,
            selector_edit_target: None,
            search_panel: None,
            navigation_panel: None,
            navigation_generation: 0,
            project_panel: None,
            project_search: jc_app::ProjectSearch::new(wakeup.clone()),
            project_options: None,
            project_workers: options.search_workers.unwrap_or(2),
            replace_panel: None,
            replace_generation: 0,
            git_status_due: false,
            git_context: None,
            git_repositories: git_repositories::Repositories::new(wakeup.clone()),
            git_source: jc_app::git_read::GitBlobSource::Head,
            git_panel: None,
            git_workspace: Default::default(),
            git_code: Default::default(),
            git_patch: Default::default(),
            git_index: jc_app::git_index::GitIndexController::new(wakeup.clone()),
            git_commit: jc_app::git_commit::CommitController::new(wakeup.clone()),
            git_view: Default::default(),
            git_diff: jc_app::git_diff::GitDiffController::new(),
            git_status: jc_app::git_status_tasks::GitStatusController::new(wakeup.clone()),
            git_revision: jc_app::git_revision::GitRevisionController::new(wakeup.clone()),
            git_status_options: None,
            jobs: jc_app::JobSystem::new(Default::default(), wakeup.clone())
                .map_err(|e| format!("Admisión de tarea nativa: {e:?}"))?,
            progress_schema: Default::default(),
            encoding_target: None,
            smoke_clipboard: None,
            quick_open: QuickOpenController::new(wakeup.clone()),
            wakeup,
            bridge,
            deliveries: 0,
            status: "Listo".into(),
            width: 1000,
            height: 650,
            cell_width: 9,
            cell_height: 18,
            dpi: dpi::Dpi::BASE,
            ui_text_size: appearance::DEFAULT_UI,
            editor_text_size: appearance::DEFAULT_EDITOR,
            editor_zoom: appearance::DEFAULT_ZOOM,
            zoom: Default::default(),
            minimap_visible: true,
            minimap_width: appearance::DEFAULT_MINIMAP,
            editor_theme: appearance::Theme::Current,
            font,
            ui_font,
            ascii_runs: [false; 128],
            owned_ui_font: !ui_created.is_null(),
            owned_font: !created.is_null(),
            focused: false,
            chrome_hover: None,
            maximized: false,
            icon: null_mut(),
            pending_surrogate: None,
            ime_target: None,
            next_untitled: 0,
            paint_count: 0,
            paint_errors: 0,
            syntax_pending: false,
            tab_scroll: Default::default(),
            tab_hits: None,
            syntax_targets: [None, None],
            save_count: 0,
            smoke: options.smoke.is_some(),
            smoke_root: options
                .smoke
                .as_ref()
                .and_then(|s| s.fixture.parent())
                .and_then(|p| p.canonicalize().ok()),
            smoke_save_paths: Default::default(),
            smoke_close_decisions: Default::default(),
            save_all_active: false,
            pending_save_all: None,
            key_override: None,
        };
        if let Some(root) = options
            .smoke
            .as_ref()
            .and_then(|s| s.fixture.parent())
            .or(options.project_root.as_deref())
        {
            state.set_project_root(root.to_owned())?;
        }
        if let Some(smoke) = &options.smoke {
            if smoke.performance_paged_only {
                state.workbench.set_large_open_wakeup(state.wakeup.clone());
                state.startup_files.extend(options.paths.iter().cloned());
                state.advance_startup_files();
            } else if smoke.measures_performance() {
                for path in &options.paths {
                    state
                        .workbench
                        .open_file(path, options.encoding)
                        .map_err(|e| e.to_string())?;
                }
            } else {
                state
                    .workbench
                    .open_file(&smoke.fixture, Some(TextEncoding::Utf8))
                    .map_err(|e| e.to_string())?;
            }
        } else {
            if !cfg!(test) {
                state.enable_recovery()?;
            }
            state.workbench.set_large_open_wakeup(state.wakeup.clone());
            state.startup_files.extend(options.paths.iter().cloned());
            state.advance_startup_files();
        }
        state.ensure_encoding_selector();
        if options.project_root.is_some() {
            state.explorer_visible = true;
        }
        Ok(state)
    }
    fn advance_startup_files(&mut self) {
        while !self.workbench.large_open_pending() {
            let Some(path) = self.startup_files.pop_front() else {
                if self.workbench.tabs().tabs().is_empty() {
                    self.new_document();
                }
                return;
            };
            match self.workbench.request_file_open(path, self.encoding, false) {
                Ok(Some(_)) => {}
                Ok(None) => self.status = "Copiando archivo grande en segundo plano…".into(),
                Err(error) => self.error(error),
            }
        }
    }
    fn file_opened(&mut self) {
        self.status = "Abierto".into();
        self.explorer_focused = false;
        self.bottom.focused = false;
        self.ensure_encoding_selector();
    }
    fn poll_large_file(&mut self) -> bool {
        if self.selector.is_some()
            || self.search_panel.is_some()
            || self.navigation_panel.is_some()
            || self.project_visible()
            || self.replace_panel.is_some()
            || self.source_visible()
            || self.snippet_gui.is_some()
            || self.open_diff_visible
            || self.markdown_gui.is_some()
            || self.git_panel.is_some()
            || self.git_code.active
        {
            return false;
        }
        let Some(result) = self.workbench.poll_large_open() else {
            return false;
        };
        match result {
            Ok(tab)
                if self
                    .workbench
                    .active_document()
                    .is_some_and(|doc| doc.tab_id == tab) =>
            {
                self.file_opened();
            }
            Ok(_) => self.status = "Archivo grande abierto en una pestaña".into(),
            Err(error) => self.error(error),
        }
        if !self.startup_files.is_empty() || self.workbench.tabs().tabs().is_empty() {
            self.advance_startup_files();
            self.ensure_encoding_selector();
        }
        self.periodic_recovery
            .note_activity(std::time::Instant::now());
        true
    }
    fn new_document(&mut self) {
        self.bottom.focused = false;
        self.explorer_focused = false;
        self.workbench.new_file(
            untitled::next_name(&self.workbench),
            self.encoding.unwrap_or(TextEncoding::Utf8),
        );
    }
    fn error(&mut self, error: impl std::fmt::Display) {
        self.status = error.to_string().chars().take(240).collect();
    }
    fn context(&self) -> jc_app::CommandContext {
        let mut context = self.workbench.context();
        context.gui_appearance = true;
        let explorer_focus = self.explorer_focused
            || (self.selector_explorer_focus
                && self
                    .selector
                    .as_ref()
                    .is_some_and(|s| s.kind() == SelectorKind::Palette));
        context.focus = if self.bottom.focused {
            PrimaryFocus::BottomPanel
        } else if explorer_focus {
            PrimaryFocus::Sidebar
        } else {
            PrimaryFocus::Editor
        };
        context.explorer_focused = explorer_focus;
        context.editor_input = if self
            .selector
            .as_ref()
            .is_some_and(|s| s.kind() == SelectorKind::Palette)
        {
            self.selector_editor_input
                && self.selector_edit_target.is_some()
                && self.selector_edit_target == self.source_edit_target()
        } else {
            !explorer_focus
                && !self.bottom.focused
                && self.selector.is_none()
                && self.search_panel.is_none()
                && self.navigation_panel.is_none()
                && !self.project_visible()
                && self.replace_panel.is_none()
                && self.git_panel.is_none()
        };
        if explorer_focus {
            context.can_open_to_side = if self.outline_focused() {
                self.outline_ready()
            } else {
                self.explorer_view
                    .selected()
                    .and_then(|id| self.explorer.node(id))
                    .is_some_and(|n| !n.is_dir())
            };
        }
        context.has_project = self.project_root.is_some();
        context.has_database_schema = self.progress_schema.has_catalog();
        context.project_search_running =
            self.project_search.state == jc_app::ProjectSearchState::Running;
        context.project_replace_ready = self.project_root.as_deref() == self.project_search.root()
            && self.project_search.can_replace()
            && self
                .replace_panel
                .as_ref()
                .is_none_or(|p| p.stage != replace_panel::ReplaceStage::Applying);
        context.git_repository_available = context.has_project;
        context.git_diff_available = context.has_project
            && context.has_document
            && self
                .workbench
                .active_document()
                .is_some_and(|d| d.session.encoding().is_some());
        context.git_diff_running = self.git_code.active || self.git_diff_running();
        context.git_status_running = matches!(
            self.git_status.state(),
            jc_app::git_status_tasks::GitStatusTaskState::Queued
                | jc_app::git_status_tasks::GitStatusTaskState::Running
        );
        // Availability is an intention gate, never a read of another owner's
        // clipboard. Content is requested only by explicit Paste.
        if !context.rectangular_selection {
            context.clipboard_has_text = context.has_document;
        }
        if explorer_focus {
            context.clipboard_has_text = false;
            context.has_selection = false;
        }
        self.open_diff_context(&mut context);
        self.source_context(&mut context);
        self.snippet_context(&mut context);
        if self.markdown_gui.is_some() || self.git_code.active || self.zoom.open {
            context.editor_input = false;
            context.has_selection = false;
        }
        context
    }
    fn execute(&mut self, command: CommandId) -> Action {
        self.cancel_pointer();
        if command == CommandId::ProjectDataConnections && self.project_root.is_none() {
            self.status = "Abre una carpeta de proyecto para configurar conexiones de datos".into();
            return Action::None;
        }
        if !self
            .registry
            .is_enabled(command, &self.entry_context(command))
        {
            self.status = "Comando no disponible en el contexto actual".into();
            return Action::None;
        }
        if let Some(group) = native_menu::group_for_command(command) {
            return Action::NativeMenu(group);
        }
        if command == CommandId::ViewEditorZoom {
            self.close_selector();
            self.toggle_zoom();
            return Action::None;
        }
        self.zoom.open = false;
        if command == CommandId::ViewAppearance {
            self.open_selector(SelectorKind::Appearance);
            return Action::None;
        }
        if self.selector.is_some() && selector_input::field_command(command) {
            return self.selector_field_action(command);
        }
        if self.selector.is_none() && command == CommandId::EditPaste {
            if let Some(action) = self.field_paste_action() {
                return action;
            }
        }
        if open_diff_panel::is_command(command) {
            self.close_source_ui();
            self.snippet_gui = None;
            self.markdown_gui = None;
            return self.open_diff_command(command);
        }
        if source_panel::is_command(command) {
            self.open_diff_visible = false;
            self.snippet_gui = None;
            self.markdown_gui = None;
            return self.source_command(command);
        }
        if snippet_panel::is_command(command) {
            self.markdown_gui = None;
            return self.snippet_command(command);
        }
        if matches!(
            command,
            CommandId::EditCompleteWithAi | CommandId::EditCompleteBlockWithAi
        ) {
            self.close_selector();
            self.request_omnicore_completion(command == CommandId::EditCompleteBlockWithAi);
            return Action::None;
        }
        if command == CommandId::EditSelectionWithAi {
            self.close_selector();
            return Action::EditOmniCoreSelection;
        }
        if command == CommandId::EditToggleAutomaticAi {
            self.close_selector();
            self.toggle_omnicore_automatic();
            return Action::None;
        }
        if command == CommandId::EditSuggestNextWithAi {
            self.close_selector();
            self.request_omnicore_next_edit();
            return Action::None;
        }
        if command == CommandId::ViewMarkdownPreview {
            return self.toggle_markdown();
        }
        if !snippet_panel::keep_for(command) && command != CommandId::ViewToggleInvisibles {
            self.open_diff_visible = false;
            self.close_source_ui();
            self.snippet_gui = None;
            self.markdown_gui = None;
        }
        match command {
            CommandId::ProgressCancel => {
                self.status = if self.progress_schema.cancel() {
                    "Cancelando actualización de esquema ABL…"
                } else {
                    jc_progress::editor::cancel(&mut self.jobs)
                }
                .into();
                return Action::None;
            }
            CommandId::ProgressRefreshSchema => {
                self.request_progress_schema();
                return Action::None;
            }
            CommandId::ViewDatabase => {
                self.show_progress_schema();
                return Action::None;
            }
            CommandId::ProgressBrowseRecords => {
                if self.progress_schema.busy() || self.jobs.has_job(jc_progress::editor::JOB_KEY) {
                    self.error("Espera a que termine la tarea ABL antes de consultar registros");
                    return Action::None;
                }
                self.close_selector();
                return Action::BrowseRecords;
            }
            command if jc_progress::editor::action(command).is_some() => {
                if self.progress_schema.busy() {
                    self.error("Ya hay una actualización de esquema en curso");
                    return Action::None;
                }
                let action = jc_progress::editor::action(command).unwrap();
                match jc_progress::editor::submit(
                    action,
                    &self.workbench,
                    &mut self.jobs,
                    self.project_root.as_deref(),
                ) {
                    Ok(()) => {
                        self.show_bottom(CommandId::ViewOutput);
                        self.status = "Tarea ABL en curso".into();
                    }
                    Err(error) => self.error(error),
                }
                return Action::None;
            }
            CommandId::ViewToggleBottomPanel | CommandId::ViewProblems | CommandId::ViewOutput => {
                self.show_bottom(command);
                return Action::None;
            }
            command if rectangle_input::is_rectangle(command) => {
                match self.workbench.execute(command) {
                    Ok(_) => self.rectangle_status(),
                    Err(error) => self.error(error),
                }
                return Action::None;
            }
            CommandId::FileReopenClosed => {
                self.reopen_tab();
                return Action::None;
            }
            CommandId::SearchSelectionNext | CommandId::SearchSelectionPrevious => {
                self.search_selection(command);
                return Action::None;
            }
            CommandId::SearchGoToPosition => {
                self.open_navigation_panel();
                return Action::None;
            }
            CommandId::SearchHistoryPrevious
            | CommandId::SearchHistoryNext
            | CommandId::SearchToggleCase
            | CommandId::SearchToggleWholeWord
            | CommandId::SearchToggleRegex => {
                self.execute_search_option(command);
                return Action::None;
            }
            CommandId::NavigateBack | CommandId::NavigateForward => {
                self.close_selector();
                self.bottom.focused = false;
                self.workbench.cancel_search_navigation();
            }
            CommandId::ViewExplorer => {
                self.show_explorer();
                return Action::None;
            }
            CommandId::ExplorerRefresh => {
                if self.outline_focused() {
                    self.refresh_outline();
                } else {
                    self.refresh_explorer();
                }
                return Action::None;
            }
            CommandId::ExplorerCancel => {
                if self.outline_focused() {
                    self.exit_outline();
                } else {
                    self.cancel_explorer();
                }
                return Action::None;
            }
            CommandId::FocusNext | CommandId::FocusPrevious => {
                self.cycle_regions(command == CommandId::FocusNext);
                return Action::None;
            }
            CommandId::EditorOpenToSide if self.explorer_focused => {
                if self.outline_focused() {
                    self.activate_outline(true);
                } else {
                    self.activate_explorer(true, false);
                }
                return Action::None;
            }
            CommandId::GitRepositoryStatus | CommandId::GitStatusRefresh => {
                return self.request_git(git_panel::GitIntent::Status)
            }
            CommandId::GitCloneWorktree => {
                if let Some(root) = self
                    .git_root()
                    .map(std::path::Path::to_owned)
                    .or_else(|| self.project_root.clone())
                {
                    if self.git_root().is_some() {
                        self.request_git(git_panel::GitIntent::Status);
                    } else {
                        self.validate_git_context();
                        self.close_selector();
                        self.git_panel = Some(git_panel::GitIntent::Status);
                    }
                    self.git_commit.open_worktree_clone(root);
                    self.git_workspace.invalidate();
                }
                return Action::None;
            }
            CommandId::ViewGit => {
                if self.git_panel.is_some() && !self.git_code.active {
                    return self.open_git_code_from_workspace();
                }
                return self.open_document_git_history();
            }
            CommandId::GitDiffRefresh => {
                if self.git_code.active {
                    return self.open_git_code(self.git_source);
                }
                return self.request_git(git_panel::GitIntent::Diff(self.git_source));
            }
            CommandId::GitDiffHead => {
                return self.open_git_code(jc_app::git_read::GitBlobSource::Head)
            }
            CommandId::GitDiffIndex => {
                return self.open_git_code(jc_app::git_read::GitBlobSource::Index)
            }
            CommandId::GitDiffCancel => {
                if self.git_code.active {
                    return self.close_git_code();
                }
                self.cancel_git(git_panel::GitIntent::Diff(
                    jc_app::git_read::GitBlobSource::Head,
                ));
                return Action::None;
            }
            CommandId::GitStatusCancel => {
                self.cancel_git(git_panel::GitIntent::Status);
                return Action::None;
            }
            CommandId::GitConfigureExecutable => {
                self.status = "Motor Git integrado en CIDE".into();
                return Action::None;
            }
            CommandId::SearchReplaceProject => {
                self.open_project_replace();
                return Action::None;
            }
            CommandId::SearchProject => {
                self.open_project_search(false);
                return Action::None;
            }
            CommandId::SearchCancelProject => {
                self.project_search.cancel();
                self.status = "Búsqueda del proyecto cancelada".into();
                return Action::None;
            }
            CommandId::ViewSearchResults => {
                self.open_project_search(true);
                return Action::None;
            }
            CommandId::SearchFind | CommandId::SearchReplace => {
                self.open_search_panel(command == CommandId::SearchReplace);
                return Action::None;
            }
            CommandId::FileChooseEncoding => {
                self.open_encoding_selector();
                return Action::None;
            }
            CommandId::EditCopy | CommandId::EditCut | CommandId::EditPaste
                if !self.context().rectangular_selection
                    && (command != CommandId::EditPaste
                        || !self.workbench.clipboard_is_rectangular()) =>
            {
                return Action::Clipboard(command)
            }
            CommandId::FileOpenFolder => {
                self.close_selector();
                return Action::ChooseFolder;
            }
            CommandId::ProjectDataConnections => {
                self.close_selector();
                return Action::DataConnections;
            }
            CommandId::PaletteOpen => {
                self.open_selector(SelectorKind::Palette);
                return Action::None;
            }
            CommandId::FileQuickOpen => {
                self.open_selector(SelectorKind::Files);
                return Action::None;
            }
            CommandId::FileNew => {
                self.new_document();
                return Action::None;
            }
            CommandId::EditorFocusPrimary
            | CommandId::EditorOpenToSide
            | CommandId::EditorFocusSecondary
            | CommandId::EditorSplitRight
            | CommandId::EditorOneGroup
            | CommandId::TabNext
            | CommandId::TabPrevious => {
                self.explorer_focused = false;
                self.bottom.focused = false;
            }
            CommandId::Tab1
            | CommandId::Tab2
            | CommandId::Tab3
            | CommandId::Tab4
            | CommandId::Tab5
            | CommandId::Tab6
            | CommandId::Tab7
            | CommandId::Tab8
            | CommandId::Tab9 => {
                self.explorer_focused = false;
                self.bottom.focused = false;
            }
            CommandId::FileOpen => return Action::Open(false),
            CommandId::FileSaveAs => return Action::SaveAs,
            CommandId::FileClose => return Action::CloseTab,
            CommandId::FileExit => return Action::Exit,
            CommandId::FileSaveAll => {
                if self.save_all_active {
                    self.status = "Guardar todo ya está en curso".into();
                    return Action::None;
                }
                return Action::SaveAll;
            }
            CommandId::FileSave
                if self
                    .workbench
                    .active_document()
                    .is_some_and(|document| document.session.is_new()) =>
            {
                return Action::SaveAs
            }
            CommandId::FileSave if self.workbench.active_paged_byte().is_some() => {
                match self.workbench.request_save_active(None) {
                    Ok(_) => self.status = "Guardando archivo en segundo plano…".into(),
                    Err(error) => self.error(error),
                }
                return Action::None;
            }
            _ => {}
        }
        let edit_before = self
            .workbench
            .active_document()
            .map(|d| (d.id, d.session.document().revision()));
        match self.workbench.execute(command) {
            Ok(CommandOutcome::View(CommandId::ViewToggleInvisibles)) => {
                self.status = if self.workbench.invisibles_visible() {
                    "Caracteres invisibles: representación ASCII activada"
                } else {
                    "Caracteres invisibles ocultos"
                }
                .into()
            }
            Ok(CommandOutcome::None) => {
                if command == CommandId::SearchCancel {
                    self.status = "Búsqueda documental/rectángulo cancelados".into();
                    return Action::None;
                }
                if line_edit::is_line_edit(command) {
                    let label = self
                        .registry
                        .get(command)
                        .map_or("Edición de línea", |s| s.label);
                    let edit_after = self
                        .workbench
                        .active_document()
                        .map(|d| (d.id, d.session.document().revision()));
                    self.status = if edit_before == edit_after {
                        format!("Sin cambios · {label}")
                    } else {
                        format!("{label} · cambios sin guardar")
                    };
                    return Action::None;
                }
                if command == CommandId::FileSave {
                    self.save_count += 1;
                    self.schedule_git_status();
                }
                self.status = if self.context().rectangular_selection {
                    "Listo · el portapapeles rectangular sigue interno"
                } else {
                    "Listo · portapapeles de Windows · vista por grafemas"
                }
                .into();
            }
            Ok(CommandOutcome::View(
                CommandId::EditorSplitRight
                | CommandId::EditorOneGroup
                | CommandId::EditorFocusPrimary
                | CommandId::EditorFocusSecondary
                | CommandId::EditorTogglePin
                | CommandId::EditorToggleScrollLink
                | CommandId::EditorOpenToSide
                | CommandId::NavigateBack
                | CommandId::NavigateForward,
            )) => self.status = "Listo · grupos de editor compartidos".into(),
            Ok(_) => {
                self.status =
                    "Este comando requiere un flujo aún no conectado en esta interfaz".into()
            }
            Err(error) => self.error(error),
        }
        Action::None
    }
    fn execute_menu(&mut self, command: CommandId) -> Action {
        if !self
            .registry
            .is_enabled(command, &self.entry_context(command))
        {
            return self.execute(command);
        }
        if self.selector.is_some() && selector_input::field_command(command) {
            return self.execute(command);
        }
        if command == CommandId::EditPaste && self.field_paste_action().is_some() {
            return self.execute(command);
        }
        let explorer_focus = self.explorer_focused || self.selector_explorer_focus;
        if !matches!(
            command,
            CommandId::ViewToggleInvisibles
                | CommandId::SearchHistoryPrevious
                | CommandId::SearchHistoryNext
                | CommandId::SearchToggleCase
                | CommandId::SearchToggleWholeWord
                | CommandId::SearchToggleRegex
                | CommandId::PaletteOpen
        ) {
            self.close_selector();
        }
        if matches!(
            command,
            CommandId::ViewExplorer
                | CommandId::ExplorerRefresh
                | CommandId::ExplorerCancel
                | CommandId::EditorOpenToSide
                | CommandId::FocusNext
                | CommandId::FocusPrevious
                | CommandId::PaletteOpen
                | CommandId::FileQuickOpen
                | CommandId::FileChooseEncoding
        ) {
            self.explorer_focused = explorer_focus;
        }
        self.execute(command)
    }
    fn modifiers(&self) -> Modifiers {
        if let Some(value) = self.key_override {
            return value;
        }
        if self.smoke {
            return Modifiers::NONE;
        }
        let mut modifiers = Modifiers::NONE;
        for (key, flag) in [
            (0x10, Modifiers::SHIFT),
            (0x11, Modifiers::CTRL),
            (0x12, Modifiers::ALT),
        ] {
            if unsafe { GetKeyState(key) } < 0 {
                modifiers = modifiers.union(flag);
            }
        }
        modifiers
    }
    fn key(&mut self, code: usize) -> (bool, Action) {
        if !matches!(code, 0x10..=0x12) {
            self.cancel_pointer();
        }
        self.pending_surrogate = None;
        let modifiers = self.modifiers();
        if self.zoom.open {
            if let Some(group) = native_menu::mnemonic(code, modifiers) {
                self.zoom.open = false;
                return (true, Action::NativeMenu(group));
            }
            let action = if modifiers == Modifiers::NONE {
                self.zoom_key(code)
            } else {
                Action::None
            };
            return (true, action);
        }
        if self.git_panel.is_some() && self.git_commit.view().is_some() {
            if code == b'V' as usize && modifiers == Modifiers::CTRL {
                return (
                    true,
                    self.git_commit
                        .input_token()
                        .map_or(Action::None, Action::PasteGitCommit),
                );
            }
            if self.git_create_commit_key(code, modifiers) {
                return (true, Action::None);
            }
        }
        if let Some(group) = native_menu::mnemonic(code, modifiers) {
            return (true, Action::NativeMenu(group));
        }
        if code == 0x79 && modifiers == Modifiers::NONE {
            return (true, Action::NativeMenu(native_menu::NativeMenuGroup::File));
        }
        if code == usize::from(b'L')
            && modifiers == Modifiers::CTRL
            && self.pointer_source_available()
            && self.context().focus == jc_app::PrimaryFocus::Editor
        {
            self.attach_current_code_to_omnicore();
            return (true, Action::None);
        }
        if let Some(action) = self.omnicore_key(code, modifiers) {
            return (true, action);
        }
        let key = match code {
            0x20 => Some(Key::Char(' ')),
            0x09 => Some(Key::Tab),
            0x0d => Some(Key::Enter),
            0x1b => Some(Key::Escape),
            0x21 => Some(Key::PageUp),
            0x22 => Some(Key::PageDown),
            0x23 => Some(Key::End),
            0x24 => Some(Key::Home),
            0x25 => Some(Key::Left),
            0x26 => Some(Key::Up),
            0x27 => Some(Key::Right),
            0x28 => Some(Key::Down),
            0x30..=0x5a => char::from_u32(code as u32).map(|ch| Key::Char(ch.to_ascii_lowercase())),
            0xbf => Some(Key::Char('/')),
            0x70..=0x87 => Some(Key::Function((code - 0x70 + 1) as u8)),
            _ => None,
        };
        if let Some(key) = key {
            let global = self.keymap.resolve(
                KeyScope::Global,
                KeyStroke::new(modifiers, key),
                &self.registry,
                &self.context(),
            );
            if let Resolution::Disabled(
                CommandId::SearchGoToPosition
                | CommandId::NavigateBack
                | CommandId::NavigateForward,
            ) = global
            {
                self.status = "Comando de navegación no disponible en el contexto actual".into();
                return (true, Action::None);
            }
            if let Resolution::Enabled(
                command @ (CommandId::ViewToggleInvisibles
                | CommandId::FileExit
                | CommandId::SearchReplaceProject
                | CommandId::SearchGoToPosition
                | CommandId::NavigateBack
                | CommandId::NavigateForward
                | CommandId::ViewMarkdownPreview
                | CommandId::PaletteOpen),
            ) = global
            {
                if command != CommandId::SearchReplaceProject || self.replace_panel.is_none() {
                    return (true, self.execute(command));
                }
            }
        }
        if self.selector.is_some() {
            return (true, self.selector_key(code, modifiers));
        }
        if self.snippet_gui.is_some() {
            return (true, self.snippet_key(code, modifiers));
        }
        if self.source_visible() {
            return (true, self.source_key(code, modifiers));
        }
        if self.open_diff_visible {
            return (true, self.open_diff_key(code, modifiers));
        }
        if code == 0x1b && modifiers == Modifiers::NONE && self.cancel_omnicore_selection() {
            return (true, Action::None);
        }
        if code == 0x1b && modifiers == Modifiers::NONE && self.clear_omnicore_next_edit() {
            self.status = "Siguiente edición cancelada".into();
            return (true, Action::None);
        }
        if self.markdown_gui.is_some() {
            return (true, self.markdown_key(code, modifiers));
        }
        if self.git_panel.is_some() {
            if self.git_code.active {
                return (true, self.git_code_key(code, modifiers));
            }
            return (true, self.git_key(code, modifiers));
        }
        if self.replace_panel.is_some() {
            return (true, self.replace_key(code, modifiers));
        }
        if self.project_visible() {
            return (true, self.project_key(code, modifiers));
        }
        if self.search_panel.is_some() {
            return (true, self.search_key(code, modifiers));
        }
        if self.navigation_panel.is_some() {
            return (true, self.navigation_key(code, modifiers));
        }
        if self.selector.is_some() {
            return (true, self.selector_key(code, modifiers));
        }
        if self.bottom.focused {
            return (true, self.bottom_key(code, key, modifiers));
        }
        if self.explorer_focused {
            if let Some(key) = key {
                match self.keymap.resolve(
                    KeyScope::Sidebar,
                    KeyStroke::new(modifiers, key),
                    &self.registry,
                    &self.context(),
                ) {
                    Resolution::Enabled(command) if supported(command) => {
                        return (true, self.execute(command))
                    }
                    Resolution::Disabled(command) if supported(command) => {
                        self.status = "Comando no disponible en el explorador".into();
                        return (true, Action::None);
                    }
                    _ => {}
                }
            }
            self.explorer_key(code, modifiers);
            return (true, Action::None);
        }
        if self.omnicore_completion_key(code, modifiers) {
            return (true, Action::None);
        }
        if let Some(key) = key {
            let resolution = self.keymap.resolve(
                KeyScope::Editor,
                KeyStroke::new(modifiers, key),
                &self.registry,
                &self.context(),
            );
            match resolution {
                Resolution::Enabled(command) if supported(command) => {
                    return (true, self.execute(command))
                }
                Resolution::Disabled(command) if supported(command) => {
                    self.status = "Comando no disponible".into();
                    return (true, Action::None);
                }
                _ => {}
            }
        }
        if modifiers.contains(Modifiers::CTRL) || modifiers.contains(Modifiers::ALT) {
            return (false, Action::None);
        }
        let select = modifiers.contains(Modifiers::SHIFT);
        let result = match code {
            0x08 => self.workbench.backspace(),
            0x2e => self.workbench.delete_forward(),
            0x25 => self.workbench.move_left(select),
            0x27 => self.workbench.move_right(select),
            0x26 => self.workbench.move_vertical(-1, select),
            0x28 => self.workbench.move_vertical(1, select),
            0x24 => self.workbench.move_home(select),
            0x23 => self.workbench.move_end(select),
            0x21 | 0x22 => {
                let rows = paint::text_areas(self)
                    .into_iter()
                    .find(|area| area.pane == self.workbench.panes().active())
                    .map_or(1, |area| area.capacity_rows.max(1))
                    as isize;
                self.workbench
                    .move_vertical(if code == 0x21 { -rows } else { rows }, select)
            }
            _ => return (false, Action::None),
        };
        if let Err(error) = result {
            self.error(error);
        }
        (true, Action::None)
    }
    fn character(&mut self, unit: u16) {
        if unit < 0x20 || unit == 0x7f {
            return;
        }
        if (0xd800..=0xdbff).contains(&unit) {
            if self.pending_surrogate.replace(unit).is_some() {
                self.status = "Se rechazó un sustituto alto UTF16 sin pareja".into();
            }
            return;
        }
        let scalar = if (0xdc00..=0xdfff).contains(&unit) {
            let Some(high) = self.pending_surrogate.take() else {
                self.status = "Se rechazó un sustituto bajo UTF16 sin pareja".into();
                return;
            };
            0x10000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(unit) - 0xdc00)
        } else {
            if self.pending_surrogate.take().is_some() {
                self.status = "Se rechazó una secuencia UTF16 sin pareja".into();
                return;
            }
            u32::from(unit)
        };
        if let Some(ch) = char::from_u32(scalar) {
            self.insert_character(ch);
        }
    }
    fn insert_character(&mut self, ch: char) {
        if self.zoom.open {
            return;
        }
        self.cancel_pointer();
        if ch.is_control() {
            return;
        }
        if self.omnicore_character(ch) {
            return;
        }
        let mut bytes = [0u8; 4];
        if self.selector.is_some() {
            self.edit_selector_input(jc_app::FieldEdit::Insert(ch.encode_utf8(&mut bytes)), false);
            return;
        }
        if self.snippet_gui.is_some() {
            self.snippet_character(ch);
            return;
        }
        if self.source_visible() {
            self.source_character(ch);
            return;
        }
        if self.open_diff_visible || self.markdown_gui.is_some() {
            return;
        }
        if self.git_panel.is_some() {
            if self.git_commit.view().is_some() {
                self.git_commit
                    .edit(jc_app::FieldEdit::Insert(ch.encode_utf8(&mut bytes)));
                self.git_workspace.invalidate();
            } else {
                self.git_commit_search_character(ch);
            }
            return;
        }
        if self.replace_panel.is_some() {
            self.edit_replace_field(jc_app::FieldEdit::Insert(ch.encode_utf8(&mut bytes)));
            return;
        }
        if self.project_visible() {
            self.edit_project_field(jc_app::FieldEdit::Insert(ch.encode_utf8(&mut bytes)));
            return;
        }
        if self.search_panel.is_some() {
            self.edit_search_input(jc_app::FieldEdit::Insert(ch.encode_utf8(&mut bytes)));
            return;
        }
        if self.navigation_panel.is_some() {
            self.edit_navigation_input(jc_app::FieldEdit::Insert(ch.encode_utf8(&mut bytes)));
            return;
        }
        if self.selector.is_some() {
            self.edit_selector_input(jc_app::FieldEdit::Insert(ch.encode_utf8(&mut bytes)), false);
            return;
        }
        if self.explorer_focused {
            return;
        }
        if self.bottom.focused {
            return;
        }
        match self.workbench.insert_text(ch.encode_utf8(&mut bytes)) {
            Ok(()) => {
                self.note_omnicore_typing();
                self.status = "Listo".into();
            }
            Err(error) => self.error(error),
        }
    }
}
fn supported(command: CommandId) -> bool {
    native_menu::is_supported_macro(command)
        || COMMANDS.iter().any(|(_, value, _)| *value == command)
        || matches!(
            command,
            CommandId::EditDocumentStart
                | CommandId::EditDocumentEnd
                | CommandId::EditSelectDocumentStart
                | CommandId::EditSelectDocumentEnd
                | CommandId::EditIndent
                | CommandId::EditOutdent
                | CommandId::EditorOpenToSide
                | CommandId::SearchCancel
                | CommandId::EditRectangleLeft
                | CommandId::EditRectangleRight
                | CommandId::EditRectangleUp
                | CommandId::EditRectangleDown
                | CommandId::Tab1
                | CommandId::Tab2
                | CommandId::Tab3
                | CommandId::Tab4
                | CommandId::Tab5
                | CommandId::Tab6
                | CommandId::Tab7
                | CommandId::Tab8
                | CommandId::Tab9
        )
}

/// Box remains Rust-owned during CreateWindowEx. Only after success does the
/// HWND acquire ownership; failed creation cannot double-free an NCCREATE pointer.
struct Owner {
    client_capture: Cell<bool>,
    state: RefCell<State>,
    buffer: RefCell<back_buffer::Buffer>,
    frame: RefCell<Option<std::rc::Rc<paint::Frame>>>,
    frame_dirty: Cell<bool>,
    frame_preparations: Cell<usize>,
    menu_theme: RefCell<Option<std::rc::Rc<menu_theme::Theme>>>,
    hwnd_owned: Cell<bool>,
    uia_id: usize,
    uia_events: Cell<Option<uia::EventState>>,
}
// Caller bounds the returned reference to a live HWND; never retain it across
// DestroyWindow. Dialog/message calls are made only after RefCell guards end.
unsafe fn owner<'window>(hwnd: Hwnd) -> Option<&'window Owner> {
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Owner;
    pointer.as_ref()
}

unsafe extern "system" fn window_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        procedure(hwnd, message, wparam, lparam)
    })) {
        Ok(value) => value,
        Err(_) => {
            PostQuitMessage(1);
            0
        }
    }
}
unsafe fn procedure(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize {
    if message == 0x83 && chrome::calc_client(hwnd, wparam, lparam) {
        return 0;
    }
    if message == WM_NCCREATE {
        let create = &*(lparam as *const CreateStruct);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.parameter as isize);
        if let Some(owner) = owner(hwnd) {
            owner.state.borrow().bridge.attach(hwnd);
        }
        // DefWindowProc establishes native non-client state, including the
        // supplied window title. Attaching our owner does not replace it.
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    if message == WM_NCDESTROY {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Owner;
        if !pointer.is_null() {
            (*pointer).state.borrow().bridge.detach();
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        if !pointer.is_null() && (*pointer).hwnd_owned.get() {
            drop(Box::from_raw(pointer));
        }
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    if message == WM_DESTROY {
        PostQuitMessage(0);
        return 0;
    }
    let Some(owner) = owner(hwnd) else {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    };
    if message == uia::GET_OBJECT && lparam as i32 == -25 {
        return uia::get_object(uia::Window::new(hwnd, owner.uia_id), wparam, lparam);
    }
    if message == uia::REQUEST {
        if uia::serve(uia::Window::new(hwnd, owner.uia_id), wparam, &owner.state) {
            owner.frame_dirty.set(true);
            InvalidateRect(hwnd, std::ptr::null(), 0);
            PostMessageW(hwnd, uia::NOTIFY, owner.uia_id, 0);
        }
        return 0;
    }
    if message == uia::NOTIFY {
        if wparam == owner.uia_id {
            uia::notify(hwnd);
        }
        return 0;
    }
    if message == 0x84 {
        let layout = {
            let state = owner.state.borrow();
            state.chrome_layout()
        };
        return chrome::hit_test(hwnd, lparam, layout);
    }
    if message == dpi::CHANGED {
        let Some(rect) = (lparam as *const Rect).as_ref().copied() else {
            return 0;
        };
        if let Ok(mut state) = owner.state.try_borrow_mut() {
            // The previous frame stores font handles owned by State.
            owner.frame.borrow_mut().take();
            owner.frame_dirty.set(true);
            if let Err(error) =
                state.refresh_dpi(hwnd, dpi::Dpi::from_raw((wparam >> 16) as u16 as u32))
            {
                state.error(error);
            }
            if let Some(theme) = owner.menu_theme.borrow().as_ref() {
                if let Err(error) = theme.set_text_scale(state.dpi, state.ui_text_size) {
                    state.error(error);
                }
            }
        } else {
            return 0;
        }
        // SetWindowPos may synchronously deliver WM_SIZE. No model borrow crosses it.
        dpi::apply_rect(hwnd, rect);
        release_idle_capture(hwnd);
        InvalidateRect(hwnd, std::ptr::null(), 0);
        return 0;
    }
    if matches!(message, 0x2b | 0x2c | 0x120) {
        let theme = owner.menu_theme.borrow().clone();
        if let Some(theme) = theme {
            if message == 0x120 {
                if let Some(result) = theme.menu_char(wparam, lparam) {
                    return result;
                }
            } else {
                let result = if message == 0x2b {
                    theme.draw(lparam)
                } else {
                    theme.measure(lparam)
                };
                match result {
                    Ok(true) => return 1,
                    Ok(false) => {}
                    Err(error) => {
                        if let Ok(mut state) = owner.state.try_borrow_mut() {
                            state.paint_errors += 1;
                            state.error(error);
                        }
                        return 0;
                    }
                }
            }
        }
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    // Standard client printing also supports hidden-window/native-render probes.
    // It consumes the same immutable frame and retained DIB as ordinary paint;
    // the caller owns its target HDC. Printing never publishes Markdown hits.
    const WM_PRINTCLIENT: u32 = 0x0318;
    let printing = message == WM_PRINTCLIENT;
    if printing && (wparam == 0 || lparam & 4 == 0) {
        return 0;
    }
    if message == WM_PAINT || printing {
        let frame = {
            let Ok(mut state) = owner.state.try_borrow_mut() else {
                return DefWindowProcW(hwnd, message, wparam, lparam);
            };
            state.paint_count += 1;
            let cached = if !owner.frame_dirty.get() {
                owner.frame.borrow().clone()
            } else {
                None
            };
            if let Some(frame) = cached {
                frame
            } else {
                owner.frame.borrow_mut().take();
                let frame = std::rc::Rc::new(paint::prepare(&mut state));
                state.explorer_hits = frame.explorer_hits();
                state.git_workspace.hits = frame.git_hits();
                state.outline.hits = frame.outline_hits();
                state.omnicore.hits = frame.omnicore_hits();
                state.bottom_hits = frame.bottom_hits();
                state.tab_hits = Some(frame.tab_hits());
                *owner.frame.borrow_mut() = Some(frame.clone());
                owner.frame_dirty.set(false);
                owner
                    .frame_preparations
                    .set(owner.frame_preparations.get().saturating_add(1));
                frame
            }
        };
        let mut native_paint = (!printing).then(|| NativePaint {
            hwnd,
            paint: std::mem::zeroed(),
        });
        let dc = match native_paint.as_mut() {
            Some(paint) => BeginPaint(hwnd, &mut paint.paint),
            None => wparam as Hdc,
        };
        if !dc.is_null() {
            let mut composed = false;
            let result = owner
                .buffer
                .try_borrow_mut()
                .map_err(|_| "La superficie GUI ya está en uso".to_owned())
                .and_then(|mut buffer| {
                    buffer.paint(dc, frame.width, frame.height, |memory| {
                        let result = paint::draw(memory, &frame);
                        composed = result.is_ok();
                        result
                    })
                });
            if !printing {
                if result.is_ok() && composed {
                    frame.publish_selection_preview();
                    frame.publish_omnicore_completion();
                }
                if let Ok(mut state) = owner.state.try_borrow_mut() {
                    state.publish_markdown_hits(if result.is_ok() && composed {
                        frame.markdown_hits()
                    } else {
                        None
                    });
                    state.git_code.hits = if result.is_ok() && composed {
                        frame.git_code_hits()
                    } else {
                        None
                    };
                }
            }
            if let Err(error) = result {
                if let Ok(mut state) = owner.state.try_borrow_mut() {
                    state.paint_errors += 1;
                    state.error(error);
                }
            }
        } else if let Ok(mut state) = owner.state.try_borrow_mut() {
            state.paint_errors += 1;
            state.error("BeginPaint no devolvió un contexto de dispositivo");
        }
        return 0;
    }
    if message == ime::START {
        let caret = {
            let Ok(mut state) = owner.state.try_borrow_mut() else {
                return DefWindowProcW(hwnd, message, wparam, lparam);
            };
            state.pending_surrogate = None;
            if state.clear_omnicore_completion() {
                owner.frame_dirty.set(true);
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            state.ime_target = state.ime_editor_target();
            state.ime_target.and_then(|_| state.ime_caret())
        };
        if let Some(caret) = caret {
            ime::place(hwnd, caret);
        }
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    if message == ime::END {
        if let Ok(mut state) = owner.state.try_borrow_mut() {
            state.ime_target = None;
        }
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    let ime_result = if message == ime::COMPOSITION && lparam & ime::RESULT != 0 {
        let target = owner
            .state
            .try_borrow()
            .ok()
            .and_then(|state| state.ime_target);
        target.map(|target| (target, ime::read_result(hwnd)))
    } else {
        None
    };
    let handled_ime = ime_result.is_some();
    // No RefCell borrow crosses dialogs, DestroyWindow or SendMessage. Modal
    // Win32 calls may reenter this callback and prepare another read-only frame.
    // Screen coordinates and OS settings are read outside the model borrow.
    let wheel = if message == WM_MOUSEWHEEL {
        let mut point = Point {
            x: (lparam as u16) as i16 as i32,
            y: ((lparam as usize >> 16) as u16) as i16 as i32,
        };
        let mut lines = 3u32;
        SystemParametersInfoW(0x68, 0, (&mut lines as *mut u32).cast(), 0);
        (ScreenToClient(hwnd, &mut point) != 0).then_some((point, lines))
    } else {
        None
    };
    let mut action = Action::None;
    let mut handled = true;
    let mut redraw = true;
    {
        let Ok(mut state) = owner.state.try_borrow_mut() else {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        };
        let editing_event = matches!(
            message,
            WM_CHAR | WM_UNICHAR | WM_KEYDOWN | WM_SYSKEYDOWN | WM_COMMAND
        ) || handled_ime;
        let before_edit = editing_event
            .then(|| state.capture_omnicore_before_edit())
            .flatten();
        match message {
            WM_CREATE => {
                if state.workbench.large_open_pending() {
                    state.wakeup.signal();
                }
                shell::dark_caption(hwnd);
                state.icon = GetClassLongPtrW(hwnd, -14) as Handle;
                if let Some(theme) = owner.menu_theme.borrow().clone() {
                    if let Err(error) = theme.apply_palette(menu_theme::Palette::current()) {
                        state.error(error);
                    }
                }
                if let Err(error) = state.refresh_dpi(hwnd, dpi::for_window(hwnd)) {
                    state.error(error);
                }
                if let Some(theme) = owner.menu_theme.borrow().as_ref() {
                    if let Err(error) = theme.set_text_scale(state.dpi, state.ui_text_size) {
                        state.error(error);
                    }
                }
            }
            TASKS_MESSAGE => {
                redraw = state.poll_tasks();
                redraw |= state.poll_periodic_recovery();
            }
            0x113 if wparam == recovery::TIMER_ID => {
                redraw = state.poll_periodic_recovery();
            }
            0x113 if wparam == omnicore_panel::completion::IDLE_TIMER => {
                recovery::KillTimer(hwnd, wparam);
                redraw = state.poll_omnicore_idle();
                if let Some(delay) = state.omnicore_idle_delay() {
                    recovery::SetTimer(hwnd, wparam, delay, 0);
                }
            }
            WM_SIZE => {
                state.cancel_pointer();
                state.zoom.open = false;
                state.interaction_epoch = state.interaction_epoch.saturating_add(1);
                state.explorer_hits = None;
                state.outline.hits = None;
                state.bottom_hits = None;
                state.tab_hits = None;
                state.maximized = wparam == 2;
                let mut rect = Rect::default();
                if GetClientRect(hwnd, &mut rect) != 0 {
                    state.width = rect.right.max(1);
                    state.height = rect.bottom.max(1);
                    if explorer_panel::region(&state).is_none() {
                        state.explorer_focused = false;
                    }
                    if bottom_panel::region(&state).is_none() {
                        state.bottom.focused = false;
                    }
                }
            }
            WM_SETFOCUS => {
                state.focused = true;
                state.refresh_git_repositories();
                state.schedule_git_status();
            }
            WM_KILLFOCUS => {
                state.cancel_pointer();
                state.zoom.open = false;
                state.interaction_epoch = state.interaction_epoch.saturating_add(1);
                state.focused = false;
                state.pending_surrogate = None;
            }
            WM_ERASEBKGND => return 1,
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                if wparam == 0x0d
                    && lparam & (1 << 30) != 0
                    && state
                        .git_commit
                        .view()
                        .is_some_and(|view| view.merge_review().is_some())
                    && state.modifiers() == Modifiers::CTRL
                {
                    return 0;
                }
                let result = state.key(wparam);
                handled = result.0;
                action = result.1;
            }
            WM_KEYUP | WM_SYSKEYUP if wparam == 0x79 => {
                redraw = false;
            }
            WM_SYSCHAR
                if char::from_u32(wparam as u32).is_some_and(|ch| {
                    native_menu::mnemonic(ch.to_ascii_uppercase() as usize, Modifiers::ALT)
                        .is_some()
                }) =>
            {
                redraw = false;
            }
            WM_CHAR => state.character(wparam as u16),
            ime::COMPOSITION if handled_ime => {
                let (target, result) = ime_result.unwrap();
                state.ime_commit(target, result);
            }
            WM_UNICHAR => {
                if wparam == 0xffff {
                    return 1;
                }
                if let Some(ch) = char::from_u32(wparam as u32) {
                    state.insert_character(ch);
                }
            }
            WM_COMMAND if lparam == 0 => {
                if state.accept_menu_selection() {
                    if wparam & 0xffff == OPEN_SIDE {
                        if state.registry.is_enabled(
                            CommandId::FileOpen,
                            &state.entry_context(CommandId::FileOpen),
                        ) {
                            state.close_selector();
                            action = Action::Open(true);
                        }
                    } else if let Some((_, command, _)) =
                        COMMANDS.iter().find(|(id, _, _)| *id == wparam & 0xffff)
                    {
                        action = state.execute_menu(*command);
                    }
                }
            }
            WM_INITMENU => {
                state.cancel_pointer();
                state.menu_stamp = Some(Box::new(state.current_menu_stamp()));
                if let Err(error) = native_menu::refresh(hwnd, &state) {
                    state.error(error);
                }
            }
            WM_MOUSEWHEEL => {
                redraw = false;
                if state.zoom.open {
                    action = state.zoom_step(((wparam >> 16) as u16 as i16 as i32).signum());
                } else if let Some((point, lines)) = wheel {
                    let lines = state.smoke_wheel_lines.unwrap_or(lines);
                    let delta = ((wparam >> 16) as u16) as i16 as i32;
                    redraw = state.omnicore_wheel(point.x, point.y, delta)
                        || state.omnicore_diff_wheel(point.x, point.y, delta)
                        || state.pointer_wheel(point.x, point.y, delta, lines, wparam & 0xffff);
                } else {
                    state.cancel_pointer();
                }
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let x = (lparam as u16) as i16 as i32;
                let y = ((lparam as usize >> 16) as u16) as i16 as i32;
                if !state.omnicore_region().is_some_and(|rect| {
                    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
                }) {
                    state.blur_omnicore();
                }
                state.cancel_pointer();
                let chrome_action = chrome::action_at(state.chrome_layout(), x, y);
                let zoom_action = if chrome_action.is_none() {
                    state.zoom_down(x, y)
                } else {
                    state.zoom.open = false;
                    None
                };
                let tab_action = if message == WM_LBUTTONDOWN
                    && chrome_action.is_none()
                    && zoom_action.is_none()
                    && (state.git_panel.is_none() || state.git_code.active)
                {
                    state.tab_click(x, y)
                } else {
                    None
                };
                if let Some(chrome_action) = chrome_action {
                    action = match chrome_action {
                        chrome::Action::Minimize => Action::Minimize,
                        chrome::Action::MaxRestore => Action::MaxRestore,
                        chrome::Action::Exit => Action::Exit,
                        chrome::Action::Menu(group) => Action::NativeMenu(group),
                    };
                } else if let Some(zoom_action) = zoom_action {
                    action = zoom_action;
                } else if let Some(tab_action) = tab_action {
                    action = tab_action;
                } else if let Some(git_action) = state.status_git_click(x, y) {
                    action = git_action;
                } else if state.status_click(x, y) {
                    // Status controls consume the click before source hit-testing.
                } else if let Some(omnicore_action) = state.omnicore_click(x, y) {
                    action = omnicore_action;
                } else if state.omnicore_diff_click(x, y) {
                    // Comparison coordinates never fall through to the document.
                } else if state.git_code.active {
                    if message == WM_LBUTTONDOWN && state.begin_explorer_scroll(x, y) {
                        action = Action::CapturePointer;
                    } else if !state.explorer_click(x, y, message == WM_LBUTTONDBLCLK) {
                        action = state.git_code_click(x, y);
                    }
                } else if state.git_panel.is_some() {
                    action = state.git_workspace_click(x, y);
                } else if state.markdown_gui.is_some() {
                    if !state.explorer_click(x, y, message == WM_LBUTTONDBLCLK)
                        && message == WM_LBUTTONDOWN
                    {
                        let to_side = state.modifiers().contains(Modifiers::CTRL);
                        action = state.markdown_click(x, y, to_side);
                    }
                } else if state.pointer_source_available()
                    && message == WM_LBUTTONDOWN
                    && state.begin_explorer_scroll(x, y)
                {
                    action = Action::CapturePointer;
                } else if state.pointer_source_available()
                    && !state.bottom_click(x, y, message == WM_LBUTTONDBLCLK)
                    && !state.explorer_click(x, y, message == WM_LBUTTONDBLCLK)
                    && message == WM_LBUTTONDOWN
                {
                    let modifiers = state.modifiers();
                    let started = if state.begin_editor_scroll(x, y) {
                        true
                    } else if modifiers.contains(Modifiers::ALT)
                        && !modifiers.contains(Modifiers::CTRL)
                    {
                        state.begin_rectangle_drag(x, y)
                    } else {
                        state.begin_linear_drag(x, y, modifiers.contains(Modifiers::SHIFT))
                    };
                    if started {
                        action = Action::CapturePointer;
                    }
                }
            }
            WM_MOUSEMOVE | WM_LBUTTONUP => {
                redraw = state.zoom.preview.is_some()
                    || state.git_workspace.dragging()
                    || state.git_code.dragging()
                    || state.explorer_scroll.drag.is_some()
                    || state.rectangle_drag.is_some()
                    || state.linear_drag.is_some()
                    || state.editor_scroll_drag.is_some();
                let point = (
                    (lparam as u16) as i16 as i32,
                    ((lparam as usize >> 16) as u16) as i16 as i32,
                );
                let hover = chrome::action_at(state.chrome_layout(), point.0, point.1);
                let hover_changed = state.chrome_hover != hover;
                if hover_changed {
                    state.chrome_hover = hover;
                    redraw = true;
                }
                if wparam & 1 != 0 || message == WM_LBUTTONUP {
                    let x = (lparam as u16) as i16 as i32;
                    let y = ((lparam as usize >> 16) as u16) as i16 as i32;
                    if state.zoom.preview.is_some() {
                        action = state.zoom_drag(y, message == WM_LBUTTONUP);
                    } else if state.git_code.dragging() {
                        redraw = state.update_git_code_drag(x, y) || hover_changed;
                    } else if state.git_workspace.dragging() {
                        redraw = state.update_git_drag(x, y) || hover_changed;
                    } else if state.explorer_scroll.drag.is_some() {
                        redraw = state.update_explorer_scroll(x, y) || hover_changed;
                    } else if state.editor_scroll_drag.is_some() {
                        redraw = state.update_editor_scroll(x, y) || hover_changed;
                    } else if state.rectangle_drag.is_some() {
                        state.update_rectangle_drag(x, y);
                    } else {
                        state.update_linear_drag(x, y);
                    }
                }
                if wparam & 1 == 0 || message == WM_LBUTTONUP {
                    state.cancel_pointer();
                }
            }
            WM_CAPTURECHANGED | WM_CANCELMODE => {
                if message == WM_CAPTURECHANGED && GetCapture() != hwnd {
                    owner.client_capture.set(false);
                }
                state.cancel_pointer();
                if message == WM_CANCELMODE {
                    state.zoom.open = false;
                }
            }
            WM_CLOSE => action = Action::Exit,
            _ => handled = false,
        }
        if state.exit_ready {
            action = Action::FinishExit;
        }
        if editing_event {
            state.remember_omnicore_edit(before_edit);
        }
        state.schedule_search();
        redraw |= state.validate_project_search();
        redraw |= state.validate_git_context();
        redraw |= state.validate_open_diff();
        redraw |= state.validate_source();
        redraw |= state.validate_omnicore_completion();
        redraw |= state.validate_omnicore_next_edit();
        if matches!(message, WM_CHAR | WM_UNICHAR) {
            if let Some(delay) = state.omnicore_idle_delay() {
                recovery::SetTimer(hwnd, omnicore_panel::completion::IDLE_TIMER, delay, 0);
            }
        }
        state.schedule_outline();
        state.schedule_git_discovery();
        state.schedule_omnicore();
        state.validate_explorer_scroll();
        state.validate_rectangle_drag();
        state.validate_linear_drag();
        if redraw && !matches!(message, TASKS_MESSAGE | 0x113 | WM_CREATE) {
            state
                .periodic_recovery
                .note_activity(std::time::Instant::now());
        }
    }
    if !handled {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    release_idle_capture(hwnd);
    recovery::arm(hwnd);
    let clipboard_edit = matches!(
        action,
        Action::Clipboard(CommandId::EditPaste | CommandId::EditCut)
    );
    let before_clipboard = clipboard_edit
        .then(|| owner.state.borrow().capture_omnicore_before_edit())
        .flatten();
    perform(hwnd, action);
    if message == TASKS_MESSAGE {
        redraw |= save_flow::poll_paged_save(hwnd);
    }
    if let Some(current) = self::owner(hwnd) {
        let mut state = current.state.borrow_mut();
        if clipboard_edit {
            state.remember_omnicore_edit(before_clipboard);
        }
        if message != TASKS_MESSAGE && state.workbench.large_open_pending() {
            redraw |= state.poll_large_file();
        }
        state.schedule_search();
        state.schedule_outline();
        state.schedule_git_discovery();
        state.schedule_omnicore();
    }
    release_idle_capture(hwnd);
    if redraw && IsWindow(hwnd) != 0 {
        if let Some(owner) = self::owner(hwnd) {
            owner.frame_dirty.set(true);
        }
        InvalidateRect(hwnd, std::ptr::null(), 0);
        uia::notify(hwnd);
    }
    if handled_ime && lparam & ime::PREEDIT != 0 && IsWindow(hwnd) != 0 {
        // Preserve native preedit for mixed commit/composition messages.
        // Forwarding result flags would produce duplicate WM_CHAR input.
        DefWindowProcW(hwnd, message, 0, lparam & ime::PREEDIT)
    } else if message == WM_UNICHAR {
        1
    } else {
        0
    }
}

struct NativePaint {
    hwnd: Hwnd,
    paint: PaintStruct,
}
impl Drop for NativePaint {
    fn drop(&mut self) {
        if !self.paint.dc.is_null() {
            unsafe {
                EndPaint(self.hwnd, &self.paint);
            }
        }
    }
}

unsafe fn file_dialog(hwnd: Hwnd, save: bool) -> Result<Option<PathBuf>, String> {
    file_dialog_named(hwnd, save, None)
}
unsafe fn file_dialog_named(
    hwnd: Hwnd,
    save: bool,
    title: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    file_dialog_options(hwnd, save, title, false)
}

unsafe fn file_dialog_options(
    hwnd: Hwnd,
    save: bool,
    title: Option<&str>,
    patch: bool,
) -> Result<Option<PathBuf>, String> {
    let mut file = vec![0u16; 32768];
    let filter = wide(if patch {
        "Parche Git (*.patch)\0*.patch\0"
    } else {
        "Todos los archivos\0*.*\0"
    });
    let patch_extension = wide("patch");
    let title = wide(title.unwrap_or(if save {
        "Guardar fuente como"
    } else {
        "Abrir fuente"
    }));
    let mut options: OpenFileName = std::mem::zeroed();
    options.size = std::mem::size_of::<OpenFileName>() as u32;
    options.owner = hwnd;
    options.filter = filter.as_ptr();
    options.file = file.as_mut_ptr();
    options.file_len = file.len() as u32;
    options.title = title.as_ptr();
    options.flags = 0x80000 | 0x800 | 0x8 | 0x4;
    if patch {
        options.flags |= 0x0001_0000; // OFN_NOTESTFILECREATE
        options.default_extension = patch_extension.as_ptr();
    } else {
        options.flags |= if save { 0x2 } else { 0x1000 };
    }
    let accepted = if save {
        GetSaveFileNameW(&mut options)
    } else {
        GetOpenFileNameW(&mut options)
    };
    if accepted == 0 {
        let error = CommDlgExtendedError();
        return if error == 0 {
            Ok(None)
        } else {
            Err(format!("Falló el diálogo nativo de archivos ({error:#x})"))
        };
    }
    let end = file
        .iter()
        .position(|unit| *unit == 0)
        .ok_or("Resultado inválido del diálogo de archivos")?;
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(
        &file[..end],
    ))))
}
unsafe fn release_idle_capture(hwnd: Hwnd) {
    // Release can synchronously send WM_CAPTURECHANGED; drop the borrow first.
    let idle = owner(hwnd).is_some_and(|m| {
        if !m.client_capture.get() {
            return false;
        }
        let state = m.state.borrow();
        state.zoom.preview.is_none()
            && !state.git_workspace.dragging()
            && !state.git_code.dragging()
            && state.rectangle_drag.is_none()
            && state.linear_drag.is_none()
            && state.editor_scroll_drag.is_none()
            && state.explorer_scroll.drag.is_none()
    });
    if idle && GetCapture() == hwnd {
        if let Some(m) = owner(hwnd) {
            m.client_capture.set(false);
        }
        ReleaseCapture();
    }
}

unsafe fn perform(hwnd: Hwnd, action: Action) {
    if matches!(
        action,
        Action::Open(_)
            | Action::SaveAs
            | Action::ChooseFolder
            | Action::ChooseOmniCore
            | Action::ChooseOmniCoreModel
            | Action::ConfirmOmniCoreEdit
            | Action::EditOmniCoreSelection
            | Action::DataConnections
            | Action::ExportGitPatch(_)
    ) && owner(hwnd).is_some_and(|owner| owner.state.borrow().smoke)
    {
        if let Some(owner) = owner(hwnd) {
            owner.state.borrow_mut().status =
                "La prueba impidió un diálogo nativo inesperado".into();
        }
        return;
    }
    match action {
        Action::Appearance(choice) => appearance::apply(hwnd, choice),
        Action::CapturePointer => {
            SetCapture(hwnd);
            if let Some(m) = owner(hwnd) {
                m.client_capture.set(GetCapture() == hwnd);
            }
            if GetCapture() != hwnd {
                if let Some(model) = owner(hwnd) {
                    let mut state = model.state.borrow_mut();
                    state.cancel_pointer();
                    state.error("Captura del ratón no disponible; selección conservada");
                }
            }
        }
        Action::NativeMenu(group) => {
            if let Err(error) = native_menu::open(hwnd, group) {
                if let Some(model) = owner(hwnd) {
                    model.state.borrow_mut().error(error);
                }
            }
        }
        Action::None => {}
        Action::ExportGitPatch(comparison) => git_patch::choose_destination(hwnd, comparison),
        Action::OpenWeb(url) => {
            if let Err(error) = jc_platform::web_link::open(&url) {
                if let Some(owner) = owner(hwnd) {
                    owner.state.borrow_mut().error(error);
                }
            }
        }
        Action::Clipboard(command) => clipboard_flow::perform(hwnd, command),
        Action::PasteSearch(generation) => clipboard_flow::paste_search(hwnd, generation),
        Action::PasteGitSearch(token) => git_workspace::paste_commit_query(hwnd, token),
        Action::PasteGitCommit(token) => git_workspace::paste_commit_message(hwnd, token),
        Action::PasteOmniCore(stamp) => omnicore_panel::paste(hwnd, stamp),
        Action::PasteNavigation(generation) => clipboard_flow::paste_navigation(hwnd, generation),
        Action::PasteSnippet(target) => clipboard_flow::paste_snippet(hwnd, target),
        Action::SelectorClipboard(command, target) => {
            clipboard_flow::selector_transfer(hwnd, command, *target)
        }
        Action::PasteProject(generation) => clipboard_flow::paste_project(hwnd, generation),
        Action::PasteReplacement(generation) => clipboard_flow::paste_replacement(hwnd, generation),
        Action::SaveAll => save_flow::perform(hwnd),
        Action::DataConnections => data_connections::open(hwnd),
        Action::BrowseRecords => records::open(hwnd),
        Action::ChooseOmniCoreModel => omnicore_panel::choose_model(hwnd),
        Action::ConfirmOmniCoreEdit => omnicore_panel::confirm_edit(hwnd),
        Action::EditOmniCoreSelection => omnicore_panel::edit_selection(hwnd),
        Action::ChooseOmniCore => {
            let (root, initial) = owner(hwnd)
                .map(|model| {
                    let state = model.state.borrow();
                    (
                        state.project_root.clone(),
                        state.omnicore_executable_folder(),
                    )
                })
                .unwrap_or_default();
            let selected = folder::choose_file(hwnd, initial.as_deref());
            if let Some(owner) = owner(hwnd) {
                let mut state = owner.state.borrow_mut();
                if state.project_root == root {
                    match selected {
                        Ok(Some(path)) => state.connect_omnicore(path),
                        Ok(None) => {}
                        Err(error) => state.error(error),
                    }
                }
            }
        }
        Action::ChooseFolder => {
            let initial = owner(hwnd).and_then(|o| o.state.borrow().project_root.clone());
            let selected = folder::choose_folder(hwnd, initial.as_deref());
            if let Some(owner) = owner(hwnd) {
                let mut state = owner.state.borrow_mut();
                match selected {
                    Ok(Some(path)) => {
                        if let Err(error) = state.set_project_root(path) {
                            state.error(error);
                        } else {
                            state.show_explorer();
                        }
                    }
                    Ok(None) => {}
                    Err(error) => state.error(error),
                }
            }
        }
        Action::Open(side) => {
            let initial = owner(hwnd).and_then(|o| {
                let state = o.state.borrow();
                state.project_root.clone().or_else(|| {
                    state
                        .workbench
                        .active_path()
                        .and_then(|p| p.parent())
                        .map(|p| p.to_owned())
                })
            });
            let selected = folder::choose_file(hwnd, initial.as_deref());
            if let Some(owner) = owner(hwnd) {
                let mut state = owner.state.borrow_mut();
                match selected {
                    Ok(Some(path)) => {
                        let encoding = state.encoding;
                        match state.workbench.request_file_open(path, encoding, side) {
                            Ok(Some(_)) => state.file_opened(),
                            Ok(None) => {
                                state.status = "Copiando archivo grande en segundo plano…".into();
                            }
                            Err(error) => state.error(error),
                        }
                    }
                    Ok(None) => {}
                    Err(error) => state.error(error),
                }
            }
        }
        Action::SaveAs => {
            let captured = owner(hwnd)
                .ok_or_else(|| "Se cerró la ventana de Guardar como".to_owned())
                .and_then(|model| {
                    let state = model.state.borrow();
                    let document = state
                        .workbench
                        .active_document()
                        .ok_or("No hay documento activo para Guardar como")?;
                    state
                        .workbench
                        .capture_save_target(document.id, document.session.document().revision())
                        .map_err(|e| e.to_string())
                });
            let target = match captured {
                Ok(target) => target,
                Err(error) => {
                    if let Some(model) = owner(hwnd) {
                        model.state.borrow_mut().error(error);
                    }
                    return;
                }
            };
            let selected = file_dialog(hwnd, true);
            if let Some(owner) = owner(hwnd) {
                let mut state = owner.state.borrow_mut();
                match selected {
                    Ok(Some(path)) => match state.workbench.request_save_target(target, Some(path))
                    {
                        Ok(true) => {
                            state.save_count += 1;
                            state.status = "Guardado".into();
                            state.schedule_git_status();
                        }
                        Ok(false) => state.status = "Guardando archivo en segundo plano…".into(),
                        Err(error) => state.error(error),
                    },
                    Ok(None) => {}
                    Err(error) => state.error(error),
                }
            }
        }
        Action::CloseTab => close_flow::perform(hwnd),
        Action::Minimize => {
            ShowWindow(hwnd, 6);
        }
        Action::MaxRestore => {
            let maximized = owner(hwnd).is_some_and(|o| o.state.borrow().maximized);
            ShowWindow(hwnd, if maximized { 9 } else { 3 });
        }
        Action::Exit => {
            if let Some(owner) = owner(hwnd) {
                let result = owner.state.borrow_mut().request_exit();
                match result {
                    Ok(false) => {
                        owner.frame_dirty.set(true);
                        InvalidateRect(hwnd, std::ptr::null(), 0);
                        return;
                    }
                    Err(error) => {
                        owner
                            .state
                            .borrow_mut()
                            .error(format!("No se cerró el editor: {error}"));
                        owner.frame_dirty.set(true);
                        InvalidateRect(hwnd, std::ptr::null(), 0);
                        return;
                    }
                    Ok(true) => {}
                }
            }
            DestroyWindow(hwnd);
        }
        Action::FinishExit => {
            DestroyWindow(hwnd);
        }
    }
}

pub(super) fn run(options: GuiOptions) -> Result<(), String> {
    run_with_smoke(options, smoke::run)
}

fn run_with_smoke(
    options: GuiOptions,
    exercise: unsafe fn(Hwnd, &crate::SmokeOptions) -> Result<(), String>,
) -> Result<(), String> {
    if options
        .search_workers
        .is_some_and(|n| !(1..=4).contains(&n))
    {
        return Err("Las tareas del proyecto requieren1..4 trabajadores".into());
    }
    if options
        .smoke
        .as_ref()
        .is_some_and(|s| !s.measures_performance())
        && !options.paths.is_empty()
    {
        return Err("El modo de prueba no admite rutas de fuente al iniciar".into());
    }
    if let Some(smoke) = &options.smoke {
        smoke::validate(smoke)?;
        if smoke.measures_performance() {
            performance_probe::validate_sources(&options.paths, smoke)?;
        }
    }
    // The HWND adopts this thread context when it is created.
    let _dpi_context = unsafe { dpi::ThreadContext::enter() };
    let mut owner = Box::new(Owner {
        client_capture: Cell::new(false),
        state: RefCell::new(State::new(&options)?),
        buffer: RefCell::new(back_buffer::Buffer::default()),
        frame: RefCell::new(None),
        frame_dirty: Cell::new(true),
        frame_preparations: Cell::new(0),
        menu_theme: RefCell::new(None),
        hwnd_owned: Cell::new(false),
        uia_id: uia::identity(),
        uia_events: Cell::new(None),
    });
    let class = wide(&format!("JCIDE.Native.{}", std::process::id()));
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let descriptor = WndClass {
            style: 3 | 8,
            proc: Some(window_proc),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: LoadIconW(instance, 101usize as *const u16),
            cursor: LoadCursorW(null_mut(), 32512usize as *const u16),
            background: null_mut(),
            menu_name: std::ptr::null(),
            class_name: class.as_ptr(),
        };
        if RegisterClassW(&descriptor) == 0 {
            return Err(os_error("RegisterClassW"));
        }
        let (menu, menu_theme) = match native_menu::create(&owner.state.borrow().keymap) {
            Ok(menu) => menu,
            Err(error) => {
                UnregisterClassW(class.as_ptr(), instance);
                return Err(error);
            }
        };
        // Retain this outer Rc until the HWND and its native menus are gone.
        owner.menu_theme.replace(Some(menu_theme.clone()));
        let restored_size = owner.state.borrow().restore_window;
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("CIDE").as_ptr(),
            0x00cf0000,
            0x80000000u32 as i32,
            0x80000000u32 as i32,
            restored_size.map_or(1000, |v| v.0),
            restored_size.map_or(700, |v| v.1),
            null_mut(),
            menu,
            instance,
            (&mut *owner as *mut Owner).cast(),
        );
        if hwnd.is_null() {
            DestroyMenu(menu);
            UnregisterClassW(class.as_ptr(), instance);
            return Err(os_error("CreateWindowExW"));
        }
        owner.hwnd_owned.set(true);
        let _ = Box::into_raw(owner);
        if let Err(error) = chrome::apply_frame(hwnd) {
            DestroyWindow(hwnd);
            UnregisterClassW(class.as_ptr(), instance);
            return Err(error);
        }
        recovery::arm(hwnd);
        if options.smoke.is_none() {
            let maximized = restored_size.is_some_and(|v| v.2);
            ShowWindow(hwnd, if maximized { 3 } else { 5 });
            SetFocus(hwnd);
            UpdateWindow(hwnd);
        } else if PostMessageW(hwnd, SMOKE_MESSAGE, 0, 0) == 0 {
            let error = os_error("Post owned smoke message");
            DestroyWindow(hwnd);
            UnregisterClassW(class.as_ptr(), instance);
            return Err(error);
        }
        let mut smoke_result = None;
        let mut message: Msg = std::mem::zeroed();
        let result = loop {
            // A cooperative search may post its next slot continuously. Give
            // hardware input and an invalidated paint bounded priority before
            // consuming that work, rather than starving them behind posted jobs.
            for _ in 0..32 {
                let mut input: Msg = std::mem::zeroed();
                if IsWindow(hwnd) == 0
                    || (PeekMessageW(&mut input, hwnd, WM_KEYDOWN, WM_UNICHAR, 1) == 0
                        && PeekMessageW(&mut input, hwnd, 0x200, 0x20e, 1) == 0)
                {
                    break;
                }
                TranslateMessage(&input);
                DispatchMessageW(&input);
            }
            let mut painting: Msg = std::mem::zeroed();
            if IsWindow(hwnd) != 0 && PeekMessageW(&mut painting, hwnd, WM_PAINT, WM_PAINT, 1) != 0
            {
                DispatchMessageW(&painting);
            }
            let received = GetMessageW(&mut message, null_mut(), 0, 0);
            if received < 0 {
                break Err(os_error("GetMessageW"));
            }
            if received == 0 {
                break if message.wparam == 0 {
                    Ok(())
                } else {
                    Err("Falló el callback de la ventana nativa".into())
                };
            }
            if message.hwnd == hwnd && message.message == SMOKE_MESSAGE {
                if let Some(config) = &options.smoke {
                    smoke_result = Some(exercise(hwnd, config));
                }
                // Test failures must also close without a modal dirty prompt.
                if IsWindow(hwnd) != 0 {
                    DestroyWindow(hwnd);
                }
                continue;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        };
        if IsWindow(hwnd) != 0 {
            DestroyWindow(hwnd);
        }
        UnregisterClassW(class.as_ptr(), instance);
        result?;
        smoke_result.unwrap_or(Ok(()))
    }
}
