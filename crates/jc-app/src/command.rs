use crate::focus::PrimaryFocus;
use jc_languages::by_id;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CommandCategory {
    File,
    Edit,
    Search,
    Project,
    Build,
    View,
    Navigation,
    Tools,
    Menu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CommandId {
    FileNew,
    FileOpen,
    FileQuickOpen,
    FileOpenFolder,
    FileChooseEncoding,
    FileSave,
    FileSaveAs,
    FileSaveAll,
    FileClose,
    FileReopenClosed,
    FileExit,

    EditUndo,
    EditRedo,
    EditCut,
    EditCopy,
    EditPaste,
    EditSelectAll,
    EditDocumentStart,
    EditDocumentEnd,
    EditSelectDocumentStart,
    EditSelectDocumentEnd,
    EditDuplicate,
    EditDeleteLine,
    EditMoveLineUp,
    EditMoveLineDown,
    EditToggleLineComment,
    EditToggleBlockComment,
    EditIndent,
    EditOutdent,
    EditJoinLines,
    EditSplitLine,
    EditRectangleStart,
    EditRectangleLeft,
    EditRectangleRight,
    EditRectangleUp,
    EditRectangleDown,
    EditRectangleCancel,

    SearchFind,
    SearchReplace,
    SearchFindNext,
    SearchFindPrevious,
    SearchSelectionNext,
    SearchSelectionPrevious,
    SearchGoToPosition,
    SearchCancel,
    SearchHistoryPrevious,
    SearchHistoryNext,
    SearchProject,
    SearchCancelProject,
    SearchReplaceProject,
    SearchToggleCase,
    SearchToggleWholeWord,
    SearchToggleRegex,

    PaletteOpen,

    TabNext,
    TabPrevious,
    Tab1,
    Tab2,
    Tab3,
    Tab4,
    Tab5,
    Tab6,
    Tab7,
    Tab8,
    Tab9,

    ViewExplorer,
    ExplorerRefresh,
    ExplorerCancel,
    ViewToggleInvisibles,
    ViewMarkdownPreview,
    ViewDatabase,
    ViewGit,
    GitDiffHead,
    GitDiffIndex,
    GitDiffRefresh,
    GitDiffCancel,
    GitConfigureExecutable,
    GitRepositoryStatus,
    GitStatusRefresh,
    GitStatusCancel,
    DiffCompareOpen,
    DiffRefreshOpen,
    DiffCancelOpen,
    DiffOpenNavigateLeft,
    DiffOpenNavigateRight,
    ViewSymbols,
    SourceReferences,
    SourceCalls,
    SourceIncoming,
    SourceDependencies,
    SourceComplete,
    SourceRename,
    SourceRefresh,
    SourceCancel,
    SourceAccept,
    SourceOpenToSide,
    EditInsertSnippet,
    EditSurroundWith,
    SnippetNext,
    SnippetPrevious,
    SnippetAccept,
    SnippetCancel,
    ViewToggleBottomPanel,
    ViewTerminal,
    ViewProblems,
    ViewOutput,
    ViewSearchResults,
    EditorSplitRight,
    EditorOneGroup,
    EditorFocusPrimary,
    EditorFocusSecondary,
    EditorTogglePin,
    EditorToggleScrollLink,
    EditorOpenToSide,
    NavigateBack,
    NavigateForward,

    FocusNext,
    FocusPrevious,

    ProgressCheck,
    ProgressCompile,
    ProgressRun,
    ProgressRefreshSchema,

    MenuFile,
    MenuEdit,
    MenuSearch,
    MenuProject,
    MenuBuild,
    MenuTools,
    MenuView,
    MenuHelp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandContext {
    pub focus: PrimaryFocus,
    /// The frontend's focused control targets source editing (including an
    /// editor-invoked menu/palette), rather than a modal field or read model.
    pub editor_input: bool,
    /// A frontend field handles clipboard/select-all locally, never source edits.
    pub field_input: bool,
    pub has_document: bool,
    pub document_dirty: bool,
    pub dirty_documents: usize,
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_selection: bool,
    pub rectangular_selection: bool,
    pub clipboard_has_text: bool,
    /// Frontend has a working paste handler for an otherwise unselected field.
    pub field_paste: bool,
    pub has_project: bool,
    pub explorer_focused: bool,
    pub project_search_running: bool,
    pub project_replace_ready: bool,
    pub document_search_running: bool,
    pub search_history_available: bool,
    pub active_language: Option<String>,
    pub has_progress_profile: bool,
    pub has_database_schema: bool,
    pub can_reopen_closed_tab: bool,
    pub terminal_available: bool,
    pub git_diff_available: bool,
    pub git_diff_running: bool,
    pub git_repository_available: bool,
    pub git_status_running: bool,
    pub open_diff_available: bool,
    pub open_diff_active: bool,
    pub open_diff_ready: bool,
    pub source_symbols_available: bool,
    pub source_references_available: bool,
    pub source_calls_available: bool,
    pub source_incoming_available: bool,
    pub source_dependencies_available: bool,
    pub source_completion_available: bool,
    pub source_rename_available: bool,
    pub source_active: bool,
    pub source_ready: bool,
    pub source_side_ready: bool,
    pub snippet_insert_available: bool,
    pub snippet_surround_available: bool,
    pub snippet_active: bool,
    pub snippet_next: bool,
    pub snippet_previous: bool,
    pub snippet_ready: bool,
    // Shared pane navigation state; keymap handlers are connected by the frontend.
    pub split_editor: bool,
    pub reference_pinned: bool,
    pub nav_back_available: bool,
    pub nav_forward_available: bool,
    pub can_open_to_side: bool,
}

impl Default for CommandContext {
    fn default() -> Self {
        Self {
            focus: PrimaryFocus::Editor,
            editor_input: true,
            field_input: false,
            has_document: false,
            document_dirty: false,
            dirty_documents: 0,
            can_undo: false,
            can_redo: false,
            has_selection: false,
            rectangular_selection: false,
            clipboard_has_text: false,
            field_paste: false,
            has_project: false,
            explorer_focused: false,
            project_search_running: false,
            project_replace_ready: false,
            document_search_running: false,
            search_history_available: false,
            active_language: None,
            has_progress_profile: false,
            has_database_schema: false,
            can_reopen_closed_tab: false,
            terminal_available: false,
            git_diff_available: false,
            git_diff_running: false,
            git_repository_available: false,
            git_status_running: false,
            open_diff_available: false,
            open_diff_active: false,
            open_diff_ready: false,
            source_symbols_available: false,
            source_references_available: false,
            source_calls_available: false,
            source_incoming_available: false,
            source_dependencies_available: false,
            source_completion_available: false,
            source_rename_available: false,
            source_active: false,
            source_ready: false,
            source_side_ready: false,
            snippet_insert_available: false,
            snippet_surround_available: false,
            snippet_active: false,
            snippet_next: false,
            snippet_previous: false,
            snippet_ready: false,
            split_editor: false,
            reference_pinned: false,
            nav_back_available: false,
            nav_forward_available: false,
            can_open_to_side: false,
        }
    }
}

impl CommandContext {
    pub fn is_abl(&self) -> bool {
        self.active_language
            .as_deref()
            .is_some_and(|id| id.eq_ignore_ascii_case("abl"))
    }
}

pub type EnablePredicate = fn(&CommandContext) -> bool;

#[derive(Clone, Copy)]
pub struct CommandSpec {
    pub id: CommandId,
    pub category: CommandCategory,
    pub label: &'static str,
    pub enabled: EnablePredicate,
}

impl std::fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandSpec")
            .field("id", &self.id)
            .field("category", &self.category)
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct CommandRegistry {
    specs: BTreeMap<CommandId, CommandSpec>,
}

impl CommandRegistry {
    pub fn new(specs: impl IntoIterator<Item = CommandSpec>) -> Result<Self, CommandId> {
        let mut map = BTreeMap::new();
        for spec in specs {
            if map.insert(spec.id, spec).is_some() {
                return Err(spec.id);
            }
        }
        Ok(Self { specs: map })
    }

    pub fn get(&self, id: CommandId) -> Option<&CommandSpec> {
        self.specs.get(&id)
    }

    pub fn contains(&self, id: CommandId) -> bool {
        self.specs.contains_key(&id)
    }

    pub fn is_enabled(&self, id: CommandId, context: &CommandContext) -> bool {
        self.get(id).is_some_and(|spec| (spec.enabled)(context))
    }

    pub fn iter(&self) -> impl Iterator<Item = &CommandSpec> {
        self.specs.values()
    }
}

fn always(_: &CommandContext) -> bool {
    true
}
fn document_search_running(ctx: &CommandContext) -> bool {
    ctx.document_search_running || ctx.rectangular_selection
}
fn search_history(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.search_history_available
}
fn never(_: &CommandContext) -> bool {
    false
}
fn has_document(ctx: &CommandContext) -> bool {
    ctx.has_document
}
fn markdown_preview(ctx: &CommandContext) -> bool {
    ctx.has_document
        && ctx
            .active_language
            .as_deref()
            .and_then(jc_languages::by_id)
            .is_some_and(|p| p.syntax_profile() == jc_languages::syntax::SyntaxProfile::Markdown)
}
fn split(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.split_editor
}
fn nav_back(ctx: &CommandContext) -> bool {
    ctx.nav_back_available
}
fn nav_forward(ctx: &CommandContext) -> bool {
    ctx.nav_forward_available
}
fn open_to_side(ctx: &CommandContext) -> bool {
    ctx.can_open_to_side
}
fn dirty_document(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.document_dirty
}
fn dirty_documents(ctx: &CommandContext) -> bool {
    ctx.dirty_documents > 0
}
fn can_undo(ctx: &CommandContext) -> bool {
    rectangle_editor(ctx) && ctx.can_undo
}
fn can_redo(ctx: &CommandContext) -> bool {
    rectangle_editor(ctx) && ctx.can_redo
}
fn rectangle_editor(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.focus == PrimaryFocus::Editor && ctx.editor_input
}
fn rectangle_active(ctx: &CommandContext) -> bool {
    rectangle_editor(ctx) && ctx.rectangular_selection
}
fn linear_document(ctx: &CommandContext) -> bool {
    ctx.has_document && !ctx.rectangular_selection
}
fn linear_editor(ctx: &CommandContext) -> bool {
    linear_document(ctx) && ctx.focus == PrimaryFocus::Editor && ctx.editor_input
}
fn source_selection(ctx: &CommandContext) -> bool {
    linear_editor(ctx) && ctx.has_selection
}
fn paste(ctx: &CommandContext) -> bool {
    (rectangle_editor(ctx) || ctx.field_input || ctx.field_paste) && ctx.clipboard_has_text
}
fn source_clipboard_selection(ctx: &CommandContext) -> bool {
    (rectangle_editor(ctx) || ctx.field_input) && ctx.has_selection
}
fn select_all(ctx: &CommandContext) -> bool {
    rectangle_editor(ctx) || ctx.field_input
}
fn block_comment(ctx: &CommandContext) -> bool {
    linear_editor(ctx)
        && ctx
            .active_language
            .as_deref()
            .and_then(by_id)
            .is_some_and(|profile| profile.block_comment.is_some())
}
fn line_comment(ctx: &CommandContext) -> bool {
    linear_editor(ctx)
        && ctx
            .active_language
            .as_deref()
            .and_then(by_id)
            .is_some_and(|profile| profile.preferred_comment().is_some())
}
fn project(ctx: &CommandContext) -> bool {
    ctx.has_project
}
fn explorer(ctx: &CommandContext) -> bool {
    ctx.has_project && ctx.explorer_focused
}
fn explorer_focus(ctx: &CommandContext) -> bool {
    ctx.explorer_focused
}
fn git_diff(ctx: &CommandContext) -> bool {
    ctx.git_diff_available
}
fn git_diff_running(ctx: &CommandContext) -> bool {
    ctx.git_diff_running
}
fn git_repository(ctx: &CommandContext) -> bool {
    ctx.git_repository_available
}
fn git_status_running(ctx: &CommandContext) -> bool {
    ctx.git_status_running
}
fn snippet_insert(ctx: &CommandContext) -> bool {
    ctx.editor_input
        && ctx.focus == PrimaryFocus::Editor
        && !ctx.has_selection
        && !ctx.rectangular_selection
        && ctx.snippet_insert_available
}
fn snippet_surround(ctx: &CommandContext) -> bool {
    ctx.editor_input
        && ctx.focus == PrimaryFocus::Editor
        && ctx.has_selection
        && !ctx.rectangular_selection
        && ctx.snippet_surround_available
}
fn snippet_active(ctx: &CommandContext) -> bool {
    ctx.snippet_active
}
fn snippet_next(ctx: &CommandContext) -> bool {
    ctx.snippet_next
}
fn snippet_previous(ctx: &CommandContext) -> bool {
    ctx.snippet_previous
}
fn snippet_ready(ctx: &CommandContext) -> bool {
    ctx.snippet_ready
}
fn source_symbols(ctx: &CommandContext) -> bool {
    ctx.source_symbols_available
}
fn source_references(ctx: &CommandContext) -> bool {
    ctx.source_references_available
}
fn source_calls(ctx: &CommandContext) -> bool {
    ctx.source_calls_available
}
fn source_incoming(ctx: &CommandContext) -> bool {
    ctx.source_incoming_available
}
fn source_dependencies(ctx: &CommandContext) -> bool {
    ctx.source_dependencies_available
}
fn source_completion(ctx: &CommandContext) -> bool {
    ctx.editor_input
        && ctx.focus == PrimaryFocus::Editor
        && !ctx.rectangular_selection
        && !ctx.has_selection
        && ctx.source_completion_available
}
fn source_rename(ctx: &CommandContext) -> bool {
    ctx.source_rename_available && !ctx.rectangular_selection && !ctx.has_selection
}
fn source_active(ctx: &CommandContext) -> bool {
    ctx.source_active
}
fn source_ready(ctx: &CommandContext) -> bool {
    ctx.source_ready
}
fn source_side_ready(ctx: &CommandContext) -> bool {
    ctx.source_side_ready
}
fn open_diff_available(ctx: &CommandContext) -> bool {
    ctx.open_diff_available
}
fn open_diff_active(ctx: &CommandContext) -> bool {
    ctx.open_diff_active
}
fn open_diff_ready(ctx: &CommandContext) -> bool {
    ctx.open_diff_ready
}
fn project_search_running(ctx: &CommandContext) -> bool {
    ctx.project_search_running
}
fn project_replace_ready(ctx: &CommandContext) -> bool {
    ctx.project_replace_ready
}
fn reopen(ctx: &CommandContext) -> bool {
    ctx.can_reopen_closed_tab
}
fn terminal(ctx: &CommandContext) -> bool {
    ctx.terminal_available
}
fn abl(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.is_abl() && ctx.has_progress_profile
}
fn abl_project(ctx: &CommandContext) -> bool {
    ctx.has_project && ctx.has_progress_profile
}

macro_rules! spec {
    ($id:ident, $cat:ident, $label:literal, $enabled:ident) => {
        CommandSpec {
            id: CommandId::$id,
            category: CommandCategory::$cat,
            label: $label,
            enabled: $enabled,
        }
    };
}

pub fn default_registry() -> CommandRegistry {
    let specs: Vec<CommandSpec> = vec![
        spec!(FileNew, File, "Nuevo", always),
        spec!(FileOpen, File, "Abrir…", always),
        spec!(
            FileQuickOpen,
            File,
            "Abrir archivo del proyecto rápidamente",
            project
        ),
        spec!(FileOpenFolder, File, "Abrir carpeta…", always),
        spec!(
            FileChooseEncoding,
            File,
            "Elegir codificación del documento…",
            has_document
        ),
        spec!(FileSave, File, "Guardar", dirty_document),
        spec!(FileSaveAs, File, "Guardar como…", has_document),
        spec!(FileSaveAll, File, "Guardar todo", dirty_documents),
        spec!(FileClose, File, "Cerrar", has_document),
        spec!(FileReopenClosed, File, "Reabrir pestaña cerrada", reopen),
        spec!(FileExit, File, "Salir", always),
        spec!(EditUndo, Edit, "Deshacer", can_undo),
        spec!(EditRedo, Edit, "Rehacer", can_redo),
        spec!(EditCut, Edit, "Cortar", source_clipboard_selection),
        spec!(EditCopy, Edit, "Copiar", source_clipboard_selection),
        spec!(EditPaste, Edit, "Pegar", paste),
        spec!(EditSelectAll, Edit, "Seleccionar todo", select_all),
        spec!(
            EditDocumentStart,
            Navigation,
            "Ir al inicio del documento",
            rectangle_editor
        ),
        spec!(
            EditDocumentEnd,
            Navigation,
            "Ir al final del documento",
            rectangle_editor
        ),
        spec!(
            EditSelectDocumentStart,
            Navigation,
            "Seleccionar hasta el inicio del documento",
            rectangle_editor
        ),
        spec!(
            EditSelectDocumentEnd,
            Navigation,
            "Seleccionar hasta el final del documento",
            rectangle_editor
        ),
        spec!(
            EditDuplicate,
            Edit,
            "Duplicar selección/línea",
            linear_editor
        ),
        spec!(EditDeleteLine, Edit, "Eliminar línea", linear_editor),
        spec!(EditMoveLineUp, Edit, "Mover línea arriba", linear_editor),
        spec!(EditMoveLineDown, Edit, "Mover línea abajo", linear_editor),
        spec!(
            EditToggleLineComment,
            Edit,
            "Alternar comentario",
            line_comment
        ),
        spec!(
            EditToggleBlockComment,
            Edit,
            "Alternar comentario de bloque",
            block_comment
        ),
        spec!(EditIndent, Edit, "Aumentar sangría", rectangle_editor),
        spec!(EditOutdent, Edit, "Reducir sangría", linear_editor),
        spec!(EditJoinLines, Edit, "Unir líneas", linear_editor),
        spec!(EditSplitLine, Edit, "Dividir línea", linear_editor),
        spec!(
            EditRectangleStart,
            Edit,
            "Rectángulo: iniciar desde cursor/selección",
            rectangle_editor
        ),
        spec!(
            EditRectangleLeft,
            Edit,
            "Rectángulo: extender a la izquierda",
            rectangle_editor
        ),
        spec!(
            EditRectangleRight,
            Edit,
            "Rectángulo: extender a la derecha",
            rectangle_editor
        ),
        spec!(
            EditRectangleUp,
            Edit,
            "Rectángulo: extender arriba",
            rectangle_editor
        ),
        spec!(
            EditRectangleDown,
            Edit,
            "Rectángulo: extender abajo",
            rectangle_editor
        ),
        spec!(
            EditRectangleCancel,
            Edit,
            "Rectángulo: cancelar",
            rectangle_active
        ),
        spec!(SearchFind, Search, "Buscar", has_document),
        spec!(SearchReplace, Search, "Reemplazar", has_document),
        spec!(SearchFindNext, Search, "Buscar siguiente", has_document),
        spec!(SearchFindPrevious, Search, "Buscar anterior", has_document),
        spec!(
            SearchSelectionNext,
            Search,
            "Buscar selección siguiente",
            source_selection
        ),
        spec!(
            SearchSelectionPrevious,
            Search,
            "Buscar selección anterior",
            source_selection
        ),
        spec!(
            SearchGoToPosition,
            Search,
            "Ir a línea/columna",
            has_document
        ),
        spec!(
            SearchCancel,
            Search,
            "Cancelar búsqueda del documento/rectángulo",
            document_search_running
        ),
        spec!(
            SearchHistoryPrevious,
            Search,
            "Consulta anterior",
            search_history
        ),
        spec!(
            SearchHistoryNext,
            Search,
            "Consulta siguiente",
            search_history
        ),
        spec!(SearchProject, Search, "Buscar en el proyecto", project),
        spec!(
            SearchReplaceProject,
            Search,
            "Reemplazar en el proyecto",
            project_replace_ready
        ),
        spec!(
            SearchCancelProject,
            Search,
            "Cancelar búsqueda del proyecto",
            project_search_running
        ),
        spec!(SearchToggleCase, Search, "Distinguir mayúsculas", always),
        spec!(SearchToggleWholeWord, Search, "Palabra completa", always),
        spec!(SearchToggleRegex, Search, "Expresión regular", always),
        spec!(PaletteOpen, Tools, "Paleta de comandos…", always),
        spec!(TabNext, Navigation, "Pestaña siguiente", has_document),
        spec!(TabPrevious, Navigation, "Pestaña anterior", has_document),
        spec!(Tab1, Navigation, "Pestaña 1", has_document),
        spec!(Tab2, Navigation, "Pestaña 2", has_document),
        spec!(Tab3, Navigation, "Pestaña 3", has_document),
        spec!(Tab4, Navigation, "Pestaña 4", has_document),
        spec!(Tab5, Navigation, "Pestaña 5", has_document),
        spec!(Tab6, Navigation, "Pestaña 6", has_document),
        spec!(Tab7, Navigation, "Pestaña 7", has_document),
        spec!(Tab8, Navigation, "Pestaña 8", has_document),
        spec!(Tab9, Navigation, "Última pestaña", has_document),
        spec!(ViewExplorer, View, "Explorador", always),
        spec!(
            ExplorerRefresh,
            View,
            "Explorador: actualizar directorio",
            explorer
        ),
        spec!(
            ExplorerCancel,
            View,
            "Explorador: cancelar carga/volver al editor",
            explorer_focus
        ),
        spec!(
            ViewToggleInvisibles,
            View,
            "Mostrar caracteres invisibles",
            always
        ),
        spec!(ViewDatabase, View, "Base de datos/esquema", never),
        spec!(
            ViewMarkdownPreview,
            View,
            "Vista previa de Markdown",
            markdown_preview
        ),
        spec!(
            ViewGit,
            View,
            "Git: diferencias del archivo activo",
            git_diff
        ),
        spec!(GitDiffHead, View, "Git: comparar copia de HEAD", git_diff),
        spec!(
            GitDiffIndex,
            View,
            "Git: comparar copia del índice",
            git_diff
        ),
        spec!(GitDiffRefresh, View, "Git: actualizar copia", git_diff),
        spec!(
            GitDiffCancel,
            View,
            "Git: cancelar comparación",
            git_diff_running
        ),
        spec!(
            GitConfigureExecutable,
            View,
            "Git: motor integrado",
            git_repository
        ),
        spec!(
            GitRepositoryStatus,
            View,
            "Git: estado del repositorio (solo lectura)",
            git_repository
        ),
        spec!(
            GitStatusRefresh,
            View,
            "Git: actualizar estado del repositorio",
            git_repository
        ),
        spec!(
            GitStatusCancel,
            View,
            "Git: cancelar lectura del estado",
            git_status_running
        ),
        spec!(
            DiffCompareOpen,
            View,
            "Comparar grupos abiertos (solo lectura)",
            open_diff_available
        ),
        spec!(
            DiffRefreshOpen,
            View,
            "Actualizar comparación de grupos abiertos",
            open_diff_available
        ),
        spec!(
            DiffCancelOpen,
            View,
            "Cancelar comparación de grupos abiertos",
            open_diff_active
        ),
        spec!(
            DiffOpenNavigateLeft,
            View,
            "Abrir bloque de diferencias en el grupo izquierdo",
            open_diff_ready
        ),
        spec!(
            DiffOpenNavigateRight,
            View,
            "Abrir bloque de diferencias en el grupo derecho",
            open_diff_ready
        ),
        spec!(
            EditInsertSnippet,
            Edit,
            "Insertar plantilla",
            snippet_insert
        ),
        spec!(
            EditSurroundWith,
            Edit,
            "Envolver selección",
            snippet_surround
        ),
        spec!(SnippetNext, Edit, "Plantilla: paso siguiente", snippet_next),
        spec!(
            SnippetPrevious,
            Edit,
            "Plantilla: paso anterior",
            snippet_previous
        ),
        spec!(
            SnippetAccept,
            Edit,
            "Plantilla: aplicar propuesta revisada",
            snippet_ready
        ),
        spec!(SnippetCancel, Edit, "Plantilla: cancelar", snippet_active),
        spec!(ViewSymbols, View, "Símbolos", source_symbols),
        spec!(
            SourceReferences,
            Navigation,
            "Referencias del documento",
            source_references
        ),
        spec!(SourceCalls, Navigation, "Llama a", source_calls),
        spec!(SourceIncoming, Navigation, "Llamado por", source_incoming),
        spec!(
            SourceDependencies,
            Navigation,
            "Dependencias e inclusiones",
            source_dependencies
        ),
        spec!(
            SourceComplete,
            Edit,
            "Completar palabra local",
            source_completion
        ),
        spec!(SourceRename, Edit, "Renombrar símbolo ABL", source_rename),
        spec!(
            SourceRefresh,
            Navigation,
            "Actualizar servicio del documento",
            source_active
        ),
        spec!(
            SourceCancel,
            Navigation,
            "Cancelar servicio del documento",
            source_active
        ),
        spec!(
            SourceAccept,
            Navigation,
            "Aceptar elemento del documento",
            source_ready
        ),
        spec!(
            SourceOpenToSide,
            Navigation,
            "Abrir elemento del documento al lado",
            source_side_ready
        ),
        spec!(
            ViewToggleBottomPanel,
            View,
            "Mostrar/ocultar panel inferior",
            always
        ),
        spec!(ViewTerminal, View, "Terminal", terminal),
        spec!(ViewProblems, View, "Problemas", always),
        spec!(ViewOutput, View, "Salida", always),
        spec!(ViewSearchResults, View, "Resultados de búsqueda", always),
        spec!(EditorSplitRight, View, "Dividir a la derecha", has_document),
        spec!(EditorOneGroup, View, "Un grupo", split),
        spec!(
            EditorFocusPrimary,
            Navigation,
            "Enfocar grupo 1",
            has_document
        ),
        spec!(EditorFocusSecondary, Navigation, "Enfocar grupo 2", split),
        spec!(EditorTogglePin, View, "Anclar/desanclar referencia", split),
        spec!(
            EditorToggleScrollLink,
            View,
            "Vincular/desvincular desplazamiento",
            split
        ),
        spec!(EditorOpenToSide, Navigation, "Abrir al lado", open_to_side),
        spec!(NavigateBack, Navigation, "Navegar atrás", nav_back),
        spec!(NavigateForward, Navigation, "Navegar adelante", nav_forward),
        spec!(FocusNext, Navigation, "Enfocar área siguiente", always),
        spec!(FocusPrevious, Navigation, "Enfocar área anterior", always),
        spec!(ProgressCheck, Build, "Comprobar sintaxis", abl),
        spec!(ProgressCompile, Build, "Compilar", abl),
        spec!(ProgressRun, Build, "Ejecutar", abl),
        spec!(
            ProgressRefreshSchema,
            Build,
            "Actualizar esquema de base de datos",
            abl_project
        ),
        spec!(MenuFile, Menu, "Menú Archivo", always),
        spec!(MenuEdit, Menu, "Menú Editar", always),
        spec!(MenuSearch, Menu, "Menú Buscar", always),
        spec!(MenuProject, Menu, "Menú Proyecto", always),
        spec!(MenuBuild, Menu, "Menú Compilar", always),
        spec!(MenuTools, Menu, "Menú Herramientas", always),
        spec!(MenuView, Menu, "Menú Ver", always),
        spec!(MenuHelp, Menu, "Menú Ayuda", always),
    ];
    CommandRegistry::new(specs).expect("built-in command ids are unique")
}
