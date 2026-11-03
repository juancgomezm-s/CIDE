<!-- Generated from reviewed source by work/build_gui_inventory.py; each CommandId appears once. -->
| CommandId | Tecla compartida (scope:stroke) | Menú GUI / paleta | Predicado central | Estado, ruta / evidencia |
|---|---|---|---|---|
| `FileNew` | Global:ctrl('n') | File / habilitable | `always` | Conectado: State.new_document → Workbench.new_file. **NATIVE** |
| `FileOpen` | Global:ctrl('o') | File / habilitable | `always` | Conectado: Action.Open → native dialog → Workbench.open_file. **NATIVE** |
| `FileQuickOpen` | Global:ctrl('p') | File / habilitable | `project` | Conectado: open_selector(Files) → QuickOpenController → open_project_file. **SELECTOR** |
| `FileOpenFolder` | Global:ctrl_shift('o') | File / habilitable | `always` | Conectado: Action.ChooseFolder → set_project_root. **SELECTOR** |
| `FileChooseEncoding` | Editor:ctrl_alt('e') | File / habilitable | `has_document` | Conectado: open_encoding_selector → Workbench.set_encoding. **VISIBLE** |
| `FileSave` | Global:ctrl('s') | File / habilitable | `dirty_document` | Conectado: Workbench.save_active / Action.SaveAs para nuevos. **NATIVE** |
| `FileSaveAs` | Global:ctrl_shift('s') | File / habilitable | `has_document` | Conectado: Action.SaveAs → save_flow. **NATIVE** |
| `FileSaveAll` | Global:ctrl_alt('s') | File / habilitable | `dirty_documents` | Conectado: Action.SaveAll → save_flow/save_plan. **SAVE** |
| `FileClose` | Global:ctrl('w') | File / habilitable | `has_document` | Conectado: Action.CloseTab → close_flow. **NATIVE** |
| `FileReopenClosed` | Global:ctrl_shift('t') | File / habilitable | `reopen` | Conectado: reopen_tab → try_reopen_closed. **REOPEN** |
| `FileExit` | Global:alt(f(4)) | File / habilitable | `always` | Conectado: Action.Exit → confirmación dirty → DestroyWindow. **NATIVE** |
| `EditUndo` | Editor:ctrl('z') | Edit / habilitable | `can_undo` | Conectado: State.execute → Workbench.execute::EditUndo. **NATIVE** |
| `EditRedo` | Editor:ctrl('y') | Edit / habilitable | `can_redo` | Conectado: State.execute → Workbench.execute::EditRedo. **NATIVE** |
| `EditCut` | Editor:ctrl('x') | Edit / habilitable | `source_clipboard_selection` | Conectado: Action.Clipboard / Workbench.cut_selection rectangular. **VISIBLE** |
| `EditCopy` | Editor:ctrl('c') | Edit / habilitable | `source_clipboard_selection` | Conectado: Action.Clipboard / Workbench.copy_selection rectangular. **VISIBLE** |
| `EditPaste` | Editor:ctrl('v') | Edit / habilitable | `paste` | Conectado: Action.Clipboard / Workbench.paste rectangular. **VISIBLE** |
| `EditSelectAll` | Editor:ctrl('a') | Edit / habilitable | `rectangle_editor` | Conectado: State.execute → Workbench.execute::EditSelectAll. **NATIVE** |
| `EditDuplicate` | Editor:ctrl('d') | Edit / habilitable | `linear_editor` | Conectado: State.execute → Workbench.execute::EditDuplicate. **LINE** |
| `EditDeleteLine` | Editor:ctrl_shift('d') | Edit / habilitable | `linear_editor` | Conectado: State.execute → Workbench.execute::EditDeleteLine. **LINE** |
| `EditMoveLineUp` | Editor:alt(Up) | Edit / habilitable | `linear_editor` | Conectado: State.execute → Workbench.execute::EditMoveLineUp. **LINE** |
| `EditMoveLineDown` | Editor:alt(Down) | Edit / habilitable | `linear_editor` | Conectado: State.execute → Workbench.execute::EditMoveLineDown. **LINE** |
| `EditToggleLineComment` | Editor:ctrl('/') | Edit / habilitable | `line_comment` | Conectado: State.execute → Workbench.execute::EditToggleLineComment. **LINE** |
| `EditToggleBlockComment` | Editor:ctrl_shift('/') | Edit / habilitable | `block_comment` | Conectado: State.execute → Workbench.execute::EditToggleBlockComment. **LINE** |
| `EditIndent` | Editor:plain(Tab) | Edit / habilitable | `rectangle_editor` | Conectado: State.execute → Workbench.execute::EditIndent. **ACTUAL** |
| `EditOutdent` | Editor:shift(Tab) | Edit / habilitable | `linear_editor` | Conectado: State.execute → Workbench.execute::EditOutdent. **ACTUAL** |
| `EditJoinLines` | Editor:ctrl_shift('j') | Edit / habilitable | `linear_editor` | Conectado: State.execute → Workbench.execute::EditJoinLines. **ACTUAL** |
| `EditSplitLine` | Editor:plain(Enter) | Edit / habilitable | `linear_editor` | Conectado: State.execute → Workbench.execute::EditSplitLine. **ACTUAL** |
| `EditRectangleStart` | — | Edit / habilitable | `rectangle_editor` | Conectado: State.execute → Workbench.execute::EditRectangleStart. **RECT** |
| `EditRectangleLeft` | Editor:alt_shift(Left) | Edit / habilitable | `rectangle_editor` | Conectado: State.execute → Workbench.execute::EditRectangleLeft. **RECT** |
| `EditRectangleRight` | Editor:alt_shift(Right) | Edit / habilitable | `rectangle_editor` | Conectado: State.execute → Workbench.execute::EditRectangleRight. **RECT** |
| `EditRectangleUp` | Editor:alt_shift(Up) | Edit / habilitable | `rectangle_editor` | Conectado: State.execute → Workbench.execute::EditRectangleUp. **RECT** |
| `EditRectangleDown` | Editor:alt_shift(Down) | Edit / habilitable | `rectangle_editor` | Conectado: State.execute → Workbench.execute::EditRectangleDown. **RECT** |
| `EditRectangleCancel` | — | Edit / habilitable | `rectangle_active` | Conectado: State.execute → Workbench.execute::EditRectangleCancel. **RECT** |
| `SearchFind` | Editor:ctrl('f') | Search / habilitable | `has_document` | Conectado: open_search_panel(false). **VISIBLE** |
| `SearchReplace` | Editor:ctrl('h') | Search / habilitable | `has_document` | Conectado: open_search_panel(true). **VISIBLE** |
| `SearchFindNext` | Editor:plain(f(3))<br>Find:plain(f(3))<br>Replace:plain(f(3))<br>Find:plain(Enter)<br>Replace:plain(Enter) | Search / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::SearchFindNext. **VISIBLE** |
| `SearchFindPrevious` | Editor:shift(f(3))<br>Find:shift(f(3))<br>Replace:shift(f(3))<br>Find:shift(Enter)<br>Replace:shift(Enter) | Search / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::SearchFindPrevious. **VISIBLE** |
| `SearchSelectionNext` | Editor:ctrl(f(3)) | Search / habilitable | `source_selection` | Conectado: search_selection → find_selection(true). **REOPEN** |
| `SearchSelectionPrevious` | Editor:ctrl_shift(f(3)) | Search / habilitable | `source_selection` | Conectado: search_selection → find_selection(false). **REOPEN** |
| `SearchGoToPosition` | Global:ctrl('g') | Navigation / habilitable | `has_document` | Conectado: open_navigation_panel → go_to_position. **NAV** |
| `SearchCancel` | Editor:plain(Escape) | Search / habilitable | `document_search_running` | Conectado: State.execute → Workbench.execute::SearchCancel. **REOPEN** |
| `SearchHistoryPrevious` | Find:alt(Up)<br>Replace:alt(Up) | Navigation / habilitable | `search_history` | Conectado: execute_search_option → historial/opciones Workbench. **NAV** |
| `SearchHistoryNext` | Find:alt(Down)<br>Replace:alt(Down) | Navigation / habilitable | `search_history` | Conectado: execute_search_option → historial/opciones Workbench. **NAV** |
| `SearchProject` | Global:ctrl_shift('f') | Search / habilitable | `project` | Conectado: open_project_search(false). **PROJECT** |
| `SearchCancelProject` | BottomPanel:plain(Escape) | Search / habilitable | `project_search_running` | Conectado: ProjectSearch.cancel. **PROJECT** |
| `SearchReplaceProject` | Global:ctrl_shift('h') | Search / habilitable | `project_replace_ready` | Conectado: open_project_replace → preview/apply step. **GIT** |
| `SearchToggleCase` | Find:alt('c')<br>Replace:alt('c') | Search / habilitable | `always` | Conectado: execute_search_option → historial/opciones Workbench. **VISIBLE** |
| `SearchToggleWholeWord` | Find:alt('w')<br>Replace:alt('w') | Search / habilitable | `always` | Conectado: execute_search_option → historial/opciones Workbench. **VISIBLE** |
| `SearchToggleRegex` | Find:alt('r')<br>Replace:alt('r') | — / deshabilitada | `never` | Pendiente: never; motor/dependencia pendiente de autorización, no construir parser propio.. **Sin recorrido GUI** |
| `PaletteOpen` | Global:ctrl_shift('p') | View / habilitable | `always` | Conectado: open_selector(Palette) → accept_selector → execute. **SELECTOR** |
| `TabNext` | Global:ctrl(Tab)<br>Global:ctrl(PageDown) | View / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::TabNext. **NATIVE** |
| `TabPrevious` | Global:ctrl_shift(Tab)<br>Global:ctrl(PageUp) | View / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::TabPrevious. **NATIVE** |
| `Tab1` | Global:alt('1') | Navigation / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::Tab1. **REOPEN** |
| `Tab2` | Global:alt('2') | Navigation / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::Tab2. **REOPEN** |
| `Tab3` | Global:alt('3') | Navigation / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::Tab3. **REOPEN** |
| `Tab4` | Global:alt('4') | Navigation / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::Tab4. **REOPEN** |
| `Tab5` | Global:alt('5') | Navigation / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::Tab5. **REOPEN** |
| `Tab6` | Global:alt('6') | Navigation / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::Tab6. **REOPEN** |
| `Tab7` | Global:alt('7') | Navigation / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::Tab7. **REOPEN** |
| `Tab8` | Global:alt('8') | Navigation / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::Tab8. **REOPEN** |
| `Tab9` | Global:alt('9') | Navigation / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::Tab9. **REOPEN** |
| `ViewExplorer` | Global:ctrl_shift('e') | View / habilitable | `always` | Conectado: show_explorer. **EXP** |
| `ExplorerRefresh` | Sidebar:plain(f(5)) | Project / habilitable | `explorer` | Conectado: refresh_explorer. **EXP** |
| `ExplorerCancel` | Sidebar:plain(Escape) | Project / habilitable | `explorer_focus` | Conectado: cancel_explorer. **EXP** |
| `ViewToggleInvisibles` | Global:ctrl_alt('i') | View / habilitable | `always` | Conectado: State.execute → Workbench.execute::ViewToggleInvisibles. **VISIBLE** |
| `ViewDatabase` | Global:ctrl_alt('d') | — / deshabilitada | `never` | Pendiente: never; capacidad de esquema/proveedor ausente.. **Sin recorrido GUI** |
| `ViewGit` | Global:ctrl_shift('g') | Git / habilitable | `git_diff` | Conectado: request_git → controller compartido → TASKS_MESSAGE. **GIT** |
| `GitDiffHead` | — | Git / habilitable | `git_diff` | Conectado: request_git → controller compartido → TASKS_MESSAGE. **GIT** |
| `GitDiffIndex` | — | Git / habilitable | `git_diff` | Conectado: request_git → controller compartido → TASKS_MESSAGE. **GIT** |
| `GitDiffRefresh` | — | Git / habilitable | `git_diff` | Conectado: request_git → controller compartido → TASKS_MESSAGE. **GIT** |
| `GitDiffCancel` | — | Git / habilitable | `git_diff_running` | Conectado: cancel_git → controller compartido. **GIT** |
| `GitConfigureExecutable` | — | Git / habilitable | `git_repository` | Conectado: Action.ConfigureGit → selector nativo → revalidación. **GIT** |
| `GitRepositoryStatus` | — | Git / habilitable | `git_repository` | Conectado: request_git → controller compartido → TASKS_MESSAGE. **GIT** |
| `GitStatusRefresh` | — | Git / habilitable | `git_repository` | Conectado: request_git → controller compartido → TASKS_MESSAGE. **GIT** |
| `GitStatusCancel` | — | Git / habilitable | `git_status_running` | Conectado: cancel_git → controller compartido. **GIT** |
| `ViewSymbols` | — | — / deshabilitada | `never` | Pendiente: never; capacidad de símbolos/proveedor ausente.. **Sin recorrido GUI** |
| `ViewToggleBottomPanel` | Global:ctrl('j') | — / deshabilitada | `always` | Pendiente: Sin host GUI del panel inferior genérico; Ctrl+J no ejecuta Toggle.. **Sin recorrido GUI** |
| `ViewTerminal` | Global:ctrl('`')<br>BottomPanel:alt('4') | — / deshabilitada | `terminal` | Pendiente: Terminal embebido no disponible; no abrir proceso por inferencia.. **Sin recorrido GUI** |
| `ViewProblems` | BottomPanel:alt('1') | — / deshabilitada | `always` | Pendiente: Sin presentación GUI de diagnósticos F3 (no proveedor nuevo necesario).. **Sin recorrido GUI** |
| `ViewOutput` | BottomPanel:alt('2') | — / deshabilitada | `always` | Pendiente: Sin presentación GUI del log F3 acotado.. **Sin recorrido GUI** |
| `ViewSearchResults` | BottomPanel:alt('3') | Search / habilitable | `always` | Conectado: open_project_search(true). **PROJECT** |
| `EditorSplitRight` | Global:alt_shift('2') | View / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::EditorSplitRight. **NATIVE** |
| `EditorOneGroup` | Global:alt_shift('1') | View / habilitable | `split` | Conectado: State.execute → Workbench.execute::EditorOneGroup. **NATIVE** |
| `EditorFocusPrimary` | Global:ctrl('1') | View / habilitable | `has_document` | Conectado: State.execute → Workbench.execute::EditorFocusPrimary. **NATIVE** |
| `EditorFocusSecondary` | Global:ctrl('2') | View / habilitable | `split` | Conectado: State.execute → Workbench.execute::EditorFocusSecondary. **NATIVE** |
| `EditorTogglePin` | Global:ctrl_alt('p') | View / habilitable | `split` | Conectado: State.execute → Workbench.execute::EditorTogglePin. **NATIVE** |
| `EditorToggleScrollLink` | Global:ctrl_alt('l') | View / habilitable | `split` | Conectado: State.execute → Workbench.execute::EditorToggleScrollLink. **ACTUAL** |
| `EditorOpenToSide` | Editor:ctrl(Enter)<br>Sidebar:ctrl(Enter) | Project / habilitable | `open_to_side` | Conectado: activate_explorer(true) / Workbench.open_active_to_side. **EXP** |
| `NavigateBack` | Global:alt(Left) | Navigation / habilitable | `nav_back` | Conectado: State.execute → Workbench.execute::NavigateBack. **NAV** |
| `NavigateForward` | Global:alt(Right) | Navigation / habilitable | `nav_forward` | Conectado: State.execute → Workbench.execute::NavigateForward. **NAV** |
| `FocusNext` | Global:plain(f(6)) | Project / habilitable | `always` | Conectado: alternar explorer_focused si región visible. **EXP** |
| `FocusPrevious` | Global:shift(f(6)) | Project / habilitable | `always` | Conectado: alternar explorer_focused si región visible. **EXP** |
| `ProgressCheck` | Global:plain(f(7)) | — / deshabilitada | `abl` | Fuera del bloque: Sin proveedor validado ni perfil GUI; no supported. Deuda del enum, M3.. **No ejecutado** |
| `ProgressCompile` | Global:ctrl(f(7)) | — / deshabilitada | `abl` | Fuera del bloque: Sin proveedor validado ni perfil GUI; no supported. Deuda del enum, M3.. **No ejecutado** |
| `ProgressRun` | Global:plain(f(5)) | — / deshabilitada | `abl` | Fuera del bloque: Sin proveedor validado ni perfil GUI; no supported. Deuda del enum, M3.. **No ejecutado** |
| `ProgressRefreshSchema` | Global:shift(f(7)) | — / deshabilitada | `abl_project` | Fuera del bloque: Sin proveedor validado ni perfil GUI; no supported. Deuda del enum, M3.. **No ejecutado** |
| `MenuFile` | Global:alt('f') | — / deshabilitada | `always` | Adaptación nativa parcial: Menú Win32 equivalente por Alt; no ejecuta OpenMenu del registro.. **Solo inspección estática; aceleradores físicos pendientes** |
| `MenuEdit` | Global:alt('e') | — / deshabilitada | `always` | Adaptación nativa parcial: Menú Win32 equivalente por Alt; no ejecuta OpenMenu del registro.. **Solo inspección estática; aceleradores físicos pendientes** |
| `MenuSearch` | Global:alt('s') | — / deshabilitada | `always` | Adaptación nativa parcial: Menú Win32 equivalente por Alt; no ejecuta OpenMenu del registro.. **Solo inspección estática; aceleradores físicos pendientes** |
| `MenuProject` | Global:alt('p') | — / deshabilitada | `always` | Adaptación nativa parcial: Menú Win32 equivalente por Alt; no ejecuta OpenMenu del registro.. **Solo inspección estática; aceleradores físicos pendientes** |
| `MenuBuild` | Global:alt('b') | — / deshabilitada | `always` | Adaptación nativa parcial: Sin menú nativo equivalente; Tools agrupa Palette en View, Build espera proveedor, Help sin contenido definido.. **Solo inspección estática; aceleradores físicos pendientes** |
| `MenuTools` | Global:alt('t') | — / deshabilitada | `always` | Adaptación nativa parcial: Sin menú nativo equivalente; Tools agrupa Palette en View, Build espera proveedor, Help sin contenido definido.. **Solo inspección estática; aceleradores físicos pendientes** |
| `MenuView` | Global:alt('v') | — / deshabilitada | `always` | Adaptación nativa parcial: Menú Win32 equivalente por Alt; no ejecuta OpenMenu del registro.. **Solo inspección estática; aceleradores físicos pendientes** |
| `MenuHelp` | Global:alt('h') | — / deshabilitada | `always` | Adaptación nativa parcial: Sin menú nativo equivalente; Tools agrupa Palette en View, Build espera proveedor, Help sin contenido definido.. **Solo inspección estática; aceleradores físicos pendientes** |
