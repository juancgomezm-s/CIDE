# M2 — Auditoría de matriz de teclado

Bloque del 2026-11-02. Contraste estático entre
[contrato TUI](TUI_INTERACTION.md), registro/keymap, handlers y regresiones
existentes. No es una revisión manual del teclado ni una certificación de todas
las terminales. Las seis pruebas nuevas fueron integradas y pasaron en la suite local de 209
casos; este worker no ejecutó Cargo, fixtures pesados ni navegadores. El integrador
concentró compilaciones y controles en una ventana serial.

Propiedad de este frente: este documento y
`crates/jc-tui/src/app/keyboard_matrix_tests.rs`. No modifica APIs, handlers,
registro, módulos/exportaciones, PLAN ni VALIDACION. El integrador declara el
módulo de pruebas dentro de `jc-tui/src/app/mod.rs` y registra su resultado.

## Evidencia y huecos

«Existente» identifica una regresión que ya estaba en el repositorio y cuya
evidencia local histórica se conserva en [VALIDACION](VALIDACION.md). «Nueva»
identifica código preparado en este bloque, todavía sin ejecución por este
worker. Ninguna de las dos etiquetas implica una prueba física/manual actual.

| Ruta | Cobertura existente encontrada | Hueco y aporte de este bloque |
|---|---|---|
| Registro/atajos/menús | `interaction.rs`: consistencia, disponibilidad, shortcut compartido y overrides por scope | Se conserva; no se agregan copias de esas tablas en tests |
| Tab/Shift+Tab | Resolución de keymap y `keyboard_edit_matrix_keeps_legacy_bytes_and_non_editor_tab_scope` | Nueva: cambiar realmente Find/Replacement, pegar en cada campo y confirmar sin editar antes de Enter |
| F6/Shift+F6 y Esc | `FocusManager` probado en aislamiento; TUI recorre parte del foco en split/palette | Nueva: secuencia por eventos sobre áreas visibles, overlay que congela foco y doble Esc que restaura área/editor |
| Deshabilitados | Registro rechaza Progress fuera de contexto; palette chequea disponibilidad | Nueva: misma acción Database/Schema por shortcut, palette y menú no debe activar una vista |
| Cierre de vistas | F1/panes comprueban sesión/undo único y cierre programático | Nueva: Ctrl+W cierra una vista compartida sucia sin descartar; la última pide decisión; Esc cancela y D descarta |
| Cambio de pestañas | Resolución de alias y TabManager; vistas persistentes probadas por modelo | Nueva: Ctrl+Tab, Ctrl+BackTab y Ctrl+PageUp/Down preservan selección; Alt+1 en panel no cambia tab |
| Eventos extendidos | Normalización ASCII y BackTab en `input.rs` | Nueva: Release no duplica inserción y Repeat sí entrega una repetición por evento |
| Find/F3/selección/historial/Go to | Tests S0/TUI y recorrido ConPTY previo | No duplicar; ampliar terminales/atajos físicos queda pendiente |
| Proyecto/preview/reemplazo | Tests TUI de prompts/cancelación/confirmación y ConPTY previo | No lanzar fixtures de proyecto en paralelo con la ventana RAM |
| Menús altos/terminal corto | Scroll a comando y opacidad de menú probados | Falta matriz manual de todos los aceleradores, wrap y terminales reales |
| Encoding legacy | Prompt conserva elección; edición/undo en las tres páginas y S0 | Falta cierre con Save/error desde diálogo TUI sobre archivos reales; `jc-files` ya cubre guardado/conflictos por separado |
| Grupos/pin/scroll/historial | Modelo y recorrido de palette/foco/dibujo TUI | Nueva ruta Ctrl+W; no afirmar revisión manual exhaustiva de combinaciones |
| Portapapeles/IME/ratón | `Event::Paste` y portapapeles interno cubiertos parcialmente | Portapapeles del sistema, composición real, selección gráfica y terminales que alteran atajos siguen abiertos |

Los tests nuevos usan documentos en memoria pequeños. No escriben archivos de
usuario ni crean un proceso de lenguaje. Comprueban bytes, revisiones,
selecciones, pestañas, foco y resultado de acciones; no comparan tablas privadas
o una copia de la implementación de cada handler.

## Hallazgo de producto remitido al integrador

En la ruta de menú auditada, `UiOverlay::Menu` procesaba Enter cerrando el overlay
y llamando `dispatch` sin comprobar `registry.is_enabled`. La ruta de tecla sí
resolvía `Resolution::Disabled`, y palette sí comprobaba el registro. Por tanto
el menú podía ejecutar una acción que el mismo registro mostraba deshabilitada.

Disparador sin escrituras: documento normal → Alt+V → Down (Database / Schema,
deshabilitado) → Enter. La expectativa es conservar Explorer y el documento;
la ruta auditada podía abrir `SidebarView::Database`. Un ejemplo de mayor impacto
potencial es File/Save deshabilitado sobre un documento limpio: no debe despacharse
un guardado desde un item deshabilitado. No se ejecutó ese ejemplo de escritura.

`disabled_database_command_has_no_effect_from_shortcut_palette_or_menu` conserva
la regresión observable. El integrador corrigió el dispatcher central TUI para comprobar disponibilidad
antes de cualquier efecto y ejecutó satisfactoriamente la regresión en el bloque. El test no presupone
un panel Git deshabilitado, porque su proveedor de solo lectura avanza en otro
frente; Database/Schema conserva su disponibilidad diferida a Progress real.

## Desajustes documentales encontrados

El contrato TUI conserva etiquetas históricas que deben reconciliarse en una
actualización central, no mediante handlers inventados:

- Go to Line y Find Selected aparecen como pendientes, aunque S0 ya implementó
  Ctrl+G escalar y búsqueda de selección.
- Alt+R figura como toggle regex en Find; el registro actual lo deshabilita y S0
  no tiene motor regex. La UI no debe prometer un regex funcional.
- La lista de futuras teclas vuelve a incluir Ctrl+G como reserva. Debe distinguir
  la operación escalar implementada de navegación semántica todavía pendiente.

La precedencia del keymap no equivale a ejecutar cualquier shortcut global detrás
de un diálogo modal: cada overlay consume su navegación/entrada. Esta auditoría
comprueba que Tab no edita el documento y que Esc/F6 conservan el foco; no añade
atajos globales para cerrar o mutar documentos detrás de un diálogo.

## Cierre de validación

Pruebas preparadas en `keyboard_matrix_tests.rs`:

1. `replace_tab_and_backtab_route_paste_to_fields_before_one_undoable_commit`.
2. `f6_and_escape_follow_visible_areas_and_restore_the_area_under_a_palette`.
3. `disabled_database_command_has_no_effect_from_shortcut_palette_or_menu`.
4. `closing_a_shared_view_preserves_dirty_document_then_last_view_requires_a_choice`.
5. `tab_aliases_preserve_selection_and_bottom_panel_alt_one_does_not_select_a_tab`.
6. `enhanced_keyboard_release_does_not_repeat_an_edit_but_repeat_events_do`.

El módulo fue declarado con `#[cfg(test)]`, formateado y ejecutado en la ventana
serial: las seis regresiones y la suite completa de 209 pasaron, además de fmt,
Clippy workspace/all-targets -D warnings, release y ambos recorridos ConPTY. Si la regresión de menú falla, corregir
la ruta productiva y repetir lo afectado; no debilitar la expectativa para aceptar
la ejecución de items deshabilitados. El cierre de workspace/Clippy/release y la
evidencia final pertenecen al integrador.

Sigue pendiente la matriz manual con terminal/versión/protocolo identificados:
aceleradores de todos los menús, rutas Save/Save As/Close con errores/conflictos,
Ctrl+I/Tab y BackTab, Ctrl+Shift+/, combinaciones Alt/Shift que el terminal no
distinga, y recuperación por palette. Los eventos sintéticos no acreditan que el
teclado físico o el terminal entreguen todas esas combinaciones. No se ejecutó
CI remoto ni se cambió seguridad, configuración, servicios, GPU o Qwen.
