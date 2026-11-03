# Inventario de paridad universal GUI — 2026-11-03

**Actualización posterior:** el [host Problems/Output y foco de tres regiones](M2B_BOTTOM_PANELS.md)
ya está integrado. Cierra las tres rutas ViewToggleBottomPanel/ViewProblems/ViewOutput
y amplía FocusNext/Previous; ahora88 comandos tienen ruta GUI. La matriz/JSON de
esta auditoría se conservan como evidencia histórica de `8fbd879`, sin regenerarlos
ni atribuirles pruebas posteriores. Las otras limitaciones siguen vigentes.

Revisión de la base `78769e9`, registro, Workbench, keymap, menús/paleta y handlers
reales; no una lista de funciones futuras. La [matriz completa](M2B_GUI_COMMAND_MATRIX.md)
incluye una fila por cada uno de los **104 CommandId**, predicado, binding con scope,
menú nativo, disponibilidad de paleta, ruta y evidencia. El [JSON de fuentes](evidence/m2b-gui-inventory-2026-11-03.json)
conserva hashes y datos de cada fila. La inspección estática no ejecuta comandos ni
certifica todos sus recorridos. No se repitieron suites históricas para completar filas.

## Cómo interpretar la matriz

`Conectado` exige entrada real y ruta de ejecución: `supported` solo no basta.
La paleta lista todo el registro (hasta256), pero habilita únicamente
`supported && predicate`. Una entrada deshabilitada sigue visible; no es una
implementación ni un comando oculto. Teclas muestran el **binding compartido**:
no implican que el frontend lo admita ni que atraviese un modal. Menús indicados
son los de Win32, no el MenuModel TUI. `—` significa sin esa entrada.
La ejecución directa de `OpenMenu(Menu*)` no aplica al frontend Win32: tiene
menús nativos. Sus equivalencias faltantes sí se registran como huecos. Servicios
ausentes (regex, símbolos, esquema, Progress) son pendientes, no casos aceptados
por declararlos «no aplicables».

Las rutas se verificaron en `windows.rs` (COMMANDS, context, execute,
execute_menu, key, procedure y create_menu), `selector_flow.rs` (accept_selector),
`selector.rs` (prepare_palette) y `jc-app/workbench/commands.rs`. La GUI termina
en el Workbench o los controladores compartidos; no tiene otro editor editable.

`editor_input` requiere fuente Editor real o invocador de paleta todavía vigente
(DocumentId/revisión/encoding/ViewId/pane/caret/anchor y geometría rectangular).
Find/Replace, Go to, selectores, proyecto, reemplazo, Git y Explorer no satisfacen
ese destino. Antes de cerrar overlays, `execute_menu` comprueba disponibilidad;
al aceptar paleta vuelve a comprobar. Las teclas de cada modal se consumen en
su handler, salvo las acciones globales que `key` deja pasar expresamente.
No asumir que todos los bindings Global están disponibles dentro de todo modal.

Predicados de edición: `rectangle_editor`=documento+Editor+editor_input;
`linear_editor` agrega ausencia de rectángulo. `source_selection` agrega selección
lineal; Copy/Cut admiten rectángulo. Undo/Redo exigen posibilidad de historia;
Paste obtiene contenido externo solo a petición explícita (la disponibilidad no
lee el clipboard ajeno). Comentarios requieren metadata del perfil. `split`
exige documento y segundo grupo; pin y enlace de scroll son independientes.
Los demás predicados se conservan literalmente en la matriz y su definición
canónica está en `jc-app/src/command.rs`: no se duplican reglas en este documento.

## Evidencia histórica, con alcance explícito

Los códigos de la matriz remiten a cierres de **bloques**, no a prueba individual
de cada combinación teclado/menú/paleta o de cada comando que comparta bloque.
Una ruta inspeccionada sin caso individual documentado conserva esa laguna;
no se promueve a validada físicamente por aparecer en un enum o reporte agregado.
Fechas locales de documentación: 2 y3 de noviembre2026; reportes usan UTC.

| Código | Evidencia existente | Qué acredita y qué limita |
|---|---|---|
| NATIVE | [Primer GUI](M2B_NATIVE_WINDOWS.md), 2026-11-02,339 Rust selectivos/16 HWND | Archivo/edición/selección/Join/Split/grupos/guardado y GDI sobre fixture propio; no diálogos físicos ni todas las teclas |
| SELECTOR | [Selectores](M2B_NATIVE_SELECTORS.md), 2026-11-02,350 Rust/23 HWND | Quick Open/Palette/Folder y resultados/versiones; no Ctrl+V en selector |
| VISIBLE | [Invisibles/multilínea](M2_INVISIBLES_MULTILINE.md), 2026-11-02,376 Rust selectivos/29 HWND | Presentación, búsqueda/campos/encoding/clipboard transportado en memoria; no interoperabilidad OS |
| SAVE | [Save All](M2B_SAVE_ALL.md), 2026-11-02,45 dirigidos/32 HWND | Targets, decisiones inyectadas, cancel/error/pendientes; no interacción física con diálogo |
| PROJECT | [Search Plus GUI](M2B_PROJECT_SEARCH_NATIVE.md), 2026-11-02,391 Rust selectivos/41 HWND | Consultas, resultados versionados/navegación/cancel y raíz; no búsqueda regex |
| GIT | [Reemplazo/Git](M2B_PROJECT_REPLACE_GIT.md), 2026-11-03,53 dirigidos/58 HWND | Preview/aplicación por archivo y lectura Git; no escrituras Git ni paths status abiertos |
| EXP | [Explorer](M2B_SHARED_EXPLORER.md), 2026-11-03,352 relacionados/70 HWND/6 ConPTY | Catálogo compartido, apertura/foco/versiones, split/pin/compact; symlink opcional omitido |
| NAV | [Navegación](M2B_NATIVE_NAVIGATION.md), 2026-11-03,78 dirigidos/14 HWND | Go to/historial por grupo/búsquedas y stale targets; no LSP/display positions |
| LINE | [Líneas/comentarios](M2B_NATIVE_LINE_EDIT.md), 2026-11-03,23 dirigidos/14 HWND | Seis ediciones compartidas, perfiles, foco/stale/split/undo; no ejecución ABL |
| REOPEN | [Selección/reapertura](M2B_SELECTION_REOPEN.md), 2026-11-03,11 dirigidos/15 HWND | Selección literal/cancel/reabrir/tabs, conflictos/revisión; no recuperación de borradores |
| RECT | [Rectángulo](M2B_RECTANGLE_GUI.md), 2026-11-03,26 dirigidos/15 HWND | Repertorio acotado, TAB4/virtual, clipboard interno, drag por mensajes/captura; no Unicode general ni mouse físico |
| ACTUAL | Bloque de edición/grupos de esta revisión | Se registra su resultado de cierre abajo; no sumarlo a históricos como corrida global |

## Huecos concretos encontrados y decisión

El inventario inicial encontró84 comandos `supported`,82 entradas COMMANDS.
Indent/Outdent tenían tecla y paleta, sin menú. El enlace de scroll tenía binding
Ctrl+Alt+L y operación Workbench, pero faltaba `supported`: ni tecla, menú ni
aceptación de paleta lo ejecutaban. Join/Split e Indent/Outdent usaban predicados
sin `editor_input`, permitiendo intención de fuente desde menú detrás de un modal.

Bloque admitido: **edición y grupos**. Exponer Indent/Outdent en Edit y enlace de
scroll en View/paleta/tecla, agregar indicador persistente, alinear los cuatro
permisos de edición con el control real e integrar sus flujos antes de validar.
Workbench conserva transacciones/bytes/vistas/undo y el delta de scroll existente;
no sincronizar carets ni inventar otro modelo de viewport. Tras ese cableado hay
**85 comandos supported y85 entradas COMMANDS**. Eso no cierra los otros19 ni M2B.

Pendientes independientes concretos:

| Área | Estado y próximo requisito |
|---|---|
| Panel inferior, Problems, Output | **Hueco GUI real**: ViewToggleBottomPanel/ViewProblems/ViewOutput no supported; Workbench devuelve solo View. Falta host/presentación de resultados F3, selección y foco. La TUI sí lo implementa; no declarar paridad ni culpar a un proveedor ausente por toda la UI. Requiere otro bloque de integración de paneles, sin construir AVM |
| FocusNext/Previous | GUI alterna Editor/Explorer visible, sin recorrido por panel inferior genérico. Conectado, paridad parcial; incorporar la tercera región al construir el host anterior |
| SearchResults | GUI ofrece panel de proyecto específico; no equivale a un host universal de todos los paneles |
| Terminal | terminal_available=false; no terminal embebido ni proceso a iniciar en este bloque |
| Regex | never; admisión documentada, dependencia nueva no autorizada. Mantener deshabilitado |
| Database/Symbols/Progress* | Capacidades/proveedor interno pendientes, fuera del bloque. No implementar por inferencia ni presentar placeholders como proveedor |
| MenuFile/Edit/Search/Project/View | Adaptación nativa por Alt a menús Win32; no ruta OpenMenu compartida. Aceleradores físicos aún sin aceptación |
| MenuBuild/Tools/Help | Sin equivalencia completa: Palette está en View, Build espera proveedor, Help no tiene contenido/flujo definido. No crear handlers decorativos |
| Entrada en selectores | QuickOpen/Palette/Encoding aceptan WM_CHAR/Backspace y navegación, sin Ctrl+V/Home-End de caret/selección de campo. Campo limitado, no fuente; mejora de entrada pendiente separada |
| Mouse y edición general | Click/Shift+click y drag rectangular existen. Selección lineal continua por arrastre, wheel, graphemes/shaping/bidi y ancho general no se deducen del bloque rectangular; pendientes GUI |

La siguiente integración universal concreta es el **host de paneles y foco de tres
regiones**, consumiendo resultados existentes F3 sin proveedor nuevo. Este informe
no define por sí solo contratos nuevos de proveedores, semántica de terminal o Help.
El siguiente bloque requiere concretar sus destinos/filas/cancelación/estado antes
de implementarlos; no se mezclan con el cierre finito de edición/grupos.

## Puertas que siguen abiertas

OS clipboard rectangular externo y certificación del clipboard lineal, IME, DPI,
accesibilidad, pantalla/mouse físicos y RAM global; página legacy real aún no
confirmada. Unicode general, opcional symlink omitido y dos notices GNU pendientes
del ZIP de desarrollo. No repetir perfiles cerrados para esta inspección. Sin
dependencias/proveedores/AVM/instalaciones, CI/push/publicación ni cambios de seguridad.

## Cierre del bloque integrado

Construcción completa antes de checks, un integrador y worker existente con un
archivo exclusivo de smoke. **Dos casos Rust dirigidos aprobados**: uno nuevo TUI
combinado (cuatro encodings, modal/Explorer, edición y undo) y el parser CLI afectado.
**12 checks nativos aprobados**, cinco nuevos de este bloque más siete de arranque/
pintura. No corrida completa del workspace ni secciones nativas antiguas repetidas.
fmt, Clippy workspace/all-targets `-D warnings` y release workspace bins/examples
aprobados sobre Rust1.85.1 MSVC x64 offline.

La nueva sección `parity` del ejecutable propio comprueba Indent/Outdent por
Tab/ShiftTab/menú/paleta, BOM/EOL mixtos/ñ en cuatro encodings, undo/redo por acción,
foco modal/Explorer y target de paleta cambiado; Join/Split y restricciones de
rectángulo. CtrlAltL/menú/paleta enlazan/desenlazan/reanudan **delta vertical** sin
sincronizar caret, documento o pin. Resize real de HWND propio360/1400 conserva
split/link/pin; composición repetida y bitmap no cambian views ni admiten jobs.
One Group apaga enlace e indicador. Memdocs se descartan; fixture13B, identidad/
revisión y catálogo originales y metadata Git sintética quedan intactos. Un
WM_PAINT, cero errores. Clipboard OS y Save no se ejecutan en esta sección.

El primer Clippy señaló construcción ineficiente de texto del fixture con
format/collect; se sustituyó por repeat y pasó el nuevo Clippy antes de release.
Ese fallo se retiene; las pruebas funcionales pasaron en su primera ejecución.
Los contadores no se suman con históricos para afirmar una corrida global.

[Hashes y logs](evidence/m2b-gui-parity-2026-11-03-metadata.json),
[reporte nativo](evidence/m2b-gui-parity-2026-11-03-native-report.json),
[scroll enlazado GDI](evidence/m2b-gui-parity-2026-11-03-native-parity.png),
[fixture restaurado](evidence/m2b-gui-parity-2026-11-03-native-frame.png).
Ambos PNG revisados visualmente tras conversión BMP→PNG sin pérdida; no pantalla física.

Paquete local `work/packages/CIDE-windows-dev-gui-parity-2026-11-03.zip`,
3.995.333 bytes, SHA-256 `8e24537ebd50add819233f04ebf6cd349f7231585b11e66be6058d1e6a59eaf6`.
CRC y hashes de manifiesto de 69 entradas verificados. [Resultado](evidence/m2b-gui-parity-2026-11-03-package.json);
dos notices GNU pendientes, sin certificación de distribución.
